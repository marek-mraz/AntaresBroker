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

/// Scope Query evaluation (4.19) — `|`/`,` = OR, `(a;b)` = AND (parenthesis
/// grouping), `+` one level, trailing `#` the subtree incl. the node, `/#`
/// any non-empty scope.
pub fn scope_matches(scope_q: &str, doc: &Value) -> bool {
    let scopes: Vec<&str> = doc
        .get("scope")
        .and_then(Value::as_array)
        .map(|a| a.iter().filter_map(Value::as_str).collect())
        .unwrap_or_default();
    scope_q.split([',', '|']).any(|and_group| {
        and_group
            .trim()
            .trim_start_matches('(')
            .trim_end_matches(')')
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

/// 4.3.5 NGSI-LD API structure: Core API mandatory; Distributed API mandatory for
/// distributed/federated deployments; Temporal API and Registry API integrated
/// locally here (Table 4.3.5-2 row "integrated temporal + integrated Context
/// Registry"); JSONLDContext API implemented, optional Snapshot API not offered.
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
                // 5.5.10: non-create operations targeting a non-existing
                // Tenant answer NonexistentTenant 404; create operations
                // implicitly create the Tenant.
                .layer(axum::middleware::from_fn_with_state(
                    state.clone(),
                    tenant_exists_layer,
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

/// 5.5.10 Multi-Tenant Behaviour: Tenants are created implicitly by create
/// operations (Create Entity 5.6.1, Batch Create/Upsert 5.6.7/5.6.8, Create
/// Temporal 5.6.11, Create Subscription 5.8.1, Register Context Source
/// 5.9.2, Create CSource Subscription 5.11.2); "all other NGSI-LD
/// operations … that target a non-existing Tenant should raise an error of
/// type NonexistentTenant". Malformed tenant headers pass through — the
/// handler's own parse answers 400.
async fn tenant_exists_layer(
    axum::extract::State(st): axum::extract::State<AppState>,
    req: axum::http::Request<axum::body::Body>,
    next: axum::middleware::Next,
) -> Response {
    let path = req.uri().path().trim_start_matches(API_ROOT);
    let implicit_create = req.method() == axum::http::Method::POST
        && matches!(
            path,
            "/entities"
                | "/entityOperations/create"
                | "/entityOperations/upsert"
                | "/temporal/entities"
                | "/subscriptions"
                | "/csourceRegistrations"
                | "/csourceSubscriptions"
        );
    // tenant-independent resources: broker identity (6.33) answers for any
    // tenant (the per-tenant alias exists before any data does), and hosted
    // @contexts are stored tenant-less (5.13).
    let tenant_free = path.starts_with("/info/") || path.starts_with("/jsonldContexts");
    if !implicit_create && !tenant_free {
        if let Some(t) = req
            .headers()
            .get("NGSILD-Tenant")
            .and_then(|v| v.to_str().ok())
            .and_then(|s| antares_model::TenantId::new(s).ok())
        {
            match st.store.tenant_exists(&t) {
                Ok(false) => {
                    let mut resp = crate::negotiate::ApiError::from(NgsiError::NonexistentTenant(
                        format!("tenant {} does not exist", t.as_str()),
                    ))
                    .into_response();
                    // 6.3.14: a request-supplied NGSILD-Tenant SHALL be
                    // present in the response — error responses included.
                    crate::negotiate::echo_tenant(&t, &mut resp);
                    return resp;
                }
                Err(e) => return crate::negotiate::ApiError::from(e).into_response(),
                Ok(true) => {}
            }
        }
    }
    next.run(req).await
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
        // Table 5.2.40-1: in the multi-tenancy case the alias "shall be
        // identifying a specific Tenant within a registered Context Source",
        // so what this resource serves depends on NGSILD-Tenant — a peer
        // retrieves it per tenant and registers it as `contextSourceAlias`.
        let alias = crate::federation::alias_for(&state.host_alias, &tenant);
        let body = serde_json::json!({
            "id": format!("urn:ngsi-ld:ContextSourceIdentity:{alias}"),
            "type": "ContextSourceIdentity",
            "contextSourceAlias": alias,
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

    /// 5.5.11.1: in Batch Create the FIRST occurrence creates the entity,
    /// any subsequent instance of the same id is reported as an error
    /// (already exists). 5.5.11.4: in Batch Delete the first occurrence
    /// deletes, subsequent ones report an error (does not exist).
    #[tokio::test]
    async fn clause_5_5_11_duplicate_ids_in_create_and_delete() {
        let app = app();
        let batch = serde_json::json!([
            {"id": "urn:ngsi-ld:Building:c-dup", "type": "Building",
             "speed": {"type": "Property", "value": 1}},
            {"id": "urn:ngsi-ld:Building:c-dup", "type": "Building",
             "speed": {"type": "Property", "value": 2}}
        ]);
        let resp = app
            .clone()
            .oneshot(
                Request::post("/ngsi-ld/v1/entityOperations/create")
                    .header("Content-Type", "application/json")
                    .header("Content-Length", batch.to_string().len())
                    .body(Body::from(batch.to_string()))
                    .expect("req"),
            )
            .await
            .expect("resp");
        assert_eq!(
            resp.status(),
            StatusCode::MULTI_STATUS,
            "one ok + one error"
        );
        let body = body_json(resp).await;
        assert_eq!(
            body["success"],
            serde_json::json!(["urn:ngsi-ld:Building:c-dup"])
        );
        assert_eq!(body["errors"].as_array().map(Vec::len), Some(1));
        assert!(
            body["errors"][0].to_string().contains("AlreadyExists"),
            "second occurrence is an already-exists error: {body}"
        );
        // the FIRST occurrence created the entity
        let resp = app
            .clone()
            .oneshot(
                Request::get("/ngsi-ld/v1/entities/urn:ngsi-ld:Building:c-dup")
                    .body(Body::empty())
                    .expect("req"),
            )
            .await
            .expect("resp");
        let doc = body_json(resp).await;
        assert_eq!(doc["speed"]["value"], 1, "first occurrence wins the create");
        // batch delete with the id twice: first deletes, second errors
        let del = serde_json::json!(["urn:ngsi-ld:Building:c-dup", "urn:ngsi-ld:Building:c-dup"]);
        let resp = app
            .clone()
            .oneshot(
                Request::post("/ngsi-ld/v1/entityOperations/delete")
                    .header("Content-Type", "application/json")
                    .header("Content-Length", del.to_string().len())
                    .body(Body::from(del.to_string()))
                    .expect("req"),
            )
            .await
            .expect("resp");
        assert_eq!(resp.status(), StatusCode::MULTI_STATUS);
        let body = body_json(resp).await;
        assert_eq!(
            body["success"],
            serde_json::json!(["urn:ngsi-ld:Building:c-dup"])
        );
        assert!(
            body["errors"][0].to_string().contains("ResourceNotFound"),
            "second delete occurrence is a not-found error: {body}"
        );
        // and the entity is really gone
        let resp = app
            .oneshot(
                Request::get("/ngsi-ld/v1/entities/urn:ngsi-ld:Building:c-dup")
                    .body(Body::empty())
                    .expect("req"),
            )
            .await
            .expect("resp");
        assert_eq!(resp.status(), StatusCode::NOT_FOUND);
    }

    /// 4.6.6: duplicate instances of one Entity in a batch array "shall come
    /// in chronological order" — the broker applies them sequentially, so
    /// the LAST occurrence's state wins, never the first.
    #[tokio::test]
    async fn batch_duplicate_instances_apply_in_array_order() {
        let app = app();
        let batch = serde_json::json!([
            {"id": "urn:ngsi-ld:Building:dup", "type": "Building",
             "speed": {"type": "Property", "value": 1},
             "old": {"type": "Property", "value": true}},
            {"id": "urn:ngsi-ld:Building:dup", "type": "Building",
             "speed": {"type": "Property", "value": 2}}
        ]);
        let resp = app
            .clone()
            .oneshot(
                Request::post("/ngsi-ld/v1/entityOperations/upsert")
                    .header("Content-Type", "application/json")
                    .header("Content-Length", batch.to_string().len())
                    .body(Body::from(batch.to_string()))
                    .expect("req"),
            )
            .await
            .expect("resp");
        assert!(
            resp.status() == StatusCode::CREATED || resp.status() == StatusCode::NO_CONTENT,
            "upsert with duplicates succeeds: {}",
            resp.status()
        );
        let resp = app
            .clone()
            .oneshot(
                Request::get("/ngsi-ld/v1/entities/urn:ngsi-ld:Building:dup")
                    .body(Body::empty())
                    .expect("req"),
            )
            .await
            .expect("resp");
        assert_eq!(resp.status(), StatusCode::OK);
        let doc = body_json(resp).await;
        assert_eq!(doc["speed"]["value"], 2, "later instance wins");
        assert_ne!(doc["speed"]["value"], 1, "first instance must not survive");
        // default upsert is REPLACE: the second instance replaced the first
        // wholesale, so the first-only attribute is gone too
        assert!(
            doc.get("old").is_none(),
            "replace semantics: first instance's attrs must not linger"
        );
    }

    /// 4.6.4 Supported Content: "implementations shall preserve the
    /// representation of the content of the values provided by the context
    /// information providers and return the original content" — the
    /// script-injection characters < > " ' = ; ( ) are stored and served
    /// verbatim, never HTML/unicode-escaped and never rejected.
    #[tokio::test]
    async fn dangerous_content_preserved_verbatim() {
        let app = app();
        let payload = "<script>alert('x')</script> \"quoted\" = ; ( )";
        let entity = serde_json::json!({
            "id": "urn:ngsi-ld:Building:content",
            "type": "Building",
            "note": {"type": "Property", "value": payload}
        });
        let resp = app
            .clone()
            .oneshot(
                Request::post("/ngsi-ld/v1/entities")
                    .header("Content-Type", "application/json")
                    .header("Content-Length", entity.to_string().len())
                    .body(Body::from(entity.to_string()))
                    .expect("req"),
            )
            .await
            .expect("resp");
        assert_eq!(resp.status(), StatusCode::CREATED, "content never rejected");
        let resp = app
            .clone()
            .oneshot(
                Request::get("/ngsi-ld/v1/entities/urn:ngsi-ld:Building:content")
                    .body(Body::empty())
                    .expect("req"),
            )
            .await
            .expect("resp");
        assert_eq!(resp.status(), StatusCode::OK);
        let raw = resp.into_body().collect().await.expect("body").to_bytes();
        let text = std::str::from_utf8(&raw).expect("utf-8");
        // original content, not an escaped rendering of it
        assert!(!text.contains("&lt;"), "no HTML escaping");
        assert!(!text.contains("\\u003c"), "no unicode escaping");
        let doc: serde_json::Value = serde_json::from_str(text).expect("json");
        assert_eq!(doc["note"]["value"], payload, "value returned verbatim");
    }

    /// 4.6.1 Supported text encodings: UTF-8 JSON accepted and exposed;
    /// a non-UTF-8 body is not valid JSON → InvalidRequest 400.
    #[tokio::test]
    async fn utf8_encoding_accepted_and_non_utf8_rejected() {
        let app = app();
        // multibyte UTF-8 round-trips byte-exact
        let entity = serde_json::json!({
            "id": "urn:ngsi-ld:Building:utf8",
            "type": "Building",
            "label": {"type": "Property", "value": "žltý kôň — 100 €"}
        });
        let resp = app
            .clone()
            .oneshot(
                Request::post("/ngsi-ld/v1/entities")
                    .header("Content-Type", "application/json")
                    .header("Content-Length", entity.to_string().len())
                    .body(Body::from(entity.to_string()))
                    .expect("req"),
            )
            .await
            .expect("resp");
        assert_eq!(resp.status(), StatusCode::CREATED);
        let resp = app
            .clone()
            .oneshot(
                Request::get("/ngsi-ld/v1/entities/urn:ngsi-ld:Building:utf8")
                    .body(Body::empty())
                    .expect("req"),
            )
            .await
            .expect("resp");
        assert_eq!(resp.status(), StatusCode::OK);
        let raw = resp.into_body().collect().await.expect("body").to_bytes();
        let text = std::str::from_utf8(&raw).expect("response is valid UTF-8");
        assert!(!text.contains('\u{FFFD}'), "no mojibake in output");
        let doc: serde_json::Value = serde_json::from_str(text).expect("json");
        assert_eq!(doc["label"]["value"], "žltý kôň — 100 €");

        // invalid UTF-8 byte in the body → InvalidRequest, not BadRequestData
        let mut bad = b"{\"id\": \"urn:ngsi-ld:Building:b\", \"type\": \"".to_vec();
        bad.extend_from_slice(&[0xFF, 0xFE]);
        bad.extend_from_slice(b"\"}");
        let resp = app
            .clone()
            .oneshot(
                Request::post("/ngsi-ld/v1/entities")
                    .header("Content-Type", "application/json")
                    .header("Content-Length", bad.len())
                    .body(Body::from(bad.clone()))
                    .expect("req"),
            )
            .await
            .expect("resp");
        assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
        let err = body_json(resp).await;
        assert_eq!(
            err["type"],
            "https://uri.etsi.org/ngsi-ld/errors/InvalidRequest"
        );
        assert_ne!(
            err["type"], "https://uri.etsi.org/ngsi-ld/errors/BadRequestData",
            "syntactic (encoding) failure is InvalidRequest, not BadRequestData"
        );
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
        // 5.5.10: an unknown tenant on a non-create op is NonexistentTenant
        // 404 — and 6.3.14 still requires the tenant header on the response.
        let resp = app()
            .oneshot(
                Request::get("/ngsi-ld/v1/entities?type=Building")
                    .header("NGSILD-Tenant", "city-01")
                    .body(Body::empty())
                    .expect("req"),
            )
            .await
            .expect("resp");
        assert_eq!(resp.status(), StatusCode::NOT_FOUND);
        assert_eq!(
            resp.headers()
                .get("NGSILD-Tenant")
                .map(|v| v.to_str().expect("ascii")),
            Some("city-01")
        );
        // once the tenant exists (implicit creation), the query echoes on 200
        let app = app();
        let entity = serde_json::json!({"id": "urn:ngsi-ld:B:t1", "type": "Building"});
        let body = entity.to_string();
        let resp = app
            .clone()
            .oneshot(
                Request::post("/ngsi-ld/v1/entities")
                    .header("Content-Type", "application/json")
                    .header("Content-Length", body.len())
                    .header("NGSILD-Tenant", "city-01")
                    .body(Body::from(body))
                    .expect("req"),
            )
            .await
            .expect("resp");
        assert_eq!(resp.status(), StatusCode::CREATED);
        let resp = app
            .oneshot(
                Request::get("/ngsi-ld/v1/entities?type=Building")
                    .header("NGSILD-Tenant", "city-01")
                    .body(Body::empty())
                    .expect("req"),
            )
            .await
            .expect("resp");
        assert_eq!(resp.status(), StatusCode::OK);
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

#[cfg(test)]
mod clause_4_19 {
    use super::scope_matches;
    use serde_json::json;

    fn doc(scopes: &[&str]) -> serde_json::Value {
        json!({"id": "urn:x", "type": ["T"], "scope": scopes})
    }

    /// 4.19 EXAMPLES 1-3: direct scope, `#` subtree (including the node
    /// itself), `+` single-level wildcard, `/#` any non-empty scope.
    #[test]
    fn wildcards_and_direct_scopes() {
        assert!(scope_matches("/Madrid", &doc(&["/Madrid"])));
        assert!(!scope_matches("/Madrid", &doc(&["/Madrid/Gardens"])));
        for s in [
            "/Madrid/Gardens",
            "/Madrid/Gardens/ParqueNorte",
            "/Madrid/Gardens/ParqueNorte/Parterre1",
        ] {
            assert!(scope_matches("/Madrid/Gardens/#", &doc(&[s])), "{s}");
        }
        assert!(!scope_matches(
            "/Madrid/Gardens/#",
            &doc(&["/Madrid/Sights"])
        ));
        assert!(scope_matches(
            "/Madrid/+/ParqueNorte",
            &doc(&["/Madrid/Sights/ParqueNorte"])
        ));
        assert!(!scope_matches(
            "/Madrid/+/ParqueNorte",
            &doc(&["/Madrid/ParqueNorte"])
        ));
        assert!(scope_matches("/#", &doc(&["/Anything"])));
        assert!(
            !scope_matches("/#", &doc(&[])),
            "no scope = no match for /#"
        );
    }

    /// 4.19 EXAMPLES 4/5: conjunction needs parentheses; disjunction is `|`
    /// OR the compatibility comma.
    #[test]
    fn conjunction_and_both_or_spellings() {
        let both = doc(&["/Madrid/Districts", "/CompanyA"]);
        let only_b = doc(&["/CompanyB"]);
        let only_madrid = doc(&["/Madrid/Districts"]);
        assert!(scope_matches("(/Madrid/Districts;/CompanyA)", &both));
        assert!(
            !scope_matches("(/Madrid/Districts;/CompanyA)", &only_madrid),
            "conjunction requires ALL scopes"
        );
        for sel in [
            "(/Madrid/Districts;/CompanyA)|/CompanyB",
            "(/Madrid/Districts;/CompanyA),/CompanyB",
        ] {
            assert!(scope_matches(sel, &both), "{sel}");
            assert!(scope_matches(sel, &only_b), "{sel}");
            assert!(!scope_matches(sel, &only_madrid), "{sel}");
        }
    }
}

#[cfg(test)]
mod clause_5_2_8 {
    use super::*;
    use axum::body::Body;
    use axum::http::{Request, StatusCode};
    use tower::ServiceExt;

    async fn post_reg(entities: serde_json::Value) -> StatusCode {
        let app = router(AppState::new("t528".into()));
        let body = serde_json::json!({
            "id": format!("urn:ngsi-ld:ContextSourceRegistration:528-{}",
                          entities.to_string().len()),
            "type": "ContextSourceRegistration",
            "information": [{"entities": entities}],
            "endpoint": "http://cs.example.org:1026"
        })
        .to_string();
        let req = Request::post("/ngsi-ld/v1/csourceRegistrations")
            .header("Content-Type", "application/json")
            .header("Content-Length", body.len())
            .body(Body::from(body))
            .expect("req");
        app.oneshot(req).await.expect("resp").status()
    }

    /// Table 5.2.8-1: type is "String or String[]" — the array form must be
    /// accepted; id must be a URI; idPattern must be a valid regex.
    #[tokio::test]
    async fn entity_info_type_accepts_the_array_form() {
        assert_eq!(
            post_reg(serde_json::json!([{"type": ["Building", "Vehicle"]}])).await,
            StatusCode::CREATED,
            "String[] type is legal"
        );
        assert_eq!(
            post_reg(serde_json::json!([{"type": "Building"}])).await,
            StatusCode::CREATED
        );
        assert_eq!(
            post_reg(serde_json::json!([{"type": [] }])).await,
            StatusCode::BAD_REQUEST,
            "an empty type array names no Entity Type"
        );
        assert_eq!(
            post_reg(serde_json::json!([{"type": "Building", "id": "not a uri"}])).await,
            StatusCode::BAD_REQUEST
        );
        assert_eq!(
            post_reg(serde_json::json!([{"type": "Building", "idPattern": "urn:[" }])).await,
            StatusCode::BAD_REQUEST
        );
    }
}

#[cfg(test)]
mod clause_5_2_9 {
    use super::*;
    use axum::body::Body;
    use axum::http::{Request, StatusCode};
    use tower::ServiceExt;

    async fn post_reg(extra: serde_json::Value) -> StatusCode {
        let app = router(AppState::new("t529".into()));
        let mut doc = serde_json::json!({
            "id": format!("urn:ngsi-ld:ContextSourceRegistration:529-{}",
                          extra.to_string().len()),
            "type": "ContextSourceRegistration",
            "information": [{"entities": [{"type": "Building"}]}],
            "endpoint": "http://cs.example.org:1026"
        });
        for (k, v) in extra.as_object().expect("obj") {
            doc[k] = v.clone();
        }
        let body = doc.to_string();
        let req = Request::post("/ngsi-ld/v1/csourceRegistrations")
            .header("Content-Type", "application/json")
            .header("Content-Length", body.len())
            .body(Body::from(body))
            .expect("req");
        app.oneshot(req).await.expect("resp").status()
    }

    /// Table 5.2.9-1 value spaces: operations limited to the 4.20 names and
    /// groups; localOnly boolean; contextSourceAlias a non-empty RFC 7230
    /// pseudonym token; refreshRate an ISO 8601 duration; datasetId URIs or
    /// @none; scope per the 4.18 grammar; geometries per 4.7; description
    /// and registrationName non-empty strings.
    #[tokio::test]
    async fn registration_member_value_spaces() {
        use serde_json::json;
        let cases: &[(serde_json::Value, StatusCode)] = &[
            (json!({"operations": ["bogusOp"]}), StatusCode::BAD_REQUEST),
            (
                json!({"operations": ["updateOps", "retrieveEntity"]}),
                StatusCode::CREATED,
            ),
            (json!({"localOnly": "yes"}), StatusCode::BAD_REQUEST),
            (json!({"localOnly": true}), StatusCode::CREATED),
            (json!({"contextSourceAlias": ""}), StatusCode::BAD_REQUEST),
            (
                json!({"contextSourceAlias": "has space"}),
                StatusCode::BAD_REQUEST,
            ),
            (json!({"contextSourceAlias": "cs1"}), StatusCode::CREATED),
            (json!({"refreshRate": "5 minutes"}), StatusCode::BAD_REQUEST),
            (json!({"refreshRate": "PT5M"}), StatusCode::CREATED),
            (
                json!({"datasetId": ["urn:ds:1", "@none"]}),
                StatusCode::CREATED,
            ),
            (json!({"datasetId": ["not a uri"]}), StatusCode::BAD_REQUEST),
            (json!({"scope": "9bad"}), StatusCode::BAD_REQUEST),
            (json!({"scope": ["/Madrid", "/A/B_2"]}), StatusCode::CREATED),
            (json!({"description": ""}), StatusCode::BAD_REQUEST),
            (json!({"registrationName": ""}), StatusCode::BAD_REQUEST),
            (json!({"location": 5}), StatusCode::BAD_REQUEST),
            (
                json!({"location": {"type": "Point", "coordinates": [8, 40]}}),
                StatusCode::CREATED,
            ),
        ];
        for (extra, want) in cases {
            let got = post_reg(extra.clone()).await;
            assert_eq!(got, *want, "extra={extra}");
        }
    }
}

#[cfg(test)]
mod clause_5_2_10 {
    use super::*;
    use axum::body::Body;
    use axum::http::{Request, StatusCode};
    use tower::ServiceExt;

    async fn post_info(info: serde_json::Value) -> StatusCode {
        let app = router(AppState::new("t5210".into()));
        let body = serde_json::json!({
            "id": format!("urn:ngsi-ld:ContextSourceRegistration:5210-{}",
                          info.to_string().len()),
            "type": "ContextSourceRegistration",
            "information": [info],
            "endpoint": "http://cs.example.org:1026"
        })
        .to_string();
        let req = Request::post("/ngsi-ld/v1/csourceRegistrations")
            .header("Content-Type", "application/json")
            .header("Content-Length", body.len())
            .body(Body::from(body))
            .expect("req");
        app.oneshot(req).await.expect("resp").status()
    }

    /// Table 5.2.10-1: empty arrays are not allowed for entities,
    /// propertyNames or relationshipNames; non-empty name lists are fine.
    #[tokio::test]
    async fn registration_info_empty_arrays_are_rejected() {
        use serde_json::json;
        assert_eq!(
            post_info(json!({"entities": []})).await,
            StatusCode::BAD_REQUEST
        );
        assert_eq!(
            post_info(json!({"propertyNames": []})).await,
            StatusCode::BAD_REQUEST,
            "empty propertyNames"
        );
        assert_eq!(
            post_info(json!({"relationshipNames": []})).await,
            StatusCode::BAD_REQUEST,
            "empty relationshipNames"
        );
        assert_eq!(
            post_info(json!({"entities": [{"type": "Building"}],
                "propertyNames": ["speed"],
                "relationshipNames": ["isParked"]}))
            .await,
            StatusCode::CREATED
        );
    }
}

#[cfg(test)]
mod clause_5_2_11 {
    use super::*;
    use axum::body::Body;
    use axum::http::{Request, StatusCode};
    use tower::ServiceExt;

    async fn post_interval(iv: serde_json::Value) -> StatusCode {
        let app = router(AppState::new("t5211".into()));
        let body = serde_json::json!({
            "id": format!("urn:ngsi-ld:ContextSourceRegistration:5211-{}",
                          iv.to_string().len()),
            "type": "ContextSourceRegistration",
            "information": [{"entities": [{"type": "Building"}]}],
            "endpoint": "http://cs.example.org:1026",
            "observationInterval": iv
        })
        .to_string();
        let req = Request::post("/ngsi-ld/v1/csourceRegistrations")
            .header("Content-Type", "application/json")
            .header("Content-Length", body.len())
            .body(Body::from(body))
            .expect("req");
        app.oneshot(req).await.expect("resp").status()
    }

    /// Table 5.2.11-1: startAt is a mandatory DateTime; endAt optional but a
    /// DateTime when present (absent = open interval).
    #[tokio::test]
    async fn time_interval_member_rules() {
        use serde_json::json;
        assert_eq!(
            post_interval(json!({"endAt": "2030-01-01T00:00:00Z"})).await,
            StatusCode::BAD_REQUEST,
            "startAt mandatory"
        );
        assert_eq!(
            post_interval(json!({"startAt": "2020-01-01"})).await,
            StatusCode::BAD_REQUEST,
            "a Date is not a DateTime"
        );
        assert_eq!(
            post_interval(json!({"startAt": "2020-01-01T00:00:00Z",
                "endAt": "not a date"}))
            .await,
            StatusCode::BAD_REQUEST
        );
        assert_eq!(
            post_interval(json!({"startAt": "2020-01-01T00:00:00Z"})).await,
            StatusCode::CREATED,
            "open interval"
        );
        assert_eq!(
            post_interval(json!({"startAt": "2020-01-01T00:00:00Z",
                "endAt": "2030-01-01T00:00:00Z"}))
            .await,
            StatusCode::CREATED
        );
    }
}

#[cfg(test)]
mod clause_5_2_12 {
    use super::*;
    use axum::body::Body;
    use axum::http::{Request, StatusCode};
    use tower::ServiceExt;

    async fn post_sub(doc: serde_json::Value) -> StatusCode {
        let app = router(AppState::new("t5212".into()));
        let body = doc.to_string();
        let req = Request::post("/ngsi-ld/v1/subscriptions")
            .header("Content-Type", "application/json")
            .header("Content-Length", body.len())
            .body(Body::from(body))
            .expect("req");
        app.oneshot(req).await.expect("resp").status()
    }

    /// 5.2.12: "At least one of (a) entities or (b) watchedAttributes shall
    /// be present, unless the member localOnly is set to true".
    #[tokio::test]
    async fn local_only_waives_the_selector_requirement() {
        use serde_json::json;
        let base = |extra: serde_json::Value| {
            let mut d = json!({
                "id": format!("urn:ngsi-ld:Subscription:5212-{}", extra.to_string().len()),
                "type": "Subscription",
                "notification": {"endpoint": {"uri": "http://client.example.org/cb"}}
            });
            for (k, v) in extra.as_object().expect("obj") {
                d[k] = v.clone();
            }
            d
        };
        assert_eq!(
            post_sub(base(json!({}))).await,
            StatusCode::BAD_REQUEST,
            "no selector and no localOnly"
        );
        assert_eq!(
            post_sub(base(json!({"localOnly": true}))).await,
            StatusCode::CREATED,
            "localOnly=true waives entities/watchedAttributes"
        );
        assert_eq!(
            post_sub(base(json!({"localOnly": false}))).await,
            StatusCode::BAD_REQUEST
        );
        // the exclusions stay intact
        assert_eq!(
            post_sub(base(json!({"watchedAttributes": ["speed"],
                "timeInterval": 5})))
            .await,
            StatusCode::BAD_REQUEST
        );
    }
}

#[cfg(test)]
mod clause_5_2_13 {
    use super::*;
    use axum::body::Body;
    use axum::http::{Request, StatusCode};
    use tower::ServiceExt;

    async fn post_geoq(geoq: serde_json::Value) -> StatusCode {
        let app = router(AppState::new("t5213".into()));
        let body = serde_json::json!({
            "id": format!("urn:ngsi-ld:Subscription:5213-{}", geoq.to_string().len()),
            "type": "Subscription",
            "entities": [{"type": "Vehicle"}],
            "geoQ": geoq,
            "notification": {"endpoint": {"uri": "http://client.example.org/cb"}}
        })
        .to_string();
        let req = Request::post("/ngsi-ld/v1/subscriptions")
            .header("Content-Type", "application/json")
            .header("Content-Length", body.len())
            .body(Body::from(body))
            .expect("req");
        app.oneshot(req).await.expect("resp").status()
    }

    /// Table 5.2.13-1: coordinates as JSON Array OR string form, geometry
    /// from the legal set (no GeometryCollection), georel per 4.10,
    /// geoproperty optional.
    #[tokio::test]
    async fn subscription_geoquery_member_rules() {
        use serde_json::json;
        assert_eq!(
            post_geoq(json!({"georel": "near;maxDistance==2000",
                "geometry": "Point", "coordinates": [8, 40]}))
            .await,
            StatusCode::CREATED
        );
        assert_eq!(
            post_geoq(json!({"georel": "within", "geometry": "Polygon",
                "coordinates": "[[[0,0],[4,0],[4,4],[0,4],[0,0]]]",
                "geoproperty": "observationSpace"}))
            .await,
            StatusCode::CREATED,
            "string-encoded coordinates (4.7.1) are legal"
        );
        assert_eq!(
            post_geoq(json!({"georel": "within",
                "geometry": "GeometryCollection", "coordinates": []}))
            .await,
            StatusCode::BAD_REQUEST
        );
        assert_eq!(
            post_geoq(json!({"georel": "touches", "geometry": "Point",
                "coordinates": [8, 40]}))
            .await,
            StatusCode::BAD_REQUEST
        );
        assert_eq!(
            post_geoq(json!({"geometry": "Point", "coordinates": [8, 40]})).await,
            StatusCode::BAD_REQUEST,
            "georel is mandatory"
        );
    }
}

#[cfg(test)]
mod clause_5_2_14 {
    use super::*;
    use axum::body::Body;
    use axum::http::{Request, StatusCode};
    use tower::ServiceExt;

    async fn post_notif(n: serde_json::Value) -> StatusCode {
        let app = router(AppState::new("t5214".into()));
        let mut notif = serde_json::json!({"endpoint": {"uri": "http://client.example.org/cb"}});
        for (k, v) in n.as_object().expect("obj") {
            notif[k] = v.clone();
        }
        let body = serde_json::json!({
            "id": format!("urn:ngsi-ld:Subscription:5214-{}", n.to_string().len()),
            "type": "Subscription",
            "entities": [{"type": "Vehicle"}],
            "notification": notif
        })
        .to_string();
        let req = Request::post("/ngsi-ld/v1/subscriptions")
            .header("Content-Type", "application/json")
            .header("Content-Length", body.len())
            .body(Body::from(body))
            .expect("req");
        app.oneshot(req).await.expect("resp").status()
    }

    /// Table 5.2.14.1-1: join limited to flat/inline/@none, joinLevel a
    /// positive integer, sysAttrs/showChanges booleans, attributes may not
    /// name id/type/scope (it is "a synonym for pick, except that id, type,
    /// scope are not allowed").
    #[tokio::test]
    async fn notification_params_value_spaces() {
        use serde_json::json;
        assert_eq!(
            post_notif(json!({"join": "sideways"})).await,
            StatusCode::BAD_REQUEST
        );
        assert_eq!(
            post_notif(json!({"join": "inline"})).await,
            StatusCode::CREATED
        );
        assert_eq!(
            post_notif(json!({"joinLevel": 0})).await,
            StatusCode::BAD_REQUEST
        );
        assert_eq!(
            post_notif(json!({"joinLevel": 1.5})).await,
            StatusCode::BAD_REQUEST
        );
        assert_eq!(
            post_notif(json!({"join": "flat", "joinLevel": 2})).await,
            StatusCode::CREATED
        );
        assert_eq!(
            post_notif(json!({"sysAttrs": "yes"})).await,
            StatusCode::BAD_REQUEST
        );
        assert_eq!(
            post_notif(json!({"showChanges": "yes"})).await,
            StatusCode::BAD_REQUEST
        );
        assert_eq!(
            post_notif(json!({"attributes": ["id"]})).await,
            StatusCode::BAD_REQUEST,
            "attributes may not name id/type/scope"
        );
        assert_eq!(
            post_notif(json!({"attributes": ["speed"]})).await,
            StatusCode::CREATED
        );
    }
}

#[cfg(test)]
mod clause_5_2_14_2 {
    use super::*;
    use axum::body::Body;
    use axum::http::{Request, StatusCode};
    use http_body_util::BodyExt;
    use tower::ServiceExt;

    /// Table 5.2.14.2-1: lastFailure/lastNotification/lastSuccess/timesSent
    /// are output-only — provided ones "shall ignore them" on create, and
    /// retrieval never echoes fabricated values.
    #[tokio::test]
    async fn output_only_members_are_ignored_on_input() {
        let st = AppState::new("t52142".into());
        let app = router(st);
        let body = serde_json::json!({
            "id": "urn:ngsi-ld:Subscription:52142",
            "type": "Subscription",
            "entities": [{"type": "Vehicle"}],
            "notification": {
                "endpoint": {"uri": "http://client.example.org/cb"},
                "timesSent": 999,
                "lastNotification": "1999-01-01T00:00:00Z",
                "lastSuccess": "1999-01-01T00:00:00Z",
                "lastFailure": "1999-01-01T00:00:00Z"
            }
        })
        .to_string();
        let req = Request::post("/ngsi-ld/v1/subscriptions")
            .header("Content-Type", "application/json")
            .header("Content-Length", body.len())
            .body(Body::from(body))
            .expect("req");
        let resp = app.clone().oneshot(req).await.expect("resp");
        assert_eq!(
            resp.status(),
            StatusCode::CREATED,
            "providing them is not an error"
        );
        let resp = app
            .oneshot(
                Request::get("/ngsi-ld/v1/subscriptions/urn:ngsi-ld:Subscription:52142")
                    .body(Body::empty())
                    .expect("req"),
            )
            .await
            .expect("resp");
        assert_eq!(resp.status(), StatusCode::OK);
        let bytes = resp.into_body().collect().await.expect("body").to_bytes();
        let doc: serde_json::Value = serde_json::from_slice(&bytes).expect("json");
        let n = &doc["notification"];
        for k in [
            "timesSent",
            "lastNotification",
            "lastSuccess",
            "lastFailure",
        ] {
            assert!(
                n.get(k).is_none(),
                "client-fabricated {k} must be ignored, got {}",
                n[k]
            );
        }
    }
}

#[cfg(test)]
mod clause_5_2_15 {
    use super::*;
    use axum::body::Body;
    use axum::http::{Request, StatusCode};
    use tower::ServiceExt;

    async fn post_ep(ep: serde_json::Value) -> StatusCode {
        let app = router(AppState::new("t5215".into()));
        let body = serde_json::json!({
            "id": format!("urn:ngsi-ld:Subscription:5215-{}", ep.to_string().len()),
            "type": "Subscription",
            "entities": [{"type": "Vehicle"}],
            "notification": {"endpoint": ep}
        })
        .to_string();
        let req = Request::post("/ngsi-ld/v1/subscriptions")
            .header("Content-Type", "application/json")
            .header("Content-Length", body.len())
            .body(Body::from(body))
            .expect("req");
        app.oneshot(req).await.expect("resp").status()
    }

    /// Table 5.2.15-1: uri mandatory, accept value space, cooldown/timeout
    /// > 0, receiverInfo/notifierInfo as KeyValuePair[] (5.2.22).
    #[tokio::test]
    async fn endpoint_member_rules() {
        use serde_json::json;
        let uri = "http://client.example.org/cb";
        assert_eq!(
            post_ep(json!({"accept": "application/json"})).await,
            StatusCode::BAD_REQUEST,
            "uri mandatory"
        );
        assert_eq!(
            post_ep(json!({"uri": uri, "accept": "text/plain"})).await,
            StatusCode::BAD_REQUEST
        );
        assert_eq!(
            post_ep(json!({"uri": uri, "cooldown": 0})).await,
            StatusCode::BAD_REQUEST
        );
        assert_eq!(
            post_ep(json!({"uri": uri, "timeout": -1})).await,
            StatusCode::BAD_REQUEST
        );
        assert_eq!(
            post_ep(json!({"uri": uri,
                "receiverInfo": [{"key": "Authorization", "value": "Bearer x"}],
                "cooldown": 500, "timeout": 3000}))
            .await,
            StatusCode::CREATED
        );
        assert_eq!(
            post_ep(json!({"uri": uri, "receiverInfo": ["junk"]})).await,
            StatusCode::BAD_REQUEST,
            "receiverInfo entries must be {{key, value}} pairs"
        );
        assert_eq!(
            post_ep(json!({"uri": uri, "notifierInfo": [{"novalue": true}]})).await,
            StatusCode::BAD_REQUEST,
            "notifierInfo entries must be {{key, value}} pairs"
        );
    }
}

#[cfg(test)]
mod clause_5_2_16 {
    use super::*;
    use axum::body::Body;
    use axum::http::{Request, StatusCode};
    use http_body_util::BodyExt;
    use tower::ServiceExt;

    /// Tables 5.2.16-1/5.2.17-1: a partial batch failure answers 207 with
    /// success = URI array and errors = BatchEntityError[] (entityId +
    /// RFC 7807 ProblemDetails).
    #[tokio::test]
    async fn batch_result_and_entity_error_shapes() {
        let app = router(AppState::new("t5216".into()));
        let body = serde_json::json!([
            {"id": "urn:ngsi-ld:V:ok", "type": "Vehicle"},
            {"id": "not a uri", "type": "Vehicle"}
        ])
        .to_string();
        let req = Request::post("/ngsi-ld/v1/entityOperations/create")
            .header("Content-Type", "application/json")
            .header("Content-Length", body.len())
            .body(Body::from(body))
            .expect("req");
        let resp = app.oneshot(req).await.expect("resp");
        assert_eq!(resp.status(), StatusCode::MULTI_STATUS);
        let bytes = resp.into_body().collect().await.expect("body").to_bytes();
        let doc: serde_json::Value = serde_json::from_slice(&bytes).expect("json");
        assert_eq!(doc["success"], serde_json::json!(["urn:ngsi-ld:V:ok"]));
        let errs = doc["errors"].as_array().expect("errors array");
        assert_eq!(errs.len(), 1);
        let e = &errs[0];
        assert!(e.get("entityId").is_some());
        let pd = &e["error"];
        for k in ["type", "title", "status"] {
            assert!(
                pd.get(k).is_some(),
                "ProblemDetails member {k} missing: {pd}"
            );
        }
        assert!(
            pd["type"].as_str().unwrap_or("").contains("errors/"),
            "error.type is the NGSI-LD error URI"
        );
    }
}

#[cfg(test)]
mod clause_5_2_18 {
    use super::*;
    use axum::body::Body;
    use axum::http::{Request, StatusCode};
    use http_body_util::BodyExt;
    use tower::ServiceExt;

    /// Tables 5.2.18-1/5.2.19-1: partial attribute update answers 207 with
    /// updated = names and notUpdated = {attributeName, reason}[].
    #[tokio::test]
    async fn update_result_and_not_updated_details_shape() {
        let app = router(AppState::new("t5218".into()));
        let body = serde_json::json!({"id": "urn:ngsi-ld:V:5218", "type": "Vehicle",
            "speed": {"type": "Property", "value": 1}})
        .to_string();
        let req = Request::post("/ngsi-ld/v1/entities")
            .header("Content-Type", "application/json")
            .header("Content-Length", body.len())
            .body(Body::from(body))
            .expect("req");
        assert_eq!(
            app.clone().oneshot(req).await.expect("r").status(),
            StatusCode::CREATED
        );
        // noOverwrite append: speed exists (skipped), brand is new (applied)
        let frag = serde_json::json!({
            "speed": {"type": "Property", "value": 2},
            "brand": {"type": "Property", "value": "x"}})
        .to_string();
        let req =
            Request::post("/ngsi-ld/v1/entities/urn:ngsi-ld:V:5218/attrs?options=noOverwrite")
                .header("Content-Type", "application/json")
                .header("Content-Length", frag.len())
                .body(Body::from(frag))
                .expect("req");
        let resp = app.oneshot(req).await.expect("resp");
        assert_eq!(resp.status(), StatusCode::MULTI_STATUS);
        let bytes = resp.into_body().collect().await.expect("body").to_bytes();
        let doc: serde_json::Value = serde_json::from_slice(&bytes).expect("json");
        let updated = doc["updated"].as_array().expect("updated");
        assert!(
            updated
                .iter()
                .any(|u| u.as_str().unwrap_or("").contains("brand")),
            "brand applied: {doc}"
        );
        let nu = doc["notUpdated"].as_array().expect("notUpdated");
        assert_eq!(nu.len(), 1, "{doc}");
        assert!(nu[0]["attributeName"]
            .as_str()
            .unwrap_or("")
            .contains("speed"));
        assert!(!nu[0]["reason"].as_str().unwrap_or("").is_empty());
        assert!(
            nu[0].get("registrationId").is_none(),
            "local failure carries no registrationId"
        );
    }
}

#[cfg(test)]
mod clause_5_2_21 {
    use super::*;
    use axum::body::Body;
    use axum::http::{Request, StatusCode};
    use http_body_util::BodyExt;
    use serde_json::json;
    use tower::ServiceExt;

    async fn temporal_query(qs: &str) -> StatusCode {
        let app = router(AppState::new("t5221".into()));
        let req = Request::get(format!(
            "/ngsi-ld/v1/temporal/entities?type=Vehicle&timerel=after&timeAt=2020-01-01T00:00:00Z{qs}"
        ))
        .body(Body::empty())
        .expect("req");
        app.oneshot(req).await.expect("resp").status()
    }

    /// Table 5.2.21-1: lastN is a POSITIVE integer; aggrMethods entries are
    /// limited to the 4.5.19 methods; endTimeAt is mandatory for between.
    #[tokio::test]
    async fn temporal_query_member_value_spaces() {
        assert_eq!(
            temporal_query("&lastN=0").await,
            StatusCode::BAD_REQUEST,
            "lastN=0"
        );
        assert_eq!(temporal_query("&lastN=-3").await, StatusCode::BAD_REQUEST);
        assert_eq!(temporal_query("&lastN=5").await, StatusCode::OK);
        assert_eq!(
            temporal_query("&aggrMethods=bogus").await,
            StatusCode::BAD_REQUEST
        );
        assert_eq!(
            temporal_query("&options=aggregatedValues").await,
            StatusCode::BAD_REQUEST,
            "aggregatedValues without aggrMethods"
        );
        assert_eq!(
            temporal_query("&options=aggregatedValues&aggrMethods=avg,max").await,
            StatusCode::OK
        );
        let app = router(AppState::new("t5221b".into()));
        let req = Request::get(
            "/ngsi-ld/v1/temporal/entities?type=V&timerel=between&timeAt=2020-01-01T00:00:00Z",
        )
        .body(Body::empty())
        .expect("req");
        assert_eq!(
            app.oneshot(req).await.expect("resp").status(),
            StatusCode::BAD_REQUEST,
            "between without endTimeAt"
        );
    }

    /// POST Query (5.2.23) carrying a temporalQ object (5.2.21 JSON form).
    async fn post_tq(tq: serde_json::Value, qs: &str) -> (StatusCode, String) {
        let app = router(AppState::new("t5221j".into()));
        let body = json!({
            "type": "Query",
            "entities": [{"type": "Vehicle"}],
            "temporalQ": tq
        })
        .to_string();
        let req = Request::post(format!("/ngsi-ld/v1/temporal/entityOperations/query{qs}"))
            .header("Content-Type", "application/json")
            .header("Content-Length", body.len())
            .body(Body::from(body))
            .expect("req");
        let resp = app.oneshot(req).await.expect("resp");
        let status = resp.status();
        let bytes = resp.into_body().collect().await.expect("body").to_bytes();
        (status, String::from_utf8_lossy(&bytes).into_owned())
    }

    /// Table 5.2.21-1 (JSON form): lastN is a positive INTEGER — zero,
    /// negative, fractional and string values are all outside the value
    /// space; timerel is limited to before/after/between; endTimeAt is
    /// mandatory for between; timeproperty is limited to the four 4.8 names.
    #[tokio::test]
    async fn temporal_query_json_member_value_spaces() {
        let ok = json!({"timerel": "after", "timeAt": "2020-01-01T00:00:00Z"});
        let with = |k: &str, v: serde_json::Value| {
            let mut t = ok.clone();
            t[k] = v;
            t
        };
        let (st, body) = post_tq(with("lastN", json!(5)), "").await;
        assert_eq!(st, StatusCode::OK, "{body}");
        assert!(!body.contains("BadRequestData"), "{body}");
        for bad in [json!(0), json!(-3), json!(2.5), json!("5")] {
            let (st, body) = post_tq(with("lastN", bad.clone()), "").await;
            assert_eq!(st, StatusCode::BAD_REQUEST, "lastN={bad}");
            assert!(body.contains("lastN"), "{body}");
        }
        let (st, _) = post_tq(with("timerel", json!("bogus")), "").await;
        assert_eq!(st, StatusCode::BAD_REQUEST);
        let (st, _) = post_tq(with("timerel", json!("between")), "").await;
        assert_eq!(st, StatusCode::BAD_REQUEST, "between without endTimeAt");
        let (st, _) = post_tq(with("timeproperty", json!("expiresAt")), "").await;
        assert_eq!(st, StatusCode::BAD_REQUEST, "timeproperty outside 4.8 set");
        let (st, _) = post_tq(with("timeAt", json!(20200101)), "").await;
        assert_eq!(st, StatusCode::BAD_REQUEST, "timeAt must be a string");
    }

    /// Table 5.2.21-1: aggrMethods (comma separated list of string — both
    /// the string and string-array spellings) and aggrPeriodDuration are
    /// carried by the JSON TemporalQuery and honoured when
    /// aggregatedValues is requested via format/options.
    #[tokio::test]
    async fn temporal_query_json_aggregation_members() {
        let base = json!({"timerel": "after", "timeAt": "2020-01-01T00:00:00Z"});
        let with = |k: &str, v: serde_json::Value| {
            let mut t = base.clone();
            t[k] = v;
            t
        };
        let (st, body) = post_tq(
            with("aggrMethods", json!(["avg", "max"])),
            "?format=aggregatedValues",
        )
        .await;
        assert_eq!(st, StatusCode::OK, "array aggrMethods honoured: {body}");
        let (st, body) = post_tq(
            with("aggrMethods", json!("avg,max")),
            "?format=aggregatedValues",
        )
        .await;
        assert_eq!(st, StatusCode::OK, "string aggrMethods honoured: {body}");
        let (st, body) = post_tq(
            with("aggrMethods", json!(["bogus"])),
            "?format=aggregatedValues",
        )
        .await;
        assert_eq!(st, StatusCode::BAD_REQUEST);
        assert!(body.contains("aggrMethods"), "{body}");
        let (st, _) = post_tq(with("aggrMethods", json!(42)), "?format=aggregatedValues").await;
        assert_eq!(st, StatusCode::BAD_REQUEST, "aggrMethods wrong JSON type");
        let mut t = base.clone();
        t["aggrMethods"] = json!(["avg"]);
        t["aggrPeriodDuration"] = json!("bogus");
        let (st, body) = post_tq(t, "?format=aggregatedValues").await;
        assert_eq!(st, StatusCode::BAD_REQUEST);
        assert!(body.contains("aggrPeriodDuration"), "{body}");
    }

    /// 5.2.21 used from Subscription.temporalQ (5.2.12, CSR subscriptions):
    /// timerel and timeAt are cardinality 1 — a temporalQ violating the
    /// data type is rejected at subscription creation.
    #[tokio::test]
    async fn subscription_temporal_q_validated() {
        async fn post_sub(tq: serde_json::Value) -> StatusCode {
            let app = router(AppState::new("t5221s".into()));
            let body = json!({
                "id": format!("urn:ngsi-ld:Subscription:5221-{}", tq.to_string().len()),
                "type": "Subscription",
                "entities": [{"type": "Building"}],
                "notification": {"endpoint": {"uri": "http://client.example.org/cb"}},
                "temporalQ": tq
            })
            .to_string();
            let req = Request::post("/ngsi-ld/v1/subscriptions")
                .header("Content-Type", "application/json")
                .header("Content-Length", body.len())
                .body(Body::from(body))
                .expect("req");
            app.oneshot(req).await.expect("resp").status()
        }
        assert_eq!(
            post_sub(json!({
                "timerel": "after",
                "timeAt": "2020-06-01T22:07:00Z",
                "timeproperty": "createdAt"
            }))
            .await,
            StatusCode::CREATED,
            "official fixture shape stays creatable"
        );
        assert_eq!(
            post_sub(json!({"timerel": "after"})).await,
            StatusCode::BAD_REQUEST,
            "timeAt is cardinality 1"
        );
        assert_eq!(
            post_sub(json!({"timeAt": "2020-06-01T22:07:00Z"})).await,
            StatusCode::BAD_REQUEST,
            "timerel is cardinality 1"
        );
        assert_eq!(
            post_sub(json!({"timerel": "bogus", "timeAt": "2020-06-01T22:07:00Z"})).await,
            StatusCode::BAD_REQUEST
        );
        assert_eq!(
            post_sub(json!("after")).await,
            StatusCode::BAD_REQUEST,
            "temporalQ must be an object"
        );
    }
}

#[cfg(test)]
mod clause_5_2_22 {
    use super::*;
    use axum::body::Body;
    use axum::http::{Request, StatusCode};
    use serde_json::json;
    use tower::ServiceExt;

    async fn post(path: &str, doc: serde_json::Value, tenant: &str) -> StatusCode {
        let app = router(AppState::new(tenant.into()));
        let body = doc.to_string();
        let req = Request::post(format!("/ngsi-ld/v1/{path}"))
            .header("Content-Type", "application/json")
            .header("Content-Length", body.len())
            .body(Body::from(body))
            .expect("req");
        app.oneshot(req).await.expect("resp").status()
    }

    /// Table 5.2.22-1: key AND value are Strings, both cardinality 1 —
    /// enforced on the notification endpoint's receiverInfo/notifierInfo.
    #[tokio::test]
    async fn endpoint_info_values_must_be_strings() {
        let sub = |info_key: &str, entries: serde_json::Value| {
            let mut d = json!({
                "type": "Subscription",
                "entities": [{"type": "Building"}],
                "notification": {"endpoint": {"uri": "http://client.example.org/cb"}}
            });
            d["notification"]["endpoint"][info_key] = entries;
            d
        };
        assert_eq!(
            post(
                "subscriptions",
                sub(
                    "receiverInfo",
                    json!([{"key": "Authorization", "value": "Bearer x"}])
                ),
                "t5222a"
            )
            .await,
            StatusCode::CREATED,
            "string values stay creatable"
        );
        for bad in [
            json!(42),
            json!({"a": 1}),
            json!(["x"]),
            json!(null),
            json!(true),
        ] {
            assert_eq!(
                post(
                    "subscriptions",
                    sub("receiverInfo", json!([{"key": "K", "value": bad}])),
                    "t5222a"
                )
                .await,
                StatusCode::BAD_REQUEST,
                "receiverInfo value {bad} is not a String"
            );
        }
        assert_eq!(
            post(
                "subscriptions",
                sub("notifierInfo", json!([{"key": "K", "value": 7}])),
                "t5222a"
            )
            .await,
            StatusCode::BAD_REQUEST,
            "notifierInfo value must be a String"
        );
    }

    /// Table 5.2.22-1 via 5.2.9 contextSourceInfo: every pair's value is a
    /// String — a non-string value on a custom key is rejected at
    /// registration, not at first forward.
    #[tokio::test]
    async fn context_source_info_values_must_be_strings() {
        let csr = |info: serde_json::Value| {
            json!({
                "type": "ContextSourceRegistration",
                "endpoint": "http://peer.example/ngsi-ld/v1",
                "information": [{"entities": [{"type": "Building"}]}],
                "contextSourceInfo": info
            })
        };
        assert_eq!(
            post(
                "csourceRegistrations",
                csr(json!([{"key": "X-Auth-Token", "value": "abc"}])),
                "t5222b"
            )
            .await,
            StatusCode::CREATED,
            "string values stay registrable"
        );
        for bad in [json!(123), json!(["a"]), json!({"v": 1}), json!(null)] {
            assert_eq!(
                post(
                    "csourceRegistrations",
                    csr(json!([{"key": "X-Custom", "value": bad}])),
                    "t5222b"
                )
                .await,
                StatusCode::BAD_REQUEST,
                "contextSourceInfo value {bad} is not a String"
            );
        }
        assert_eq!(
            post(
                "csourceRegistrations",
                csr(json!([{"key": 5, "value": "v"}])),
                "t5222b"
            )
            .await,
            StatusCode::BAD_REQUEST,
            "key must be a String"
        );
    }
}

#[cfg(test)]
mod clause_5_2_44 {
    use super::*;
    use axum::body::Body;
    use axum::http::{Request, StatusCode};
    use http_body_util::BodyExt;
    use serde_json::json;
    use tower::ServiceExt;

    /// Table 5.2.44-1: aggrParams carries aggrMethods (comma separated list
    /// of strings — string and string-array spellings) + aggrPeriodDuration,
    /// honoured on the temporal operation when aggregatedValues is requested.
    #[tokio::test]
    async fn aggregation_params_members() {
        let app = router(AppState::new("t5244".into()));
        let send = |doc: serde_json::Value, qs: &'static str| {
            let app = app.clone();
            async move {
                let body = doc.to_string();
                let req = Request::post(format!("/ngsi-ld/v1/temporal/entityOperations/query{qs}"))
                    .header("Content-Type", "application/json")
                    .header("Content-Length", body.len())
                    .body(Body::from(body))
                    .expect("req");
                let resp = app.oneshot(req).await.expect("resp");
                let status = resp.status();
                let bytes = resp.into_body().collect().await.expect("body").to_bytes();
                (status, String::from_utf8_lossy(&bytes).into_owned())
            }
        };
        let with_ap = |ap: serde_json::Value| {
            json!({"type": "Query", "entities": [{"type": "Vehicle"}],
                "temporalQ": {"timerel": "after", "timeAt": "2020-01-01T00:00:00Z"},
                "aggrParams": ap})
        };
        let (st, body) = send(
            with_ap(json!({"aggrMethods": ["avg"], "aggrPeriodDuration": "PT1H"})),
            "?format=aggregatedValues",
        )
        .await;
        assert_eq!(st, StatusCode::OK, "array spelling honoured: {body}");
        assert!(!body.contains("BadRequestData"), "{body}");
        let (st, _) = send(
            with_ap(json!({"aggrMethods": "avg,max"})),
            "?format=aggregatedValues",
        )
        .await;
        assert_eq!(st, StatusCode::OK, "string spelling honoured");
        let (st, body) = send(
            with_ap(json!({"aggrMethods": ["bogus"]})),
            "?format=aggregatedValues",
        )
        .await;
        assert_eq!(st, StatusCode::BAD_REQUEST);
        assert!(body.contains("aggrMethods"), "{body}");
        let (st, body) = send(
            with_ap(json!({"aggrMethods": ["avg"], "aggrPeriodDuration": "bogus"})),
            "?format=aggregatedValues",
        )
        .await;
        assert_eq!(st, StatusCode::BAD_REQUEST);
        assert!(body.contains("aggrPeriodDuration"), "{body}");
        let (st, _) = send(
            with_ap(json!({"aggrMethods": 42})),
            "?format=aggregatedValues",
        )
        .await;
        assert_eq!(st, StatusCode::BAD_REQUEST, "aggrMethods wrong JSON type");
        let (st, _) = send(with_ap(json!("avg")), "?format=aggregatedValues").await;
        assert_eq!(st, StatusCode::BAD_REQUEST, "aggrParams must be an object");
    }
}

#[cfg(test)]
mod clause_5_2_43 {
    use super::*;
    use axum::body::Body;
    use axum::http::{Request, StatusCode};
    use http_body_util::BodyExt;
    use serde_json::json;
    use tower::ServiceExt;

    async fn send(
        app: &axum::Router,
        method: &str,
        path: &str,
        doc: Option<serde_json::Value>,
    ) -> (StatusCode, String) {
        let mut b = Request::builder()
            .method(method)
            .uri(format!("/ngsi-ld/v1/{path}"));
        let body = match doc {
            Some(d) => {
                let s = d.to_string();
                b = b
                    .header("Content-Type", "application/json")
                    .header("Content-Length", s.len());
                Body::from(s)
            }
            None => Body::empty(),
        };
        let resp = app
            .clone()
            .oneshot(b.body(body).expect("req"))
            .await
            .expect("resp");
        let status = resp.status();
        let bytes = resp.into_body().collect().await.expect("body").to_bytes();
        (status, String::from_utf8_lossy(&bytes).into_owned())
    }

    /// Table 5.2.43-1: orderBy maps to the 4.23 keys, coordinates (JSON
    /// array, mandatory for dist ordering) + geometry (default Point) form
    /// the reference geometry; collation is rejected loudly while only
    /// codepoint order is offered (4.23.1 named gap).
    #[tokio::test]
    async fn ordering_params_members() {
        let app = router(AppState::new("t5243".into()));
        for (id, lon, lat) in [
            ("urn:ngsi-ld:Vehicle:near", 8.01, 40.01),
            ("urn:ngsi-ld:Vehicle:far", 10.0, 45.0),
        ] {
            let (st, body) = send(
                &app,
                "POST",
                "entities",
                Some(json!({"id": id, "type": "Vehicle",
                    "location": {"type": "GeoProperty",
                        "value": {"type": "Point", "coordinates": [lon, lat]}}})),
            )
            .await;
            assert_eq!(st, StatusCode::CREATED, "{body}");
        }
        let q = "entityOperations/query";
        let with_ordering = |o: serde_json::Value| json!({"type": "Query", "entities": [{"type": "Vehicle"}], "ordering": o});
        let (st, body) = send(
            &app,
            "POST",
            q,
            Some(with_ordering(json!({"orderBy": ["location;dist-asc"],
                "coordinates": [8, 40], "geometry": "Point"}))),
        )
        .await;
        assert_eq!(st, StatusCode::OK, "{body}");
        let near = body.find("urn:ngsi-ld:Vehicle:near").expect("near in body");
        let far = body.find("urn:ngsi-ld:Vehicle:far").expect("far in body");
        assert!(near < far, "dist-asc must order near before far: {body}");
        let (st, _) = send(
            &app,
            "POST",
            q,
            Some(with_ordering(json!({"orderBy": ["location;dist-asc"]}))),
        )
        .await;
        assert_eq!(
            st,
            StatusCode::BAD_REQUEST,
            "dist ordering without coordinates"
        );
        let (st, _) = send(
            &app,
            "POST",
            q,
            Some(with_ordering(json!({"orderBy": ["location;dist-asc"],
                "coordinates": "8,40"}))),
        )
        .await;
        assert_eq!(
            st,
            StatusCode::BAD_REQUEST,
            "coordinates must be a JSON array"
        );
        let (st, _) = send(
            &app,
            "POST",
            q,
            Some(with_ordering(json!({"orderBy": ["id;asc"], "geometry": 5}))),
        )
        .await;
        assert_eq!(st, StatusCode::BAD_REQUEST, "geometry must be a string");
        let (st, body) = send(
            &app,
            "POST",
            q,
            Some(with_ordering(json!({"orderBy": ["id;asc"],
                "collation": "de-u-co-phonebk"}))),
        )
        .await;
        assert_eq!(
            st,
            StatusCode::BAD_REQUEST,
            "unsupported collation is loud: {body}"
        );
        assert!(body.contains("collation"), "{body}");
        // GET twin: orderGeometry is a legal query parameter
        let (st, body) = send(
            &app,
            "GET",
            "entities?type=Vehicle&orderBy=location;dist-asc&orderFrom=[8,40]&orderGeometry=Point",
            None,
        )
        .await;
        assert_eq!(st, StatusCode::OK, "{body}");
    }
}

#[cfg(test)]
mod clause_5_2_34 {
    use super::*;
    use axum::body::Body;
    use axum::http::{Request, StatusCode};
    use serde_json::json;
    use tower::ServiceExt;

    async fn post_csr(management: serde_json::Value) -> StatusCode {
        let app = router(AppState::new("t5234".into()));
        let body = json!({
            "type": "ContextSourceRegistration",
            "endpoint": "http://peer.example/ngsi-ld/v1",
            "information": [{"entities": [{"type": "Building"}]}],
            "management": management
        })
        .to_string();
        let req = Request::post("/ngsi-ld/v1/csourceRegistrations")
            .header("Content-Type", "application/json")
            .header("Content-Length", body.len())
            .body(Body::from(body))
            .expect("req");
        app.oneshot(req).await.expect("resp").status()
    }

    /// Table 5.2.34-1: cacheDuration is an ISO 8601 duration, cooldown and
    /// timeout are numbers greater than 0, localOnly is a boolean, and the
    /// member itself is an object.
    #[tokio::test]
    async fn registration_management_info_value_spaces() {
        assert_eq!(
            post_csr(json!({"cacheDuration": "PT5M", "cooldown": 500,
                "timeout": 3000, "localOnly": true}))
            .await,
            StatusCode::CREATED,
            "conformant management info stays registrable"
        );
        for (label, m) in [
            ("non-object", json!("yes")),
            ("bad cacheDuration", json!({"cacheDuration": "bogus"})),
            ("cacheDuration wrong type", json!({"cacheDuration": 300})),
            ("cooldown zero", json!({"cooldown": 0})),
            ("timeout negative", json!({"timeout": -5})),
            ("timeout wrong type", json!({"timeout": "3000"})),
            ("localOnly wrong type", json!({"localOnly": "yes"})),
        ] {
            assert_eq!(
                post_csr(m).await,
                StatusCode::BAD_REQUEST,
                "management {label}"
            );
        }
    }
}

#[cfg(test)]
mod clause_5_2_33 {
    use super::*;
    use axum::body::Body;
    use axum::http::{Request, StatusCode};
    use http_body_util::BodyExt;
    use serde_json::json;
    use tower::ServiceExt;

    async fn send(app: &axum::Router, path: &str, doc: &serde_json::Value) -> (StatusCode, String) {
        let body = doc.to_string();
        let req = Request::post(format!("/ngsi-ld/v1/{path}"))
            .header("Content-Type", "application/json")
            .header("Content-Length", body.len())
            .body(Body::from(body))
            .expect("req");
        let resp = app.clone().oneshot(req).await.expect("resp");
        let status = resp.status();
        let bytes = resp.into_body().collect().await.expect("body").to_bytes();
        (status, String::from_utf8_lossy(&bytes).into_owned())
    }

    /// Table 5.2.33-1: id is "String or String[]" of valid URIs, type is
    /// mandatory, and "id takes precedence over idPattern".
    #[tokio::test]
    async fn entity_selector_id_forms_and_precedence() {
        let app = router(AppState::new("t5233a".into()));
        for id in [
            "urn:ngsi-ld:Vehicle:A1",
            "urn:ngsi-ld:Vehicle:A2",
            "urn:ngsi-ld:Vehicle:B1",
        ] {
            let (st, body) = send(
                &app,
                "entities",
                &json!({"id": id, "type": "Vehicle",
                        "speed": {"type": "Property", "value": 1}}),
            )
            .await;
            assert_eq!(st, StatusCode::CREATED, "{body}");
        }
        let q = "entityOperations/query";
        let sel = |e: serde_json::Value| json!({"type": "Query", "entities": [e]});
        let (st, body) = send(
            &app,
            q,
            &sel(json!({"type": "Vehicle",
                "id": ["urn:ngsi-ld:Vehicle:A1", "urn:ngsi-ld:Vehicle:A2"]})),
        )
        .await;
        assert_eq!(st, StatusCode::OK, "id array form: {body}");
        assert!(body.contains("urn:ngsi-ld:Vehicle:A1"), "{body}");
        assert!(body.contains("urn:ngsi-ld:Vehicle:A2"), "{body}");
        assert!(!body.contains("urn:ngsi-ld:Vehicle:B1"), "{body}");
        let (st, body) = send(
            &app,
            q,
            &sel(json!({"type": "Vehicle", "id": "urn:ngsi-ld:Vehicle:A1",
                "idPattern": "^urn:ngsi-ld:Vehicle:B.*$"})),
        )
        .await;
        assert_eq!(st, StatusCode::OK, "{body}");
        assert!(
            body.contains("urn:ngsi-ld:Vehicle:A1"),
            "id takes precedence over idPattern: {body}"
        );
        let (st, _) = send(&app, q, &sel(json!({"id": "urn:ngsi-ld:Vehicle:A1"}))).await;
        assert_eq!(st, StatusCode::BAD_REQUEST, "type is mandatory (5.2.33)");
        let (st, _) = send(
            &app,
            q,
            &sel(json!({"type": "Vehicle", "id": ["urn:ngsi-ld:Vehicle:A1", 5]})),
        )
        .await;
        assert_eq!(st, StatusCode::BAD_REQUEST, "id entries must be strings");
        let (st, _) = send(&app, q, &sel(json!({"type": "Vehicle", "id": "not a uri"}))).await;
        assert_eq!(st, StatusCode::BAD_REQUEST, "id must be a valid URI");
    }

    /// 5.2.33 in Subscription.entities: the String[] id form is accepted at
    /// creation.
    #[tokio::test]
    async fn subscription_selector_id_array() {
        let app = router(AppState::new("t5233b".into()));
        let sub = |e: serde_json::Value| {
            json!({"type": "Subscription", "entities": [e],
                "notification": {"endpoint": {"uri": "http://client.example.org/cb"}}})
        };
        let (st, body) = send(
            &app,
            "subscriptions",
            &sub(json!({"type": "Building",
                "id": ["urn:ngsi-ld:Building:a", "urn:ngsi-ld:Building:b"]})),
        )
        .await;
        assert_eq!(st, StatusCode::CREATED, "{body}");
        let (st, _) = send(
            &app,
            "subscriptions",
            &sub(json!({"type": "Building", "id": ["urn:ngsi-ld:Building:a", 5]})),
        )
        .await;
        assert_eq!(st, StatusCode::BAD_REQUEST);
    }
}

#[cfg(test)]
mod clause_5_2_23 {
    use super::*;
    use axum::body::Body;
    use axum::http::{Request, StatusCode};
    use http_body_util::BodyExt;
    use serde_json::json;
    use tower::ServiceExt;

    async fn send(
        app: &axum::Router,
        method: &str,
        path: &str,
        doc: &serde_json::Value,
    ) -> (StatusCode, String) {
        let body = doc.to_string();
        let req = Request::builder()
            .method(method)
            .uri(format!("/ngsi-ld/v1/{path}"))
            .header("Content-Type", "application/json")
            .header("Content-Length", body.len())
            .body(Body::from(body))
            .expect("req");
        let resp = app.clone().oneshot(req).await.expect("resp");
        let status = resp.status();
        let bytes = resp.into_body().collect().await.expect("body").to_bytes();
        (status, String::from_utf8_lossy(&bytes).into_owned())
    }

    fn query(extra: serde_json::Value) -> serde_json::Value {
        let mut d = json!({"type": "Query", "entities": [{"type": "Vehicle"}]});
        for (k, v) in extra.as_object().expect("obj") {
            d[k] = v.clone();
        }
        d
    }

    /// Table 5.2.23-1: member value spaces — empty arrays are not allowed
    /// (entities/attrs/pick/omit), string members must be strings, joinLevel
    /// is a positive integer, geoQ/ordering are objects, and
    /// temporalQ/aggrParams are only allowed on the temporal operation.
    #[tokio::test]
    async fn query_body_member_value_spaces() {
        let app = router(AppState::new("t5223a".into()));
        let q = "entityOperations/query";
        let (st, body) = send(&app, "POST", q, &query(json!({}))).await;
        assert_eq!(st, StatusCode::OK, "control: {body}");
        assert!(!body.contains("BadRequestData"), "{body}");
        for (label, doc) in [
            ("type not Query", json!({"type": "NotQuery"})),
            ("entities empty", json!({"type": "Query", "entities": []})),
            (
                "entities not array",
                query(json!({"entities": "Vehicle"})).clone(),
            ),
            ("attrs empty", query(json!({"attrs": []}))),
            ("attrs non-string entry", query(json!({"attrs": [7]}))),
            ("pick empty", query(json!({"pick": []}))),
            ("omit empty", query(json!({"omit": []}))),
            ("q not a string", query(json!({"q": 42}))),
            ("csf not a string", query(json!({"csf": 42}))),
            (
                "datasetId not an array",
                query(json!({"datasetId": "urn:x"})),
            ),
            (
                "joinLevel zero",
                query(json!({"join": "inline", "joinLevel": 0})),
            ),
            (
                "joinLevel fractional",
                query(json!({"join": "inline", "joinLevel": 2.5})),
            ),
            ("geoQ not an object", query(json!({"geoQ": "near"}))),
            ("ordering not an object", query(json!({"ordering": "asc"}))),
            (
                "splitEntities not a boolean",
                query(json!({"splitEntities": "yes"})),
            ),
            (
                "entityMap not a boolean",
                query(json!({"entityMap": "yes"})),
            ),
            (
                "temporalQ only for the temporal operation",
                query(json!({"temporalQ": {"timerel": "after", "timeAt": "2020-01-01T00:00:00Z"}})),
            ),
            (
                "aggrParams only for the temporal operation",
                query(json!({"aggrParams": {"aggrMethods": "avg"}})),
            ),
        ] {
            let (st, body) = send(&app, "POST", q, &doc).await;
            assert_eq!(st, StatusCode::BAD_REQUEST, "{label}: {body}");
        }
        // entities selector: non-object entries violate 5.2.33 EntitySelector[]
        let (st, _) = send(
            &app,
            "POST",
            q,
            &json!({"type": "Query", "entities": ["Vehicle"]}),
        )
        .await;
        assert_eq!(
            st,
            StatusCode::BAD_REQUEST,
            "entities entries must be objects"
        );
    }

    /// Table 5.2.23-1: q/pick/omit conveyed in the body are honoured exactly
    /// like their 6.3.7 query-parameter twins.
    #[tokio::test]
    async fn query_body_members_are_honoured() {
        let app = router(AppState::new("t5223b".into()));
        for (id, speed) in [
            ("urn:ngsi-ld:Vehicle:A1", 80),
            ("urn:ngsi-ld:Vehicle:A2", 120),
        ] {
            let (st, body) = send(
                &app,
                "POST",
                "entities",
                &json!({"id": id, "type": "Vehicle",
                        "speed": {"type": "Property", "value": speed},
                        "brand": {"type": "Property", "value": "Mercedes"}}),
            )
            .await;
            assert_eq!(st, StatusCode::CREATED, "{body}");
        }
        let q = "entityOperations/query";
        let (st, body) = send(&app, "POST", q, &query(json!({"q": "speed>100"}))).await;
        assert_eq!(st, StatusCode::OK, "{body}");
        assert!(body.contains("urn:ngsi-ld:Vehicle:A2"), "{body}");
        assert!(
            !body.contains("urn:ngsi-ld:Vehicle:A1"),
            "q must filter: {body}"
        );
        let (st, body) = send(&app, "POST", q, &query(json!({"pick": ["speed"]}))).await;
        assert_eq!(st, StatusCode::OK, "{body}");
        assert!(body.contains("speed"), "{body}");
        assert!(!body.contains("brand"), "pick must project: {body}");
        let (st, body) = send(&app, "POST", q, &query(json!({"omit": ["speed"]}))).await;
        assert_eq!(st, StatusCode::OK, "{body}");
        assert!(body.contains("brand"), "{body}");
        assert!(!body.contains("speed"), "omit must remove: {body}");
    }

    /// Table 5.2.23-1 on the temporal operation (5.7.4): the entities
    /// selector's id member selects, temporalQ is accepted, and containedBy
    /// is "Only applicable for the Retrieve Entity and Query Entities
    /// operations".
    #[tokio::test]
    async fn temporal_query_body_entity_selector() {
        let app = router(AppState::new("t5223c".into()));
        for id in ["urn:ngsi-ld:Vehicle:T1", "urn:ngsi-ld:Vehicle:T2"] {
            let (st, body) = send(
                &app,
                "POST",
                "temporal/entities",
                &json!({"id": id, "type": "Vehicle",
                        "speed": [{"type": "Property", "value": 1,
                                   "observedAt": "2020-08-01T12:00:00Z"}]}),
            )
            .await;
            assert!(st.is_success(), "{st} {body}");
        }
        let tq = json!({"timerel": "after", "timeAt": "2020-01-01T00:00:00Z"});
        let doc = json!({"type": "Query",
            "entities": [{"type": "Vehicle", "id": "urn:ngsi-ld:Vehicle:T1"}],
            "temporalQ": tq});
        let (st, body) = send(&app, "POST", "temporal/entityOperations/query", &doc).await;
        assert_eq!(st, StatusCode::OK, "{body}");
        assert!(body.contains("urn:ngsi-ld:Vehicle:T1"), "{body}");
        assert!(
            !body.contains("urn:ngsi-ld:Vehicle:T2"),
            "entities id selector must narrow the temporal query: {body}"
        );
        let doc = json!({"type": "Query", "entities": [{"type": "Vehicle"}],
            "temporalQ": tq, "containedBy": ["urn:ngsi-ld:Vehicle:T2"]});
        let (st, _) = send(&app, "POST", "temporal/entityOperations/query", &doc).await;
        assert_eq!(
            st,
            StatusCode::BAD_REQUEST,
            "containedBy is not applicable to 5.7.4"
        );
    }
}
