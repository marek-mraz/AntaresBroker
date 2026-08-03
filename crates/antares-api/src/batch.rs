//! Batch operations /entityOperations/* (5.6.7–5.6.10, 5.6.20, 5.7.2-POST;
//! resources 6.14–6.17, 6.23, 6.31).

use crate::entities::{filter_entities, merge_into, paginate, stamp_new};
use crate::negotiate::*;
use crate::repr::{apply, parse_repr};
use crate::state::{now_iso, AppState};
use antares_jsonld::{compact_entity, expand_entity, ExpandOpts};
use antares_model::NgsiError;
use antares_sql::store::Kind;
use axum::body::Bytes;
use axum::extract::{Query, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use serde_json::{json, Value};
use std::collections::HashMap;

use crate::negotiate::CleanParams;

/// Parse a batch body: JSON array of entity documents; per-document context
/// resolution (ld+json ⇒ each doc's own @context; json ⇒ Link header).
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
    let value: Value = serde_json::from_slice(body)
        .map_err(|e| NgsiError::InvalidRequest(format!("body is not valid JSON: {e}")))?;
    let items = value
        .as_array()
        .filter(|a| !a.is_empty())
        .ok_or_else(|| {
            NgsiError::BadRequestData("batch body must be a non-empty JSON array".into())
        })?;
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
            Err(
                NgsiError::BadRequestData("application/json entity carries @context".into())
                    .into(),
            )
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
    let mut out = BatchOutcome {
        success: vec![],
        errors: vec![],
    };
    let mut created_ids: Vec<String> = vec![];
    let mut any_created = false;
    let mut any_updated = false;
    for (item, ctx) in items {
        let id_hint = item
            .get("id")
            .and_then(Value::as_str)
            .map(str::to_owned);
        let ctx = match ctx {
            Ok(c) => c,
            Err(e) => {
                out.errors.push(err_entry(id_hint.as_deref(), &ngsi_of(e)));
                continue;
            }
        };
        let run = || -> Result<(String, bool, bool), NgsiError> {
            let obj = item
                .as_object()
                .ok_or_else(|| NgsiError::BadRequestData("entity must be an object".into()))?;
            let fragment_ok = mode == BatchMode::Merge;
            let mut expanded = expand_entity(
                obj,
                &ctx,
                ExpandOpts {
                    fragment: false,
                    allow_null: fragment_ok,
                    temporal: false,
                },
            )?;
            let id = expanded["id"].as_str().expect("validated").to_owned();
            let ts = now_iso();
            match mode {
                BatchMode::Create => {
                    stamp_new(&mut expanded, &ts);
                    if !st.store.create(&tenant, Kind::Entity, &id, expanded.clone()) {
                        return Err(NgsiError::AlreadyExists(format!(
                            "entity {id} already exists"
                        )));
                    }
                    crate::entities::mirror_record(st, &tenant, &expanded);
                    Ok((id, true, false))
                }
                BatchMode::Upsert => {
                    let existed = st.store.get(&tenant, Kind::Entity, &id).is_some();
                    if existed && update_mode == Some("update") {
                        let res = st.store.mutate(&tenant, Kind::Entity, &id, |doc| {
                            merge_into(doc, &expanded, &ts);
                            Ok::<(), NgsiError>(())
                        });
                        if let Some(Err(e)) = res {
                            return Err(e);
                        }
                        crate::entities::mirror_record(st, &tenant, &expanded);
                    } else {
                        stamp_new(&mut expanded, &ts);
                        st.store.upsert(&tenant, Kind::Entity, &id, expanded.clone());
                        crate::entities::mirror_record(st, &tenant, &expanded);
                    }
                    Ok((id, !existed, false))
                }
                BatchMode::Update | BatchMode::Merge => {
                    if st.store.get(&tenant, Kind::Entity, &id).is_none() {
                        return Err(NgsiError::ResourceNotFound(format!(
                            "entity {id} not found"
                        )));
                    }
                    // batch update with noOverwrite: existing attributes are
                    // left alone; if any existed, the entity is a partial
                    // failure (005_02 ⇒ 207)
                    let mut skipped_existing = false;
                    let res = st.store.mutate(&tenant, Kind::Entity, &id, |doc| {
                        if mode == BatchMode::Update && no_overwrite {
                            let target = doc.as_object_mut().expect("entity object");
                            for (k, v) in expanded.as_object().expect("expanded") {
                                if matches!(
                                    k.as_str(),
                                    "id" | "type" | "scope" | "createdAt" | "modifiedAt"
                                ) {
                                    continue;
                                }
                                if target.contains_key(k) {
                                    skipped_existing = true;
                                } else {
                                    target.insert(k.clone(), v.clone());
                                }
                            }
                            target.insert("modifiedAt".into(), Value::String(ts.clone()));
                        } else {
                            merge_into(doc, &expanded, &ts);
                        }
                        Ok::<(), NgsiError>(())
                    });
                    if let Some(Err(e)) = res {
                        return Err(e);
                    }
                    crate::entities::mirror_record(st, &tenant, &expanded);
                    Ok((id, false, skipped_existing))
                }
            }
        };
        match run() {
            Ok((id, created_now, partial)) => {
                if created_now {
                    any_created = true;
                    if !created_ids.contains(&id) {
                        created_ids.push(id.clone());
                    }
                } else {
                    any_updated = true;
                }
                if partial {
                    out.errors.push(err_entry(
                        Some(&id),
                        &NgsiError::BadRequestData(
                            "some attributes already existed (noOverwrite)".into(),
                        ),
                    ));
                } else if !out.success.contains(&Value::String(id.clone())) {
                    out.success.push(Value::String(id));
                }
            }
            Err(e) => out.errors.push(err_entry(id_hint.as_deref(), &e)),
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
        let ids = value
            .as_array()
            .filter(|a| !a.is_empty())
            .ok_or_else(|| {
                NgsiError::BadRequestData("batch delete body must be a non-empty array".into())
            })?;
        let mut out = BatchOutcome {
            success: vec![],
            errors: vec![],
        };
        for id in ids {
            let Some(id) = id.as_str() else {
                out.errors.push(err_entry(
                    None,
                    &NgsiError::BadRequestData("entity id must be a string".into()),
                ));
                continue;
            };
            if st.store.delete(&tenant, Kind::Entity, id) {
                out.success.push(Value::String(id.to_owned()));
            } else {
                out.errors.push(err_entry(
                    Some(id),
                    &NgsiError::ResourceNotFound(format!("entity {id} not found")),
                ));
            }
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
    check_params(params, &["limit", "offset", "count", "options", "format", "local"])?;
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
    let matches = filter_entities(st, &tenant, &vp, &parsed.ctx)?;
    let mut page_params = params.clone();
    page_params.extend(vp.clone());
    let (page, count_hdr, _links) =
        paginate(st, &page_params, matches, "/ngsi-ld/v1/entityOperations/query")?;
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
                let page_ids: Vec<&str> =
                    page.iter().filter_map(|d| d["id"].as_str()).collect();
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
