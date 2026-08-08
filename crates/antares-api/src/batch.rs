//! Batch operations /entityOperations/* (5.6.7–5.6.10, 5.6.20, 5.7.2-POST;
//! resources 6.14–6.17, 6.23, 6.31).

use crate::entities::{merge_into, paginate, stamp_new};
use crate::negotiate::*;
use crate::repr::{apply, parse_repr};
use crate::state::{now_iso, AppState};
use antares_jsonld::{expand_entity, ExpandOpts};
use antares_model::NgsiError;
use axum::body::Bytes;
use axum::extract::State;
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use serde_json::{json, Value};
use std::collections::HashMap;

use crate::negotiate::CleanParams;

/// Parse a batch body: JSON array of entity documents; per-document context
/// resolution (ld+json ⇒ each doc's own @context; json ⇒ Link header).
/// J6: batch bodies are the ingest hot path — sonic-rs (3–4× parse) behind
/// the `sonic` feature, serde_json always compiled as the fallback (§6.1).
#[cfg(feature = "sonic")]
fn parse_batch_body(body: &[u8]) -> Result<Value, String> {
    sonic_rs::from_slice(body).map_err(|e| e.to_string())
}
#[cfg(not(feature = "sonic"))]
fn parse_batch_body(body: &[u8]) -> Result<Value, String> {
    serde_json::from_slice(body).map_err(|e| e.to_string())
}

async fn parse_batch(
    st: &AppState,
    headers: &HeaderMap,
    body: &[u8],
) -> ApiResult<Vec<(Value, ApiResult<std::sync::Arc<antares_jsonld::Context>>)>> {
    let ct = content_type(headers);
    let ld = match ct.as_str() {
        "application/json" => false,
        "application/ld+json" => true,
        _ => return Err(ApiError::Bare(StatusCode::UNSUPPORTED_MEDIA_TYPE)),
    };
    let value: Value = parse_batch_body(body)
        .map_err(|e| NgsiError::InvalidRequest(format!("body is not valid JSON: {e}")))?;
    let items = value.as_array().filter(|a| !a.is_empty()).ok_or_else(|| {
        NgsiError::BadRequestData("batch body must be a non-empty JSON array".into())
    })?;
    // I2: batch entity count cap (§16.3)
    if items.len() > crate::bounds::MAX_BATCH_ITEMS {
        return Err(NgsiError::BadRequestData(format!(
            "batch of {} exceeds the {}-entity limit",
            items.len(),
            crate::bounds::MAX_BATCH_ITEMS
        ))
        .into());
    }
    let link = link_context(headers);
    if ld && link.is_some() {
        return Err(NgsiError::BadRequestData(
            "application/ld+json batch must not also carry a Link @context (6.3.5)".into(),
        )
        .into());
    }
    let mut out = Vec::new();
    for item in items {
        let ctx = if ld {
            match item.get("@context") {
                Some(c) => st.loader.resolve(c).await.map_err(ApiError::from),
                None => Err(NgsiError::BadRequestData(
                    "ld+json batch entity without @context".into(),
                )
                .into()),
            }
        } else if item.get("@context").is_some() {
            Err(NgsiError::BadRequestData("application/json entity carries @context".into()).into())
        } else {
            match &link {
                Some(url) => st
                    .loader
                    .resolve(&Value::String(url.clone()))
                    .await
                    .map_err(ApiError::from),
                None => Ok(st.loader.core()),
            }
        };
        out.push((item.clone(), ctx));
    }
    Ok(out)
}

struct BatchOutcome {
    success: Vec<Value>,
    errors: Vec<Value>,
}

impl BatchOutcome {
    fn respond(
        self,
        tenant: &antares_model::TenantId,
        all_ok_status: StatusCode,
        body_on_ok: bool,
    ) -> Response {
        if self.errors.is_empty() {
            if body_on_ok {
                let mut resp = (
                    all_ok_status,
                    [(axum::http::header::CONTENT_TYPE, "application/json")],
                    axum::Json(Value::Array(self.success)),
                )
                    .into_response();
                echo_tenant(tenant, &mut resp);
                resp
            } else {
                let mut resp = all_ok_status.into_response();
                echo_tenant(tenant, &mut resp);
                resp
            }
        } else {
            multi_status(
                json!({"success": self.success, "errors": self.errors}),
                tenant,
            )
        }
    }
}

fn err_entry(id: Option<&str>, e: &NgsiError) -> Value {
    json!({
        "entityId": id.unwrap_or("unknown"),
        "error": problem_value(e),
    })
}

fn ngsi_of(e: ApiError) -> NgsiError {
    match e {
        ApiError::Ngsi(n) => n,
        ApiError::Bare(code) => NgsiError::BadRequestData(format!("HTTP {code}")),
    }
}

// ---------- POST /entityOperations/create (5.6.7) ----------

pub async fn batch_create(
    State(st): State<AppState>,
    CleanParams(params): CleanParams,
    headers: HeaderMap,
    body: Bytes,
) -> Response {
    match batch_write(&st, &params, &headers, &body, BatchMode::Create).await {
        Ok(r) => r,
        Err(e) => e.into_response(),
    }
}

pub async fn batch_upsert(
    State(st): State<AppState>,
    CleanParams(params): CleanParams,
    headers: HeaderMap,
    body: Bytes,
) -> Response {
    match batch_write(&st, &params, &headers, &body, BatchMode::Upsert).await {
        Ok(r) => r,
        Err(e) => e.into_response(),
    }
}

pub async fn batch_update(
    State(st): State<AppState>,
    CleanParams(params): CleanParams,
    headers: HeaderMap,
    body: Bytes,
) -> Response {
    match batch_write(&st, &params, &headers, &body, BatchMode::Update).await {
        Ok(r) => r,
        Err(e) => e.into_response(),
    }
}

pub async fn batch_merge(
    State(st): State<AppState>,
    CleanParams(params): CleanParams,
    headers: HeaderMap,
    body: Bytes,
) -> Response {
    match batch_write(&st, &params, &headers, &body, BatchMode::Merge).await {
        Ok(r) => r,
        Err(e) => e.into_response(),
    }
}

#[derive(Clone, Copy, PartialEq)]
enum BatchMode {
    Create,
    Upsert,
    Update,
    Merge,
}

async fn batch_write(
    st: &AppState,
    params: &HashMap<String, String>,
    headers: &HeaderMap,
    body: &[u8],
    mode: BatchMode,
) -> ApiResult<Response> {
    let tenant = tenant_from(headers)?;
    check_params(params, &["options", "local"])?;
    let update_mode = params.get("options").map(String::as_str); // replace|update for upsert; noOverwrite|overwrite for update
    let no_overwrite = update_mode == Some("noOverwrite");
    let items = parse_batch(st, headers, body).await?;
    // distributed batch (4.3.6): one forwarded request per matching source
    let mut fwd_items: Vec<(
        serde_json::Map<String, Value>,
        std::sync::Arc<antares_jsonld::Context>,
    )> = Vec::new();
    let mut spec = crate::csource::CsrSpec::default();
    let mut spec_types = Vec::new();
    let mut spec_ids = Vec::new();
    let mut spec_attrs = Vec::new();
    for (item, ctx) in &items {
        let (Some(o), Ok(c)) = (item.as_object(), ctx.as_ref()) else {
            continue;
        };
        if let Some(id) = o.get("id").and_then(Value::as_str) {
            spec_ids.push(id.to_owned());
        }
        match o.get("type") {
            Some(Value::String(t)) => spec_types.push(c.expand_key(t)),
            Some(Value::Array(a)) => {
                spec_types.extend(a.iter().filter_map(Value::as_str).map(|t| c.expand_key(t)))
            }
            _ => {}
        }
        for k in o.keys() {
            if !matches!(k.as_str(), "id" | "type" | "scope" | "@context") {
                spec_attrs.push(c.expand_key(k));
            }
        }
        fwd_items.push((o.clone(), c.clone()));
    }
    if !spec_types.is_empty() {
        spec.types = Some(spec_types);
    }
    if !spec_ids.is_empty() {
        spec.ids = Some(spec_ids);
    }
    if !spec_attrs.is_empty() {
        spec.attrs = Some(spec_attrs);
    }
    let fed_regs = crate::federation::write_regs(st, &tenant, &spec, &st.loader.core(), params);
    if !fed_regs.is_empty() && crate::federation::via_loop(headers, &st.host_alias) {
        return Ok(crate::federation::loop_508(&tenant));
    }
    let mut out = BatchOutcome {
        success: vec![],
        errors: vec![],
    };
    let mut created_ids: Vec<String> = vec![];
    let mut any_created = false;
    let mut any_updated = false;
    let proxies: Vec<&crate::federation::FedReg> =
        fed_regs.iter().filter(|r| r.is_proxy()).collect();
    // C5: creates are collected and written as ONE multi-row statement (§4);
    // upsert/update/merge run batched per round below (C5+, audit 2026-08-08).
    let mut pending_creates: Vec<(String, Value)> = Vec::new();
    let mut prepped: Vec<(String, Value)> = Vec::new();
    for (item, ctx) in items {
        let id_hint = item.get("id").and_then(Value::as_str).map(str::to_owned);
        let ctx = match ctx {
            Ok(c) => c,
            Err(e) => {
                out.errors.push(err_entry(id_hint.as_deref(), &ngsi_of(e)));
                continue;
            }
        };
        // proxied (exclusive/redirect) attributes are never stored locally
        let item = if proxies.is_empty() {
            item
        } else if let Some(o) = item.as_object() {
            let (rest, has_attrs) = crate::federation::strip_proxied(o, &proxies, &ctx);
            if !has_attrs {
                continue; // wholly proxied: no local part for this item
            }
            Value::Object(rest)
        } else {
            item
        };
        if mode == BatchMode::Create {
            let prep = || -> Result<(String, Value), NgsiError> {
                let obj = item
                    .as_object()
                    .ok_or_else(|| NgsiError::BadRequestData("entity must be an object".into()))?;
                let mut expanded = expand_entity(obj, &ctx, ExpandOpts::default())?;
                let id = expanded["id"].as_str().expect("validated").to_owned();
                stamp_new(&mut expanded, &now_iso());
                Ok((id, expanded))
            };
            match prep() {
                Ok(pair) => pending_creates.push(pair),
                Err(e) => out.errors.push(err_entry(id_hint.as_deref(), &e)),
            }
            continue;
        }
        // C5+ phase 1 (audit 2026-08-08): expansion/validation per item only;
        // the store operations run BATCHED below — one transaction per round
        // instead of one per item.
        let prep = || -> Result<(String, Value), NgsiError> {
            let obj = item
                .as_object()
                .ok_or_else(|| NgsiError::BadRequestData("entity must be an object".into()))?;
            let fragment_ok = mode == BatchMode::Merge;
            let expanded = expand_entity(
                obj,
                &ctx,
                ExpandOpts {
                    fragment: false,
                    allow_null: fragment_ok,
                    temporal: false,
                    ..Default::default()
                },
            )?;
            let id = expanded["id"].as_str().expect("validated").to_owned();
            Ok((id, expanded))
        };
        match prep() {
            Ok(pair) => prepped.push(pair),
            Err(e) => out.errors.push(err_entry(id_hint.as_deref(), &e)),
        }
    }
    // C5+ phase 2: duplicates of one id keep their sequential semantics by
    // splitting into rounds — the Nth occurrence of an id lands in round N,
    // rounds execute in order, and within a round every id is unique.
    let mut rounds: Vec<Vec<(String, Value)>> = Vec::new();
    {
        let mut occurrence: HashMap<String, usize> = HashMap::new();
        for (id, doc) in prepped {
            let n = occurrence.entry(id.clone()).or_insert(0);
            if rounds.len() <= *n {
                rounds.push(Vec::new());
            }
            rounds[*n].push((id, doc));
            *n += 1;
        }
    }
    for round in rounds {
        let ts = now_iso();
        match mode {
            BatchMode::Create => unreachable!("creates take the batch path above"),
            BatchMode::Upsert => {
                // options=update: merge into existing rows first; ids that
                // turn out absent (or vanish mid-flight) fall through to the
                // replace batch — never a silent success (TOCTOU fix).
                let mut replaces: Vec<(String, Value)> = Vec::new();
                if update_mode == Some("update") {
                    let ids: Vec<String> = round.iter().map(|(id, _)| id.clone()).collect();
                    let docs: HashMap<&str, &Value> =
                        round.iter().map(|(id, d)| (id.as_str(), d)).collect();
                    let res = st.store.batch_mutate(&tenant, &ids, |id, doc| {
                        merge_into(doc, docs[id], &ts);
                        Ok::<(), NgsiError>(())
                    })?;
                    for ((id, expanded), r) in round.iter().zip(res) {
                        match r {
                            Some(Err(e)) => out.errors.push(err_entry(Some(id), &e)),
                            Some(Ok(())) => {
                                any_updated = true;
                                crate::entities::mirror_record(st, &tenant, expanded);
                                if !out.success.contains(&Value::String(id.clone())) {
                                    out.success.push(Value::String(id.clone()));
                                }
                            }
                            None => replaces.push((id.clone(), expanded.clone())),
                        }
                    }
                } else {
                    replaces = round;
                }
                if !replaces.is_empty() {
                    for (_, doc) in replaces.iter_mut() {
                        stamp_new(doc, &ts);
                    }
                    let flags = st.store.batch_upsert(&tenant, replaces.clone())?;
                    for ((id, expanded), created) in replaces.iter().zip(flags) {
                        if created {
                            any_created = true;
                            if !created_ids.contains(id) {
                                created_ids.push(id.clone());
                            }
                        } else {
                            any_updated = true;
                        }
                        crate::entities::mirror_record(st, &tenant, expanded);
                        if !out.success.contains(&Value::String(id.clone())) {
                            out.success.push(Value::String(id.clone()));
                        }
                    }
                }
            }
            BatchMode::Update | BatchMode::Merge => {
                let ids: Vec<String> = round.iter().map(|(id, _)| id.clone()).collect();
                let docs: HashMap<&str, &Value> =
                    round.iter().map(|(id, d)| (id.as_str(), d)).collect();
                // batch update with noOverwrite: existing attribute instances
                // are left alone; if any existed, the entity is a partial
                // failure (005_02 ⇒ 207)
                let mut skipped: HashMap<String, bool> = HashMap::new();
                let res = st.store.batch_mutate(&tenant, &ids, |id, doc| {
                    let expanded = docs[id];
                    if mode == BatchMode::Update && no_overwrite {
                        // noOverwrite is instance-level: only instances
                        // whose datasetId already exists are skipped
                        let target = doc.as_object_mut().expect("entity object");
                        for (k, v) in expanded.as_object().expect("expanded") {
                            if matches!(
                                k.as_str(),
                                "id" | "type" | "scope" | "createdAt" | "modifiedAt"
                            ) {
                                continue;
                            }
                            let incoming: Vec<Value> = v.as_array().cloned().unwrap_or_default();
                            match target.get_mut(k).and_then(Value::as_array_mut) {
                                None => {
                                    target.insert(k.clone(), Value::Array(incoming));
                                }
                                Some(cur) => {
                                    for ni in incoming {
                                        let ds = ni.get("datasetId").and_then(Value::as_str);
                                        if cur.iter().any(|ci| {
                                            ci.get("datasetId").and_then(Value::as_str) == ds
                                        }) {
                                            skipped.insert(id.to_owned(), true);
                                        } else {
                                            cur.push(ni);
                                        }
                                    }
                                }
                            }
                        }
                        target.insert("modifiedAt".into(), Value::String(ts.clone()));
                    } else {
                        merge_into(doc, expanded, &ts);
                    }
                    Ok::<(), NgsiError>(())
                })?;
                for ((id, expanded), r) in round.iter().zip(res) {
                    match r {
                        None => out.errors.push(err_entry(
                            Some(id),
                            &NgsiError::ResourceNotFound(format!("entity {id} not found")),
                        )),
                        Some(Err(e)) => out.errors.push(err_entry(Some(id), &e)),
                        Some(Ok(())) => {
                            any_updated = true;
                            crate::entities::mirror_record(st, &tenant, expanded);
                            if skipped.get(id).copied().unwrap_or(false) {
                                out.errors.push(err_entry(
                                    Some(id),
                                    &NgsiError::BadRequestData(
                                        "some attributes already existed (noOverwrite)".into(),
                                    ),
                                ));
                            } else if !out.success.contains(&Value::String(id.clone())) {
                                out.success.push(Value::String(id.clone()));
                            }
                        }
                    }
                }
            }
        }
    }
    // C5: the collected creates, one multi-row statement, one transaction.
    if !pending_creates.is_empty() {
        let flags = st.store.batch_create(&tenant, pending_creates.clone())?;
        for ((id, expanded), created) in pending_creates.iter().zip(flags) {
            if created {
                any_created = true;
                if !created_ids.contains(id) {
                    created_ids.push(id.clone());
                }
                crate::entities::mirror_record(st, &tenant, expanded);
                if !out.success.contains(&Value::String(id.clone())) {
                    out.success.push(Value::String(id.clone()));
                }
            } else {
                out.errors.push(err_entry(
                    Some(id),
                    &NgsiError::AlreadyExists(format!("entity {id} already exists")),
                ));
            }
        }
    }
    // 5.6.8: a 201 upsert body lists ONLY the newly created ids
    if mode == BatchMode::Upsert && any_created {
        out.success = created_ids.into_iter().map(Value::String).collect();
    }
    let (status, body_on_ok) = match mode {
        BatchMode::Create => (StatusCode::CREATED, true),
        BatchMode::Upsert => {
            if any_created {
                (StatusCode::CREATED, true)
            } else {
                let _ = any_updated;
                (StatusCode::NO_CONTENT, false)
            }
        }
        BatchMode::Update | BatchMode::Merge => (StatusCode::NO_CONTENT, false),
    };
    if !fed_regs.is_empty() {
        let (op, res_path) = match mode {
            BatchMode::Create => ("createBatch", "create"),
            BatchMode::Upsert => ("upsertBatch", "upsert"),
            BatchMode::Update => ("updateBatch", "update"),
            BatchMode::Merge => ("mergeBatch", "merge"),
        };
        let src = fwd_items
            .first()
            .map(|(_, c)| c.source.clone())
            .unwrap_or(Value::Null);
        let ctx_url = crate::federation::ctx_link_url(headers, &src);
        let mut query: Vec<(String, String)> = Vec::new();
        if let Some(o) = params.get("options") {
            query.push(("options".into(), o.clone()));
        }
        let mut parts = vec![crate::federation::Part {
            status: if out.errors.is_empty() {
                status.as_u16()
            } else {
                207
            },
            detail: "local batch".into(),
        }];
        for reg in &fed_regs {
            if reg.mode == "exclusive" && !reg.supports(op) {
                parts.push(crate::federation::conflict_part(op));
                continue;
            }
            let arr: Vec<Value> = fwd_items
                .iter()
                .filter_map(|(o, c)| crate::federation::reduce_to_scope(o, reg, c))
                .collect();
            if arr.is_empty() {
                continue;
            }
            parts.push(
                crate::federation::forward_part(
                    st,
                    reqwest::Method::POST,
                    format!("{}/ngsi-ld/v1/entityOperations/{res_path}", reg.endpoint),
                    &query,
                    headers,
                    &tenant,
                    reg,
                    &ctx_url,
                    Some(Value::Array(arr)),
                )
                .await,
            );
        }
        return Ok(crate::federation::combine(
            parts,
            out.respond(&tenant, status, body_on_ok),
            &tenant,
        ));
    }
    Ok(out.respond(&tenant, status, body_on_ok))
}

// ---------- POST /entityOperations/delete (5.6.10) ----------

pub async fn batch_delete(
    State(st): State<AppState>,
    CleanParams(params): CleanParams,
    headers: HeaderMap,
    body: Bytes,
) -> Response {
    let go = async {
        let tenant = tenant_from(&headers)?;
        check_params(&params, &["local"])?;
        let ct = content_type(&headers);
        if ct != "application/json" && ct != "application/ld+json" {
            return Err(ApiError::Bare(StatusCode::UNSUPPORTED_MEDIA_TYPE));
        }
        let value: Value = serde_json::from_slice(&body)
            .map_err(|e| NgsiError::InvalidRequest(format!("body is not valid JSON: {e}")))?;
        let ids = value.as_array().filter(|a| !a.is_empty()).ok_or_else(|| {
            NgsiError::BadRequestData("batch delete body must be a non-empty array".into())
        })?;
        let spec = crate::csource::CsrSpec {
            ids: Some(
                ids.iter()
                    .filter_map(Value::as_str)
                    .map(str::to_owned)
                    .collect(),
            ),
            ..Default::default()
        };
        let regs = crate::federation::write_regs(&st, &tenant, &spec, &st.loader.core(), &params);
        let proxied = regs.iter().any(|r| r.is_proxy());
        let mut out = BatchOutcome {
            success: vec![],
            errors: vec![],
        };
        // C5: one multi-row DELETE for the whole batch; flags in input order.
        let id_strs: Vec<String> = ids
            .iter()
            .filter_map(Value::as_str)
            .map(str::to_owned)
            .collect();
        let mut flags = st.store.batch_delete(&tenant, &id_strs)?.into_iter();
        for id in ids {
            let Some(id) = id.as_str() else {
                out.errors.push(err_entry(
                    None,
                    &NgsiError::BadRequestData("entity id must be a string".into()),
                ));
                continue;
            };
            if flags.next().unwrap_or(false) || proxied {
                // proxied entities are never stored locally — a local miss is
                // not an error under exclusive/redirect (4.3.6.3)
                // 5.6.10 deletes carry the same temporal-deletion semantics
                // as 5.6.6 — without this, batch-deleted entities live on in
                // the temporal store (N7b: the reset's batch delete leaked
                // every prior suite's Buildings into the orderBy queries).
                crate::entities::mirror_delete_entity(&st, &tenant, id);
                out.success.push(Value::String(id.to_owned()));
            } else {
                out.errors.push(err_entry(
                    Some(id),
                    &NgsiError::ResourceNotFound(format!("entity {id} not found")),
                ));
            }
        }
        if !regs.is_empty() {
            if crate::federation::via_loop(&headers, &st.host_alias) {
                return Ok(crate::federation::loop_508(&tenant));
            }
            let ctx_url = crate::federation::ctx_link_url(&headers, &st.loader.core().source);
            let mut parts = vec![crate::federation::Part {
                status: if out.errors.is_empty() { 204 } else { 207 },
                detail: "local batch delete".into(),
            }];
            for reg in &regs {
                if reg.mode == "exclusive" && !reg.supports("deleteBatch") {
                    parts.push(crate::federation::conflict_part("deleteBatch"));
                    continue;
                }
                parts.push(
                    crate::federation::forward_part(
                        &st,
                        reqwest::Method::POST,
                        format!("{}/ngsi-ld/v1/entityOperations/delete", reg.endpoint),
                        &[],
                        &headers,
                        &tenant,
                        reg,
                        &ctx_url,
                        Some(Value::Array(ids.clone())),
                    )
                    .await,
                );
            }
            return Ok(crate::federation::combine(
                parts,
                out.respond(&tenant, StatusCode::NO_CONTENT, false),
                &tenant,
            ));
        }
        Ok::<_, ApiError>(out.respond(&tenant, StatusCode::NO_CONTENT, false))
    };
    go.await.unwrap_or_else(|e| e.into_response())
}

// ---------- POST /entityOperations/query (6.23) ----------

pub async fn batch_query(
    State(st): State<AppState>,
    CleanParams(params): CleanParams,
    headers: HeaderMap,
    body: Bytes,
) -> Response {
    match batch_query_inner(&st, &params, &headers, &body).await {
        Ok(r) => r,
        Err(e) => e.into_response(),
    }
}

async fn batch_query_inner(
    st: &AppState,
    params: &HashMap<String, String>,
    headers: &HeaderMap,
    body: &[u8],
) -> ApiResult<Response> {
    let tenant = tenant_from(headers)?;
    check_params(
        params,
        &["limit", "offset", "count", "options", "format", "local"],
    )?;
    // POST query IS Query Entities: geo+json is a valid Accept here (6.3.15)
    let accept = parse_accept_geo(headers)?;
    let parsed = parse_body(&st.loader, headers, body, BodyKind::Standard).await?;
    let q = parsed
        .value
        .as_object()
        .ok_or_else(|| NgsiError::BadRequestData("query body must be an object".into()))?;
    if q.get("type").and_then(Value::as_str) != Some("Query") {
        return Err(NgsiError::BadRequestData("body type must be Query (5.2.23)".into()).into());
    }
    // Convert Query members into virtual params reusing the GET filter path.
    let mut vp: HashMap<String, String> = HashMap::new();
    if let Some(es) = q.get("entities").and_then(Value::as_array) {
        let types: Vec<String> = es
            .iter()
            .filter_map(|e| e.get("type").and_then(Value::as_str).map(str::to_owned))
            .collect();
        if !types.is_empty() {
            vp.insert("type".into(), types.join(","));
        }
        let ids: Vec<&str> = es
            .iter()
            .filter_map(|e| e.get("id").and_then(Value::as_str))
            .collect();
        if !ids.is_empty() {
            vp.insert("id".into(), ids.join(","));
        }
        let pats: Vec<&str> = es
            .iter()
            .filter_map(|e| e.get("idPattern").and_then(Value::as_str))
            .collect();
        if !pats.is_empty() {
            vp.insert("idPattern".into(), pats.join("|"));
        }
    }
    if let Some(j) = q.get("join").and_then(Value::as_str) {
        vp.insert("join".into(), j.to_owned());
    }
    if let Some(jl) = q.get("joinLevel").and_then(Value::as_f64) {
        vp.insert("joinLevel".into(), (jl as i64).to_string());
    }
    for k in ["q", "scopeQ", "lang"] {
        if let Some(v) = q.get(k).and_then(Value::as_str) {
            vp.insert(k.into(), v.to_owned());
        }
    }
    if let Some(attrs) = q.get("attrs").and_then(Value::as_array) {
        let l: Vec<&str> = attrs.iter().filter_map(Value::as_str).collect();
        vp.insert("attrs".into(), l.join(","));
    }
    if let Some(g) = q.get("geoQ").and_then(Value::as_object) {
        for k in ["georel", "geometry", "geoproperty"] {
            if let Some(v) = g.get(k).and_then(Value::as_str) {
                vp.insert(k.into(), v.to_owned());
            }
        }
        if let Some(c) = g.get("coordinates") {
            vp.insert("coordinates".into(), c.to_string());
        }
    }
    if let Some(l) = params.get("local") {
        vp.insert("local".into(), l.clone());
    }
    let fed = if crate::federation::active(&vp)
        && !crate::federation::via_loop(headers, &st.host_alias)
    {
        crate::federation::fed_query(st, &tenant, headers, &parsed.ctx, &vp).await
    } else {
        Vec::new()
    };
    let matches = crate::entities::filter_entities_fed(st, &tenant, &vp, &parsed.ctx, fed)?;
    let mut page_params = params.clone();
    page_params.extend(vp.clone());
    let (page, count_hdr, _links) = paginate(
        st,
        &page_params,
        matches,
        "/ngsi-ld/v1/entityOperations/query",
    )?;
    let repr = parse_repr(params, &parsed.ctx)?;
    let join = crate::entities::parse_join(&vp)?;
    let mut payload: Vec<Value> = page
        .iter()
        .filter_map(|doc| {
            let shaped = apply(doc, &repr);
            if repr.pick.is_some() && shaped.as_object().is_some_and(|o| o.is_empty()) {
                return None;
            }
            Some(crate::entities::compact_for(&repr, &shaped, &parsed.ctx))
        })
        .collect();
    if let Some((mode, level)) = &join {
        match mode.as_str() {
            "inline" => {
                for p in &mut payload {
                    crate::entities::inline_join(st, &tenant, &parsed.ctx, &repr, p, *level);
                }
            }
            "flat" => {
                let mut linked = std::collections::BTreeMap::new();
                for doc in &page {
                    crate::entities::collect_flat(st, &tenant, &repr, doc, *level, &mut linked);
                }
                let page_ids: Vec<&str> = page.iter().filter_map(|d| d["id"].as_str()).collect();
                for (id, (ldoc, lrepr)) in linked {
                    if !page_ids.contains(&id.as_str()) {
                        payload.push(crate::entities::compact_for(
                            &lrepr,
                            &apply(&ldoc, &lrepr),
                            &parsed.ctx,
                        ));
                    }
                }
            }
            _ => {}
        }
    }
    let out = if accept == Accept::GeoJson {
        crate::entities::to_geojson_collection(payload, None, &parsed.ctx)
    } else {
        Value::Array(payload)
    };
    let mut resp = respond(StatusCode::OK, out, &parsed.ctx, accept, &tenant);
    if let Some(total) = count_hdr {
        if let Ok(v) = total.to_string().parse() {
            resp.headers_mut().insert("NGSILD-Results-Count", v);
        }
    }
    Ok(resp)
}
