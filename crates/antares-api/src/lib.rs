//! NGSI-LD HTTP binding (docs/deep-analysis.md §9.3): axum routers, thin
//! handlers per spec operation.
#![cfg_attr(test, allow(clippy::unwrap_used))]

pub mod attrs;
pub mod batch;
pub mod bounds;
pub mod conformance;
pub mod contexts;
pub mod csource;
pub mod egress;
pub mod entities;
pub mod federation;
pub mod geo;
pub mod negotiate;
pub mod notify;
pub mod qeval;
pub mod repr;
pub mod state;
pub mod subscriptions;
pub mod temporal;
pub mod types_attrs;

pub use state::AppState;

/// N2: spawn for both targets — tokio natively, the JS microtask queue on
/// wasm32 (no tokio runtime exists in a browser). Call sites are identical;
/// only the executor differs. Send is required natively (worker threads) and
/// meaningless on single-threaded wasm.
#[cfg(not(target_arch = "wasm32"))]
pub(crate) fn spawn<F>(fut: F)
where
    F: std::future::Future<Output = ()> + Send + 'static,
{
    tokio::spawn(fut);
}

#[cfg(target_arch = "wasm32")]
pub(crate) fn spawn<F>(fut: F)
where
    F: std::future::Future<Output = ()> + 'static,
{
    wasm_bindgen_futures::spawn_local(fut);
}

/// N3 (wasm only): a page-registered notification sink. A browser page has no
/// inbound socket to receive notification callbacks on, so a subscription
/// whose endpoint matches the registered URL prefix is delivered to page JS
/// instead of the network. Endpoints outside the prefix still leave via
/// fetch — the N7a Node tier registers nothing and keeps pure HTTP delivery.
#[cfg(target_arch = "wasm32")]
pub mod page_sink {
    use std::sync::OnceLock;

    type Hook = (String, Box<dyn Fn(&str, &[u8]) -> bool + Send + Sync>);
    static HOOK: OnceLock<Hook> = OnceLock::new();

    /// Register the sink (once per module instance).
    pub fn set(prefix: String, h: Box<dyn Fn(&str, &[u8]) -> bool + Send + Sync>) {
        let _ = HOOK.set((prefix, h));
    }

    /// True when the page sink claimed (and thus delivered) this endpoint.
    pub fn try_deliver(url: &str, body: &[u8]) -> bool {
        match HOOK.get() {
            Some((prefix, h)) if url.starts_with(prefix.as_str()) => h(url, body),
            _ => false,
        }
    }
}

use antares_model::{NgsiError, API_ROOT};
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::{delete, get, patch, post};
use axum::Router;
use negotiate::{echo_tenant, respond, tenant_from, Accept, ApiError};
use serde_json::Value;

/// Instance members that are NOT sub-attributes (shared list).
pub(crate) fn repr_reserved(k: &str) -> bool {
    matches!(
        k,
        "type"
            | "value"
            | "object"
            | "objectType"
            | "datasetId"
            | "observedAt"
            | "unitCode"
            | "lang"
            | "languageMap"
            | "vocab"
            | "json"
            | "valueList"
            | "objectList"
            | "createdAt"
            | "modifiedAt"
            | "deletedAt"
            | "instanceId"
            | "previousValue"
            | "previousObject"
            | "previousLanguageMap"
    )
}

/// Scope Query evaluation (4.19) — `,`=OR `;`=AND, `+` one level, `#` subtree.
pub fn scope_matches(scope_q: &str, doc: &Value) -> bool {
    let scopes: Vec<&str> = doc
        .get("scope")
        .and_then(Value::as_array)
        .map(|a| a.iter().filter_map(Value::as_str).collect())
        .unwrap_or_default();
    scope_q.split(',').any(|and_group| {
        and_group
            .split(';')
            .all(|pat| scopes.iter().any(|s| scope_pattern_matches(pat.trim(), s)))
    })
}

fn scope_pattern_matches(pat: &str, scope: &str) -> bool {
    if pat == "/#" {
        return true;
    }
    let pseg: Vec<&str> = pat.split('/').filter(|s| !s.is_empty()).collect();
    let sseg: Vec<&str> = scope.split('/').filter(|s| !s.is_empty()).collect();
    let mut i = 0;
    for (pi, p) in pseg.iter().enumerate() {
        if *p == "#" {
            // multi-level wildcard: matches the rest (including nothing)
            return pi == pseg.len() - 1;
        }
        let Some(sv) = sseg.get(i) else { return false };
        if *p != "+" && p != sv {
            return false;
        }
        i += 1;
    }
    i == sseg.len()
}

pub fn router(state: AppState) -> Router {
    let api = Router::new()
        // entities (6.4/6.5)
        .route(
            "/entities",
            post(entities::create_entity)
                .get(entities::query_entities)
                .delete(entities::purge_entities)
                .patch(missing_entity_id)
                .put(missing_entity_id),
        )
        .route(
            "/entities/{id}",
            get(entities::retrieve_entity)
                .patch(entities::merge_entity)
                .put(entities::replace_entity)
                .delete(entities::delete_entity),
        )
        // attrs (6.6/6.7)
        .route(
            "/entities/{id}/attrs",
            post(attrs::append_attrs).patch(attrs::update_attrs),
        )
        .route(
            "/entities/{id}/attrs/{attr}",
            // GET is the 2.0 #14 pre-adoption (H3, §15.1)
            get(entities::retrieve_entity_attr)
                .patch(attrs::partial_update_attr)
                .put(attrs::replace_attr)
                .delete(attrs::delete_attr),
        )
        // 2.0 #15 pre-adoption (H3): the bare attribute value
        .route(
            "/entities/{id}/attrs/{attr}/value",
            get(entities::retrieve_entity_attr_value),
        )
        // batch (6.14–6.17, 6.23, 6.31)
        .route("/entityOperations/create", post(batch::batch_create))
        .route("/entityOperations/upsert", post(batch::batch_upsert))
        .route("/entityOperations/update", post(batch::batch_update))
        .route("/entityOperations/delete", post(batch::batch_delete))
        .route("/entityOperations/merge", post(batch::batch_merge))
        .route("/entityOperations/query", post(batch::batch_query))
        // subscriptions (6.10/6.11)
        .route(
            "/subscriptions",
            post(subscriptions::create_subscription).get(subscriptions::query_subscriptions),
        )
        .route(
            "/subscriptions/{id}",
            get(subscriptions::retrieve_subscription)
                .patch(subscriptions::update_subscription)
                .delete(subscriptions::delete_subscription),
        )
        // csourceRegistrations (6.8/6.9)
        .route(
            "/csourceRegistrations",
            post(csource::create_registration).get(csource::query_registrations),
        )
        .route(
            "/csourceRegistrations/{id}",
            get(csource::retrieve_registration)
                .patch(csource::update_registration)
                .delete(csource::delete_registration),
        )
        // csourceSubscriptions (6.12/6.13)
        .route(
            "/csourceSubscriptions",
            post(subscriptions::create_csource_subscription)
                .get(subscriptions::query_csource_subscriptions),
        )
        .route(
            "/csourceSubscriptions/{id}",
            get(subscriptions::retrieve_csource_subscription)
                .patch(subscriptions::update_csource_subscription)
                .delete(subscriptions::delete_csource_subscription),
        )
        // temporal (6.18–6.22, 6.24)
        .route(
            "/temporal/entities",
            post(temporal::upsert_temporal).get(temporal::query_temporal),
        )
        .route(
            "/temporal/entities/{id}",
            get(temporal::retrieve_temporal).delete(temporal::delete_temporal),
        )
        .route(
            "/temporal/entities/{id}/attrs",
            post(temporal::add_temporal_attrs),
        )
        .route(
            "/temporal/entities/{id}/attrs/{attr}",
            delete(temporal::delete_temporal_attr),
        )
        .route(
            "/temporal/entities/{id}/attrs/{attr}/{instance}",
            patch(temporal::modify_temporal_instance).delete(temporal::delete_temporal_instance),
        )
        .route(
            "/temporal/entityOperations/query",
            post(temporal::batch_temporal_query),
        )
        // discovery (6.25–6.28)
        .route("/types", get(types_attrs::entity_types))
        .route("/types/{type}", get(types_attrs::entity_type_info))
        .route("/attributes", get(types_attrs::attributes))
        .route("/attributes/{attr}", get(types_attrs::attribute_info))
        // jsonldContexts (6.29/6.30)
        .route(
            "/jsonldContexts",
            post(contexts::add_context).get(contexts::list_contexts),
        )
        .route(
            "/jsonldContexts/{id}",
            get(contexts::serve_context).delete(contexts::delete_context),
        )
        // info (6.33)
        .route("/info/sourceIdentity", get(source_identity));

    Router::new()
        .route("/q/health", get(health))
        // K12: Prometheus text format. 404 until the broker installs the
        // renderer — the api crate never depends on an exporter (§9.2).
        .route("/q/metrics", get(metrics_endpoint))
        // 6.3.6/6.3.21: Prefer: ngsi-ld=<version> → 4.3.6.8 amendment +
        // Preference-Applied (+203 when altered) on every API response.
        // OPTIONS (2.0 #59 pre-adoption, H3): axum's MethodRouter already
        // computes the exact per-route Allow set for its 405s — the layer
        // turns an OPTIONS 405 into 204 + that same Allow. HEAD (#58) needs
        // nothing: axum's get() serves HEAD natively.
        .nest(
            API_ROOT,
            api.layer(axum::middleware::from_fn(conformance::prefer_version_layer))
                // I2 bounds wall: URI length, body size, JSON depth — checked
                // before any parse (size-check-before-parse, WS-44 class).
                .layer(axum::middleware::from_fn_with_state(
                    state.clone(),
                    bounds::bounds_layer,
                ))
                // axum's built-in extractor limit defaults to 2 MiB and fires
                // BEFORE the bounds wall's documented cap — a 3 MiB body was
                // 413'd although /q/health advertises maxBodyBytes = 4 MiB
                // (found by mutation-testing the 413 wall, 2026-08-09). One
                // number governs both walls.
                .layer(axum::extract::DefaultBodyLimit::max(bounds::MAX_BODY_BYTES)),
        )
        .fallback(not_found)
        // outermost on purpose: axum attaches the 405 Allow header in the
        // Router itself, above any nested layer — only a layer wrapping the
        // WHOLE router sees it.
        .layer(axum::middleware::from_fn(options_204))
        // K12: outermost so the duration covers the full stack.
        .layer(axum::middleware::from_fn(http_metrics_layer))
        .with_state(state)
}

/// K12: /q/metrics — Prometheus exposition, rendered by the closure the
/// broker installed; 404 when no recorder exists (tests, embedded builds).
async fn metrics_endpoint(axum::extract::State(state): axum::extract::State<AppState>) -> Response {
    match &state.metrics_render {
        Some(render) => Response::builder()
            .status(axum::http::StatusCode::OK)
            .header(
                axum::http::header::CONTENT_TYPE,
                "text/plain; version=0.0.4",
            )
            .body(axum::body::Body::from(render()))
            .unwrap_or_else(|_| axum::http::StatusCode::INTERNAL_SERVER_ERROR.into_response()),
        None => axum::http::StatusCode::NOT_FOUND.into_response(),
    }
}

/// K12: request counter + duration histogram, labelled by method and status
/// class only (bounded cardinality — §16.1.7).
async fn http_metrics_layer(
    req: axum::http::Request<axum::body::Body>,
    next: axum::middleware::Next,
) -> Response {
    let method = req.method().as_str().to_owned();
    // N2 clock rule: std Instant panics on wasm32.
    #[cfg(not(target_arch = "wasm32"))]
    let start = std::time::Instant::now();
    #[cfg(target_arch = "wasm32")]
    let start = web_time::Instant::now();
    let resp = next.run(req).await;
    let class = match resp.status().as_u16() {
        100..=199 => "1xx",
        200..=299 => "2xx",
        300..=399 => "3xx",
        400..=499 => "4xx",
        _ => "5xx",
    };
    metrics::counter!("antares_http_requests_total", "method" => method.clone(), "status" => class)
        .increment(1);
    metrics::histogram!("antares_http_request_duration_seconds", "method" => method)
        .record(start.elapsed().as_secs_f64());
    resp
}

/// 2.0 #59 pre-adoption (H3): OPTIONS → 204 No Content + the Allow set the
/// method router computed. Non-OPTIONS traffic passes through untouched.
async fn options_204(
    req: axum::http::Request<axum::body::Body>,
    next: axum::middleware::Next,
) -> Response {
    let is_options = req.method() == axum::http::Method::OPTIONS;
    let resp = next.run(req).await;
    if is_options && resp.status() == axum::http::StatusCode::METHOD_NOT_ALLOWED {
        // Keep the response PARTS: axum carries the computed Allow set as a
        // response extension and turns it into the header above all layers —
        // preserving the extensions preserves the Allow header.
        let (mut parts, _) = resp.into_parts();
        parts.status = axum::http::StatusCode::NO_CONTENT;
        return Response::from_parts(parts, axum::body::Body::empty());
    }
    resp
}

async fn health(
    axum::extract::State(state): axum::extract::State<AppState>,
) -> (StatusCode, axum::Json<serde_json::Value>) {
    // K1 drain: the load balancer decides on the STATUS CODE, so draining has
    // to be a 503 — a 200 body saying "DRAINING" would keep traffic arriving.
    // This flips BEFORE the listener stops accepting, which is the whole point
    // of the ordering: the LB must stop routing while the socket still works.
    let draining = state.draining.load(std::sync::atomic::Ordering::Relaxed);
    let code = if draining {
        StatusCode::SERVICE_UNAVAILABLE
    } else {
        StatusCode::OK
    };
    let mut body = serde_json::json!({
        "status": if draining { "DRAINING" } else { "UP" },
        "store": state.store_mode.as_str(),
    });
    // B13: in `file` mode commits serialize behind one writer — the queue
    // depth (current, peak) is the signal that decides the group-commit lever.
    if state.store_mode == antares_sql::StoreMode::File {
        if let Some((depth, peak)) = state.store.commit_queue() {
            body["commitQueueDepth"] = depth.into();
            body["commitQueuePeak"] = peak.into();
        }
    }
    // I2: configured caps + rejection counters (§16.3 observability).
    body["limits"] = state.limits.snapshot();
    // J7: jemalloc heap stats (RSS ≈ live×1.2 is the §2.1 target).
    if let Some(mem) = &state.mem_stats {
        body["memory"] = mem();
    }
    (code, axum::Json(body))
}

/// CIM 009 5.15.1 / 6.33 — Context Source identity.
async fn source_identity(
    axum::extract::State(state): axum::extract::State<AppState>,
    headers: HeaderMap,
) -> Response {
    let go = async {
        let tenant = tenant_from(&headers)?;
        let accept = negotiate::parse_accept(&headers)?;
        let ctx = state.loader.core();
        let uptime = state.started.elapsed().as_secs();
        // Table 5.2.40-1: contextSourceAlias, contextSourceUptime and
        // contextSourceTimeAt are all cardinality 1. `hostAlias`/`uptime` were
        // not spec members at all — neither expands to an NGSI-LD IRI, so the
        // payload was not valid JSON-LD against the core context either.
        let body = serde_json::json!({
            "id": format!("urn:ngsi-ld:ContextSourceIdentity:{}", state.host_alias),
            "type": "ContextSourceIdentity",
            "contextSourceAlias": state.host_alias,
            "contextSourceUptime": format!("PT{uptime}S"),
            "contextSourceTimeAt": crate::state::now_iso(),
        });
        Ok::<_, ApiError>(respond(
            axum::http::StatusCode::OK,
            body,
            &ctx,
            accept,
            &tenant,
        ))
    };
    go.await.unwrap_or_else(|e| e.into_response())
}

/// PATCH/PUT on the entities collection: the entity id is missing — 400.
async fn missing_entity_id(headers: HeaderMap) -> Response {
    let tenant = tenant_from(&headers).unwrap_or_default();
    let mut resp = ApiError::from(NgsiError::BadRequestData(
        "entity id is required in the request path".into(),
    ))
    .into_response();
    echo_tenant(&tenant, &mut resp);
    resp
}

async fn not_found(headers: HeaderMap, uri: axum::http::Uri) -> Response {
    let path = uri.path().to_owned();
    let tenant = tenant_from(&headers).unwrap_or_default();
    // A path with an empty segment (…/attrs//{x}) names a resource whose
    // methods don't apply — 405 per the suite (016_02_04/06).
    if path.starts_with(API_ROOT) && path.contains("//") {
        let mut resp = axum::http::StatusCode::METHOD_NOT_ALLOWED.into_response();
        echo_tenant(&tenant, &mut resp);
        return resp;
    }
    let mut resp =
        ApiError::from(NgsiError::ResourceNotFound(format!("unknown path {path}"))).into_response();
    echo_tenant(&tenant, &mut resp);
    resp
}

// keep the Accept variants referenced (geo+json handled in entities)
#[allow(dead_code)]
fn _accept_variants(a: Accept) -> Accept {
    a
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::body::Body;
    use axum::http::{Request, StatusCode};
    use http_body_util::BodyExt;
    use tower::ServiceExt;

    fn app() -> Router {
        router(AppState::new("antares-test".into()))
    }

    async fn body_json(resp: Response) -> serde_json::Value {
        let bytes = resp.into_body().collect().await.expect("body").to_bytes();
        serde_json::from_slice(&bytes).expect("json body")
    }

    #[tokio::test]
    async fn health_is_up() {
        let resp = app()
            .oneshot(Request::get("/q/health").body(Body::empty()).expect("req"))
            .await
            .expect("resp");
        assert_eq!(resp.status(), StatusCode::OK);
        assert_eq!(body_json(resp).await["status"], "UP");
    }

    #[tokio::test]
    async fn prefer_ngsild_version_downgrades_and_203s() {
        // 6.3.6/6.3.21 + 4.3.6.8: Prefer: ngsi-ld=1.4 on a retrieve of an
        // entity holding a 1.8-era attribute type → amended payload,
        // Preference-Applied echo, 203 Non-Authoritative.
        let app = app();
        let entity = serde_json::json!({
            "id": "urn:ngsi-ld:Building:pref1",
            "type": "Building",
            "spec": {"type": "JsonProperty", "json": {"k": 1}}
        });
        let resp = app
            .clone()
            .oneshot(
                Request::post("/ngsi-ld/v1/entities")
                    .header("Content-Type", "application/json")
                    .header("Content-Length", (entity.to_string()).len())
                    .body(Body::from(entity.to_string()))
                    .expect("req"),
            )
            .await
            .expect("resp");
        assert_eq!(resp.status(), StatusCode::CREATED);

        let resp = app
            .clone()
            .oneshot(
                Request::get("/ngsi-ld/v1/entities/urn:ngsi-ld:Building:pref1")
                    .header("Prefer", "ngsi-ld=1.4")
                    .body(Body::empty())
                    .expect("req"),
            )
            .await
            .expect("resp");
        assert_eq!(resp.status(), StatusCode::NON_AUTHORITATIVE_INFORMATION);
        assert_eq!(
            resp.headers()
                .get("Preference-Applied")
                .map(|v| v.to_str().unwrap()),
            Some("ngsi-ld=1.4")
        );
        let doc = body_json(resp).await;
        assert_eq!(
            doc["spec"],
            serde_json::json!({"type": "Property", "value": {"k": 1}})
        );

        // Native-version preference: applied header, payload untouched, 200.
        let resp = app
            .clone()
            .oneshot(
                Request::get("/ngsi-ld/v1/entities/urn:ngsi-ld:Building:pref1")
                    .header("Prefer", "ngsi-ld=1.9")
                    .body(Body::empty())
                    .expect("req"),
            )
            .await
            .expect("resp");
        assert_eq!(resp.status(), StatusCode::OK);
        assert_eq!(
            resp.headers()
                .get("Preference-Applied")
                .map(|v| v.to_str().unwrap()),
            Some("ngsi-ld=1.9")
        );
    }

    #[tokio::test]
    async fn i2_bounds_wall_rejects_spec_shaped() {
        // §16.3: every cap answers with the spec error, before any parse.
        let app = app();

        // JSON nesting > 64 → 400 BadRequestData
        let deep = format!(
            r#"{{"id":"urn:x:1","type":"T","a":{}1{}}}"#,
            "[".repeat(70),
            "]".repeat(70)
        );
        let resp = app
            .clone()
            .oneshot(
                Request::post("/ngsi-ld/v1/entities")
                    .header("Content-Type", "application/json")
                    .header("Content-Length", (deep).len())
                    .body(Body::from(deep))
                    .expect("req"),
            )
            .await
            .expect("resp");
        assert_eq!(resp.status(), StatusCode::BAD_REQUEST);

        // URI too long → bare 414
        let resp = app
            .clone()
            .oneshot(
                Request::get(format!("/ngsi-ld/v1/entities?q={}", "a".repeat(9000)))
                    .body(Body::empty())
                    .expect("req"),
            )
            .await
            .expect("resp");
        assert_eq!(resp.status(), StatusCode::URI_TOO_LONG);

        // body over 4 MiB → bare 413
        let resp = app
            .clone()
            .oneshot(
                Request::post("/ngsi-ld/v1/entities")
                    .header("Content-Type", "application/json")
                    .header(
                        "Content-Length",
                        ("x".repeat(bounds::MAX_BODY_BYTES + 1)).len(),
                    )
                    .body(Body::from("x".repeat(bounds::MAX_BODY_BYTES + 1)))
                    .expect("req"),
            )
            .await
            .expect("resp");
        assert_eq!(resp.status(), StatusCode::PAYLOAD_TOO_LARGE);

        // limit above the ceiling → 403 TooManyResults
        let resp = app
            .clone()
            .oneshot(
                Request::get("/ngsi-ld/v1/entities?type=T&limit=99999")
                    .body(Body::empty())
                    .expect("req"),
            )
            .await
            .expect("resp");
        assert_eq!(resp.status(), StatusCode::FORBIDDEN);
        let doc = body_json(resp).await;
        assert!(doc["type"]
            .as_str()
            .expect("type")
            .ends_with("TooManyResults"));

        // batch above the item cap → 400
        let big: Vec<serde_json::Value> = (0..bounds::MAX_BATCH_ITEMS + 1)
            .map(|i| serde_json::json!({"id": format!("urn:b:{i}"), "type": "T"}))
            .collect();
        let resp = app
            .clone()
            .oneshot(
                Request::post("/ngsi-ld/v1/entityOperations/create")
                    .header("Content-Type", "application/json")
                    .header(
                        "Content-Length",
                        (serde_json::to_vec(&big).expect("json")).len(),
                    )
                    .body(Body::from(serde_json::to_vec(&big).expect("json")))
                    .expect("req"),
            )
            .await
            .expect("resp");
        assert_eq!(resp.status(), StatusCode::BAD_REQUEST);

        // joinLevel above the cap → 400
        let resp = app
            .clone()
            .oneshot(
                Request::get("/ngsi-ld/v1/entities?type=T&join=inline&joinLevel=99")
                    .body(Body::empty())
                    .expect("req"),
            )
            .await
            .expect("resp");
        assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn h3_preadoptions_attr_get_value_options_head() {
        // 2.0 pre-adoptions (§15.1, tasks.md H3): #14 GET .../attrs/{attrId},
        // #15 .../value, #58 HEAD, #59 OPTIONS with the route's Allow set.
        let app = app();
        let entity = serde_json::json!({
            "id": "urn:ngsi-ld:Building:h3", "type": "Building",
            "name": {"type": "Property", "value": "Hala"}
        });
        let resp = app
            .clone()
            .oneshot(
                Request::post("/ngsi-ld/v1/entities")
                    .header("Content-Type", "application/json")
                    .header("Content-Length", (entity.to_string()).len())
                    .body(Body::from(entity.to_string()))
                    .expect("req"),
            )
            .await
            .expect("resp");
        assert_eq!(resp.status(), StatusCode::CREATED);

        let resp = app
            .clone()
            .oneshot(
                Request::get("/ngsi-ld/v1/entities/urn:ngsi-ld:Building:h3/attrs/name")
                    .body(Body::empty())
                    .expect("req"),
            )
            .await
            .expect("resp");
        assert_eq!(resp.status(), StatusCode::OK);
        let doc = body_json(resp).await;
        assert_eq!(doc["type"], "Property");
        assert_eq!(doc["value"], "Hala");

        let resp = app
            .clone()
            .oneshot(
                Request::get("/ngsi-ld/v1/entities/urn:ngsi-ld:Building:h3/attrs/name/value")
                    .body(Body::empty())
                    .expect("req"),
            )
            .await
            .expect("resp");
        assert_eq!(resp.status(), StatusCode::OK);
        assert_eq!(body_json(resp).await, serde_json::json!("Hala"));

        // absent attribute → 404 ResourceNotFound
        let resp = app
            .clone()
            .oneshot(
                Request::get("/ngsi-ld/v1/entities/urn:ngsi-ld:Building:h3/attrs/nope")
                    .body(Body::empty())
                    .expect("req"),
            )
            .await
            .expect("resp");
        assert_eq!(resp.status(), StatusCode::NOT_FOUND);

        // #59: OPTIONS answers 204 + Allow computed from the route
        let resp = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("OPTIONS")
                    .uri("/ngsi-ld/v1/entities")
                    .body(Body::empty())
                    .expect("req"),
            )
            .await
            .expect("resp");
        assert_eq!(resp.status(), StatusCode::NO_CONTENT);
        let allow = resp
            .headers()
            .get("allow")
            .expect("Allow")
            .to_str()
            .expect("str");
        assert!(
            allow.contains("GET") && allow.contains("POST"),
            "Allow: {allow}"
        );

        // #58: HEAD serves like GET, no body needed
        let resp = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("HEAD")
                    .uri("/ngsi-ld/v1/entities/urn:ngsi-ld:Building:h3")
                    .body(Body::empty())
                    .expect("req"),
            )
            .await
            .expect("resp");
        assert_eq!(resp.status(), StatusCode::OK);
    }

    /// Security audit C2 (2026-08-04): `information[].entities ×
    /// (propertyNames + relationshipNames)` was expanded into an in-memory Vec
    /// before any SQL ran, with no cardinality cap — a 4 MiB body produced on
    /// the order of 10^10 objects and OOM-killed the process. Capped at the
    /// validation boundary so it is a 403, not a dead pod.
    #[tokio::test]
    async fn registration_cardinality_is_capped_before_expansion() {
        let app = app();
        let entities: Vec<serde_json::Value> = (0..600)
            .map(|i| serde_json::json!({"id": format!("urn:e:{i}")}))
            .collect();
        let props: Vec<String> = (0..600).map(|i| format!("p{i}")).collect();
        let reg = serde_json::json!({
            "type": "ContextSourceRegistration",
            "endpoint": "http://peer.example/ngsi-ld/v1",
            "information": [{"entities": entities, "propertyNames": props}]
        });
        let resp = app
            .clone()
            .oneshot(
                Request::post("/ngsi-ld/v1/csourceRegistrations")
                    .header("Content-Type", "application/json")
                    .header("Content-Length", (reg.to_string()).len())
                    .body(Body::from(reg.to_string()))
                    .expect("req"),
            )
            .await
            .expect("resp");
        assert_eq!(
            resp.status(),
            StatusCode::FORBIDDEN,
            "TooComplexQuery = 403"
        );

        // a registration of ordinary size is untouched by the cap
        let reg = serde_json::json!({
            "type": "ContextSourceRegistration",
            "endpoint": "http://peer.example/ngsi-ld/v1",
            "information": [{"entities": [{"type": "Vehicle"}],
                             "propertyNames": ["speed", "heading"]}]
        });
        let resp = app
            .clone()
            .oneshot(
                Request::post("/ngsi-ld/v1/csourceRegistrations")
                    .header("Content-Type", "application/json")
                    .header("Content-Length", (reg.to_string()).len())
                    .body(Body::from(reg.to_string()))
                    .expect("req"),
            )
            .await
            .expect("resp");
        assert_eq!(resp.status(), StatusCode::CREATED);
    }

    #[tokio::test]
    async fn tolerant_reader_echoes_unknown_members() {
        // §15.1: unknown members of Subscription/Registration documents are
        // stored and echoed, never rejected or stripped — a member added by a
        // future spec version flows through a broker that predates it (H5).
        let app = app();
        let sub = serde_json::json!({
            "id": "urn:ngsi-ld:Subscription:tol1", "type": "Subscription",
            "entities": [{"type": "Building"}],
            "notification": {"endpoint": {"uri": "http://localhost:1/x"}},
            "futureMember": {"nested": [1, 2, 3]}
        });
        let resp = app
            .clone()
            .oneshot(
                Request::post("/ngsi-ld/v1/subscriptions")
                    .header("Content-Type", "application/json")
                    .header("Content-Length", (sub.to_string()).len())
                    .body(Body::from(sub.to_string()))
                    .expect("req"),
            )
            .await
            .expect("resp");
        assert_eq!(resp.status(), StatusCode::CREATED);
        let resp = app
            .clone()
            .oneshot(
                Request::get("/ngsi-ld/v1/subscriptions/urn:ngsi-ld:Subscription:tol1")
                    .body(Body::empty())
                    .expect("req"),
            )
            .await
            .expect("resp");
        assert_eq!(resp.status(), StatusCode::OK);
        let doc = body_json(resp).await;
        assert_eq!(
            doc["futureMember"],
            serde_json::json!({"nested": [1, 2, 3]})
        );

        let reg = serde_json::json!({
            "id": "urn:ngsi-ld:ContextSourceRegistration:tol1",
            "type": "ContextSourceRegistration",
            "information": [{"entities": [{"type": "Building"}]}],
            "endpoint": "http://localhost:1/csr",
            "futureMember": "kept"
        });
        let resp = app
            .clone()
            .oneshot(
                Request::post("/ngsi-ld/v1/csourceRegistrations")
                    .header("Content-Type", "application/json")
                    .header("Content-Length", (reg.to_string()).len())
                    .body(Body::from(reg.to_string()))
                    .expect("req"),
            )
            .await
            .expect("resp");
        assert_eq!(resp.status(), StatusCode::CREATED, "registration create");
        let resp = app
            .clone()
            .oneshot(
                Request::get(
                    "/ngsi-ld/v1/csourceRegistrations/urn:ngsi-ld:ContextSourceRegistration:tol1",
                )
                .body(Body::empty())
                .expect("req"),
            )
            .await
            .expect("resp");
        assert_eq!(resp.status(), StatusCode::OK);
        let doc = body_json(resp).await;
        assert_eq!(doc["futureMember"], "kept");
    }

    #[tokio::test]
    async fn entity_create_retrieve_delete_roundtrip() {
        let app = app();
        let entity = serde_json::json!({
            "id": "urn:ngsi-ld:Building:rt1",
            "type": "Building",
            "name": {"type": "Property", "value": "Eiffel Tower"}
        });
        let resp = app
            .clone()
            .oneshot(
                Request::post("/ngsi-ld/v1/entities")
                    .header("Content-Type", "application/json")
                    .header("Content-Length", (entity.to_string()).len())
                    .body(Body::from(entity.to_string()))
                    .expect("req"),
            )
            .await
            .expect("resp");
        assert_eq!(resp.status(), StatusCode::CREATED);
        assert_eq!(
            resp.headers().get("Location").map(|v| v.to_str().unwrap()),
            Some("/ngsi-ld/v1/entities/urn:ngsi-ld:Building:rt1")
        );

        // duplicate → 409
        let resp = app
            .clone()
            .oneshot(
                Request::post("/ngsi-ld/v1/entities")
                    .header("Content-Type", "application/json")
                    .header("Content-Length", (entity.to_string()).len())
                    .body(Body::from(entity.to_string()))
                    .expect("req"),
            )
            .await
            .expect("resp");
        assert_eq!(resp.status(), StatusCode::CONFLICT);

        let resp = app
            .clone()
            .oneshot(
                Request::get("/ngsi-ld/v1/entities/urn:ngsi-ld:Building:rt1")
                    .body(Body::empty())
                    .expect("req"),
            )
            .await
            .expect("resp");
        assert_eq!(resp.status(), StatusCode::OK);
        assert!(resp.headers().contains_key("Link"));
        let body = body_json(resp).await;
        assert_eq!(body["name"]["value"], "Eiffel Tower");
        assert!(body.get("createdAt").is_none(), "sysAttrs off by default");

        let resp = app
            .clone()
            .oneshot(
                Request::delete("/ngsi-ld/v1/entities/urn:ngsi-ld:Building:rt1")
                    .body(Body::empty())
                    .expect("req"),
            )
            .await
            .expect("resp");
        assert_eq!(resp.status(), StatusCode::NO_CONTENT);

        let resp = app
            .oneshot(
                Request::get("/ngsi-ld/v1/entities/urn:ngsi-ld:Building:rt1")
                    .body(Body::empty())
                    .expect("req"),
            )
            .await
            .expect("resp");
        assert_eq!(resp.status(), StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn query_requires_filter_and_unknown_param_is_400() {
        let resp = app()
            .oneshot(
                Request::get("/ngsi-ld/v1/entities")
                    .body(Body::empty())
                    .expect("req"),
            )
            .await
            .expect("resp");
        assert_eq!(resp.status(), StatusCode::BAD_REQUEST);

        let resp = app()
            .oneshot(
                Request::get("/ngsi-ld/v1/entities?invalidParams=x&type=Building")
                    .body(Body::empty())
                    .expect("req"),
            )
            .await
            .expect("resp");
        assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
        let body = body_json(resp).await;
        assert_eq!(
            body["type"],
            "https://uri.etsi.org/ngsi-ld/errors/InvalidRequest"
        );
    }

    #[tokio::test]
    async fn unsupported_media_type_and_accept() {
        let resp = app()
            .oneshot(
                Request::post("/ngsi-ld/v1/entities")
                    .header("Content-Type", "text/plain")
                    .header("Content-Length", ("x").len())
                    .body(Body::from("x"))
                    .expect("req"),
            )
            .await
            .expect("resp");
        assert_eq!(resp.status(), StatusCode::UNSUPPORTED_MEDIA_TYPE);

        let resp = app()
            .oneshot(
                Request::get("/ngsi-ld/v1/entities?type=Building")
                    .header("Accept", "text/csv")
                    .body(Body::empty())
                    .expect("req"),
            )
            .await
            .expect("resp");
        assert_eq!(resp.status(), StatusCode::NOT_ACCEPTABLE);
    }

    #[tokio::test]
    async fn tenant_header_is_echoed_and_validated() {
        let resp = app()
            .oneshot(
                Request::get("/ngsi-ld/v1/entities?type=Building")
                    .header("NGSILD-Tenant", "city-01")
                    .body(Body::empty())
                    .expect("req"),
            )
            .await
            .expect("resp");
        assert_eq!(
            resp.headers()
                .get("NGSILD-Tenant")
                .map(|v| v.to_str().expect("ascii")),
            Some("city-01")
        );
    }

    // ---- 5.6.21.4 Purge: the five qualifying conditions -------------------

    async fn create(app: &Router, id: &str, ty: &str) {
        let entity = serde_json::json!({"id": id, "type": ty,
            "name": {"type": "Property", "value": "x"}});
        let resp = app
            .clone()
            .oneshot(
                Request::post("/ngsi-ld/v1/entities")
                    .header("Content-Type", "application/json")
                    .header("Content-Length", (entity.to_string()).len())
                    .body(Body::from(entity.to_string()))
                    .expect("req"),
            )
            .await
            .expect("resp");
        assert_eq!(resp.status(), StatusCode::CREATED);
    }

    async fn purge(app: &Router, query: &str) -> StatusCode {
        app.clone()
            .oneshot(
                Request::delete(format!("/ngsi-ld/v1/entities?{query}"))
                    .body(Body::empty())
                    .expect("req"),
            )
            .await
            .expect("resp")
            .status()
    }

    #[tokio::test]
    async fn purge_rejects_id_and_idpattern_as_the_only_filter() {
        // 5.6.21.4: id/idPattern are legal input data (5.6.21.3) but are not
        // among the five qualifying conditions — "If none of the above is
        // provided, then an error of type BadRequestData shall be raised (too
        // wide query)". `idPattern=.*` alone used to delete the whole tenant.
        let app = app();
        create(&app, "urn:ngsi-ld:Building:purge1", "Building").await;

        assert_eq!(purge(&app, "idPattern=.%2A").await, StatusCode::BAD_REQUEST);
        assert_eq!(
            purge(&app, "id=urn:ngsi-ld:Building:purge1").await,
            StatusCode::BAD_REQUEST
        );

        // the entity is still there — the guard ran before any deletion
        let resp = app
            .clone()
            .oneshot(
                Request::get("/ngsi-ld/v1/entities/urn:ngsi-ld:Building:purge1")
                    .body(Body::empty())
                    .expect("req"),
            )
            .await
            .expect("resp");
        assert_eq!(resp.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn purge_requires_a_non_system_attribute_in_attrs_and_q() {
        // 5.6.21.4 b) and c): the Attribute list / query must include "at
        // least one non-system Attribute".
        let app = app();
        assert_eq!(
            purge(&app, "attrs=createdAt").await,
            StatusCode::BAD_REQUEST
        );
        assert_eq!(
            purge(&app, "q=modifiedAt%3E%222020-01-01T00:00:00Z%22").await,
            StatusCode::BAD_REQUEST
        );
        // a real attribute qualifies
        assert_ne!(purge(&app, "attrs=name").await, StatusCode::BAD_REQUEST);
        assert_ne!(
            purge(&app, "q=name%3D%3D%22x%22").await,
            StatusCode::BAD_REQUEST
        );
    }

    #[tokio::test]
    async fn purge_accepts_each_qualifying_condition() {
        // a) type, d) georel, e) local — none may 400 (5.6.21.4)
        let app = app();
        assert_ne!(purge(&app, "type=Building").await, StatusCode::BAD_REQUEST);
        assert_ne!(purge(&app, "local=true").await, StatusCode::BAD_REQUEST);
    }

    // ---- Table 6.4.3.2-1: type=* ------------------------------------------

    #[tokio::test]
    async fn type_wildcard_selects_every_type() {
        // "\"*\" is also allowed as a value and local is implicitly set to
        // true". Expanding "*" as a term produced an IRI nothing matched, so
        // the query returned 200 with an empty array.
        let app = app();
        create(&app, "urn:ngsi-ld:Building:star1", "Building").await;
        create(&app, "urn:ngsi-ld:Vehicle:star2", "Vehicle").await;

        let resp = app
            .clone()
            .oneshot(
                Request::get("/ngsi-ld/v1/entities?type=%2A")
                    .body(Body::empty())
                    .expect("req"),
            )
            .await
            .expect("resp");
        assert_eq!(resp.status(), StatusCode::OK);
        let docs = body_json(resp).await;
        let ids: Vec<&str> = docs
            .as_array()
            .expect("array")
            .iter()
            .filter_map(|d| d["id"].as_str())
            .collect();
        assert!(ids.contains(&"urn:ngsi-ld:Building:star1"), "got {ids:?}");
        assert!(ids.contains(&"urn:ngsi-ld:Vehicle:star2"), "got {ids:?}");
    }

    #[tokio::test]
    async fn type_wildcard_conflicts_with_explicit_local_false() {
        // "…and shall not be explicitly set to false" (Table 6.4.3.2-1)
        let app = app();
        let resp = app
            .oneshot(
                Request::get("/ngsi-ld/v1/entities?type=%2A&local=false")
                    .body(Body::empty())
                    .expect("req"),
            )
            .await
            .expect("resp");
        assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
    }

    // ---- 5.7.2.4 validation bullets ---------------------------------------

    #[tokio::test]
    async fn ordering_is_rejected_only_for_distributed_execution() {
        // 5.7.2.4: "If the ordering parameter is present and the execution of
        // the operation is not limited to the local scope … BadRequestData",
        // with 4.23.1 "Sort ordering is never applied to distributed
        // operations". The subject is the EXECUTION — a query no registration
        // matches runs locally whether or not the client passed local=true.
        // Reading it as "local=true is mandatory" fails ETSI 019_19, which
        // orders without it (error.md 2026-08-09).
        let app = app();

        // no registrations → local execution → ordering is fine
        for q in [
            "/ngsi-ld/v1/entities?type=Building&orderBy=name",
            "/ngsi-ld/v1/entities?type=Building&orderBy=name&local=true",
        ] {
            let resp = app
                .clone()
                .oneshot(Request::get(q).body(Body::empty()).expect("req"))
                .await
                .expect("resp");
            assert_eq!(resp.status(), StatusCode::OK, "{q}");
        }

        // a matching registration makes it a distributed operation
        let csr = serde_json::json!({
            "id": "urn:ngsi-ld:ContextSourceRegistration:ord1",
            "type": "ContextSourceRegistration",
            "information": [{"entities": [{"type": "Building"}]}],
            "endpoint": "http://peer.invalid:9090"
        });
        let resp = app
            .clone()
            .oneshot(
                Request::post("/ngsi-ld/v1/csourceRegistrations")
                    .header("Content-Type", "application/json")
                    .header("Content-Length", (csr.to_string()).len())
                    .body(Body::from(csr.to_string()))
                    .expect("req"),
            )
            .await
            .expect("resp");
        assert_eq!(resp.status(), StatusCode::CREATED);

        let resp = app
            .clone()
            .oneshot(
                Request::get("/ngsi-ld/v1/entities?type=Building&orderBy=name")
                    .body(Body::empty())
                    .expect("req"),
            )
            .await
            .expect("resp");
        assert_eq!(resp.status(), StatusCode::BAD_REQUEST);

        // …and local=true brings it back into scope
        let resp = app
            .oneshot(
                Request::get("/ngsi-ld/v1/entities?type=Building&orderBy=name&local=true")
                    .body(Body::empty())
                    .expect("req"),
            )
            .await
            .expect("resp");
        assert_eq!(resp.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn geometry_property_requires_geojson_accept() {
        // 5.7.2.4: "If geometryProperty parameter is present and the Accept
        // Header is not set to \"application/geo+json\" … BadRequestData"
        let app = app();
        let resp = app
            .clone()
            .oneshot(
                Request::get("/ngsi-ld/v1/entities?type=Building&geometryProperty=location")
                    .body(Body::empty())
                    .expect("req"),
            )
            .await
            .expect("resp");
        assert_eq!(resp.status(), StatusCode::BAD_REQUEST);

        let resp = app
            .oneshot(
                Request::get(
                    "/ngsi-ld/v1/entities?type=Building&geometryProperty=location&local=true",
                )
                .header("Accept", "application/geo+json")
                .body(Body::empty())
                .expect("req"),
            )
            .await
            .expect("resp");
        assert_eq!(resp.status(), StatusCode::OK);
    }

    // ---- 6.3.4: Content-Length precondition -------------------------------

    #[tokio::test]
    async fn missing_content_length_is_a_bare_411() {
        // 6.3.4: "For HTTP POST, PATCH and PUT HTTP requests implementations
        // shall check … Content-Length header shall include the length of the
        // request payload body", and its absence "shall result in just a 411
        // HTTP status code (without any payload body)". No exemption is given
        // for chunked transfer.
        let app = app();
        let entity = serde_json::json!({"id": "urn:ngsi-ld:Building:cl1", "type": "Building"});

        for (method, uri) in [
            ("POST", "/ngsi-ld/v1/entities"),
            (
                "PATCH",
                "/ngsi-ld/v1/entities/urn:ngsi-ld:Building:cl1/attrs",
            ),
            ("PUT", "/ngsi-ld/v1/entities/urn:ngsi-ld:Building:cl1"),
        ] {
            let resp = app
                .clone()
                .oneshot(
                    Request::builder()
                        .method(method)
                        .uri(uri)
                        .header("Content-Type", "application/json")
                        .body(Body::from(entity.to_string()))
                        .expect("req"),
                )
                .await
                .expect("resp");
            assert_eq!(resp.status(), StatusCode::LENGTH_REQUIRED, "{method} {uri}");
            let bytes = resp.into_body().collect().await.expect("body").to_bytes();
            assert!(bytes.is_empty(), "411 carries no payload body");
        }

        // GET/DELETE are outside the clause's scope
        let resp = app
            .oneshot(
                Request::get("/ngsi-ld/v1/entities?type=Building")
                    .body(Body::empty())
                    .expect("req"),
            )
            .await
            .expect("resp");
        assert_eq!(resp.status(), StatusCode::OK);
    }

    // ---- Table 5.2.40-1: Context Source Identity --------------------------

    #[tokio::test]
    async fn source_identity_carries_the_mandated_members() {
        // contextSourceAlias / contextSourceUptime / contextSourceTimeAt are
        // all cardinality 1. The old payload used hostAlias/uptime, which are
        // not core-context terms at all.
        let resp = app()
            .oneshot(
                Request::get("/ngsi-ld/v1/info/sourceIdentity")
                    .body(Body::empty())
                    .expect("req"),
            )
            .await
            .expect("resp");
        assert_eq!(resp.status(), StatusCode::OK);
        let doc = body_json(resp).await;
        assert_eq!(doc["type"], "ContextSourceIdentity");
        assert!(doc["id"].as_str().is_some_and(|s| s.starts_with("urn:")));
        assert_eq!(doc["contextSourceAlias"], "antares-test");
        assert!(
            doc["contextSourceUptime"]
                .as_str()
                .is_some_and(|s| s.starts_with("PT") && s.ends_with('S')),
            "uptime must be an ISO 8601 duration, got {:?}",
            doc["contextSourceUptime"]
        );
        assert!(
            doc["contextSourceTimeAt"]
                .as_str()
                .is_some_and(|s| s.ends_with('Z')),
            "timeAt must be a 4.6.3 DateTime, got {:?}",
            doc["contextSourceTimeAt"]
        );
        assert!(
            doc.get("hostAlias").is_none(),
            "hostAlias is not a spec member"
        );
        assert!(doc.get("uptime").is_none(), "uptime is not a spec member");
    }

    // ---- 6.3.4: Accept precedence -----------------------------------------

    #[tokio::test]
    async fn accept_precedence_follows_the_spec_list_not_header_order() {
        // "The order of the list above is significant … the first one of the
        // list shall be selected, unless amended by … a q parameter."
        // json > ld+json > geo+json, regardless of how the client orders them.
        let app = app();
        create(&app, "urn:ngsi-ld:Building:acc1", "Building").await;

        for (accept, want) in [
            ("application/ld+json, application/json", "application/json"),
            ("application/geo+json, application/json", "application/json"),
            ("application/ld+json", "application/ld+json"),
            // an explicit q still wins over list order
            (
                "application/json;q=0.1, application/ld+json;q=0.9",
                "application/ld+json",
            ),
        ] {
            let resp = app
                .clone()
                .oneshot(
                    Request::get("/ngsi-ld/v1/entities/urn:ngsi-ld:Building:acc1")
                        .header("Accept", accept)
                        .body(Body::empty())
                        .expect("req"),
                )
                .await
                .expect("resp");
            let ct = resp
                .headers()
                .get("Content-Type")
                .and_then(|v| v.to_str().ok())
                .unwrap_or_default()
                .to_owned();
            assert!(
                ct.starts_with(want),
                "Accept: {accept} → Content-Type {ct}, expected {want}"
            );
        }
    }

    /// 6.3.10 p.275: "At least, the type Link Target Attribute shall be
    /// included ... and its value shall be exactly equal to the media type
    /// resulting from the original request" — previously emitted only for
    /// ld+json, never for plain application/json (audit V-23).
    #[tokio::test]
    async fn pagination_links_carry_the_type_attribute_for_plain_json() {
        let app = app();
        for i in 0..3 {
            let entity = serde_json::json!({
                "id": format!("urn:ngsi-ld:Building:pg{i}"),
                "type": "Building"
            });
            let resp = app
                .clone()
                .oneshot(
                    Request::post("/ngsi-ld/v1/entities")
                        .header("Content-Type", "application/json")
                        .header("Content-Length", (entity.to_string()).len())
                        .body(Body::from(entity.to_string()))
                        .expect("req"),
                )
                .await
                .expect("resp");
            assert_eq!(resp.status(), StatusCode::CREATED);
        }
        let resp = app
            .clone()
            .oneshot(
                Request::get("/ngsi-ld/v1/entities?type=Building&limit=1&offset=1")
                    .body(Body::empty())
                    .expect("req"),
            )
            .await
            .expect("resp");
        assert_eq!(resp.status(), StatusCode::OK);
        let links: Vec<String> = resp
            .headers()
            .get_all("Link")
            .iter()
            .filter_map(|v| v.to_str().ok())
            .flat_map(|v| v.split(", "))
            .filter(|l| l.contains("rel=\"next\"") || l.contains("rel=\"prev\""))
            .map(str::to_owned)
            .collect();
        assert_eq!(links.len(), 2, "expected next+prev, got {links:?}");
        for l in &links {
            assert!(
                l.contains(";type=\"application/json\""),
                "pagination link lacks the mandatory type attribute: {l}"
            );
        }
    }

    /// 4.5.9 p.63/65 (audit V-24): in the simplified temporal representation
    /// a ListProperty pairs a BARE ordered array with its timestamp under
    /// `valueLists` (EXAMPLE 3), and a ListRelationship the same under
    /// `objectLists` — not a {"valueList"/"objectList"} wrapper object.
    #[tokio::test]
    async fn temporal_values_list_types_use_bare_arrays() {
        let app = app();
        let doc = serde_json::json!({
            "id": "urn:ngsi-ld:Meeting:tv1",
            "type": "Meeting",
            "period": [
                {"type": "ListProperty", "valueList": ["First", "Second"],
                 "observedAt": "2023-01-01T00:00:00Z"},
                {"type": "ListProperty", "valueList": ["1st", "2nd"],
                 "observedAt": "2023-01-02T00:00:00Z"}
            ],
            "membersPresent": [
                {"type": "ListRelationship",
                 "objectList": ["urn:ngsi-ld:Person:Alice", "urn:ngsi-ld:Person:Bob"],
                 "observedAt": "2023-01-01T00:00:00Z"}
            ]
        });
        let resp = app
            .clone()
            .oneshot(
                Request::post("/ngsi-ld/v1/temporal/entities")
                    .header("Content-Type", "application/json")
                    .header("Content-Length", (doc.to_string()).len())
                    .body(Body::from(doc.to_string()))
                    .expect("req"),
            )
            .await
            .expect("resp");
        assert!(
            resp.status() == StatusCode::CREATED || resp.status() == StatusCode::NO_CONTENT,
            "temporal upsert failed: {}",
            resp.status()
        );
        let resp = app
            .clone()
            .oneshot(
                Request::get(
                    "/ngsi-ld/v1/temporal/entities/urn:ngsi-ld:Meeting:tv1?format=temporalValues",
                )
                .body(Body::empty())
                .expect("req"),
            )
            .await
            .expect("resp");
        assert_eq!(resp.status(), StatusCode::OK);
        let body = body_json(resp).await;
        let vl = &body["period"]["valueLists"];
        assert!(vl.is_array(), "period.valueLists missing: {body}");
        assert_eq!(
            vl[0][0],
            serde_json::json!(["First", "Second"]),
            "first pair element must be the BARE ordered array: {vl}"
        );
        assert_eq!(vl[0][1], "2023-01-01T00:00:00Z");
        let ol = &body["membersPresent"]["objectLists"];
        assert_eq!(
            ol[0][0],
            serde_json::json!(["urn:ngsi-ld:Person:Alice", "urn:ngsi-ld:Person:Bob"]),
            "objectLists pairs carry the bare URI array: {ol}"
        );
    }

    /// V-27 — 4.5.19.1 Table -1 + 5.7.4.4 p.211: string-valued Properties
    /// aggregate min/max lexicographically ("first/last value in
    /// lexicographical order"); a method the datatype is not eligible for
    /// ("sum" on strings is N/A) raises InvalidRequest; and numeric folds
    /// never leak f64::INFINITY (serialized as null) into the payload.
    #[tokio::test]
    async fn aggregation_dispatches_on_datatype_and_rejects_ineligible() {
        let app = app();
        let doc = serde_json::json!({
            "id": "urn:ngsi-ld:Building:agg1",
            "type": "Building",
            "operator": [
                {"type": "Property", "value": "alpha", "observedAt": "2023-01-01T00:00:00Z"},
                {"type": "Property", "value": "zulu",  "observedAt": "2023-01-02T00:00:00Z"},
                {"type": "Property", "value": "mike",  "observedAt": "2023-01-03T00:00:00Z"}
            ]
        });
        let resp = app
            .clone()
            .oneshot(
                Request::post("/ngsi-ld/v1/temporal/entities")
                    .header("Content-Type", "application/json")
                    .header("Content-Length", (doc.to_string()).len())
                    .body(Body::from(doc.to_string()))
                    .expect("req"),
            )
            .await
            .expect("resp");
        assert_eq!(resp.status(), StatusCode::CREATED);
        let base = "/ngsi-ld/v1/temporal/entities/urn:ngsi-ld:Building:agg1\
                    ?options=aggregatedValues&timerel=after&timeAt=2022-01-01T00:00:00Z";
        // eligible: lexicographic min/max on strings
        let resp = app
            .clone()
            .oneshot(
                Request::get(format!("{base}&aggrMethods=min,max"))
                    .body(Body::empty())
                    .expect("req"),
            )
            .await
            .expect("resp");
        assert_eq!(resp.status(), StatusCode::OK);
        let body = body_json(resp).await;
        assert_eq!(body["operator"]["min"][0][0], "alpha", "{body}");
        assert_eq!(body["operator"]["max"][0][0], "zulu", "{body}");
        // ineligible: sum over strings is N/A → InvalidRequest (400)
        let resp = app
            .clone()
            .oneshot(
                Request::get(format!("{base}&aggrMethods=sum"))
                    .body(Body::empty())
                    .expect("req"),
            )
            .await
            .expect("resp");
        assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
        let body = body_json(resp).await;
        assert_eq!(
            body["type"], "https://uri.etsi.org/ngsi-ld/errors/InvalidRequest",
            "{body}"
        );
    }
}
