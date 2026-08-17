//! /jsonldContexts management (5.13; resources 6.29/6.30).
//!
//! Three kinds (5.13.1): Hosted (client-added, served on demand),
//! Cached (externally-fetched, metadata only), ImplicitlyCreated (broker-made
//! wrappers for array @contexts on subscriptions, served on demand).

use crate::negotiate::*;
use crate::state::{now_iso, AppState};
use antares_jsonld::Loader;
use antares_model::{NgsiError, TenantId};
use axum::body::Bytes;
use axum::extract::{Path, State};
use axum::http::{header, HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use serde_json::{json, Value};
use std::collections::HashMap;

use crate::negotiate::CleanParams;

/// Base URL under which this broker publishes its own @context entries
/// (5.13.2.4 locally unique URI, 5.13.3.5 `URL`). The address is the
/// broker's, so it comes from ITS configuration — ANTARES_PUBLIC_URL, the
/// same value peers are handed as the 5.8.1.4 notification endpoint. The
/// request's `Host` header is client input and only the fallback for a
/// deployment that configures nothing.
pub(crate) fn base_url(headers: &HeaderMap) -> String {
    context_base(std::env::var("ANTARES_PUBLIC_URL").ok().as_deref(), headers)
}

fn context_base(configured: Option<&str>, headers: &HeaderMap) -> String {
    let base = match configured.map(str::trim).filter(|u| !u.is_empty()) {
        Some(url) => url.trim_end_matches('/').to_owned(),
        None => {
            let host = headers
                .get(header::HOST)
                .and_then(|h| h.to_str().ok())
                .unwrap_or("localhost:9090");
            format!("http://{host}")
        }
    };
    format!("{base}/ngsi-ld/v1/jsonldContexts")
}

/// Validate the `details` query param: absent | true | false (053_05 sends
/// `True`); anything else is 400 (052_04_02).
fn details_param(params: &HashMap<String, String>) -> Result<bool, NgsiError> {
    match params.get("details").map(|s| s.to_ascii_lowercase()) {
        None => Ok(false),
        Some(v) if v == "true" => Ok(true),
        Some(v) if v == "false" => Ok(false),
        Some(v) => Err(NgsiError::BadRequestData(format!(
            "invalid details value {v:?}"
        ))),
    }
}

fn reload_param(params: &HashMap<String, String>) -> Result<bool, NgsiError> {
    match params.get("reload").map(|s| s.to_ascii_lowercase()) {
        None => Ok(false),
        Some(v) if v == "true" => Ok(true),
        Some(v) if v == "false" => Ok(false),
        Some(v) => Err(NgsiError::BadRequestData(format!(
            "invalid reload value {v:?}"
        ))),
    }
}

/// Detailed metadata object (5.13.3.5).
fn details_obj(
    url: &str,
    local_id: &str,
    kind: &str,
    created_at: &str,
    usage: Option<&antares_jsonld::CtxUsage>,
) -> Value {
    let mut o = json!({
        "URL": url,
        "localId": local_id,
        "kind": kind,
        "createdAt": created_at,
    });
    // numberOfHits/lastUsage only for kinds where the suite's arithmetic
    // expects them (Cached + ImplicitlyCreated; 053_04 vs 053_06)
    if kind != "Hosted" {
        let hits = usage.map(|u| u.hits).unwrap_or(0);
        o["numberOfHits"] = json!(hits);
        if let Some(u) = usage {
            o["lastUsage"] = json!(u.last_usage);
        }
    }
    o
}

/// A resolved @context entry, whichever backing it has.
enum CtxEntry {
    /// Hosted or ImplicitlyCreated store entry.
    Stored(Value),
    /// External URL known through the loader (kind Cached).
    Cached(antares_jsonld::CtxUsage),
    /// Built-in core context (undeletable).
    Core(String),
}

/// Usage view of a store row — the SHARED truth (per-instance loader
/// stats split-brain behind a load balancer; the usage_bump hook keeps the
/// row's counters current from every instance).
fn row_usage(doc: &Value) -> antares_jsonld::CtxUsage {
    let created_at = doc["createdAt"].as_str().unwrap_or_default().to_owned();
    antares_jsonld::CtxUsage {
        url: doc["url"].as_str().unwrap_or_default().to_owned(),
        local_id: doc["localId"].as_str().unwrap_or_default().to_owned(),
        last_usage: doc["lastUsage"]
            .as_str()
            .map(str::to_owned)
            .unwrap_or_else(|| created_at.clone()),
        hits: doc["numberOfHits"].as_u64().unwrap_or(0),
        created_at,
    }
}

/// Build the Cached-entry view of a store row.
fn cached_from_row(doc: &Value) -> CtxEntry {
    CtxEntry::Cached(row_usage(doc))
}

/// Ownership of a stored row (5.13.1). Hosted and ImplicitlyCreated rows hold
/// term mappings authored through one tenant's requests and are visible,
/// servable and deletable only through that tenant; Cached rows are copies of
/// public documents the broker fetched and belong to no tenant. Rows written
/// before the owner member existed belong to the default tenant.
fn row_visible(doc: &Value, tenant: &TenantId) -> bool {
    doc["kind"].as_str() == Some("Cached")
        || doc["owner"].as_str().unwrap_or(TenantId::DEFAULT) == tenant.as_str()
}

/// Resolve an id to the @context it names (5.13.4.4). Every probe is a keyed
/// lookup: a store failure is an error, never "not found" — answering 404 for
/// a hiccup would tell the client to add the @context a second time.
async fn find_entry(st: &AppState, tenant: &TenantId, id: &str) -> ApiResult<Option<CtxEntry>> {
    if let Some(doc) = st.store.context_get(id)? {
        // Cached rows are addressable by their deterministic localId too.
        if doc["kind"].as_str() == Some("Cached") {
            return Ok(Some(cached_from_row(&doc)));
        }
        // another tenant's row is as absent as one that never existed
        return Ok(row_visible(&doc, tenant).then_some(CtxEntry::Stored(doc)));
    }
    // Stored entries are also addressable by their full URL (5.13.2.4): the
    // row key IS the URL's trailing segment, so this is one keyed lookup and
    // the row's own url still has to match the id.
    if let Some(pos) = id.rfind("/ngsi-ld/v1/jsonldContexts/") {
        let local_id = &id[pos + "/ngsi-ld/v1/jsonldContexts/".len()..];
        if let Some(doc) = st.store.context_get(local_id)? {
            if doc["url"].as_str() == Some(id) && row_visible(&doc, tenant) {
                return Ok(Some(CtxEntry::Stored(doc)));
            }
        }
    }
    if Loader::is_pinned_core(id) {
        return Ok(Some(CtxEntry::Core(id.to_owned())));
    }
    // Cached entries: the persisted row is the ONE existence truth (the
    // per-instance usage map split-brains behind a load balancer). The row id
    // is uuid5(url), so a URL-shaped id resolves in O(1).
    if id.starts_with("http://") || id.starts_with("https://") {
        let rid = uuid::Uuid::new_v5(&uuid::Uuid::NAMESPACE_URL, id.as_bytes()).to_string();
        if let Some(doc) = st.store.context_get(&rid)? {
            if doc["kind"].as_str() == Some("Cached") {
                return Ok(Some(cached_from_row(&doc)));
            }
        }
    }
    Ok(None)
}

// ---------- POST /jsonldContexts (5.13.2) ----------

/// 5.13.2.4 + 5.5.4: a JSON-LD local context is a string (IRI), an object of
/// term definitions, null, or an array of those — anything else is invalid
/// JSON-LD and rejected as BadRequestData.
fn valid_context_shape(v: &Value) -> bool {
    match v {
        Value::String(_) | Value::Object(_) | Value::Null => true,
        Value::Array(a) => a
            .iter()
            .all(|e| matches!(e, Value::String(_) | Value::Object(_) | Value::Null)),
        _ => false,
    }
}

/// 5.13.2.4 Add @context: store the client-supplied @context under a new
/// locally unique URI, flagged "Hosted"; the URI is returned (Location).
pub async fn add_context(
    State(st): State<AppState>,
    CleanParams(params): CleanParams,
    headers: HeaderMap,
    body: Bytes,
) -> Response {
    let go = async {
        let tenant = tenant_from(&headers)?;
        check_params(&params, &["local"])?;
        let ct = content_type(&headers);
        if !ct.is_empty() && ct != "application/ld+json" && ct != "application/json" {
            return Err(ApiError::Bare(StatusCode::UNSUPPORTED_MEDIA_TYPE));
        }
        let value: Value = serde_json::from_slice(&body)
            .map_err(|e| NgsiError::InvalidRequest(format!("body is not valid JSON: {e}")))?;
        let ctx_val = value.get("@context").cloned().ok_or_else(|| {
            // 050_02: a JSON object without @context is InvalidRequest
            NgsiError::InvalidRequest("body must carry an @context member".into())
        })?;
        if !valid_context_shape(&ctx_val) {
            return Err(NgsiError::BadRequestData("invalid JSON-LD @context value".into()).into());
        }
        let local_id = uuid::Uuid::new_v4().to_string();
        let url = format!("{}/{local_id}", base_url(&headers));
        let doc = json!({
            "url": url,
            "localId": local_id,
            "kind": "Hosted",
            "createdAt": now_iso(),
            // the adding tenant owns the entry (5.13.1 Hosted)
            "owner": tenant.as_str(),
            "body": {"@context": ctx_val.clone()},
        });
        st.store.context_put(&local_id, doc)?;
        st.loader.put_local(url.clone(), ctx_val).await;
        let mut resp = (
            StatusCode::CREATED,
            [(
                header::LOCATION,
                format!("/ngsi-ld/v1/jsonldContexts/{local_id}"),
            )],
        )
            .into_response();
        echo_tenant(&tenant, &mut resp);
        Ok::<_, ApiError>(resp)
    };
    go.await.unwrap_or_else(|e| e.into_response())
}

// ---------- GET /jsonldContexts (5.13.3) ----------

/// 5.13.3.4 List @contexts: one URL (or metadata object, 5.13.3.5) per
/// stored @context matching the kind filter; no filter → all kinds.
pub async fn list_contexts(
    State(st): State<AppState>,
    CleanParams(params): CleanParams,
    headers: HeaderMap,
) -> Response {
    let go = async {
        let tenant = tenant_from(&headers)?;
        // Table 6.29.3.2-1: details and kind are the only parameters of this
        // resource — it serves the whole list, so a pagination parameter would
        // be accepted and silently ignored.
        check_params(&params, &["kind", "details", "local"])?;
        let details = details_param(&params)?;
        let kind_filter = params.get("kind");
        if let Some(k) = kind_filter {
            if !["Hosted", "Cached", "ImplicitlyCreated"].contains(&k.as_str()) {
                return Err(NgsiError::BadRequestData(format!("invalid kind {k:?}")).into());
            }
        }
        let keep = |k: &str| kind_filter.is_none_or(|f| f == k);
        // (url, localId, kind, createdAt, usage)
        let mut entries: Vec<(
            String,
            String,
            String,
            String,
            Option<antares_jsonld::CtxUsage>,
        )> = Vec::new();
        for c in st.store.context_list()? {
            let kind = c["kind"].as_str().unwrap_or("Hosted").to_owned();
            if !keep(&kind) || !row_visible(&c, &tenant) {
                continue;
            }
            let url = c["url"].as_str().unwrap_or_default().to_owned();
            // counters from the ROW (shared truth), never this instance's map
            let usage = Some(row_usage(&c));
            entries.push((
                url,
                c["localId"].as_str().unwrap_or_default().to_owned(),
                kind,
                c["createdAt"].as_str().unwrap_or_default().to_owned(),
                usage,
            ));
        }
        // Cached entries come from the store rows walked above — the shared
        // truth every instance sees. Loader-only usage entries are NOT
        // listed (an entry another instance deleted must not resurface),
        // with one exception: pinned core contexts never get a row and stay
        // listable from local usage.
        if keep("Cached") {
            for u in st.loader.usage_list().await {
                if !Loader::is_pinned_core(&u.url) {
                    continue;
                }
                entries.push((
                    u.url.clone(),
                    u.local_id.clone(),
                    "Cached".into(),
                    u.created_at.clone(),
                    Some(u),
                ));
            }
        }
        let payload = if details {
            Value::Array(
                entries
                    .iter()
                    .map(|(url, lid, kind, created, usage)| {
                        details_obj(url, lid, kind, created, usage.as_ref())
                    })
                    .collect(),
            )
        } else {
            Value::Array(entries.iter().map(|(url, ..)| json!(url)).collect())
        };
        let mut resp = (
            StatusCode::OK,
            [(header::CONTENT_TYPE, "application/json")],
            axum::Json(payload),
        )
            .into_response();
        echo_tenant(&tenant, &mut resp);
        Ok::<_, ApiError>(resp)
    };
    go.await.unwrap_or_else(|e| e.into_response())
}

// ---------- GET /jsonldContexts/{ctxId} (5.13.4) ----------

/// 5.13.4.4 Serve @context: full content for Hosted/ImplicitlyCreated,
/// OperationNotSupported for Cached, ResourceNotFound for unknown ids;
/// details=true serves metadata for all kinds.
pub async fn serve_context(
    State(st): State<AppState>,
    Path(id): Path<String>,
    CleanParams(params): CleanParams,
    headers: HeaderMap,
) -> Response {
    let go = async {
        let tenant = tenant_from(&headers)?;
        check_params(&params, &["details", "local"])?;
        let details = details_param(&params)?;
        let entry = find_entry(&st, &tenant, &id)
            .await?
            .ok_or_else(|| NgsiError::ResourceNotFound(format!("@context {id} not found")))?;
        let payload = match &entry {
            CtxEntry::Stored(doc) => {
                let kind = doc["kind"].as_str().unwrap_or("Hosted");
                let url = doc["url"].as_str().unwrap_or_default();
                if details {
                    let usage = row_usage(doc);
                    details_obj(
                        url,
                        doc["localId"].as_str().unwrap_or_default(),
                        kind,
                        doc["createdAt"].as_str().unwrap_or_default(),
                        Some(&usage),
                    )
                } else {
                    doc["body"].clone()
                }
            }
            CtxEntry::Cached(u) => {
                if !details {
                    // Cached entries are never served on demand (5.13.4.4)
                    return Err(NgsiError::OperationNotSupported(
                        "Cached @contexts are not served on demand (5.13.4)".into(),
                    )
                    .into());
                }
                details_obj(&u.url, &u.local_id, "Cached", &u.created_at, Some(u))
            }
            CtxEntry::Core(url) => {
                if details {
                    let usage = st.loader.usage_get(url).await;
                    details_obj(url, url, "Cached", &st.started_at, usage.as_ref())
                } else {
                    return Err(NgsiError::OperationNotSupported(
                        "Cached @contexts are not served on demand (5.13.4)".into(),
                    )
                    .into());
                }
            }
        };
        // a serve counts as a hit for Cached/ImplicitlyCreated entries, after
        // the value shown in this response (053_06/053_08 arithmetic) —
        // EXCEPT broker-internal fetches (a fleet peer resolving this
        // @context through the LB): the resolving instance bumps the shared
        // row itself, a serve-side bump would double-count (053_08 fleet).
        let internal_fetch = headers.contains_key(antares_jsonld::INTERNAL_FETCH_HEADER);
        match &entry {
            CtxEntry::Stored(doc) if !internal_fetch && doc["kind"] == "ImplicitlyCreated" => {
                if let Some(u) = doc["url"].as_str() {
                    let _ = st.loader.bump_url(u).await;
                }
            }
            CtxEntry::Cached(u) if !internal_fetch => {
                let _ = st.loader.bump_url(&u.url).await;
            }
            _ => {}
        }
        let mut resp = (
            StatusCode::OK,
            [(header::CONTENT_TYPE, "application/json")],
            axum::Json(payload),
        )
            .into_response();
        echo_tenant(&tenant, &mut resp);
        Ok::<_, ApiError>(resp)
    };
    go.await.unwrap_or_else(|e| e.into_response())
}

// ---------- DELETE /jsonldContexts/{ctxId} (5.13.5) ----------

/// 5.13.5.4 Delete and Reload @context: unknown id → ResourceNotFound;
/// reload=true re-downloads a Cached @context in place (failure →
/// LdContextNotAvailable, entry kept) and is BadRequestData for other
/// kinds; without reload the entry is removed.
pub async fn delete_context(
    State(st): State<AppState>,
    Path(id): Path<String>,
    CleanParams(params): CleanParams,
    headers: HeaderMap,
) -> Response {
    let go = async {
        let tenant = tenant_from(&headers)?;
        check_params(&params, &["reload", "local"])?;
        let reload = reload_param(&params)?;
        let entry = find_entry(&st, &tenant, &id).await?;
        if reload {
            // reload is only meaningful for Cached @contexts (5.13.5.4);
            // unknown ids and non-Cached kinds are 400 (051_04_01/05)
            return match entry {
                Some(CtxEntry::Core(url)) => {
                    st.loader.refetch(&url).await.map_err(ApiError::from)?;
                    Ok(no_content(&tenant))
                }
                Some(CtxEntry::Cached(u)) => {
                    st.loader.refetch(&u.url).await.map_err(ApiError::from)?;
                    Ok(no_content(&tenant))
                }
                _ => Err(NgsiError::BadRequestData(
                    "reload is only valid for Cached @contexts (5.13.5)".into(),
                )
                .into()),
            };
        }
        match entry {
            None => Err(NgsiError::ResourceNotFound(format!("@context {id} not found")).into()),
            Some(CtxEntry::Core(_)) => {
                Err(NgsiError::BadRequestData("the core @context cannot be deleted".into()).into())
            }
            Some(CtxEntry::Cached(u)) => {
                st.loader.usage_remove(&u.url).await;
                // The write-through row shares the deterministic local id —
                // deleting the API entry must delete the persisted copy too,
                // or a restart resurrects a deleted @context (5.13.5).
                let _ = st.store.context_delete(&u.local_id);
                Ok(no_content(&tenant))
            }
            Some(CtxEntry::Stored(doc)) => {
                let lid = doc["localId"].as_str().unwrap_or(&id);
                st.store.context_delete(lid)?;
                if let Some(url) = doc.get("url").and_then(Value::as_str) {
                    st.loader.usage_remove(url).await;
                }
                Ok::<_, ApiError>(no_content(&tenant))
            }
        }
    };
    go.await.unwrap_or_else(|e| e.into_response())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn forged_host() -> HeaderMap {
        let mut h = HeaderMap::new();
        h.insert(header::HOST, "attacker.example".parse().expect("host"));
        h
    }

    fn hosted_row(url: &str, local_id: &str, owner: &TenantId) -> Value {
        json!({
            "url": url,
            "localId": local_id,
            "kind": "Hosted",
            "createdAt": now_iso(),
            "owner": owner.as_str(),
            "body": {"@context": {}},
        })
    }

    /// 5.13.4.4: a stored @context resolves by its locally unique URI —
    /// the localId and the full published URL name the SAME entry (5.13.2.4),
    /// while a URL that only ends in a known localId names no entry at all,
    /// and another tenant's Hosted row stays invisible (5.13.1).
    #[tokio::test]
    async fn clause_5_13_4_entry_resolves_by_local_id_and_by_url() {
        let st = AppState::new("antares-ctx-find".into());
        let owner = TenantId::default();
        let other = TenantId::new("beta").expect("tenant");
        let local_id = "b2a1c0de-0000-4000-8000-000000000001";
        let url = format!("http://broker.example/ngsi-ld/v1/jsonldContexts/{local_id}");
        st.store
            .context_put(local_id, hosted_row(&url, local_id, &owner))
            .expect("store the @context");
        assert!(
            find_entry(&st, &owner, local_id)
                .await
                .expect("store")
                .is_some(),
            "the localId names the entry"
        );
        assert!(
            find_entry(&st, &owner, &url)
                .await
                .expect("store")
                .is_some(),
            "the published URL names the same entry"
        );
        assert!(
            find_entry(&st, &other, &url)
                .await
                .expect("store")
                .is_none(),
            "another tenant's Hosted @context is as absent as one that never existed"
        );
        let forged = format!("http://attacker.example/ngsi-ld/v1/jsonldContexts/{local_id}");
        assert!(
            find_entry(&st, &owner, &forged)
                .await
                .expect("store")
                .is_none(),
            "a foreign URL ending in a known localId is not that entry"
        );
        assert!(find_entry(&st, &owner, "no-such-context")
            .await
            .expect("store")
            .is_none());
    }

    /// Table 6.29.3.2-1: List @contexts defines `details` and `kind` only —
    /// a pagination parameter this resource does not implement must be
    /// refused (6.3.20 InvalidRequest), never accepted and ignored.
    #[tokio::test]
    async fn clause_6_29_3_2_list_takes_only_the_table_parameters() {
        let st = AppState::new("antares-ctx-params".into());
        for bad in ["limit", "offset", "count", "bogus"] {
            let p: HashMap<String, String> =
                [(bad.to_owned(), "1".to_owned())].into_iter().collect();
            let resp = list_contexts(State(st.clone()), CleanParams(p), HeaderMap::new()).await;
            assert_eq!(
                resp.status(),
                StatusCode::BAD_REQUEST,
                "{bad:?} is not a List @contexts parameter"
            );
        }
        for good in [("details", "true"), ("kind", "Hosted")] {
            let p: HashMap<String, String> = [(good.0.to_owned(), good.1.to_owned())]
                .into_iter()
                .collect();
            let resp = list_contexts(State(st.clone()), CleanParams(p), HeaderMap::new()).await;
            assert_eq!(resp.status(), StatusCode::OK, "{good:?}");
        }
    }

    /// 5.13.2.4/5.13.3.5: the URI published for a broker-served @context
    /// names the BROKER. It is taken from configuration, so a forged `Host`
    /// header cannot make the broker advertise someone else's address, and a
    /// TLS deployment is not stuck advertising `http`.
    #[test]
    fn published_context_url_comes_from_configuration_not_the_host_header() {
        assert_eq!(
            context_base(Some("https://broker.example"), &forged_host()),
            "https://broker.example/ngsi-ld/v1/jsonldContexts"
        );
        // a configured value with a trailing slash must not double it
        assert_eq!(
            context_base(Some("https://broker.example/"), &forged_host()),
            "https://broker.example/ngsi-ld/v1/jsonldContexts"
        );
        // nothing configured: today's Host-derived URL stays the fallback
        assert_eq!(
            context_base(None, &forged_host()),
            "http://attacker.example/ngsi-ld/v1/jsonldContexts"
        );
        assert_eq!(
            context_base(Some(""), &forged_host()),
            "http://attacker.example/ngsi-ld/v1/jsonldContexts",
            "an empty setting is no setting"
        );
    }
}
