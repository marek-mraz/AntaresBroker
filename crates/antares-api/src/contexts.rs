//! /jsonldContexts management (5.13; resources 6.29/6.30).

use crate::negotiate::*;
use crate::state::{now_iso, AppState};
use antares_model::NgsiError;
use axum::body::Bytes;
use axum::extract::{Path, Query, State};
use axum::http::{header, HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use serde_json::{json, Value};
use std::collections::HashMap;

type Params = Query<HashMap<String, String>>;

fn base_url(headers: &HeaderMap) -> String {
    let host = headers
        .get(header::HOST)
        .and_then(|h| h.to_str().ok())
        .unwrap_or("localhost:9090");
    format!("http://{host}/ngsi-ld/v1/jsonldContexts")
}

// ---------- POST /jsonldContexts (5.13.2) ----------

pub async fn add_context(
    State(st): State<AppState>,
    Query(params): Params,
    headers: HeaderMap,
    body: Bytes,
) -> Response {
    let go = async {
        let tenant = tenant_from(&headers)?;
        check_params(&params, &["local"])?;
        let ct = content_type(&headers);
        if ct != "application/ld+json" && ct != "application/json" {
            return Err(ApiError::Bare(StatusCode::UNSUPPORTED_MEDIA_TYPE));
        }
        let value: Value = serde_json::from_slice(&body)
            .map_err(|e| NgsiError::InvalidRequest(format!("body is not valid JSON: {e}")))?;
        let ctx_val = value
            .get("@context")
            .cloned()
            .ok_or_else(|| NgsiError::BadRequestData("body must carry an @context member".into()))?;
        let local_id = uuid::Uuid::new_v4().to_string();
        let url = format!("{}/{local_id}", base_url(&headers));
        let doc = json!({
            "url": url,
            "localId": local_id,
            "kind": "Hosted",
            "createdAt": now_iso(),
            "body": {"@context": ctx_val.clone()},
        });
        st.store.context_put(&local_id, doc);
        st.loader.put_local(url.clone(), ctx_val).await;
        let mut resp =
            (StatusCode::CREATED, [(header::LOCATION, format!("/ngsi-ld/v1/jsonldContexts/{local_id}"))])
                .into_response();
        echo_tenant(&tenant, &mut resp);
        Ok::<_, ApiError>(resp)
    };
    go.await.unwrap_or_else(|e| e.into_response())
}

// ---------- GET /jsonldContexts (5.13.3) ----------

pub async fn list_contexts(
    State(st): State<AppState>,
    Query(params): Params,
    headers: HeaderMap,
) -> Response {
    let go = async {
        let tenant = tenant_from(&headers)?;
        check_params(&params, &["kind", "details", "local", "limit", "offset", "count"])?;
        let details = params.get("details").map(String::as_str) == Some("true");
        let kind_filter = params.get("kind");
        if let Some(k) = kind_filter {
            if !["Hosted", "Cached", "ImplicitlyCreated"].contains(&k.as_str()) {
                return Err(NgsiError::BadRequestData(format!("invalid kind {k:?}")).into());
            }
        }
        let all = st.store.context_list();
        let filtered: Vec<Value> = all
            .into_iter()
            .filter(|c| {
                kind_filter.is_none_or(|k| c.get("kind").and_then(Value::as_str) == Some(k))
            })
            .collect();
        let payload = if details {
            Value::Array(
                filtered
                    .iter()
                    .map(|c| {
                        json!({
                            "URL": c["url"],
                            "kind": c["kind"],
                            "createdAt": c["createdAt"],
                        })
                    })
                    .collect(),
            )
        } else {
            Value::Array(filtered.iter().map(|c| c["url"].clone()).collect())
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
    Query(params): Params,
    headers: HeaderMap,
) -> Response {
    let go = async {
        let tenant = tenant_from(&headers)?;
        check_params(&params, &["details", "local"])?;
        let doc = st
            .store
            .context_get(&id)
            .ok_or_else(|| NgsiError::ResourceNotFound(format!("@context {id} not found")))?;
        let details = params.get("details").map(String::as_str) == Some("true");
        let (ct, payload) = if details {
            (
                "application/json",
                json!({
                    "URL": doc["url"],
                    "kind": doc["kind"],
                    "createdAt": doc["createdAt"],
                }),
            )
        } else {
            ("application/ld+json", doc["body"].clone())
        };
        let mut resp = (
            StatusCode::OK,
            [(header::CONTENT_TYPE, ct)],
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
    Query(params): Params,
    headers: HeaderMap,
) -> Response {
    let go = async {
        let tenant = tenant_from(&headers)?;
        check_params(&params, &["reload", "local"])?;
        let doc = st
            .store
            .context_get(&id)
            .ok_or_else(|| NgsiError::ResourceNotFound(format!("@context {id} not found")))?;
        st.store.context_delete(&id);
        if let Some(url) = doc.get("url").and_then(Value::as_str) {
            st.loader.evict(url).await;
        }
        Ok::<_, ApiError>(no_content(&tenant))
    };
    go.await.unwrap_or_else(|e| e.into_response())
}
