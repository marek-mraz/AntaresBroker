// SPDX-License-Identifier: EUPL-1.2
//! 5.16 Snapshots (optional API group; resources 6.36 /snapshots,
//! 6.37 /snapshots/{id}, 6.38 /snapshots/{id}/clone; scoping 6.3.22).
//!
//! A Snapshot freezes the results of a set of queries (5.2.41) into an
//! isolated copy on which Core + Temporal API operations run implicitly
//! local (5.5.15). Implementation shape: each snapshot owns a synthetic
//! internal tenant ("snap-…"); the 6.3.22 NGSILD-Snapshot header is resolved
//! by a middleware that rewrites the request's tenant, so every existing
//! Core/Temporal handler serves snapshot content unchanged — and, because
//! no registrations exist under the synthetic tenant, all operations are
//! naturally local.
//!
//! Snapshot metadata lives in the store (Kind::Snapshot, ADR-0012) —
//! persistent modes serve snapshots across restarts; 5.5.15 still allows
//! dropping them under resource pressure (evict_over_cap).
//! Fills follow the 5.7.2.4 distributed path and page past max_limit, up to
//! the 5.5.6 result ceiling (fill_cap) and only for as long as the snapshot
//! exists — a fill whose snapshot was deleted or evicted frees its copy
//! (fill_cancelled), which nothing else could reach.
//! Resource pressure evicts lowest-priority snapshots (evict_over_cap).

use crate::negotiate::{
    check_params, created, no_content, parse_accept, parse_body, respond, tenant_from, ApiError,
    BodyKind, CleanParams,
};
use crate::state::{now_iso, AppState};
use antares_model::{NgsiError, TenantId, API_ROOT};
use antares_store::CurrentStateDriverExt;
use antares_store::Kind;
use axum::body::Bytes;
use axum::extract::{Path, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use serde_json::{json, Map, Value};
use std::collections::HashMap;

const DEFAULT_LIFETIME_SECS: i64 = 86_400; // 1 day
const MAX_LIFETIME_SECS: i64 = 604_800; // 7 days — the "configured limit"

fn bad(m: String) -> NgsiError {
    NgsiError::BadRequestData(m)
}

/// 5.5.6: an unexpected failure while filling a snapshot surfaces as
/// InternalError. Its detail reaches the client twice — in
/// snapshotQueriesDetails.problemDetails (5.2.41) and in the 5.3.4
/// SnapshotNotification body — so the internal error text stays in the
/// server log and the client-visible detail is generic.
fn opaque(what: &str, e: &dyn std::fmt::Debug) -> NgsiError {
    tracing::error!("snapshot {what} failed: {e:?}");
    NgsiError::InternalError(format!("{what} failed"))
}

/// 5.2.41: expiresAt from the suggested snapshotLifetime, bounded by the
/// system limit (5.16.1.4 "applying the configured limit").
fn expires_at(meta: &Map<String, Value>) -> Result<String, NgsiError> {
    let secs = match meta.get("snapshotLifetime").and_then(Value::as_str) {
        Some(d) => crate::entity_maps::iso8601_secs(d)
            .ok_or_else(|| {
                bad(format!(
                    "snapshotLifetime is not an ISO 8601 duration: {d:?}"
                ))
            })?
            .min(MAX_LIFETIME_SECS),
        None => DEFAULT_LIFETIME_SECS,
    };
    Ok((chrono::Utc::now() + chrono::Duration::seconds(secs))
        .to_rfc3339_opts(chrono::SecondsFormat::Millis, true))
}

fn expired(meta: &Value) -> bool {
    meta.get("expiresAt")
        .and_then(Value::as_str)
        .and_then(|e| chrono::DateTime::parse_from_rfc3339(e).ok())
        .is_some_and(|e| e < chrono::Utc::now())
}

/// The internal tenant holding the synth-tenant -> (owner, snapshot id)
/// index docs (durable reverse lookup for 6.3.22 notification stamping).
fn snap_index_tenant() -> Option<TenantId> {
    TenantId::new("snap-index").ok()
}

/// 5.2.41: a Snapshot's type is fixed to "Snapshot". The reverse-index
/// marker docs share Kind::Snapshot storage but are not Snapshots, so every
/// place that reads a document as one filters on this — a listing that does
/// not would count and select the markers as snapshots.
fn is_snapshot(meta: &Value) -> bool {
    meta.get("type").and_then(Value::as_str) == Some("Snapshot")
}

/// Registry access with lazy expiry (an expired snapshot is gone; its data
/// purge runs in the background). Snapshot docs live in the store
/// (Kind::Snapshot) so restarts keep them on persistent store modes.
pub(crate) fn snap_get(st: &AppState, tenant: &TenantId, id: &str) -> Option<Value> {
    let meta = st.store.get(tenant, Kind::Snapshot, id).ok().flatten()?;
    if !is_snapshot(&meta) {
        return None;
    }
    if expired(&meta) {
        snap_remove(st, tenant, id, &meta);
        return None;
    }
    Some(meta)
}

/// 5.16.1.4: "If the NGSI-LD endpoint already knows about this Snapshot …
/// an error of type AlreadyExists shall be raised." The store's create is
/// the atomic check-and-insert, so two concurrent creates of the same id
/// cannot both succeed and overwrite each other's synthetic tenant.
fn snap_insert(st: &AppState, tenant: &TenantId, id: &str, meta: &Value) -> Result<(), NgsiError> {
    if !st.store.create(tenant, Kind::Snapshot, id, meta.clone())? {
        return Err(NgsiError::AlreadyExists(format!(
            "snapshot {id} already exists"
        )));
    }
    // durable reverse index for snapshot_of_synth
    if let (Some(synth), Some(idx)) = (
        meta.get("__tenant").and_then(Value::as_str),
        snap_index_tenant(),
    ) {
        let _ = st.store.create(
            &idx,
            Kind::Snapshot,
            synth,
            json!({"tenant": tenant.as_str(), "snapshot": id}),
        );
    }
    Ok(())
}

/// Write back the output-only members (5.2.41 Table 5.2.41-2) of a snapshot
/// that already exists; a snapshot deleted meanwhile is not resurrected.
fn snap_put(st: &AppState, tenant: &TenantId, meta: Value) {
    let id = meta
        .get("id")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_owned();
    let _ = st.store.mutate(tenant, Kind::Snapshot, &id, |d| {
        *d = meta.clone();
        Ok::<_, std::convert::Infallible>(())
    });
}

/// Remove a snapshot everywhere: doc, synth-tenant index, data purge.
fn snap_remove(st: &AppState, tenant: &TenantId, id: &str, meta: &Value) {
    let _ = st.store.delete(tenant, Kind::Snapshot, id);
    if let (Some(synth), Some(idx)) = (
        meta.get("__tenant").and_then(Value::as_str),
        snap_index_tenant(),
    ) {
        let _ = st.store.delete(&idx, Kind::Snapshot, synth);
    }
    purge_data_bg(st, meta);
}

fn synth_tenant(meta: &Value) -> Option<TenantId> {
    meta.get("__tenant")
        .and_then(Value::as_str)
        .and_then(|t| TenantId::new(t).ok())
}

/// Free the snapshot's isolated copy. 5.5.15 permits every Core and
/// Temporal operation on a snapshot, so a snapshot-scoped request can put
/// documents of ANY kind under the synthetic tenant — once the snapshot is
/// gone none of them is reachable (no client may name the internal tenant),
/// so every kind is dropped, not only the filled data.
fn purge_synth(st: &AppState, synth: &TenantId) {
    for kind in [
        Kind::Entity,
        Kind::Subscription,
        Kind::Registration,
        Kind::CSourceSubscription,
        Kind::EntityMap,
        Kind::DistSub,
    ] {
        for doc in st.store.list(synth, kind).unwrap_or_default() {
            if let Some(id) = doc.get("id").and_then(Value::as_str) {
                let _ = st.store.delete(synth, kind, id);
            }
        }
    }
    // history lives behind its own driver seam
    for doc in st.temporal.list(synth).unwrap_or_default() {
        if let Some(id) = doc.get("id").and_then(Value::as_str) {
            let _ = st.temporal.delete(synth, id);
        }
    }
}

fn purge_data_bg(st: &AppState, meta: &Value) {
    let Some(synth) = synth_tenant(meta) else {
        return;
    };
    let st = st.clone();
    crate::spawn(async move {
        purge_synth(&st, &synth);
    });
}

/// The snapshot document as presented to clients (internal members hidden).
fn present(meta: &Value) -> Value {
    let mut out = meta.clone();
    if let Some(o) = out.as_object_mut() {
        o.remove("__tenant");
    }
    out
}

enum Mode {
    Create,
    Clone,
    Update,
}

/// 5.2.41 Table 5.2.41-1/-2 validation. Output-only members are IGNORED
/// (stripped); read-only members in the wrong mode are BadRequestData.
fn validate(body: &Value, mode: Mode) -> Result<Map<String, Value>, NgsiError> {
    let mut o = body
        .as_object()
        .cloned()
        .ok_or_else(|| bad("snapshot must be a JSON object".into()))?;
    o.remove("@context");
    // output-only members (Table 5.2.41-2) "shall be ignored"
    for k in [
        "snapshotStatus",
        "snapshotQueriesDetails",
        "snapshotTemporalQueriesDetails",
        "createdAt",
        "modifiedAt",
        "expiresAt",
        "lastUsedAt",
    ] {
        o.remove(k);
    }
    match mode {
        Mode::Create => {
            if o.get("type").and_then(Value::as_str) != Some("Snapshot") {
                return Err(bad("type must be \"Snapshot\" (5.2.41)".into()));
            }
            if !o.contains_key("snapshotQueries") && !o.contains_key("snapshotTemporalQueries") {
                return Err(bad(
                    "at least one of snapshotQueries or snapshotTemporalQueries \
                     shall be present (5.2.41)"
                        .into(),
                ));
            }
        }
        Mode::Clone | Mode::Update => {
            // "both shall be omitted when updating the Snapshot status or
            // cloning the Snapshot" — read-only after creation
            if o.contains_key("snapshotQueries") || o.contains_key("snapshotTemporalQueries") {
                return Err(bad(
                    "snapshotQueries/snapshotTemporalQueries are read-only after \
                     creation (5.2.41)"
                        .into(),
                ));
            }
            // Table 5.2.41-1: the id "cannot be later modified in update
            // operations" — of whatever JSON type it is sent as
            if matches!(mode, Mode::Update) && o.contains_key("id") {
                return Err(bad("the snapshot id cannot be modified (5.2.41)".into()));
            }
        }
    }
    for (key, temporal) in [
        ("snapshotQueries", false),
        ("snapshotTemporalQueries", true),
    ] {
        if let Some(qs) = o.get(key) {
            let arr = qs
                .as_array()
                .filter(|a| !a.is_empty())
                .ok_or_else(|| bad(format!("{key} must be a non-empty array of Query (5.2.41)")))?;
            for q in arr {
                let qo = q
                    .as_object()
                    .ok_or_else(|| bad(format!("{key} entries must be Query objects (5.2.23)")))?;
                if qo.get("type").and_then(Value::as_str) != Some("Query") {
                    return Err(bad(format!("{key} entries must have type Query (5.2.23)")));
                }
                if temporal != qo.contains_key("temporalQ") {
                    return Err(bad(format!(
                        "{key} entries must {} a temporalQ element (5.2.41)",
                        if temporal { "carry" } else { "not carry" }
                    )));
                }
            }
        }
    }
    if let Some(p) = o.get("snapshotPriority") {
        let ok = p.as_i64().is_some_and(|n| (1..=10).contains(&n));
        if !ok {
            return Err(bad(
                "snapshotPriority must be an integer between 1 and 10 (5.2.41)".into(),
            ));
        }
    }
    if let Some(l) = o.get("snapshotLifetime") {
        let ok = l
            .as_str()
            .and_then(crate::entity_maps::iso8601_secs)
            .is_some();
        if !ok {
            return Err(bad(
                "snapshotLifetime must be an ISO 8601 duration (5.2.41)".into(),
            ));
        }
    }
    if let Some(e) = o.get("endpoint") {
        if e.as_str().is_none() {
            return Err(bad("endpoint must be a URI string (5.2.41)".into()));
        }
    }
    Ok(o)
}

/// Fresh metadata for a new snapshot (create or clone) — 5.16.1.4/5.16.2.4:
/// timestamps now, status "preparing", priority default 5, bounded expiresAt.
fn new_meta(mut o: Map<String, Value>) -> Result<(String, Value), NgsiError> {
    let id = match o.get("id").and_then(Value::as_str) {
        Some(id) => {
            antares_model::EntityId::new(id)
                .map_err(|_| bad(format!("snapshot id is not a valid URI: {id:?}")))?;
            id.to_owned()
        }
        None => {
            let id = format!("urn:ngsi-ld:Snapshot:{}", uuid::Uuid::new_v4());
            o.insert("id".into(), Value::String(id.clone()));
            id
        }
    };
    let ts = now_iso();
    o.insert("type".into(), Value::String("Snapshot".into()));
    o.insert("createdAt".into(), Value::String(ts.clone()));
    o.insert("modifiedAt".into(), Value::String(ts));
    o.insert("snapshotStatus".into(), Value::String("preparing".into()));
    // 5.2.41 Table 5.2.41-2: lastUsedAt "is initialized at creation time"
    o.insert("lastUsedAt".into(), Value::String(now_iso()));
    o.insert("expiresAt".into(), Value::String(expires_at(&o)?));
    o.entry("snapshotPriority".to_owned())
        .or_insert(Value::Number(5.into()));
    o.insert(
        "__tenant".into(),
        Value::String(format!("snap-{}", uuid::Uuid::new_v4().simple())),
    );
    Ok((id, Value::Object(o)))
}

/// 5.2.41: lastUsedAt tracks "the point in time when the snapshot was most
/// recently used" — refreshed on every snapshot-scoped operation.
fn snap_touch(st: &AppState, tenant: &TenantId, id: &str) {
    let _ = st.store.mutate(tenant, Kind::Snapshot, id, |meta| {
        if let Some(o) = meta.as_object_mut() {
            o.insert("lastUsedAt".into(), Value::String(now_iso()));
        }
        Ok::<_, std::convert::Infallible>(())
    });
}

/// Reverse lookup: which (owner tenant, snapshot id) does a synthetic
/// "snap-…" tenant belong to? Used to stamp NGSILD-Snapshot on
/// notifications from snapshot-scoped subscriptions (6.3.22) without
/// leaking the internal tenant.
pub(crate) fn snapshot_of_synth(st: &AppState, synth: &str) -> Option<(TenantId, String)> {
    // only synthetic tenants are indexed; every other tenant skips the lookup
    if !synth.starts_with("snap-") {
        return None;
    }
    let idx = snap_index_tenant()?;
    let doc = st.store.get(&idx, Kind::Snapshot, synth).ok().flatten()?;
    let owner = TenantId::new(doc.get("tenant")?.as_str()?).ok()?;
    Some((owner, doc.get("snapshot")?.as_str()?.to_owned()))
}

/// 5.5.15: "If an implementation determines that it is low on resources,
/// it may delete one or more snapshots", considering snapshotPriority
/// (lowest first; earliest expiresAt breaks ties). The resource signal is
/// the per-tenant registry cap (AppState.snapshot_cap); the just-created
/// snapshot is never the victim. Evicted snapshots with an endpoint are
/// notified with expiresAt set before notifiedAt — the 5.3.4 deletion
/// encoding.
fn evict_over_cap(st: &AppState, tenant: &TenantId, keep: &str) {
    let victims: Vec<Value> = {
        let mut metas: Vec<Value> = st
            .store
            .list(tenant, Kind::Snapshot)
            .unwrap_or_default()
            .into_iter()
            .filter(is_snapshot)
            .collect();
        if metas.len() <= st.snapshot_cap {
            return;
        }
        let over = metas.len() - st.snapshot_cap;
        metas.sort_by_key(|v| {
            (
                v.get("snapshotPriority")
                    .and_then(Value::as_i64)
                    .unwrap_or(5),
                v.get("expiresAt")
                    .and_then(Value::as_str)
                    .unwrap_or("")
                    .to_owned(),
            )
        });
        metas
            .into_iter()
            .filter(|v| v.get("id").and_then(Value::as_str) != Some(keep))
            .take(over)
            .collect()
    };
    for mut meta in victims {
        if let Some(id) = meta.get("id").and_then(Value::as_str).map(str::to_owned) {
            snap_remove(st, tenant, &id, &meta);
        }
        if let Some(o) = meta.as_object_mut() {
            // deletion signal: expiresAt strictly before the notification
            o.insert("expiresAt".into(), Value::String(now_iso()));
        }
        let st2 = st.clone();
        crate::spawn(async move {
            send_notification(&st2, &meta).await;
        });
    }
}

// ---------- 5.16.1 Create Snapshot (POST /snapshots, 6.36.3.1) ----------

pub async fn create_snapshot(
    State(st): State<AppState>,
    CleanParams(params): CleanParams,
    headers: HeaderMap,
    body: Bytes,
) -> Response {
    let go = async {
        let tenant = tenant_from(&headers)?;
        check_params(&params, &["local"])?;
        // 6.3.5: the @context rules apply to the snapshot body like to any
        // other POST payload (media type, Link header, body @context)
        let v = parse_body(&st.loader, &headers, &body, BodyKind::Standard)
            .await?
            .value;
        let o = validate(&v, Mode::Create)?;
        let (id, meta) = new_meta(o)?;
        snap_insert(&st, &tenant, &id, &meta)?;
        evict_over_cap(&st, &tenant, &id);
        let (st2, t2, id2) = (st.clone(), tenant.clone(), id.clone());
        crate::spawn(async move {
            fill_snapshot(&st2, &t2, &id2).await;
        });
        Ok::<_, ApiError>(created(format!("/ngsi-ld/v1/snapshots/{id}"), &tenant))
    };
    go.await.unwrap_or_else(|e| e.into_response())
}

/// 5.16.1.4 background fill: execute every (temporal) query, store the
/// results under the synthetic tenant, derive the status, notify.
async fn fill_snapshot(st: &AppState, tenant: &TenantId, id: &str) {
    let Some(meta) = snap_get(st, tenant, id) else {
        return;
    };
    let Some(synth) = synth_tenant(&meta) else {
        return;
    };
    let ctx = st.loader.core();
    let (mut n_fail, mut n_res, mut n_empty, mut copied) = (0usize, 0usize, 0usize, 0usize);
    let mut detail = |r: Result<usize, NgsiError>| -> Value {
        match r {
            Ok(0) => {
                n_empty += 1;
                json!({"resultStatus": "empty"})
            }
            Ok(_) => {
                n_res += 1;
                json!({"resultStatus": "success"})
            }
            Err(e) => {
                n_fail += 1;
                json!({"resultStatus": "failure",
                       "problemDetails": crate::negotiate::problem_value(&e)})
            }
        }
    };
    let cap = fill_cap(st);
    let mut q_details = Vec::new();
    for q in meta
        .get("snapshotQueries")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
    {
        if fill_cancelled(st, tenant, id, &synth) {
            return;
        }
        let r = run_query(st, tenant, &synth, q, &ctx, cap - copied).await;
        if let Ok(n) = &r {
            copied += n;
        }
        q_details.push(detail(r));
    }
    let mut tq_details = Vec::new();
    for q in meta
        .get("snapshotTemporalQueries")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
    {
        if fill_cancelled(st, tenant, id, &synth) {
            return;
        }
        let r = run_temporal_query(st, tenant, &synth, id, q, cap - copied).await;
        if let Ok(n) = &r {
            copied += n;
        }
        tq_details.push(detail(r));
    }
    if fill_cancelled(st, tenant, id, &synth) {
        return;
    }
    if copied == 0 {
        materialize_tenant(st, &synth);
    }
    let status = if n_fail == 0 && n_empty == 0 && n_res > 0 {
        "success"
    } else if n_res > 0 {
        "partial"
    } else if n_empty > 0 {
        "empty"
    } else {
        "failure"
    };
    finish(st, tenant, id, status, Some((q_details, tq_details))).await;
}

/// One 5.2.23 Query executed per the DISTRIBUTED query behaviour
/// (5.16.1.4 -> 5.7.2.4): local content plus every matching Context
/// Source; results are copied into the snapshot's synthetic tenant. The
/// query runs unpaged ("all pages are to be retrieved completely") within
/// `budget` documents — beyond it nothing is copied (5.5.6).
async fn run_query(
    st: &AppState,
    tenant: &TenantId,
    synth: &TenantId,
    q: &Value,
    ctx: &antares_jsonld::Context,
    budget: usize,
) -> Result<usize, NgsiError> {
    let qo = q
        .as_object()
        .ok_or_else(|| bad("Query must be an object".into()))?;
    let mut vp: HashMap<String, String> = HashMap::new();
    crate::batch::query_doc_params(qo, false, &mut vp)?;
    for k in ["limit", "offset", "count"] {
        vp.remove(k);
    }
    let headers = HeaderMap::new();
    let mut warnings = Vec::new();
    let fed = if crate::federation::active(&vp) {
        crate::federation::fed_query(st, tenant, &headers, ctx, &vp, &mut warnings).await
    } else {
        Vec::new()
    };
    let docs =
        crate::entities::filter_entities_fed(st, tenant, &vp, ctx, fed).map_err(|e| match e {
            ApiError::Ngsi(n) => n,
            other => opaque("query execution", &other),
        })?;
    let n = docs.len();
    if n > budget {
        return Err(too_many(n, budget));
    }
    for doc in docs {
        if let Some(id) = doc.get("id").and_then(Value::as_str) {
            let _ = st.store.create(synth, Kind::Entity, id, doc.clone());
        }
    }
    Ok(n)
}

/// One temporal Query (temporalQ mandatory) — ids via the 5.7.4.4 path,
/// full evolutions copied via the store, every page retrieved but never
/// more than `budget` evolutions (5.5.6).
async fn run_temporal_query(
    st: &AppState,
    tenant: &TenantId,
    synth: &TenantId,
    id: &str,
    q: &Value,
    budget: usize,
) -> Result<usize, NgsiError> {
    let qo = q
        .as_object()
        .ok_or_else(|| bad("Query must be an object".into()))?;
    let mut vp: HashMap<String, String> = HashMap::new();
    crate::batch::query_doc_params(qo, true, &mut vp)?;
    for k in ["limit", "offset", "count"] {
        vp.remove(k);
    }
    let mut headers = HeaderMap::new();
    if let Ok(v) = tenant.as_str().parse() {
        headers.insert("NGSILD-Tenant", v);
    }
    // 5.16.1.4: "If the size of the respective results require pagination,
    // all pages are to be retrieved completely."
    let mut n = 0usize;
    let mut offset = 0usize;
    loop {
        // a snapshot deleted (or evicted) mid-fill stops the paging; what is
        // already copied is reaped by the caller
        if snap_get(st, tenant, id).is_none() {
            break;
        }
        vp.insert("limit".into(), st.max_limit.to_string());
        vp.insert("offset".into(), offset.to_string());
        let resp = crate::temporal::query_temporal_inner(st, &vp, &headers)
            .await
            .map_err(|e| match e {
                ApiError::Ngsi(n) => n,
                other => opaque("temporal query execution", &other),
            })?;
        let bytes = axum::body::to_bytes(resp.into_body(), usize::MAX)
            .await
            .map_err(|e| opaque("temporal result read", &e))?;
        let arr: Vec<Value> = serde_json::from_slice::<Value>(&bytes)
            .ok()
            .and_then(|v| v.as_array().cloned())
            .unwrap_or_default();
        let got = arr.len();
        if n + got > budget {
            return Err(too_many(n + got, budget));
        }
        for d in arr {
            let Some(eid) = d.get("id").and_then(Value::as_str) else {
                continue;
            };
            if let Ok(Some(doc)) = st.temporal.get_temporal(
                tenant,
                eid,
                &antares_store::filter::TemporalFilter::default(),
            ) {
                let _ = st.temporal.create(synth, eid, doc);
                n += 1;
            }
        }
        if got < st.max_limit {
            break;
        }
        offset += st.max_limit;
    }
    Ok(n)
}

/// An empty snapshot still needs its synthetic tenant to exist, or scoped
/// operations would answer NonexistentTenant instead of empty results.
fn materialize_tenant(st: &AppState, synth: &TenantId) {
    let marker = "urn:antares:snapshot:marker";
    let _ = st.store.create(
        synth,
        Kind::Entity,
        marker,
        json!({"id": marker, "type": ["urn:antares:Marker"]}),
    );
    let _ = st.store.delete(synth, Kind::Entity, marker);
}

async fn finish(
    st: &AppState,
    tenant: &TenantId,
    id: &str,
    status: &str,
    details: Option<(Vec<Value>, Vec<Value>)>,
) {
    let Some(mut meta) = snap_get(st, tenant, id) else {
        return;
    };
    if let Some(o) = meta.as_object_mut() {
        o.insert("snapshotStatus".into(), Value::String(status.into()));
        o.insert("modifiedAt".into(), Value::String(now_iso()));
        if let Some((q, tq)) = details {
            if !q.is_empty() {
                o.insert("snapshotQueriesDetails".into(), Value::Array(q));
            }
            if !tq.is_empty() {
                o.insert("snapshotTemporalQueriesDetails".into(), Value::Array(tq));
            }
        }
    }
    snap_put(st, tenant, meta.clone());
    send_notification(st, &meta).await;
}

/// 5.16.6 / 5.3.4 SnapshotNotification (sent only when endpoint is set).
async fn send_notification(st: &AppState, meta: &Value) {
    let Some(uri) = meta.get("endpoint").and_then(Value::as_str) else {
        return;
    };
    if st.egress.check_url(uri).await.is_err() {
        return;
    }
    let mut body = json!({
        "id": format!("urn:ngsi-ld:SnapshotNotification:{}", uuid::Uuid::new_v4()),
        "type": "SnapshotNotification",
        "notifiedAt": now_iso(),
        "snapshotId": meta.get("id").cloned().unwrap_or_default(),
        "snapshotStatus": meta.get("snapshotStatus").cloned().unwrap_or_default(),
        "snapshotPriority": meta.get("snapshotPriority").cloned().unwrap_or_default(),
        "expiresAt": meta.get("expiresAt").cloned().unwrap_or_default(),
    });
    // 5.3.4 Table 5.3.4-1 names the temporal list
    // "temporalSnapshotQueriesDetails" (unlike 5.2.41's
    // "snapshotTemporalQueriesDetails") — the notification datatype's own
    // table governs the notification payload.
    for (from, to) in [
        ("snapshotQueriesDetails", "snapshotQueriesDetails"),
        (
            "snapshotTemporalQueriesDetails",
            "temporalSnapshotQueriesDetails",
        ),
    ] {
        if let Some(v) = meta.get(from) {
            body[to] = v.clone();
        }
    }
    let mut req = st.http.post(uri).header("Content-Type", "application/json");
    if let Some(ri) = meta.get("receiverInfo").and_then(Value::as_array) {
        for kv in ri {
            if let (Some(k), Some(v)) = (
                kv.get("key").and_then(Value::as_str),
                kv.get("value").and_then(Value::as_str),
            ) {
                req = req.header(k, v);
            }
        }
    }
    let req = req.body(serde_json::to_vec(&body).unwrap_or_default());
    let _ = antares_jsonld::io_deadline(req.send(), 8_000).await;
}

// ---------- 5.16.7 Purge Snapshots (DELETE /snapshots, 6.36.3.2) ----------

pub async fn purge_snapshots(
    State(st): State<AppState>,
    CleanParams(params): CleanParams,
    headers: HeaderMap,
) -> Response {
    let go = async {
        let tenant = tenant_from(&headers)?;
        check_params(&params, &["q", "local"])?;
        // 5.16.7.4: the query is mandatory and restricted to Snapshot members
        let q = params
            .get("q")
            .ok_or_else(|| bad("purge requires a q over Snapshot members (5.16.7.4)".into()))?;
        let ast = antares_ql::parse_q(q)?;
        // 5.16.7.4: the query is "restricted to members of the Snapshot
        // data type" (Tables 5.2.41-1/-2)
        const MEMBERS: [&str; 15] = [
            "id",
            "type",
            "snapshotQueries",
            "snapshotTemporalQueries",
            "snapshotLifetime",
            "snapshotPriority",
            "endpoint",
            "receiverInfo",
            "snapshotStatus",
            "snapshotQueriesDetails",
            "snapshotTemporalQueriesDetails",
            "createdAt",
            "modifiedAt",
            "expiresAt",
            "lastUsedAt",
        ];
        if let Some(alien) = ast
            .attribute_paths()
            .into_iter()
            .find(|a| !MEMBERS.contains(a))
        {
            return Err(bad(format!(
                "purge q is restricted to Snapshot members (5.16.7.4): {alien:?}"
            ))
            .into());
        }
        let ctx = st.loader.core();
        let victims: Vec<Value> = st
            .store
            .list(&tenant, Kind::Snapshot)
            .unwrap_or_default()
            .into_iter()
            .filter(|meta| is_snapshot(meta) && crate::csource::csf_matches(&ast, meta, &ctx))
            .collect();
        for meta in victims {
            if let Some(id) = meta.get("id").and_then(Value::as_str) {
                snap_remove(&st, &tenant, id, &meta);
            }
        }
        Ok::<_, ApiError>(no_content(&tenant))
    };
    go.await.unwrap_or_else(|e| e.into_response())
}

// ---------- 5.16.3/5.16.4/5.16.5: /snapshots/{id} (6.37) ----------

pub async fn retrieve_snapshot(
    State(st): State<AppState>,
    Path(id): Path<String>,
    CleanParams(params): CleanParams,
    headers: HeaderMap,
) -> Response {
    let go = async {
        let tenant = tenant_from(&headers)?;
        check_params(&params, &["local"])?;
        let accept = parse_accept(&headers)?;
        antares_model::EntityId::new(&id)
            .map_err(|_| bad(format!("snapshot id is not a valid URI: {id:?}")))?;
        let meta = snap_get(&st, &tenant, &id)
            .ok_or_else(|| NgsiError::ResourceNotFound(format!("snapshot {id} not found")))?;
        let ctx = st.loader.core();
        Ok::<_, ApiError>(respond(
            StatusCode::OK,
            present(&meta),
            &ctx,
            accept,
            &tenant,
        ))
    };
    go.await.unwrap_or_else(|e| e.into_response())
}

pub async fn update_snapshot(
    State(st): State<AppState>,
    Path(id): Path<String>,
    CleanParams(params): CleanParams,
    headers: HeaderMap,
    body: Bytes,
) -> Response {
    let go = async {
        let tenant = tenant_from(&headers)?;
        check_params(&params, &["local"])?;
        let accept = parse_accept(&headers)?;
        antares_model::EntityId::new(&id)
            .map_err(|_| bad(format!("snapshot id is not a valid URI: {id:?}")))?;
        // 5.16.4.4 binds this operation to 5.5.4, hence to the 6.3.5 rules
        let v = parse_body(&st.loader, &headers, &body, BodyKind::Standard)
            .await?
            .value;
        let frag = validate(&v, Mode::Update)?;
        let mut meta = snap_get(&st, &tenant, &id)
            .ok_or_else(|| NgsiError::ResourceNotFound(format!("snapshot {id} not found")))?;
        if let Some(o) = meta.as_object_mut() {
            // 5.16.4.4 / 5.5.8 merge of the updatable members
            for k in [
                "snapshotLifetime",
                "snapshotPriority",
                "endpoint",
                "receiverInfo",
            ] {
                match frag.get(k) {
                    None => {}
                    Some(Value::Null) => {
                        o.remove(k);
                    }
                    Some(v) => {
                        o.insert(k.into(), v.clone());
                    }
                }
            }
            if frag.contains_key("snapshotLifetime") {
                // "it is possible to indirectly update expiresAt" (5.2.41)
                o.insert("expiresAt".into(), Value::String(expires_at(o)?));
            }
            o.insert("modifiedAt".into(), Value::String(now_iso()));
        }
        snap_put(&st, &tenant, meta.clone());
        // 5.16.6: notifications are also sent after any status update
        let (st2, meta2) = (st.clone(), meta.clone());
        crate::spawn(async move {
            send_notification(&st2, &meta2).await;
        });
        let ctx = st.loader.core();
        Ok::<_, ApiError>(respond(
            StatusCode::OK,
            present(&meta),
            &ctx,
            accept,
            &tenant,
        ))
    };
    go.await.unwrap_or_else(|e| e.into_response())
}

pub async fn delete_snapshot(
    State(st): State<AppState>,
    Path(id): Path<String>,
    CleanParams(params): CleanParams,
    headers: HeaderMap,
) -> Response {
    let go = async {
        let tenant = tenant_from(&headers)?;
        check_params(&params, &["local"])?;
        antares_model::EntityId::new(&id)
            .map_err(|_| bad(format!("snapshot id is not a valid URI: {id:?}")))?;
        let meta = snap_get(&st, &tenant, &id)
            .ok_or_else(|| NgsiError::ResourceNotFound(format!("snapshot {id} not found")))?;
        snap_remove(&st, &tenant, &id, &meta);
        Ok::<_, ApiError>(no_content(&tenant))
    };
    go.await.unwrap_or_else(|e| e.into_response())
}

// ---------- 5.16.2 Clone Snapshot (POST /snapshots/{id}/clone, 6.38) ----------

pub async fn clone_snapshot(
    State(st): State<AppState>,
    Path(id): Path<String>,
    CleanParams(params): CleanParams,
    headers: HeaderMap,
    body: Bytes,
) -> Response {
    let go = async {
        let tenant = tenant_from(&headers)?;
        check_params(&params, &["local"])?;
        antares_model::EntityId::new(&id)
            .map_err(|_| bad(format!("snapshot id is not a valid URI: {id:?}")))?;
        let src = snap_get(&st, &tenant, &id)
            .ok_or_else(|| NgsiError::ResourceNotFound(format!("snapshot {id} not found")))?;
        // 5.16.2.3 makes the clone body optional; a body that IS sent obeys
        // the 6.3.5 @context rules
        let v: Value = if body.is_empty() {
            json!({})
        } else {
            parse_body(&st.loader, &headers, &body, BodyKind::Standard)
                .await?
                .value
        };
        let mut o = validate(&v, Mode::Clone)?;
        // the clone carries the source's (read-only) query lineage
        for k in ["snapshotQueries", "snapshotTemporalQueries"] {
            if let Some(qv) = src.get(k) {
                o.insert(k.into(), qv.clone());
            }
        }
        let (new_id, meta) = new_meta(o)?;
        snap_insert(&st, &tenant, &new_id, &meta)?;
        // a clone is a new snapshot: 5.5.15 resource pressure applies to it
        // exactly as it does to a create, so cloning cannot grow the
        // registry past the cap
        evict_over_cap(&st, &tenant, &new_id);
        let (st2, t2, sid, nid) = (st.clone(), tenant.clone(), id.clone(), new_id.clone());
        crate::spawn(async move {
            clone_fill(&st2, &t2, &sid, &nid).await;
        });
        Ok::<_, ApiError>(created(format!("/ngsi-ld/v1/snapshots/{new_id}"), &tenant))
    };
    go.await.unwrap_or_else(|e| e.into_response())
}

/// 5.16.2.4 background copy: all Entity and Temporal data of the source.
async fn clone_fill(st: &AppState, tenant: &TenantId, src_id: &str, new_id: &str) {
    let (Some(src), Some(new)) = (snap_get(st, tenant, src_id), snap_get(st, tenant, new_id))
    else {
        finish(st, tenant, new_id, "failure", None).await;
        return;
    };
    let (Some(from), Some(to)) = (synth_tenant(&src), synth_tenant(&new)) else {
        finish(st, tenant, new_id, "failure", None).await;
        return;
    };
    let mut failed = false;
    let mut copied = 0usize;
    match st.store.list(&from, Kind::Entity) {
        Ok(docs) => {
            for doc in docs {
                if let Some(id) = doc.get("id").and_then(Value::as_str) {
                    if st.store.create(&to, Kind::Entity, id, doc.clone()).is_err() {
                        failed = true;
                    } else {
                        copied += 1;
                    }
                }
            }
        }
        Err(_) => failed = true,
    }
    match st.temporal.list(&from) {
        Ok(docs) => {
            for doc in docs {
                if let Some(id) = doc.get("id").and_then(Value::as_str) {
                    if st.temporal.create(&to, id, doc.clone()).is_err() {
                        failed = true;
                    } else {
                        copied += 1;
                    }
                }
            }
        }
        Err(_) => failed = true,
    }
    if fill_cancelled(st, tenant, new_id, &to) {
        return;
    }
    if copied == 0 {
        materialize_tenant(st, &to);
    }
    finish(
        st,
        tenant,
        new_id,
        if failed { "failure" } else { "success" },
        None,
    )
    .await;
}

// ---------- 6.3.22: NGSILD-Snapshot scoping middleware ----------

/// 6.3.22 / 5.5.15: resolve the NGSILD-Snapshot header to the snapshot's
/// synthetic tenant, so every Core/Temporal handler serves the frozen copy;
/// the header is echoed on the response (and the synthetic tenant never
/// leaks into NGSILD-Tenant).
pub async fn snapshot_layer(
    State(st): State<AppState>,
    req: axum::extract::Request,
    next: axum::middleware::Next,
) -> Response {
    let Some(sid) = req
        .headers()
        .get("NGSILD-Snapshot")
        .and_then(|v| v.to_str().ok())
        .map(str::to_owned)
    else {
        return next.run(req).await;
    };
    // 6.3.22: "If the HTTP header NGSILD-Snapshot is present in the HTTP
    // request, it shall also be present in HTTP response" — every exit,
    // the unscoped and the error ones included, leaves through here
    let mut resp = scoped(&st, &sid, req, next).await;
    if let Ok(v) = sid.parse() {
        resp.headers_mut().insert("NGSILD-Snapshot", v);
    }
    resp
}

async fn scoped(
    st: &AppState,
    sid: &str,
    mut req: axum::extract::Request,
    next: axum::middleware::Next,
) -> Response {
    // the Snapshot API's own resources (6.36-6.38) are never
    // snapshot-scoped — that is the /snapshots RESOURCE, not any path
    // merely containing the word (an Attribute may be named "snapshots")
    if req
        .uri()
        .path()
        .trim_start_matches(API_ROOT)
        .starts_with("/snapshots")
    {
        return next.run(req).await;
    }
    let tenant = match tenant_from(req.headers()) {
        Ok(t) => t,
        Err(e) => return e.into_response(),
    };
    let Some(meta) = snap_get(st, &tenant, sid) else {
        return ApiError::from(NgsiError::ResourceNotFound(format!(
            "snapshot {sid} not found"
        )))
        .into_response();
    };
    let Some(synth) = synth_tenant(&meta) else {
        return ApiError::from(NgsiError::InternalError("snapshot without tenant".into()))
            .into_response();
    };
    snap_touch(st, &tenant, sid);
    if let Ok(v) = synth.as_str().parse() {
        req.headers_mut().insert("NGSILD-Tenant", v);
    }
    let mut resp = next.run(req).await;
    // restore the caller's tenant view (the synthetic one is internal)
    match tenant.as_str() {
        "default" => {
            resp.headers_mut().remove("NGSILD-Tenant");
        }
        t => {
            if let Ok(v) = t.parse() {
                resp.headers_mut().insert("NGSILD-Tenant", v);
            }
        }
    }
    resp
}

/// 5.5.6: the fill copies its results into a second (isolated) tenant, so an
/// unbounded result set doubles the stored data set in one request. The
/// implementation threshold — "up to each implementation" — is a hundred
/// full pages per snapshot; beyond it the query reports TooManyResults.
fn fill_cap(st: &AppState) -> usize {
    st.max_limit.saturating_mul(100)
}

/// 5.16.1.4 + 5.5.15: the copied data is reachable only through its snapshot
/// document, so a fill whose snapshot was deleted (or evicted under resource
/// pressure) must stop and free what it already copied.
fn fill_cancelled(st: &AppState, tenant: &TenantId, id: &str, synth: &TenantId) -> bool {
    if snap_get(st, tenant, id).is_some() {
        return false;
    }
    purge_synth(st, synth);
    true
}

/// 5.5.6: "When a query operation is producing so many results that can
/// potentially exhaust client or server resources … implementations shall
/// raise an error of type TooManyResults" — reported per query in the
/// 5.2.42 ExecutionResultDetails of the snapshot.
fn too_many(got: usize, budget: usize) -> NgsiError {
    NgsiError::TooManyResults(format!(
        "snapshot query yields {got} results, {budget} left of the snapshot ceiling"
    ))
}

#[cfg(test)]
mod clause_5_5_6 {
    use super::*;

    /// 5.5.6 InternalError: the client-visible `detail` — served in
    /// snapshotQueriesDetails.problemDetails and copied into the 5.3.4
    /// SnapshotNotification body — must be generic. The Debug/IO text of
    /// the underlying failure belongs in the server log only.
    #[test]
    fn snapshot_fill_error_detail_is_generic() {
        let inner = ApiError::Bare(StatusCode::UNSUPPORTED_MEDIA_TYPE);
        let pd = crate::negotiate::problem_value(&opaque("query execution", &inner));
        let detail = pd["detail"].as_str().unwrap_or_default();
        assert_eq!(detail, "query execution failed");
        assert!(
            !detail.contains("Bare") && !detail.contains("Unsupported"),
            "internal error text leaked into the client-visible detail: {detail}"
        );
    }
}

#[cfg(test)]
#[allow(clippy::expect_used)]
mod clause_5_16 {
    use super::*;
    use axum::body::Body;
    use axum::http::Request;
    use tower::ServiceExt;

    fn state() -> AppState {
        let mut st = AppState::new("antares-snapshots".into());
        crate::notify::wire(&mut st);
        st
    }

    /// One request through the full router — the 6.3.22 middleware included.
    async fn send(
        st: &AppState,
        method: &str,
        path: &str,
        body: Option<(&str, String)>,
        extra: &[(&str, &str)],
    ) -> (StatusCode, HeaderMap, Value) {
        let mut b = Request::builder().method(method).uri(path);
        for (k, v) in extra {
            b = b.header(*k, *v);
        }
        let req = match body {
            Some((ct, payload)) => b
                .header("Content-Type", ct)
                .header("Content-Length", payload.len())
                .body(Body::from(payload)),
            None => b.body(Body::empty()),
        }
        .expect("request");
        let res = crate::router(st.clone())
            .oneshot(req)
            .await
            .expect("response");
        let status = res.status();
        let headers = res.headers().clone();
        let bytes = axum::body::to_bytes(res.into_body(), usize::MAX)
            .await
            .expect("body");
        let body = if bytes.is_empty() {
            Value::Null
        } else {
            serde_json::from_slice(&bytes).unwrap_or(Value::Null)
        };
        (status, headers, body)
    }

    async fn post_json(st: &AppState, path: &str, doc: &Value) -> (StatusCode, HeaderMap, Value) {
        send(
            st,
            "POST",
            path,
            Some(("application/json", doc.to_string())),
            &[],
        )
        .await
    }

    fn header<'a>(h: &'a HeaderMap, name: &str) -> Option<&'a str> {
        h.get(name).and_then(|v| v.to_str().ok())
    }

    /// Create a snapshot over every Vehicle and wait for the background fill.
    async fn snapshot_over_vehicles(st: &AppState) -> String {
        let doc = json!({"type": "Snapshot",
            "snapshotQueries": [{"type": "Query", "entities": [{"type": "Vehicle"}]}]});
        let (status, headers, body) = post_json(st, "/ngsi-ld/v1/snapshots", &doc).await;
        assert_eq!(status, StatusCode::CREATED, "{body}");
        let loc = header(&headers, "Location").expect("Location").to_owned();
        for _ in 0..200 {
            let (_, _, b) = send(st, "GET", &loc, None, &[]).await;
            if b["snapshotStatus"] != "preparing" {
                return b["id"].as_str().expect("snapshot id").to_owned();
            }
            tokio::time::sleep(std::time::Duration::from_millis(20)).await;
        }
        panic!("snapshot never left preparing");
    }

    fn stored(st: &AppState, id: &str) -> Value {
        st.store
            .get(&TenantId::default(), Kind::Snapshot, id)
            .ok()
            .flatten()
            .expect("snapshot document")
    }

    /// 6.3.22: the Snapshot API's own resources are never snapshot-scoped —
    /// but that exemption is the /snapshots RESOURCE, not any path in which
    /// the word occurs. An Entity attribute named "snapshots" stays scoped
    /// to the frozen copy, so the live entity is untouched.
    #[tokio::test(flavor = "multi_thread")]
    async fn clause_6_3_22_scope_guard_matches_only_the_snapshot_resource() {
        let st = state();
        let ent = json!({"id": "urn:ngsi-ld:Vehicle:g1", "type": "Vehicle",
            "snapshots": {"type": "Property", "value": 3}});
        let (status, _, b) = post_json(&st, "/ngsi-ld/v1/entities", &ent).await;
        assert_eq!(status, StatusCode::CREATED, "{b}");
        let sid = snapshot_over_vehicles(&st).await;

        let (status, headers, b) = send(
            &st,
            "DELETE",
            "/ngsi-ld/v1/entities/urn:ngsi-ld:Vehicle:g1/attrs/snapshots",
            None,
            &[("NGSILD-Snapshot", sid.as_str())],
        )
        .await;
        assert_eq!(status, StatusCode::NO_CONTENT, "{b}");
        assert_eq!(header(&headers, "NGSILD-Snapshot"), Some(sid.as_str()));

        let (status, _, live) = send(
            &st,
            "GET",
            "/ngsi-ld/v1/entities/urn:ngsi-ld:Vehicle:g1",
            None,
            &[],
        )
        .await;
        assert_eq!(status, StatusCode::OK, "{live}");
        assert!(
            live.get("snapshots").is_some(),
            "a snapshot-scoped request deleted the LIVE attribute: {live}"
        );
        let (_, _, frozen) = send(
            &st,
            "GET",
            "/ngsi-ld/v1/entities/urn:ngsi-ld:Vehicle:g1",
            None,
            &[("NGSILD-Snapshot", sid.as_str())],
        )
        .await;
        assert!(
            frozen.get("snapshots").is_none(),
            "the frozen copy still carries the deleted attribute: {frozen}"
        );
    }

    /// 6.3.22: "If the HTTP header NGSILD-Snapshot is present in the HTTP
    /// request, it shall also be present in HTTP response" — on the
    /// unscoped Snapshot resources and on the error exits as well.
    #[tokio::test(flavor = "multi_thread")]
    async fn clause_6_3_22_header_is_echoed_on_every_exit() {
        let st = state();
        let sid = snapshot_over_vehicles(&st).await;
        let (status, headers, b) = send(
            &st,
            "GET",
            &format!("/ngsi-ld/v1/snapshots/{sid}"),
            None,
            &[("NGSILD-Snapshot", sid.as_str())],
        )
        .await;
        assert_eq!(status, StatusCode::OK, "{b}");
        assert_eq!(header(&headers, "NGSILD-Snapshot"), Some(sid.as_str()));

        let unknown = "urn:ngsi-ld:Snapshot:nope";
        let (status, headers, b) = send(
            &st,
            "GET",
            "/ngsi-ld/v1/entities?type=Vehicle",
            None,
            &[("NGSILD-Snapshot", unknown)],
        )
        .await;
        assert_eq!(status, StatusCode::NOT_FOUND, "{b}");
        assert_eq!(header(&headers, "NGSILD-Snapshot"), Some(unknown));
        assert!(
            !b.to_string().contains("snap-"),
            "the synthetic tenant leaked to the client: {b}"
        );
    }

    /// 5.16.1.4: "If the NGSI-LD endpoint already knows about this Snapshot,
    /// as there is an existing Snapshot whose id (URI) is equivalent, an
    /// error of type AlreadyExists shall be raised" — and the rejected
    /// create must not have replaced the existing snapshot.
    #[tokio::test(flavor = "multi_thread")]
    async fn clause_5_16_1_4_duplicate_id_is_already_exists() {
        let st = state();
        let mut doc = json!({"id": "urn:ngsi-ld:Snapshot:dup", "type": "Snapshot",
            "snapshotPriority": 3,
            "snapshotQueries": [{"type": "Query", "entities": [{"type": "Vehicle"}]}]});
        let (status, _, b) = post_json(&st, "/ngsi-ld/v1/snapshots", &doc).await;
        assert_eq!(status, StatusCode::CREATED, "{b}");
        doc["snapshotPriority"] = json!(9);
        let (status, _, b) = post_json(&st, "/ngsi-ld/v1/snapshots", &doc).await;
        assert_eq!(status, StatusCode::CONFLICT, "{b}");
        assert!(
            b["type"]
                .as_str()
                .unwrap_or_default()
                .ends_with("AlreadyExists"),
            "{b}"
        );
        let (_, _, got) = send(
            &st,
            "GET",
            "/ngsi-ld/v1/snapshots/urn:ngsi-ld:Snapshot:dup",
            None,
            &[],
        )
        .await;
        assert_eq!(
            got["snapshotPriority"], 3,
            "the rejected create overwrote the existing snapshot: {got}"
        );
        assert!(
            got.get("__tenant").is_none(),
            "internal member served to the client: {got}"
        );
    }

    /// 6.3.5 (5.16.4.4 → 5.5.4): snapshot bodies obey the @context rules —
    /// application/json with a body @context is BadRequestData,
    /// application/ld+json without one is BadRequestData, any other media
    /// type is a bare 415, and @context is never stored in the snapshot.
    #[tokio::test(flavor = "multi_thread")]
    async fn clause_6_3_5_snapshot_bodies_follow_the_context_rules() {
        let st = state();
        let core = "https://uri.etsi.org/ngsi-ld/v1/ngsi-ld-core-context-v1.9.jsonld";
        let plain = json!({"type": "Snapshot",
            "snapshotQueries": [{"type": "Query", "entities": [{"type": "Vehicle"}]}]});
        let mut with_ctx = plain.clone();
        with_ctx["@context"] = json!(core);

        for (ct, doc) in [
            ("application/json", &with_ctx),
            ("application/ld+json", &plain),
        ] {
            let (status, _, b) = send(
                &st,
                "POST",
                "/ngsi-ld/v1/snapshots",
                Some((ct, doc.to_string())),
                &[],
            )
            .await;
            assert_eq!(status, StatusCode::BAD_REQUEST, "{ct}: {b}");
        }
        let (status, _, _) = send(
            &st,
            "POST",
            "/ngsi-ld/v1/snapshots",
            Some(("text/plain", plain.to_string())),
            &[],
        )
        .await;
        assert_eq!(status, StatusCode::UNSUPPORTED_MEDIA_TYPE);

        let (status, headers, b) = send(
            &st,
            "POST",
            "/ngsi-ld/v1/snapshots",
            Some(("application/ld+json", with_ctx.to_string())),
            &[],
        )
        .await;
        assert_eq!(status, StatusCode::CREATED, "{b}");
        let loc = header(&headers, "Location").expect("Location").to_owned();
        let (_, _, got) = send(&st, "GET", &loc, None, &[]).await;
        assert!(
            got.get("@context").is_none(),
            "@context stored as a snapshot member: {got}"
        );
        let patch = json!({"snapshotPriority": 7, "@context": core});
        let (status, _, b) = send(
            &st,
            "PATCH",
            &loc,
            Some(("application/json", patch.to_string())),
            &[],
        )
        .await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "{b}");
    }

    /// 5.5.6: "When a query operation is producing so many results that can
    /// potentially exhaust client or server resources … implementations
    /// shall raise an error of type TooManyResults." A fill copies its
    /// results, so the ceiling is enforced before anything is copied.
    #[tokio::test(flavor = "multi_thread")]
    async fn clause_5_5_6_fill_stops_at_the_result_ceiling() {
        let mut st = state();
        st.max_limit = 1;
        st.default_limit = 1;
        for i in 0..=fill_cap(&st) {
            let ent = json!({"id": format!("urn:ngsi-ld:Vehicle:c{i}"), "type": "Vehicle"});
            let (status, _, b) = post_json(&st, "/ngsi-ld/v1/entities", &ent).await;
            assert_eq!(status, StatusCode::CREATED, "{b}");
        }
        let sid = snapshot_over_vehicles(&st).await;
        let ready = stored(&st, &sid);
        assert_eq!(ready["snapshotStatus"], "failure", "{ready}");
        let detail = &ready["snapshotQueriesDetails"][0];
        assert_eq!(detail["resultStatus"], "failure", "{ready}");
        assert!(
            detail["problemDetails"]["type"]
                .as_str()
                .unwrap_or_default()
                .ends_with("TooManyResults"),
            "{detail}"
        );
        let (status, _, list) = send(
            &st,
            "GET",
            "/ngsi-ld/v1/entities?type=Vehicle&limit=1",
            None,
            &[("NGSILD-Snapshot", sid.as_str())],
        )
        .await;
        assert_eq!(status, StatusCode::OK, "{list}");
        assert_eq!(
            list,
            json!([]),
            "the oversized result set was copied into the snapshot anyway"
        );
    }

    /// 5.16.1.4 + 5.5.15: the filled copy is reachable only through its
    /// snapshot document, so a fill whose snapshot has been deleted (or
    /// evicted) stops and frees what it already copied.
    #[tokio::test(flavor = "multi_thread")]
    async fn clause_5_16_1_4_fill_stops_and_reaps_when_its_snapshot_is_gone() {
        let st = state();
        let tenant = TenantId::default();
        let orphan = TenantId::new("snap-reap-test").expect("tenant");
        let _ = st.store.create(
            &orphan,
            Kind::Entity,
            "urn:ngsi-ld:Vehicle:r1",
            json!({"id": "urn:ngsi-ld:Vehicle:r1"}),
        );
        assert!(
            fill_cancelled(&st, &tenant, "urn:ngsi-ld:Snapshot:gone", &orphan),
            "a fill kept running after its snapshot was deleted"
        );
        assert!(
            st.store
                .list(&orphan, Kind::Entity)
                .unwrap_or_default()
                .is_empty(),
            "the orphaned copy outlived its snapshot"
        );

        let sid = snapshot_over_vehicles(&st).await;
        let synth = synth_tenant(&stored(&st, &sid)).expect("__tenant");
        let _ = st.store.create(
            &synth,
            Kind::Entity,
            "urn:ngsi-ld:Vehicle:r2",
            json!({"id": "urn:ngsi-ld:Vehicle:r2"}),
        );
        assert!(
            !fill_cancelled(&st, &tenant, &sid, &synth),
            "a live snapshot must not cancel its own fill"
        );
        assert_eq!(
            st.store
                .list(&synth, Kind::Entity)
                .unwrap_or_default()
                .len(),
            1,
            "a live snapshot's copy was reaped"
        );
    }

    /// 5.16.5.4 + 5.5.15: deleting a snapshot frees the whole isolated copy,
    /// including the subscriptions created through the 6.3.22 header — no
    /// client can reach them afterwards, so nothing may keep firing.
    #[tokio::test(flavor = "multi_thread")]
    async fn clause_5_16_5_delete_frees_the_whole_snapshot_copy() {
        let st = state();
        let sid = snapshot_over_vehicles(&st).await;
        let synth = synth_tenant(&stored(&st, &sid)).expect("__tenant");
        let sub = json!({"type": "Subscription", "entities": [{"type": "Vehicle"}],
            "notification": {"endpoint": {"uri": "http://127.0.0.1:9/none"}}});
        let (status, _, b) = send(
            &st,
            "POST",
            "/ngsi-ld/v1/subscriptions",
            Some(("application/json", sub.to_string())),
            &[("NGSILD-Snapshot", sid.as_str())],
        )
        .await;
        assert_eq!(status, StatusCode::CREATED, "{b}");
        assert!(
            !st.store
                .list(&synth, Kind::Subscription)
                .unwrap_or_default()
                .is_empty(),
            "the scoped subscription did not land in the snapshot copy"
        );

        let (status, _, _) = send(
            &st,
            "DELETE",
            &format!("/ngsi-ld/v1/snapshots/{sid}"),
            None,
            &[],
        )
        .await;
        assert_eq!(status, StatusCode::NO_CONTENT);
        for _ in 0..100 {
            let left = st
                .store
                .list(&synth, Kind::Subscription)
                .unwrap_or_default()
                .len();
            if left == 0 {
                return;
            }
            tokio::time::sleep(std::time::Duration::from_millis(20)).await;
        }
        panic!("a snapshot-scoped subscription outlived its snapshot");
    }

    /// 5.2.41 Table 5.2.41-1: the snapshot id "cannot be later modified in
    /// update operations" — whatever JSON type the client sends it as.
    #[test]
    fn clause_5_16_4_4_id_cannot_be_modified() {
        assert!(validate(&json!({"id": "urn:ngsi-ld:Snapshot:1"}), Mode::Update).is_err());
        assert!(
            validate(&json!({"id": 5}), Mode::Update).is_err(),
            "a non-string id escaped the 5.2.41 restriction"
        );
        assert!(validate(&json!({"snapshotPriority": 7}), Mode::Update).is_ok());
    }

    /// 5.16.1.4: expiresAt is set "taking into account the snapshotLifetime
    /// requested, but applying the configured limit of the NGSI-LD system";
    /// a malformed duration is BadRequestData, and a snapshot past its
    /// expiresAt is gone together with its copy.
    #[tokio::test(flavor = "multi_thread")]
    async fn clause_5_16_1_4_lifetime_is_clamped_and_expiry_removes_the_snapshot() {
        let mut o = Map::new();
        o.insert("snapshotLifetime".into(), json!("P10Y"));
        let exp = chrono::DateTime::parse_from_rfc3339(&expires_at(&o).expect("expiresAt"))
            .expect("rfc3339")
            .with_timezone(&chrono::Utc);
        let limit = chrono::Utc::now() + chrono::Duration::seconds(MAX_LIFETIME_SECS);
        assert!(exp <= limit, "the configured limit was not applied: {exp}");
        assert!(
            exp > chrono::Utc::now() + chrono::Duration::days(6),
            "the suggested lifetime was ignored: {exp}"
        );
        assert!(validate(
            &json!({"type": "Snapshot", "snapshotLifetime": "tomorrow",
                "snapshotQueries": [{"type": "Query", "entities": [{"type": "Vehicle"}]}]}),
            Mode::Create
        )
        .is_err());

        let st = state();
        let sid = snapshot_over_vehicles(&st).await;
        let synth = synth_tenant(&stored(&st, &sid)).expect("__tenant");
        let _ = st.store.create(
            &synth,
            Kind::Entity,
            "urn:ngsi-ld:Vehicle:e1",
            json!({"id": "urn:ngsi-ld:Vehicle:e1"}),
        );
        let _ = st
            .store
            .mutate(&TenantId::default(), Kind::Snapshot, &sid, |d| {
                if let Some(o) = d.as_object_mut() {
                    o.insert("expiresAt".into(), json!("2000-01-01T00:00:00.000Z"));
                }
                Ok::<_, std::convert::Infallible>(())
            });
        let (status, _, b) = send(
            &st,
            "GET",
            &format!("/ngsi-ld/v1/snapshots/{sid}"),
            None,
            &[],
        )
        .await;
        assert_eq!(status, StatusCode::NOT_FOUND, "an expired snapshot: {b}");
        for _ in 0..100 {
            if st
                .store
                .list(&synth, Kind::Entity)
                .unwrap_or_default()
                .is_empty()
            {
                return;
            }
            tokio::time::sleep(std::time::Duration::from_millis(20)).await;
        }
        panic!("the copy of an expired snapshot was not freed");
    }
}
