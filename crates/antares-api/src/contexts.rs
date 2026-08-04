//! /jsonldContexts management (5.13; resources 6.29/6.30).
//!
//! Three kinds (5.13.1): Hosted (client-added, served on demand),
//! Cached (externally-fetched, metadata only), ImplicitlyCreated (broker-made
//! wrappers for array @contexts on subscriptions, served on demand).

use crate::negotiate::*;
use crate::state::{now_iso, AppState};
use antares_jsonld::Loader;
use antares_model::NgsiError;
use axum::body::Bytes;
use axum::extract::{Path, State};
use axum::http::{header, HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use serde_json::{json, Value};
use std::collections::HashMap;

use crate::negotiate::CleanParams;

pub(crate) fn base_url(headers: &HeaderMap) -> String {
    let host = headers
        .get(header::HOST)
        .and_then(|h| h.to_str().ok())
        .unwrap_or("localhost:9090");
    format!("http://{host}/ngsi-ld/v1/jsonldContexts")
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

async fn find_entry(st: &AppState, id: &str) -> Option<CtxEntry> {
    if let Some(doc) = st.store.context_get(id).ok()? {
        return Some(CtxEntry::Stored(doc));
    }
    // stored entries are also addressable by their full URL
    if id.contains("/ngsi-ld/v1/jsonldContexts/") {
        if let Some(doc) = st
            .store
            .context_list()
            .ok()?
            .into_iter()
            .find(|c| c["url"].as_str() == Some(id))
        {
            return Some(CtxEntry::Stored(doc));
        }
    }
    if Loader::is_pinned_core(id) {
        return Some(CtxEntry::Core(id.to_owned()));
    }
    if let Some(u) = st.loader.usage_get(id).await {
        // broker-local URLs (hosted/implicit) are not Cached entries
        if !u.url.contains("/ngsi-ld/v1/jsonldContexts/") {
            return Some(CtxEntry::Cached(u));
        }
    }
    None
}

// ---------- POST /jsonldContexts (5.13.2) ----------

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
        let local_id = uuid::Uuid::new_v4().to_string();
        let url = format!("{}/{local_id}", base_url(&headers));
        let doc = json!({
            "url": url,
            "localId": local_id,
            "kind": "Hosted",
            "createdAt": now_iso(),
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

pub async fn list_contexts(
    State(st): State<AppState>,
    CleanParams(params): CleanParams,
    headers: HeaderMap,
) -> Response {
    let go = async {
        let tenant = tenant_from(&headers)?;
        check_params(
            &params,
            &["kind", "details", "local", "limit", "offset", "count"],
        )?;
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
            if !keep(&kind) {
                continue;
            }
            let url = c["url"].as_str().unwrap_or_default().to_owned();
            let usage = st.loader.usage_get(&url).await;
            entries.push((
                url,
                c["localId"].as_str().unwrap_or_default().to_owned(),
                kind,
                c["createdAt"].as_str().unwrap_or_default().to_owned(),
                usage,
            ));
        }
        if keep("Cached") {
            for u in st.loader.usage_list().await {
                if u.url.contains("/ngsi-ld/v1/jsonldContexts/") {
                    continue; // broker-local (hosted/implicit) URLs
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
        let entry = find_entry(&st, &id)
            .await
            .ok_or_else(|| NgsiError::ResourceNotFound(format!("@context {id} not found")))?;
        let payload = match &entry {
            CtxEntry::Stored(doc) => {
                let kind = doc["kind"].as_str().unwrap_or("Hosted");
                let url = doc["url"].as_str().unwrap_or_default();
                if details {
                    let usage = st.loader.usage_get(url).await;
                    details_obj(
                        url,
                        doc["localId"].as_str().unwrap_or_default(),
                        kind,
                        doc["createdAt"].as_str().unwrap_or_default(),
                        usage.as_ref(),
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
        // the value shown in this response (053_06/053_08 arithmetic)
        match &entry {
            CtxEntry::Stored(doc) if doc["kind"] == "ImplicitlyCreated" => {
                if let Some(u) = doc["url"].as_str() {
                    st.loader.bump_url(u).await;
                }
            }
            CtxEntry::Cached(u) => st.loader.bump_url(&u.url).await,
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
        let entry = find_entry(&st, &id).await;
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
                // J2 write-through row shares the deterministic local id —
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
