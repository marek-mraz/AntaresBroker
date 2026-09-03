// SPDX-License-Identifier: EUPL-1.2
//! NGSI-LD HTTP binding: axum routers, thin
//! handlers per spec operation.
#![cfg_attr(not(test), warn(clippy::expect_used))]
#![cfg_attr(test, allow(clippy::unwrap_used))]
// the Send proof of the spawned snapshot fill walks temporal → entity map
// futures deeper than the default 128 (nightly: recursion_depth_exceeding_limit)
#![recursion_limit = "256"]

/// Ask the policy engine about one operation (ADR-0020), where the request
/// enters its handler:
///
/// ```ignore
/// gate!(st, &tenant, headers, "5.6.6", ids: &[&id]).await?;
/// ```
///
/// The named members go into the [`policy::Operation`]; every other member
/// keeps the empty default of `Operation::new`. The awaited value is the
/// `Filter` the engine narrowed the operation to, or the refusal that
/// becomes a 403. It is a macro because a gate reads as one line at the
/// call site and the alternative is the same eight lines in every handler.
macro_rules! gate {
    ($st:expr, $tenant:expr, $headers:expr, $clause:expr $(, $field:ident: $value:expr)* $(,)?) => {
        $crate::policy::gate(
            $st.policy.as_ref(),
            &$crate::snapshots::asking_tenant(&$st, $tenant),
            $headers,
            &$crate::policy::Operation {
                $($field: $value,)*
                ..$crate::policy::Operation::new($clause)
            },
        )
    };
}

pub(crate) mod attrs;
pub(crate) mod batch;
pub mod bounds;
pub mod conformance;
pub(crate) mod contexts;
pub(crate) mod csource;
pub(crate) mod distsub;
pub mod egress;
pub(crate) mod entities;
pub(crate) mod entity_map;
pub(crate) mod entity_maps;
pub(crate) mod federation;
pub mod geo;
pub mod history;
pub mod mirror;
pub mod negotiate;
pub mod notify;
pub(crate) mod paging;
pub mod policy;
pub mod qeval;
pub(crate) mod regexcache;
pub(crate) mod registry;
pub(crate) mod repr;
pub(crate) mod snapshots;
pub(crate) mod stamp;
pub mod state;
pub(crate) mod subscriptions;
pub(crate) mod temporal;
pub(crate) mod temporalq;
pub(crate) mod types_attrs;

pub use antares_notifier::DeliveryPolicy;
pub use state::{AppState, TemporalRecord};

/// Spawn for both targets — tokio natively, the JS microtask queue on
/// wasm32 (no tokio runtime exists in a browser). Call sites are identical;
/// only the executor differs. Send is required natively (worker threads) and
/// meaningless on single-threaded wasm.
#[cfg(not(target_arch = "wasm32"))]
pub fn spawn<F>(fut: F)
where
    F: std::future::Future<Output = ()> + Send + 'static,
{
    BACKGROUND.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
    tokio::spawn(async move {
        fut.await;
        BACKGROUND.fetch_sub(1, std::sync::atomic::Ordering::SeqCst);
    });
}

/// Work a request left behind after its response — the remote leg of a
/// distributed subscription, an initial Context Source notification, a
/// forwarded notification, a retry. Counted so a shutdown drain waits for
/// it: a rolling update that killed these tasks left the chain half-built.
static BACKGROUND: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);

/// Request-born tasks still running.
pub fn background_tasks() -> usize {
    BACKGROUND.load(std::sync::atomic::Ordering::SeqCst)
}

/// A task that lives as long as the process (the matcher drain, the
/// interval tick): never counted, or a drain would wait on it forever.
#[cfg(not(target_arch = "wasm32"))]
pub(crate) fn spawn_loop<F>(fut: F)
where
    F: std::future::Future<Output = ()> + Send + 'static,
{
    tokio::spawn(fut);
}

#[cfg(target_arch = "wasm32")]
pub(crate) fn spawn_loop<F>(fut: F)
where
    F: std::future::Future<Output = ()> + 'static,
{
    wasm_bindgen_futures::spawn_local(fut);
}

#[cfg(target_arch = "wasm32")]
pub fn spawn<F>(fut: F)
where
    F: std::future::Future<Output = ()> + 'static,
{
    wasm_bindgen_futures::spawn_local(fut);
}

/// The browser build's notification channel: re-exported from the HTTP
/// binding, where it belongs — the page sink is how that binding delivers
/// when the runtime has no inbound socket.
#[cfg(target_arch = "wasm32")]
pub use antares_notifier::http::page_sink;

use antares_model::{NgsiError, TenantId, API_ROOT};
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::{delete, get, patch, post};
use axum::Router;
use negotiate::{echo_tenant, respond, tenant_from, ApiError};

/// 5.5.4 Fragment applied to a stored document resource — a Subscription
/// (5.8.2), a Context Source Registration (5.9.3) or a Context Source
/// Subscription (5.11.4). Each member of the already-normalized Fragment
/// replaces the target's; a member that normalization left as JSON null is
/// the removal form and takes the member out; `id` is never touched, and the
/// write stamps `modifiedAt` (4.8). Entities do NOT go through here: 5.5.12
/// Merge Patch descends into Attribute instances and value objects, which is
/// `entities::merge_instance`.
pub(crate) fn apply_doc_fragment(
    target: &mut serde_json::Map<String, serde_json::Value>,
    fragment: &serde_json::Map<String, serde_json::Value>,
    ts: &str,
) {
    for (k, v) in fragment {
        if k == "id" {
            continue;
        }
        if v.is_null() {
            target.remove(k);
        } else {
            target.insert(k.clone(), v.clone());
        }
    }
    target.insert(
        "modifiedAt".into(),
        serde_json::Value::String(ts.to_owned()),
    );
}

pub(crate) mod surface;
pub use surface::ApiSurface;

pub use antares_ql::scope::scope_matches;

/// 4.3.5 NGSI-LD API structure: Core API mandatory; Distributed API mandatory for
/// distributed/federated deployments; Temporal API and Registry API integrated
/// locally here (Table 4.3.5-2 row "integrated temporal + integrated Context
/// Registry"); JSONLDContext API implemented; optional Snapshot API offered
/// (5.16, resources 6.36-6.38, NGSILD-Snapshot scoping 6.3.22).
/// Ops-only router: what a pod WITHOUT the api role serves —
/// health, readiness and metrics, nothing else. The shipped antares-worker
/// Deployment used to answer the full read/write NGSI-LD API on its pod IP
/// (and a subscription created there was never KV-synced, because the sync
/// hooks are wired `if roles.api`); a worker now 404s the API surface.
pub fn ops_router(state: AppState) -> Router {
    // Admin only, deliberately: a worker widening its surface to whatever a
    // deployment registered is the same class of accident this router exists
    // to prevent.
    Router::new()
        .nest(Admin.prefix(), Admin.router(state.clone()))
        .with_state(state)
}

/// The broker's own operational surface: health, readiness, metrics, the
/// tenant list and purge, and the dead-letter admin. Gateway-protected like
/// every `/q` route — the broker adds no authentication of its own.
pub struct Admin;

impl Admin {
    /// Every path this surface mounts, and the only source of the route
    /// count `/q/health` reports. The destructuring in `router` binds one
    /// name per entry, so a route added to one and not the other does not
    /// compile.
    const PATHS: [&'static str; 8] = [
        "/health",
        "/ready",
        "/metrics",
        "/tenants",
        "/tenants/{tenant}",
        "/dead-letters",
        "/dead-letters/{id}",
        "/dead-letters/{id}/replay",
    ];
}

impl ApiSurface for Admin {
    fn name(&self) -> &str {
        "admin"
    }

    fn prefix(&self) -> &str {
        "/q"
    }

    fn router(&self, _st: AppState) -> Router<AppState> {
        let [health_p, ready_p, metrics_p, tenants_p, tenant_p, dl_p, dl_id_p, replay_p] =
            Self::PATHS;
        Router::new()
            .route(health_p, get(health))
            .route(ready_p, get(ready))
            // Prometheus text format. 404 until the broker installs the
            // renderer — the api crate never depends on an exporter.
            .route(metrics_p, get(metrics_endpoint))
            .route(tenants_p, get(tenants_list))
            .route(tenant_p, get(tenant_get).delete(tenant_purge))
            .route(dl_p, get(dead_letters_list))
            .route(dl_id_p, delete(dead_letter_delete))
            .route(replay_p, post(dead_letter_replay))
    }

    fn version_info(&self) -> serde_json::Value {
        serde_json::json!({"routes": Self::PATHS.len()})
    }
}

/// Installs the in-process pipeline on a state: the subscription mirror and
/// matcher (5.8.6), the interval firing, and the consumer half of a
/// distributed subscription (5.8.1.4) — a notification arriving on the
/// internal endpoint is handed to `distsub` from here, so the delivery path
/// never names it. Every root that serves requests calls this once.
pub fn wire(state: &mut AppState) {
    notify::wire_matcher(state);
    state.csource_notification = Some(std::sync::Arc::new(|st, tenant, own_id, reason, regs| {
        Box::pin(distsub::on_csource_notification(
            st, tenant, own_id, reason, regs,
        ))
    }));
}

/// An `AppState` with the notification pipeline installed, for tests that
/// exercise a route which notifies. The router does not wire it: a caller
/// that only reads never pays for the mirror seed.
#[cfg(any(test, feature = "test-kit"))]
pub fn wired_state(host_alias: &str) -> AppState {
    let mut st = AppState::new(host_alias.to_owned());
    wire(&mut st);
    st
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
            // GET is the 2.0 #14 pre-adoption
            get(entities::retrieve_entity_attr)
                .patch(attrs::partial_update_attr)
                .put(attrs::replace_attr)
                .delete(attrs::delete_attr),
        )
        // 2.0 #15 pre-adoption: the bare attribute value
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
        // EntityMaps (5.14; 6.32/6.34/6.35)
        .route(
            "/entityMaps",
            get(entity_maps::create_entity_map).post(entity_maps::create_entity_map_post),
        )
        .route(
            "/entityMaps/{id}",
            get(entity_maps::retrieve_entity_map)
                .patch(entity_maps::update_entity_map)
                .delete(entity_maps::delete_entity_map),
        )
        .route(
            "/temporal/entityMaps",
            get(entity_maps::create_temporal_entity_map)
                .post(entity_maps::create_temporal_entity_map_post),
        )
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
        // snapshots (5.16; 6.36-6.38, optional API group — offered)
        .route(
            "/snapshots",
            post(snapshots::create_snapshot).delete(snapshots::purge_snapshots),
        )
        .route(
            "/snapshots/{id}",
            get(snapshots::retrieve_snapshot)
                .patch(snapshots::update_snapshot)
                .delete(snapshots::delete_snapshot),
        )
        .route("/snapshots/{id}/clone", post(snapshots::clone_snapshot))
        // info (6.33)
        .route("/info/sourceIdentity", get(source_identity));

    // 5.8.1.4 consumer half: where forwarded subscription copies point their
    // notifications; remapped to the original subscriber. CIM 009 defines no
    // path for it — 5.2.15 makes a notification endpoint any URI, and 6.2
    // standardizes only what hangs under the API root — so it lives outside
    // the `/ngsi-ld` prefix ETSI owns, under this broker's own versioned
    // peer-facing root (ADR-0019). Being outside the API nest, it carries the
    // bounds wall and the body limit itself: a peer-facing write path must
    // not be the one route where the documented caps do not apply.
    let remote_notify = Router::new()
        .route("/ex/v1/remote-notify", post(distsub::remote_notify))
        .layer(axum::extract::DefaultBodyLimit::max(
            *bounds::MAX_BODY_BYTES,
        ))
        .layer(axum::middleware::from_fn_with_state(
            state.clone(),
            bounds::bounds_layer,
        ));

    // Every registered surface, each under its own reserved prefix — the
    // admin one by default. `with_surface` already refused a prefix outside
    // /q and /x and any overlap, so the merge here cannot shadow a route.
    // The caps are the broker's, not the API nest's: these routes carry the
    // bounds wall and the body limit for the same reason the peer-facing
    // notification endpoint above does — /q deletes Tenants and replays dead
    // letters, and a deployment's own /x routes are no less a way in.
    let mut surfaces = Router::new();
    for s in state.surfaces.iter() {
        surfaces = surfaces.nest(s.prefix(), s.router(state.clone()));
    }
    let surfaces = surfaces
        .layer(axum::extract::DefaultBodyLimit::max(
            *bounds::MAX_BODY_BYTES,
        ))
        .layer(axum::middleware::from_fn_with_state(
            state.clone(),
            bounds::bounds_layer,
        ));

    surfaces
        .merge(remote_notify)
        // 6.3.6/6.3.21: Prefer: ngsi-ld=<version> → 4.3.6.8 amendment +
        // Preference-Applied (+203 when altered) on every API response.
        // OPTIONS (2.0 #59 pre-adoption): axum's MethodRouter already
        // computes the exact per-route Allow set for its 405s — the layer
        // turns an OPTIONS 405 into 204 + that same Allow. HEAD (#58) needs
        // nothing: axum's get() serves HEAD natively.
        .nest(
            API_ROOT,
            api.layer(axum::middleware::from_fn(conformance::prefer_version_layer))
                // The temporal seam's drain: every write's history events
                // land in one driver call once the handler is done.
                .layer(axum::middleware::from_fn_with_state(
                    state.clone(),
                    history::layer,
                ))
                // 6.3.22 / 5.5.15: NGSILD-Snapshot resolves to the
                // snapshot's synthetic tenant, so the handlers below serve
                // the frozen copy.
                .layer(axum::middleware::from_fn_with_state(
                    state.clone(),
                    snapshots::snapshot_layer,
                ))
                // 5.5.10: non-create operations targeting a non-existing
                // Tenant answer NonexistentTenant 404; create operations
                // implicitly create the Tenant. Outside the snapshot layer,
                // so a client naming one of the broker's own internal
                // tenants is refused before that layer legitimately sets
                // one.
                .layer(axum::middleware::from_fn_with_state(
                    state.clone(),
                    tenant_exists_layer,
                ))
                // Bounds wall: URI length, body size, JSON depth — checked
                // before any parse (size-check-before-parse), and outside the
                // tenant and snapshot lookups so 6.3.4's bare 411/414 is not
                // spent on a store round-trip or masked by its 404.
                .layer(axum::middleware::from_fn_with_state(
                    state.clone(),
                    bounds::bounds_layer,
                ))
                // axum's built-in extractor limit defaults to 2 MiB and fires
                // BEFORE the bounds wall's documented cap — a 3 MiB body was
                // 413'd although /q/health advertises maxBodyBytes = 4 MiB
                // (found by mutation-testing the 413 wall). One
                // number governs both walls.
                .layer(axum::extract::DefaultBodyLimit::max(
                    *bounds::MAX_BODY_BYTES,
                ))
                // 6.3.14, last so it sees every exit: outside the snapshot
                // layer, which rewrites the request header to the snapshot's
                // internal tenant, and outside the bounds wall, whose bare
                // 411/414 leave through no handler at all.
                .layer(axum::middleware::from_fn(echo_tenant_layer)),
        )
        .fallback(not_found)
        // outermost on purpose: axum attaches the 405 Allow header in the
        // Router itself, above any nested layer — only a layer wrapping the
        // WHOLE router sees it.
        .layer(axum::middleware::from_fn(options_204))
        // Outermost so the duration covers the full stack.
        .layer(axum::middleware::from_fn(http_metrics_layer))
        // absent knob = no layer at all: a bare OPTIONS keeps its 204 + Allow
        .layer(tower::util::option_layer(cors_layer()))
        .with_state(state)
}

/// Browser access is off unless ANTARES_CORS_ORIGINS names the origins
/// (comma-separated, or `*` for any). NGSI-LD says nothing about CORS; the
/// Link header and the NGSILD-Tenant / NGSILD-Results-Count headers are
/// exposed so a browser client can read them.
fn cors_layer() -> Option<tower_http::cors::CorsLayer> {
    use tower_http::cors::{AllowOrigin, CorsLayer};
    let spec = std::env::var("ANTARES_CORS_ORIGINS")
        .ok()
        .filter(|s| !s.trim().is_empty())?;
    let origin = if spec.trim() == "*" {
        AllowOrigin::any()
    } else {
        AllowOrigin::list(
            spec.split(',')
                .filter_map(|o| o.trim().parse::<axum::http::HeaderValue>().ok()),
        )
    };
    let layer = CorsLayer::new()
        .allow_origin(origin)
        .allow_methods(tower_http::cors::Any)
        .allow_headers(tower_http::cors::Any)
        .expose_headers([
            axum::http::header::LINK,
            axum::http::HeaderName::from_static("ngsild-tenant"),
            axum::http::HeaderName::from_static("ngsild-results-count"),
        ]);
    Some(layer)
}

/// Tenants the broker mints for its own bookkeeping: the snapshot module's
/// internal and per-snapshot tenants share a prefix, the distributed
/// subscription inbound index is a single fixed name. A client-supplied
/// tenant may not name any of them — it would put request-shaped writes in
/// the same keyspace the broker keeps its own state in.
const INTERNAL_TENANT_PREFIX: &str = "snap-";
const INTERNAL_TENANTS: &[&str] = &["distsub-index"];

fn reserved_tenant(raw: &str) -> bool {
    raw.starts_with(INTERNAL_TENANT_PREFIX) || INTERNAL_TENANTS.contains(&raw)
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
    // 6.3.14: the tenant namespace the broker mints for itself is not a
    // legal client tenant. Snapshots run on internal tenants named
    // "snap-index" (the synthetic-tenant reverse index) and "snap-<uuid>"
    // (one per snapshot); a client naming one would read and delete another
    // tenant's snapshot bookkeeping. Rejected here, ahead of the snapshot
    // layer that legitimately rewrites the header to such a tenant.
    if let Some(raw) = req
        .headers()
        .get("NGSILD-Tenant")
        .and_then(|v| v.to_str().ok())
    {
        if reserved_tenant(raw) {
            return crate::negotiate::ApiError::from(NgsiError::BadRequestData(format!(
                "invalid NGSILD-Tenant value: {raw:?}"
            )))
            .into_response();
        }
    }
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
    // tenant (the per-tenant alias exists before any data does), and the
    // @context resources are keyed by local id across tenants (5.13) — a
    // Cached row belongs to no tenant, so the existence of the requesting
    // one decides nothing here. Ownership of a Hosted row is still enforced,
    // by `contexts::row_visible` on every serve, list and delete.
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

/// 6.3.14: "If the HTTP header `NGSILD-Tenant` is present in the HTTP
/// request, it shall also be present in HTTP response." The sentence admits
/// no exception, and the responses that most need it are the ones no handler
/// shapes: a rejection from the bounds wall, a 405, a body that never parsed.
/// An echo the handlers own is an echo some handler forgets, so it is read
/// once here, from the request the CALLER sent — the snapshot layer below
/// rewrites that header to the snapshot's internal tenant, which must never
/// reach the caller.
async fn echo_tenant_layer(
    req: axum::http::Request<axum::body::Body>,
    next: axum::middleware::Next,
) -> Response {
    let tenant = req
        .headers()
        .get("NGSILD-Tenant")
        .and_then(|v| v.to_str().ok())
        .and_then(|s| TenantId::new(s).ok());
    let mut resp = next.run(req).await;
    if let Some(t) = tenant {
        if !resp.headers().contains_key("NGSILD-Tenant") {
            crate::negotiate::echo_tenant(&t, &mut resp);
        }
    }
    resp
}

/// /q/metrics — Prometheus exposition, rendered by the closure the
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

/// Bounded method label for the request metrics: standard HTTP methods
/// pass through, anything else (extension methods are minted by the
/// client) collapses to "OTHER" so the label's cardinality stays bounded.
fn metric_method(m: &str) -> &'static str {
    match m {
        "GET" => "GET",
        "POST" => "POST",
        "PATCH" => "PATCH",
        "PUT" => "PUT",
        "DELETE" => "DELETE",
        "OPTIONS" => "OPTIONS",
        "HEAD" => "HEAD",
        _ => "OTHER",
    }
}

/// Request counter + duration histogram, labelled by method and status
/// class only (bounded cardinality).
async fn http_metrics_layer(
    req: axum::http::Request<axum::body::Body>,
    next: axum::middleware::Next,
) -> Response {
    let method = metric_method(req.method().as_str());
    // Clock rule: std Instant panics on wasm32.
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
    metrics::counter!("antares_http_requests_total", "method" => method, "status" => class)
        .increment(1);
    metrics::histogram!("antares_http_request_duration_seconds", "method" => method)
        .record(start.elapsed().as_secs_f64());
    resp
}

/// 2.0 #59 pre-adoption: OPTIONS → 204 No Content + the Allow set the
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

/// The build-time git hash (build.rs) — re-exported for --version.
pub const GIT_HASH: &str = env!("ANTARES_GIT_HASH");

/// Liveness plus the deployment's own description: which drivers are
/// running, what they are, every configured cap with its rejection counters,
/// the mounted surfaces and the notification schemes they can deliver to.
/// 503 while draining, so a load balancer stops routing here before the
/// listener stops accepting.
async fn health(
    axum::extract::State(state): axum::extract::State<AppState>,
) -> (StatusCode, axum::Json<serde_json::Value>) {
    // Drain: the load balancer decides on the STATUS CODE, so draining has
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
        "store": state.store_name,
        "temporal": state.temporal_name.as_deref().unwrap_or("none"),
        // Version surface: workspace version + build-time git hash
        // (build.rs), both asserted against the checkout by health_is_up.
        "version": env!("CARGO_PKG_VERSION"),
        "commit": env!("ANTARES_GIT_HASH"),
    });
    // What each driver runs on — engine, server version, extensions — so an
    // operator can tell two deployments of the same backend name apart. Both
    // seams answer from state captured at startup; a driver with nothing to
    // add answers an empty object and the key stays off the body.
    for (key, info) in [
        ("storeInfo", state.store.version_info()),
        ("temporalInfo", state.temporal.version_info()),
    ] {
        if info.as_object().is_some_and(|m| !m.is_empty()) {
            body[key] = info;
        }
    }
    // Where commits serialize behind one writer, the queue depth (current,
    // peak) is the signal that decides the group-commit lever. A driver
    // without such a committer reports nothing, so the branch is the
    // driver's, not a backend name read here.
    if let Some((depth, peak)) = state.store.commit_queue() {
        body["commitQueueDepth"] = depth.into();
        body["commitQueuePeak"] = peak.into();
    }
    // The policy engine this binary was started with, and how long it has
    // to answer before the seam denies for it (ADR-0020). An operator
    // reading `allow-all` here knows no engine is attached.
    body["policy"] = serde_json::json!({
        "engine": state.policy.name(),
        "timeoutMs": u64::try_from(policy::TIMEOUT.as_millis()).unwrap_or(u64::MAX),
    });
    // Configured caps + rejection counters, for observability.
    body["limits"] = state.limits.snapshot();
    // History events a driver failed to record after the write stood.
    body["temporalDrainErrors"] = history::drain_errors().into();
    // What this binary serves beside the NGSI-LD API: one entry per mounted
    // surface, its prefix and whatever the surface reports about itself.
    body["surfaces"] = serde_json::Value::Object(
        state
            .surfaces
            .iter()
            .map(|s| {
                let mut info = match s.version_info() {
                    serde_json::Value::Object(m) => m,
                    other => {
                        let mut m = serde_json::Map::new();
                        m.insert("info".into(), other);
                        m
                    }
                };
                info.insert("prefix".into(), s.prefix().into());
                (s.name().to_owned(), serde_json::Value::Object(info))
            })
            .collect(),
    );
    // Endpoint schemes this deployment can deliver notifications to: the
    // registered bindings (6.3.8, clause 7, and any a deployment added).
    // A subscription naming a scheme absent here is refused at creation.
    body["notificationSchemes"] = state.sinks.schemes().into();
    // Notifications the delivery policy gave up on (this process, since start).
    body["deadLetters"] = notify::dead_letters_written().into();
    // Changes the bounded matcher queue dropped (this process, since start).
    body["changesDropped"] = notify::changes_dropped().into();
    // Panics absorbed at the notification-task boundary, each one a change
    // whose notification was lost. Reported here and not only through the
    // metrics facade: the recorder is installed by ANTARES_TELEMETRY, which
    // is off by default, so this endpoint is where a default deployment can
    // see it at all.
    body["taskPanics"] = notify::task_panics().into();
    // Jemalloc heap stats (RSS ≈ live×1.2 is the target).
    if let Some(mem) = &state.mem_stats {
        body["memory"] = mem();
    }
    // Bus visibility: present only when the nats wiring installed the
    // provider — bus=local carries NO bus member.
    if let Some(bus) = &state.bus_stats {
        body["bus"] = bus();
    }
    (code, axum::Json(body))
}

/// The tenant an admin dead-letter call addresses: `?tenant=` (default
/// tenant when absent), grammar-checked, never a reserved internal one.
fn admin_tenant(q: &std::collections::HashMap<String, String>) -> Result<TenantId, NgsiError> {
    let raw = q.get("tenant").map_or(TenantId::DEFAULT, String::as_str);
    let bad = || NgsiError::BadRequestData(format!("invalid tenant: {raw:?}"));
    if reserved_tenant(raw) {
        return Err(bad());
    }
    TenantId::new(raw).map_err(|_| bad())
}

/// One dead letter as the listing may show it: everything on it that can be
/// a credential is blanked, the keys are not.
///
/// `receiverInfo` and `notifierInfo` are KeyValuePair arrays (5.2.22), and
/// the example content the datatype names is the HTTP Authentication header;
/// a letter written before the bindings moved behind the registry carries the
/// same values already rendered into `headers`. `notifierInfo` is opaque to
/// every sink but the endpoint's own, so the broker cannot tell which of its
/// keys names a secret and blanks them all. The stored letter keeps the
/// values a replay has to send — only what leaves through this route loses
/// them.
fn redact_letter(l: &mut serde_json::Value) {
    // Assigning through `Value`'s index panics on anything but an object,
    // and the row comes from the store rather than from this process.
    let Some(letter) = l.as_object_mut() else {
        return;
    };
    if let Some(u) = letter.get("uri").and_then(serde_json::Value::as_str) {
        let redacted = serde_json::Value::String(notify::redact_userinfo(u));
        letter.insert("uri".into(), redacted);
    }
    for member in ["receiverInfo", "notifierInfo", "headers"] {
        let Some(pairs) = letter
            .get_mut(member)
            .and_then(serde_json::Value::as_array_mut)
        else {
            continue;
        };
        for pair in pairs {
            if let Some(v) = pair.get_mut(1) {
                *v = serde_json::Value::String("[redacted]".into());
            }
        }
    }
}

/// Dead letters of one tenant, newest first; `subscription=` narrows to one
/// subscription, `limit=` bounds the page (default 100). The endpoint URI is
/// shown with its userinfo redacted and every credential the letter carries
/// is blanked.
async fn dead_letters_list(
    axum::extract::State(st): axum::extract::State<AppState>,
    axum::extract::Query(q): axum::extract::Query<std::collections::HashMap<String, String>>,
) -> Response {
    let tenant = match admin_tenant(&q) {
        Ok(t) => t,
        Err(e) => return ApiError::from(e).into_response(),
    };
    let limit = match q.get("limit") {
        None => 100usize,
        Some(v) => match v.parse::<usize>() {
            Ok(n) if n > 0 => n,
            _ => {
                return ApiError::from(NgsiError::BadRequestData(format!(
                    "limit must be a positive integer, got {v:?}"
                )))
                .into_response()
            }
        },
    };
    let mut letters = match st.store.list(&tenant, antares_store::Kind::DeadLetter) {
        Ok(l) => l,
        Err(e) => return ApiError::from(e).into_response(),
    };
    if let Some(sid) = q.get("subscription") {
        letters.retain(|l| l["subscriptionId"].as_str() == Some(sid.as_str()));
    }
    letters.sort_by(|a, b| b["lastAt"].as_str().cmp(&a["lastAt"].as_str()));
    letters.truncate(limit);
    for l in &mut letters {
        redact_letter(l);
    }
    (
        StatusCode::OK,
        axum::Json(serde_json::Value::Array(letters)),
    )
        .into_response()
}

/// Re-deliver one dead letter once through its own binding: 204 and the
/// letter is gone on success; 502 with the failure text and the letter
/// kept (attempt history extended) otherwise.
async fn dead_letter_replay(
    axum::extract::State(st): axum::extract::State<AppState>,
    axum::extract::Path(id): axum::extract::Path<String>,
    axum::extract::Query(q): axum::extract::Query<std::collections::HashMap<String, String>>,
) -> Response {
    use antares_store::{CurrentStateDriverExt as _, Kind};
    let tenant = match admin_tenant(&q) {
        Ok(t) => t,
        Err(e) => return ApiError::from(e).into_response(),
    };
    let letter = match st.store.get(&tenant, Kind::DeadLetter, &id) {
        Ok(Some(l)) => l,
        Ok(None) => {
            return ApiError::from(NgsiError::ResourceNotFound(format!("dead letter {id}")))
                .into_response()
        }
        Err(e) => return ApiError::from(e).into_response(),
    };
    match notify::replay_dead_letter(&st, &letter).await {
        Ok(()) => match st.store.delete(&tenant, Kind::DeadLetter, &id) {
            Ok(_) => StatusCode::NO_CONTENT.into_response(),
            Err(e) => ApiError::from(e).into_response(),
        },
        Err(why) => {
            let ts = state::now_iso();
            let w = why.clone();
            let _ = st.store.mutate(&tenant, Kind::DeadLetter, &id, move |d| {
                let Some(letter) = d.as_object_mut() else {
                    return Ok::<(), NgsiError>(());
                };
                let n = letter
                    .get("attempts")
                    .and_then(serde_json::Value::as_u64)
                    .unwrap_or(0)
                    + 1;
                letter.insert("attempts".into(), n.into());
                letter.insert("lastError".into(), serde_json::Value::String(w));
                letter.insert("lastAt".into(), serde_json::Value::String(ts));
                Ok::<(), NgsiError>(())
            });
            (
                StatusCode::BAD_GATEWAY,
                axum::Json(serde_json::json!({"detail": why, "id": id})),
            )
                .into_response()
        }
    }
}

/// Drop one dead letter without replaying it: 204 when it was there, 404
/// when it was not. The letter is the only copy of a notification the
/// delivery policy gave up on, so this is the one route that discards it.
async fn dead_letter_delete(
    axum::extract::State(st): axum::extract::State<AppState>,
    axum::extract::Path(id): axum::extract::Path<String>,
    axum::extract::Query(q): axum::extract::Query<std::collections::HashMap<String, String>>,
) -> Response {
    let tenant = match admin_tenant(&q) {
        Ok(t) => t,
        Err(e) => return ApiError::from(e).into_response(),
    };
    match st
        .store
        .delete(&tenant, antares_store::Kind::DeadLetter, &id)
    {
        Ok(true) => StatusCode::NO_CONTENT.into_response(),
        Ok(false) => {
            ApiError::from(NgsiError::ResourceNotFound(format!("dead letter {id}"))).into_response()
        }
        Err(e) => ApiError::from(e).into_response(),
    }
}

/// Tenant inventory: the names the backends know, sorted. Names only —
/// at the 10 000-tenant target (ADR-0001) a list carrying per-kind counts
/// would cost a count per kind per tenant, so a client picks a name here and
/// reads its detail from `GET /q/tenants/{tenant}`. Admin surface, never
/// under the API root.
async fn tenants_list(axum::extract::State(st): axum::extract::State<AppState>) -> Response {
    match st.store.tenant_ids() {
        Ok(ids) => (StatusCode::OK, axum::Json(ids)).into_response(),
        Err(e) => crate::negotiate::ApiError::from(e).into_response(),
    }
}

/// What one tenant holds: 400 for a name no request could carry, 404 when it
/// does not exist (5.5.10 keeps the default Tenant existing even when
/// empty), 200 with its counts otherwise. The path names the tenant; the
/// NGSILD-Tenant header never redirects the read.
async fn tenant_get(
    axum::extract::State(st): axum::extract::State<AppState>,
    axum::extract::Path(raw): axum::extract::Path<String>,
) -> Response {
    use crate::negotiate::ApiError;
    let bad = || {
        ApiError::from(NgsiError::BadRequestData(format!(
            "invalid tenant: {raw:?}"
        )))
        .into_response()
    };
    // the broker's internal snapshot tenants are not addressable
    if reserved_tenant(&raw) {
        return bad();
    }
    let Ok(tenant) = TenantId::new(&raw) else {
        return bad();
    };
    let stats = match st.store.tenant_stats_one(&tenant) {
        Ok(Some(s)) => s,
        Ok(None) => {
            return ApiError::from(NgsiError::ResourceNotFound(format!("tenant {raw}")))
                .into_response()
        }
        Err(e) => return ApiError::from(e).into_response(),
    };
    let instances = st.temporal.attr_instance_count(&tenant).unwrap_or(0);
    let mut row = serde_json::json!({
        "tenant": stats.tenant,
        "counts": {
            "entities": stats.entities,
            "subscriptions": stats.subscriptions,
            "csourceSubscriptions": stats.csource_subscriptions,
            "registrations": stats.registrations,
            "snapshots": stats.snapshots,
            "entityMaps": stats.entity_maps,
            "distSubs": stats.dist_subs,
            "attrInstances": instances,
        },
    });
    if let Some(c) = stats.created_at {
        row["createdAt"] = serde_json::Value::String(c);
    }
    (StatusCode::OK, axum::Json(row)).into_response()
}

/// Purge one tenant from both backends: 400 for a name no request could
/// carry, 404 when it does not exist (5.5.10), 409 while it still holds
/// distributed subscriptions (unsubscribe them first), 204 once every
/// document of it is gone. Deleted subscriptions and registrations are
/// announced to the bus mirrors the way an API delete is.
async fn tenant_purge(
    axum::extract::State(st): axum::extract::State<AppState>,
    axum::extract::Path(raw): axum::extract::Path<String>,
) -> Response {
    use crate::negotiate::ApiError;
    use antares_store::Kind;
    let bad = || {
        ApiError::from(NgsiError::BadRequestData(format!(
            "invalid tenant: {raw:?}"
        )))
        .into_response()
    };
    if reserved_tenant(&raw) {
        return bad();
    }
    let Ok(tenant) = TenantId::new(&raw) else {
        return bad();
    };
    match st.store.tenant_exists(&tenant) {
        Ok(true) => {}
        Ok(false) => {
            return ApiError::from(NgsiError::ResourceNotFound(format!("tenant {raw}")))
                .into_response()
        }
        Err(e) => return ApiError::from(e).into_response(),
    }
    // The paged walk, not `list`: the all-at-once read carries a ceiling
    // (the Postgres arm refuses past MAX_UNDECIDED_ROWS), and a purge behind
    // that ceiling means the Tenants most worth reclaiming are the ones that
    // can never be reclaimed — while their rows stay readable to anyone
    // sending the Tenant header.
    let ids = |kind: Kind| -> Result<Vec<String>, NgsiError> {
        let mut out = Vec::new();
        crate::csource::walk_docs(&st, &tenant, kind, |doc| {
            if let Some(id) = doc.get("id").and_then(|v| v.as_str()) {
                out.push(id.to_owned());
            }
            Ok(())
        })?;
        Ok(out)
    };
    let run = || -> Result<(), NgsiError> {
        // 5.8.1.4 stores one mapping document per distributed Subscription;
        // its `remotes` names the subscription copies that live AT context
        // sources. Those are deleted at their source by deleting the
        // Subscription (5.8.5.4), which a purge does not do, so a tenant
        // still holding them is refused rather than orphaning them. The
        // mapping shell of a Subscription no source has matched yet, and
        // the internal Registration Subscription beside it, hold nothing
        // remote and block nothing — the id-shaped read this used to do
        // saw neither, since a mapping document carries no `id` member.
        let mut remote_copies = false;
        crate::csource::walk_docs(&st, &tenant, Kind::DistSub, |doc| {
            if doc
                .get("remotes")
                .and_then(serde_json::Value::as_object)
                .is_some_and(|m| !m.is_empty())
            {
                remote_copies = true;
            }
            Ok(())
        })?;
        if remote_copies {
            return Err(NgsiError::Conflict(
                "tenant holds active distributed subscriptions".into(),
            ));
        }
        let subs = ids(Kind::Subscription)?;
        let csubs = ids(Kind::CSourceSubscription)?;
        let regs = ids(Kind::Registration)?;
        // 5.2.41: a Snapshot's isolated copy does not live under the Tenant.
        // It lives under the synthetic tenant its internal `__tenant` member
        // names, and the Snapshot document is the only pointer to it — so the
        // two purges below would delete that pointer and leave the copy
        // behind, reachable by nothing and freeable by nothing. Each snapshot
        // goes through the same removal a DELETE of it does, first.
        crate::csource::walk_docs(&st, &tenant, Kind::Snapshot, |meta| {
            if let Some(id) = meta.get("id").and_then(|v| v.as_str()) {
                crate::snapshots::snap_remove(&st, &tenant, id, &meta);
            }
            Ok(())
        })?;
        st.temporal.purge_tenant(&tenant)?;
        st.store.purge_tenant(&tenant)?;
        if let Some(sync) = &st.sub_sync {
            // Kept apart: the mirror entry is keyed per kind, so one id
            // naming both a Subscription and a Context Source Registration
            // Subscription needs a tombstone for each.
            for id in &subs {
                sync(&tenant, Kind::Subscription, id, None);
            }
            for id in &csubs {
                sync(&tenant, Kind::CSourceSubscription, id, None);
            }
        }
        if let Some(sync) = &st.reg_sync {
            for id in &regs {
                sync(&tenant, id, None);
            }
        }
        Ok(())
    };
    if let Err(e) = run() {
        return ApiError::from(e).into_response();
    }
    // 5.13.1: a Hosted @context belongs to the Tenant that stored it, and
    // the row carries that owner inside the document — `jsonld_contexts`
    // has no tenant column, so the tenant-keyed purge above cannot see it.
    if let Err(e) = crate::contexts::purge_tenant(&st, &tenant).await {
        return ApiError::from(e).into_response();
    }
    StatusCode::NO_CONTENT.into_response()
}

/// Readiness, distinct from `/q/health` liveness: ready = not draining AND
/// the store answers a trivial request AND (when bus=nats) the bus is
/// connected. On a Postgres failover or a NATS partition the pod is still
/// alive (liveness stays 200 — a restart fixes nothing) but must stop
/// receiving traffic, so the readinessProbe points HERE.
async fn ready(
    axum::extract::State(state): axum::extract::State<AppState>,
) -> (StatusCode, axum::Json<serde_json::Value>) {
    let draining = state.draining.load(std::sync::atomic::Ordering::Relaxed);
    let store_ok = state.store.ping().is_ok();
    let bus = state.bus_stats.as_ref().map(|b| b());
    let bus_ok = bus
        .as_ref()
        .is_none_or(|b| b.get("connected").and_then(serde_json::Value::as_bool) == Some(true));
    let ready = !draining && store_ok && bus_ok;
    let mut body = serde_json::json!({
        "status": if ready { "READY" } else { "NOT_READY" },
        "store": store_ok,
    });
    if let Some(b) = bus {
        body["bus"] = b;
    }
    let code = if ready {
        StatusCode::OK
    } else {
        StatusCode::SERVICE_UNAVAILABLE
    };
    (code, axum::Json(body))
}

/// CIM 009 5.15.1 / 6.33 — Context Source identity.
/// 5.15.1 Retrieve Context Source Identity Information: the 5.2.40
/// ContextSourceIdentity object for this source (per tenant in the
/// multi-tenancy case). The NotImplemented arm of 5.15.1.4 is vacuous here —
/// this broker can always supply its identity.
async fn source_identity(
    axum::extract::State(state): axum::extract::State<AppState>,
    headers: HeaderMap,
) -> Response {
    let go = async {
        let tenant = tenant_from(&headers)?;
        gate!(state, &tenant, &headers, "5.15.1").await?;
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

    /// The metrics method label is bounded: known HTTP methods pass
    /// through, client-minted extension methods collapse to "OTHER" so
    /// label cardinality cannot grow without bound.
    #[test]
    fn metric_method_label_is_bounded() {
        assert_eq!(metric_method("GET"), "GET");
        assert_eq!(metric_method("DELETE"), "DELETE");
        assert_eq!(metric_method("BAZQUX"), "OTHER");
        // methods are case-sensitive tokens (RFC 9110 9.1)
        assert_eq!(metric_method("get"), "OTHER");
    }

    #[tokio::test]
    async fn health_is_up() {
        let resp = app()
            .oneshot(Request::get("/q/health").body(Body::empty()).expect("req"))
            .await
            .expect("resp");
        assert_eq!(resp.status(), StatusCode::OK);
        let body = body_json(resp).await;
        assert_eq!(body["status"], "UP");
        // Version surface. The commit must be the one this binary was BUILT
        // from, so it is compared against the checkout rather than merely
        // checked for being a non-empty string: a build script that stops
        // rerunning bakes in a hash from an older commit, and /q/health then
        // names a build that is not the one running. Outside a git checkout
        // the build script reports "unknown" and there is nothing to compare.
        assert_eq!(body["version"], env!("CARGO_PKG_VERSION"));
        let head = std::process::Command::new("git")
            .args(["rev-parse", "--short", "HEAD"])
            .output()
            .ok()
            .filter(|o| o.status.success())
            .and_then(|o| String::from_utf8(o.stdout).ok());
        match head {
            Some(h) => assert_eq!(body["commit"], h.trim(), "stale build hash"),
            None => assert_eq!(body["commit"], "unknown"),
        }
    }

    /// /q/health names the temporal backend next to the store: the store's
    /// own mode when one instance serves both seams, `none` for NoTemporal.
    /// The deliverable endpoint schemes are the registered bindings and
    /// nothing else, so a client can tell what a subscription may name.
    #[tokio::test(flavor = "multi_thread")]
    async fn health_lists_the_registered_notification_schemes() {
        let st = AppState::new("h".into());
        let (code, body) = health(axum::extract::State(st)).await;
        assert_eq!(code, StatusCode::OK);
        let schemes = body.0["notificationSchemes"]
            .as_array()
            .expect("notificationSchemes")
            .iter()
            .filter_map(serde_json::Value::as_str)
            .collect::<Vec<_>>();
        assert!(
            schemes.contains(&"http") && schemes.contains(&"https"),
            "{schemes:?}"
        );
        #[cfg(feature = "mqtt")]
        assert!(
            schemes.contains(&"mqtt") && schemes.contains(&"mqtts"),
            "{schemes:?}"
        );
        assert!(
            !schemes.contains(&"ws"),
            "only registered bindings: {schemes:?}"
        );
    }

    #[tokio::test]
    async fn health_names_the_temporal_backend() {
        let body = body_json(
            app()
                .oneshot(Request::get("/q/health").body(Body::empty()).expect("req"))
                .await
                .expect("resp"),
        )
        .await;
        // whichever built-in store the harness composed, both seams name it
        let store = body["store"].as_str().expect("store name").to_owned();
        assert!(store == "memory" || store == "file", "store: {store}");
        assert_eq!(body["temporal"], store);

        let st = AppState::with_drivers(
            "antares-test".into(),
            std::sync::Arc::new(antares_sql::store::any::AnyStore::Mem(
                antares_sql::store::Store::default(),
            )),
            std::sync::Arc::new(antares_store::NoTemporal),
            "memory",
        );
        let body = body_json(
            router(st)
                .oneshot(Request::get("/q/health").body(Body::empty()).expect("req"))
                .await
                .expect("resp"),
        )
        .await;
        assert_eq!(body["store"], "memory");
        assert_eq!(body["temporal"], "none");
    }

    /// /q/health names the policy engine and the deadline one decision
    /// gets, so an operator can tell an allow-all broker from one an addon
    /// engine was registered into without reading the process environment.
    #[tokio::test]
    async fn health_names_the_policy_engine_and_its_timeout() {
        use policy::PolicyEngine as _;
        let body = body_json(
            app()
                .oneshot(Request::get("/q/health").body(Body::empty()).expect("req"))
                .await
                .expect("resp"),
        )
        .await;
        assert_eq!(body["policy"]["engine"], policy::AllowAll.name());
        assert_eq!(
            body["policy"]["timeoutMs"],
            u64::try_from(policy::TIMEOUT.as_millis()).unwrap_or(u64::MAX)
        );

        struct Named;
        impl policy::PolicyEngine for Named {
            fn name(&self) -> &str {
                "named-engine"
            }
            fn decide<'a>(
                &'a self,
                _s: &'a policy::Subject,
                _o: &'a policy::Operation<'a>,
            ) -> policy::DecisionFuture<'a> {
                Box::pin(async { policy::Decision::Allow })
            }
            fn pre_notify(
                &self,
                _s: &policy::Subject,
                _sub: &serde_json::Value,
                _n: &mut serde_json::Value,
            ) -> policy::NotifyDecision {
                policy::NotifyDecision::Deliver
            }
        }
        let st = AppState::new("h".into()).with_policy(std::sync::Arc::new(Named));
        let (_, body) = health(axum::extract::State(st)).await;
        assert_eq!(body.0["policy"]["engine"], "named-engine");
    }

    /// /q/health reports the bus — `bus: {mode,
    /// connected, reconnects}` when the nats wiring installed bus_stats,
    /// and the field is ABSENT for bus=local (no stats installed).
    #[tokio::test]
    async fn health_reports_the_bus_only_when_nats_is_wired() {
        // bus=local: no bus_stats ⇒ no `bus` member at all
        let resp = app()
            .oneshot(Request::get("/q/health").body(Body::empty()).expect("req"))
            .await
            .expect("resp");
        let body = body_json(resp).await;
        assert!(
            body.get("bus").is_none(),
            "bus member must be ABSENT for bus=local: {body}"
        );

        // nats wiring installs the closure ⇒ the live state is reported
        let mut st = AppState::new("antares-test".into());
        st.bus_stats = Some(std::sync::Arc::new(
            || serde_json::json!({"mode": "nats", "connected": true, "reconnects": 2}),
        ));
        let resp = router(st)
            .oneshot(Request::get("/q/health").body(Body::empty()).expect("req"))
            .await
            .expect("resp");
        let body = body_json(resp).await;
        assert_eq!(body["bus"]["mode"], "nats");
        assert_eq!(body["bus"]["connected"], true);
        assert_eq!(body["bus"]["reconnects"], 2);
    }

    /// /q/ready is READINESS — 200 on a healthy store, 503 the
    /// moment the bus reports disconnected (a pod that cannot process must
    /// stop receiving traffic while staying alive for liveness).
    #[tokio::test]
    async fn ready_gates_on_store_and_bus() {
        let resp = app()
            .oneshot(Request::get("/q/ready").body(Body::empty()).expect("req"))
            .await
            .expect("resp");
        assert_eq!(resp.status(), StatusCode::OK);
        assert_eq!(body_json(resp).await["status"], "READY");

        let mut st = AppState::new("antares-test".into());
        st.bus_stats = Some(std::sync::Arc::new(
            || serde_json::json!({"mode": "nats", "connected": false, "reconnects": 3}),
        ));
        let resp = router(st)
            .oneshot(Request::get("/q/ready").body(Body::empty()).expect("req"))
            .await
            .expect("resp");
        assert_eq!(
            resp.status(),
            StatusCode::SERVICE_UNAVAILABLE,
            "a disconnected bus must flip readiness"
        );
        assert_eq!(body_json(resp).await["status"], "NOT_READY");
    }

    /// A pod without the api role serves ops endpoints ONLY —
    /// the NGSI-LD surface must NOT be reachable on it.
    #[tokio::test]
    async fn ops_router_serves_no_ngsi_ld_surface() {
        let st = AppState::new("antares-test".into());
        let app = ops_router(st);
        let resp = app
            .clone()
            .oneshot(Request::get("/q/health").body(Body::empty()).expect("req"))
            .await
            .expect("resp");
        assert_eq!(resp.status(), StatusCode::OK);
        for path in ["/ngsi-ld/v1/entities", "/ngsi-ld/v1/subscriptions"] {
            let resp = app
                .clone()
                .oneshot(Request::get(path).body(Body::empty()).expect("req"))
                .await
                .expect("resp");
            assert_eq!(
                resp.status(),
                StatusCode::NOT_FOUND,
                "{path} must not exist on a worker pod"
            );
        }
    }

    /// 5.6.2.4: "no existing Entity whose id (URI), and where specified
    /// type, is equivalent … an error of type ResourceNotFound shall be
    /// raised" — the optional ?type selector (4.17) narrows the target.
    #[tokio::test]
    async fn clause_5_6_2_type_selector_gates_the_update() {
        let app = app();
        let entity = serde_json::json!({"id": "urn:ngsi-ld:B:sel", "type": "Building",
            "name": {"type": "Property", "value": "x"}});
        let body = entity.to_string();
        let resp = app
            .clone()
            .oneshot(
                Request::post("/ngsi-ld/v1/entities")
                    .header("Content-Type", "application/json")
                    .header("Content-Length", body.len())
                    .body(Body::from(body))
                    .expect("req"),
            )
            .await
            .expect("resp");
        assert_eq!(resp.status(), StatusCode::CREATED);
        let patch = |ty: &str| {
            let frag = serde_json::json!({"name": {"type": "Property", "value": "y"}}).to_string();
            Request::patch(format!(
                "/ngsi-ld/v1/entities/urn:ngsi-ld:B:sel/attrs?type={ty}"
            ))
            .header("Content-Type", "application/json")
            .header("Content-Length", frag.len())
            .body(Body::from(frag))
            .expect("req")
        };
        // wrong type: the target is "not known" under this selector → 404
        let resp = app.clone().oneshot(patch("Vehicle")).await.expect("resp");
        assert_eq!(resp.status(), StatusCode::NOT_FOUND);
        // matching type: update proceeds
        let resp = app.clone().oneshot(patch("Building")).await.expect("resp");
        assert_eq!(resp.status(), StatusCode::NO_CONTENT);
        // and the wrong-type attempt must NOT have written anything
        let resp = app
            .oneshot(
                Request::get("/ngsi-ld/v1/entities/urn:ngsi-ld:B:sel")
                    .body(Body::empty())
                    .expect("req"),
            )
            .await
            .expect("resp");
        let doc = body_json(resp).await;
        assert_eq!(doc["name"]["value"], "y");
    }

    /// 5.6.3.4: append honours the ?type selector the same way — a
    /// mismatching selector means the entity is not known (404); overwrite
    /// vs noOverwrite semantics are 5.5.8's merge_instance_sets.
    #[tokio::test]
    async fn clause_5_6_3_type_selector_gates_the_append() {
        let app = app();
        let entity = serde_json::json!({"id": "urn:ngsi-ld:B:app", "type": "Building"});
        let body = entity.to_string();
        let resp = app
            .clone()
            .oneshot(
                Request::post("/ngsi-ld/v1/entities")
                    .header("Content-Type", "application/json")
                    .header("Content-Length", body.len())
                    .body(Body::from(body))
                    .expect("req"),
            )
            .await
            .expect("resp");
        assert_eq!(resp.status(), StatusCode::CREATED);
        let append = |ty: &str| {
            let frag = serde_json::json!({"name": {"type": "Property", "value": "z"}}).to_string();
            Request::post(format!(
                "/ngsi-ld/v1/entities/urn:ngsi-ld:B:app/attrs?type={ty}"
            ))
            .header("Content-Type", "application/json")
            .header("Content-Length", frag.len())
            .body(Body::from(frag))
            .expect("req")
        };
        let resp = app.clone().oneshot(append("Vehicle")).await.expect("resp");
        assert_eq!(resp.status(), StatusCode::NOT_FOUND, "wrong type selector");
        let resp = app.clone().oneshot(append("Building")).await.expect("resp");
        assert_eq!(resp.status(), StatusCode::NO_CONTENT);
        let resp = app
            .oneshot(
                Request::get("/ngsi-ld/v1/entities/urn:ngsi-ld:B:app")
                    .body(Body::empty())
                    .expect("req"),
            )
            .await
            .expect("resp");
        let doc = body_json(resp).await;
        assert_eq!(doc["name"]["value"], "z");
    }

    /// 5.6.4.4: "If the target Attribute is scope, then an error of type
    /// BadRequestData shall be raised"; the ?type selector narrows the
    /// target entity (404 on mismatch).
    #[tokio::test]
    async fn clause_5_6_4_scope_target_and_type_selector() {
        let app = app();
        let entity = serde_json::json!({"id": "urn:ngsi-ld:B:pu", "type": "Building",
            "scope": "/Madrid", "name": {"type": "Property", "value": "x"}});
        let body = entity.to_string();
        let resp = app
            .clone()
            .oneshot(
                Request::post("/ngsi-ld/v1/entities")
                    .header("Content-Type", "application/json")
                    .header("Content-Length", body.len())
                    .body(Body::from(body))
                    .expect("req"),
            )
            .await
            .expect("resp");
        assert_eq!(resp.status(), StatusCode::CREATED);
        let patch = |attr: &str, q: &str| {
            let frag = serde_json::json!({"value": "y"}).to_string();
            Request::patch(format!(
                "/ngsi-ld/v1/entities/urn:ngsi-ld:B:pu/attrs/{attr}{q}"
            ))
            .header("Content-Type", "application/json")
            .header("Content-Length", frag.len())
            .body(Body::from(frag))
            .expect("req")
        };
        // the scope pseudo-attribute is not partially updatable
        let resp = app.clone().oneshot(patch("scope", "")).await.expect("resp");
        assert_eq!(
            resp.status(),
            StatusCode::BAD_REQUEST,
            "scope target is 400"
        );
        // type selector gates the partial update
        let resp = app
            .clone()
            .oneshot(patch("name", "?type=Vehicle"))
            .await
            .expect("resp");
        assert_eq!(resp.status(), StatusCode::NOT_FOUND);
        let resp = app
            .clone()
            .oneshot(patch("name", "?type=Building"))
            .await
            .expect("resp");
        assert_eq!(resp.status(), StatusCode::NO_CONTENT);
        let resp = app
            .oneshot(
                Request::get("/ngsi-ld/v1/entities/urn:ngsi-ld:B:pu")
                    .body(Body::empty())
                    .expect("req"),
            )
            .await
            .expect("resp");
        let doc = body_json(resp).await;
        assert_eq!(doc["name"]["value"], "y");
        assert_eq!(doc["scope"], "/Madrid", "scope untouched");
    }

    /// 5.6.5.4: the ?type selector narrows the delete target — mismatch
    /// means the entity is not known (404) and nothing is deleted.
    #[tokio::test]
    async fn clause_5_6_5_type_selector_gates_the_delete() {
        let app = app();
        let entity = serde_json::json!({"id": "urn:ngsi-ld:B:del", "type": "Building",
            "name": {"type": "Property", "value": "x"}});
        let body = entity.to_string();
        let resp = app
            .clone()
            .oneshot(
                Request::post("/ngsi-ld/v1/entities")
                    .header("Content-Type", "application/json")
                    .header("Content-Length", body.len())
                    .body(Body::from(body))
                    .expect("req"),
            )
            .await
            .expect("resp");
        assert_eq!(resp.status(), StatusCode::CREATED);
        let del = |q: &str| {
            Request::delete(format!(
                "/ngsi-ld/v1/entities/urn:ngsi-ld:B:del/attrs/name{q}"
            ))
            .body(Body::empty())
            .expect("req")
        };
        let resp = app
            .clone()
            .oneshot(del("?type=Vehicle"))
            .await
            .expect("resp");
        assert_eq!(resp.status(), StatusCode::NOT_FOUND);
        // the attribute must still exist after the gated attempt
        let resp = app
            .clone()
            .oneshot(
                Request::get("/ngsi-ld/v1/entities/urn:ngsi-ld:B:del")
                    .body(Body::empty())
                    .expect("req"),
            )
            .await
            .expect("resp");
        assert_eq!(body_json(resp).await["name"]["value"], "x");
        let resp = app
            .clone()
            .oneshot(del("?type=Building"))
            .await
            .expect("resp");
        assert_eq!(resp.status(), StatusCode::NO_CONTENT);
        let resp = app
            .oneshot(
                Request::get("/ngsi-ld/v1/entities/urn:ngsi-ld:B:del")
                    .body(Body::empty())
                    .expect("req"),
            )
            .await
            .expect("resp");
        assert!(
            body_json(resp).await.get("name").is_none(),
            "attribute deleted under the matching selector"
        );
    }

    /// 5.6.6.4: the ?type selector narrows the delete-entity target — a
    /// mismatch means the entity is not known (404) and nothing is deleted.
    #[tokio::test]
    async fn clause_5_6_6_type_selector_gates_entity_delete() {
        let app = app();
        let entity = serde_json::json!({"id": "urn:ngsi-ld:B:edel", "type": "Building"});
        let body = entity.to_string();
        let resp = app
            .clone()
            .oneshot(
                Request::post("/ngsi-ld/v1/entities")
                    .header("Content-Type", "application/json")
                    .header("Content-Length", body.len())
                    .body(Body::from(body))
                    .expect("req"),
            )
            .await
            .expect("resp");
        assert_eq!(resp.status(), StatusCode::CREATED);
        let del = |q: &str| {
            Request::delete(format!("/ngsi-ld/v1/entities/urn:ngsi-ld:B:edel{q}"))
                .body(Body::empty())
                .expect("req")
        };
        let resp = app
            .clone()
            .oneshot(del("?type=Vehicle"))
            .await
            .expect("resp");
        assert_eq!(
            resp.status(),
            StatusCode::NOT_FOUND,
            "wrong selector is 404"
        );
        let resp = app
            .clone()
            .oneshot(
                Request::get("/ngsi-ld/v1/entities/urn:ngsi-ld:B:edel")
                    .body(Body::empty())
                    .expect("req"),
            )
            .await
            .expect("resp");
        assert_eq!(
            resp.status(),
            StatusCode::OK,
            "entity survives the gated delete"
        );
        let resp = app
            .clone()
            .oneshot(del("?type=Building"))
            .await
            .expect("resp");
        assert_eq!(resp.status(), StatusCode::NO_CONTENT);
        let resp = app
            .oneshot(
                Request::get("/ngsi-ld/v1/entities/urn:ngsi-ld:B:edel")
                    .body(Body::empty())
                    .expect("req"),
            )
            .await
            .expect("resp");
        assert_eq!(resp.status(), StatusCode::NOT_FOUND);
    }

    /// 5.6.7.4: "If the input Array is empty or contains a null value in
    /// any of its items an error of type BadRequestData shall be raised" —
    /// the WHOLE request fails, nothing is created.
    #[tokio::test]
    async fn clause_5_6_7_null_item_fails_the_whole_batch() {
        let app = app();
        let body = r#"[{"id":"urn:ngsi-ld:V:nb1","type":"Vehicle"}, null]"#;
        let resp = app
            .clone()
            .oneshot(
                Request::post("/ngsi-ld/v1/entityOperations/create")
                    .header("Content-Type", "application/json")
                    .header("Content-Length", body.len())
                    .body(Body::from(body))
                    .expect("req"),
            )
            .await
            .expect("resp");
        assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
        // and the non-null sibling must NOT have been created
        let resp = app
            .oneshot(
                Request::get("/ngsi-ld/v1/entities/urn:ngsi-ld:V:nb1")
                    .body(Body::empty())
                    .expect("req"),
            )
            .await
            .expect("resp");
        assert_eq!(resp.status(), StatusCode::NOT_FOUND);
        // the empty array is 400 too
        let resp = crate::tests::app()
            .oneshot(
                Request::post("/ngsi-ld/v1/entityOperations/create")
                    .header("Content-Type", "application/json")
                    .header("Content-Length", 2)
                    .body(Body::from("[]"))
                    .expect("req"),
            )
            .await
            .expect("resp");
        assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
    }

    /// 5.6.11.4: an upsert onto an existing Temporal Evolution adds the
    /// instances AND unions new Entity Type names; a deletion-null instance
    /// (the 4.5.7 representation) is legal temporal input.
    #[tokio::test]
    async fn clause_5_6_11_temporal_upsert_type_union_and_null_instances() {
        let app = app();
        let post = |body: String| {
            Request::post("/ngsi-ld/v1/temporal/entities")
                .header("Content-Type", "application/json")
                .header("Content-Length", body.len())
                .body(Body::from(body))
                .expect("req")
        };
        let first = serde_json::json!({
            "id": "urn:ngsi-ld:V:tu", "type": "Vehicle",
            "speed": [{"type": "Property", "value": 1,
                       "observedAt": "2026-01-01T00:00:00Z"}]
        });
        let resp = app
            .clone()
            .oneshot(post(first.to_string()))
            .await
            .expect("resp");
        assert_eq!(resp.status(), StatusCode::CREATED);
        // second upsert: new type name + a deleted-instance representation
        let second = serde_json::json!({
            "id": "urn:ngsi-ld:V:tu", "type": ["Vehicle", "Truck"],
            "speed": [{"type": "Property", "value": "urn:ngsi-ld:null",
                       "observedAt": "2026-01-02T00:00:00Z"}]
        });
        let resp = app
            .clone()
            .oneshot(post(second.to_string()))
            .await
            .expect("resp");
        assert_eq!(
            resp.status(),
            StatusCode::NO_CONTENT,
            "deletion-null instances are legal temporal input (4.5.7/5.5.4)"
        );
        let resp = app
            .oneshot(
                Request::get("/ngsi-ld/v1/temporal/entities/urn:ngsi-ld:V:tu?timerel=before&timeAt=2030-01-01T00:00:00Z")
                    .body(Body::empty())
                    .expect("req"),
            )
            .await
            .expect("resp");
        assert_eq!(resp.status(), StatusCode::OK);
        let doc = body_json(resp).await;
        let types: Vec<&str> = doc["type"]
            .as_array()
            .map(|a| a.iter().filter_map(serde_json::Value::as_str).collect())
            .unwrap_or_else(|| vec![doc["type"].as_str().unwrap_or_default()]);
        assert!(
            types.contains(&"Vehicle") && types.contains(&"Truck"),
            "type names are unioned: {:?}",
            doc["type"]
        );
        // both instances present — the deletion representation included
        let speed = doc["speed"].as_array().expect("speed instances");
        assert_eq!(speed.len(), 2, "history keeps both instances: {doc}");
    }

    /// 5.6.19.4: replacing scope is BadRequestData; the ?type selector
    /// narrows the target (404 on mismatch, attribute untouched).
    #[tokio::test]
    async fn clause_5_6_19_replace_attr_scope_and_type_selector() {
        let app = app();
        let entity = serde_json::json!({"id": "urn:ngsi-ld:B:ra", "type": "Building",
            "scope": "/Madrid", "name": {"type": "Property", "value": "x"}});
        let body = entity.to_string();
        let resp = app
            .clone()
            .oneshot(
                Request::post("/ngsi-ld/v1/entities")
                    .header("Content-Type", "application/json")
                    .header("Content-Length", body.len())
                    .body(Body::from(body))
                    .expect("req"),
            )
            .await
            .expect("resp");
        assert_eq!(resp.status(), StatusCode::CREATED);
        let put = |attr: &str, q: &str| {
            let frag = serde_json::json!({"type": "Property", "value": "y"}).to_string();
            Request::put(format!(
                "/ngsi-ld/v1/entities/urn:ngsi-ld:B:ra/attrs/{attr}{q}"
            ))
            .header("Content-Type", "application/json")
            .header("Content-Length", frag.len())
            .body(Body::from(frag))
            .expect("req")
        };
        let resp = app.clone().oneshot(put("scope", "")).await.expect("resp");
        assert_eq!(
            resp.status(),
            StatusCode::BAD_REQUEST,
            "scope target is 400"
        );
        let resp = app
            .clone()
            .oneshot(put("name", "?type=Vehicle"))
            .await
            .expect("resp");
        assert_eq!(resp.status(), StatusCode::NOT_FOUND);
        let resp = app
            .clone()
            .oneshot(put("name", "?type=Building"))
            .await
            .expect("resp");
        assert_eq!(resp.status(), StatusCode::NO_CONTENT);
        let resp = app
            .oneshot(
                Request::get("/ngsi-ld/v1/entities/urn:ngsi-ld:B:ra")
                    .body(Body::empty())
                    .expect("req"),
            )
            .await
            .expect("resp");
        let doc = body_json(resp).await;
        assert_eq!(doc["name"]["value"], "y");
        assert_eq!(doc["scope"], "/Madrid", "scope untouched");
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

    /// 5.5.11.0: "All Entities and Attributes in the batch will get the same
    /// modifiedAt timestamp, so it makes sense to distinguish them via the
    /// observedAt temporal property." One batch is one instant. Reading the
    /// clock per entity spreads a large create over several milliseconds, and
    /// a Context Consumer that filters or pages on modifiedAt then sees the
    /// batch split in two — half of it before its own cursor, half after.
    #[tokio::test]
    async fn clause_5_5_11_0_one_batch_create_carries_one_timestamp() {
        let app = app();
        // Enough documents that per-entity stamping cannot stay inside one
        // millisecond: the spread is what the clause forbids, not the count.
        let batch: Vec<serde_json::Value> = (0..500)
            .map(|i| {
                serde_json::json!({
                    "id": format!("urn:ngsi-ld:Building:ts-{i}"),
                    "type": "Building",
                    "speed": {"type": "Property", "value": i},
                    "name": {"type": "Property", "value": "x".repeat(64)},
                    "near": {"type": "Relationship",
                             "object": "urn:ngsi-ld:Building:other"},
                })
            })
            .collect();
        let body = serde_json::Value::Array(batch).to_string();
        let resp = app
            .clone()
            .oneshot(
                Request::post("/ngsi-ld/v1/entityOperations/create")
                    .header("Content-Type", "application/json")
                    .header("Content-Length", body.len())
                    .body(Body::from(body))
                    .expect("req"),
            )
            .await
            .expect("resp");
        assert_eq!(resp.status(), StatusCode::CREATED);
        let resp = app
            .oneshot(
                Request::get("/ngsi-ld/v1/entities?type=Building&options=sysAttrs&limit=1000")
                    .body(Body::empty())
                    .expect("req"),
            )
            .await
            .expect("resp");
        assert_eq!(resp.status(), StatusCode::OK);
        let list = body_json(resp).await;
        let ents = list.as_array().expect("entity array");
        assert_eq!(ents.len(), 500, "every entity was created");
        // Entity level and Attribute level: stamp_new writes the same instant
        // into both, so one batch may produce exactly one value overall.
        let mut stamps: std::collections::BTreeSet<String> = std::collections::BTreeSet::new();
        for e in ents {
            let o = e.as_object().expect("entity object");
            for key in ["createdAt", "modifiedAt"] {
                stamps.insert(
                    o.get(key)
                        .and_then(serde_json::Value::as_str)
                        .unwrap_or_else(|| panic!("entity {key} is served under sysAttrs"))
                        .to_owned(),
                );
            }
            for (k, v) in o {
                if matches!(
                    k.as_str(),
                    "id" | "type" | "@context" | "createdAt" | "modifiedAt"
                ) {
                    continue;
                }
                for key in ["createdAt", "modifiedAt"] {
                    stamps.insert(
                        v.get(key)
                            .and_then(serde_json::Value::as_str)
                            .unwrap_or_else(|| panic!("attribute {k} carries {key}"))
                            .to_owned(),
                    );
                }
            }
        }
        assert_eq!(
            stamps.len(),
            1,
            "one batch, one timestamp; got {} distinct: {:?}",
            stamps.len(),
            stamps
        );
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

    /// 6.3.4 answers an over-long URI with a bare 414 and an over-large body
    /// with 413. Both are preconditions on the request itself, so they must
    /// not be spent on — or masked by — the 5.5.10 tenant lookup.
    #[tokio::test]
    async fn the_bounds_wall_answers_before_the_tenant_lookup() {
        let app = app();
        let long = "x".repeat(crate::bounds::MAX_URI_BYTES + 1);
        let resp = app
            .clone()
            .oneshot(
                Request::get(format!("/ngsi-ld/v1/entities?type=T&idPattern={long}"))
                    .header("NGSILD-Tenant", "ghost")
                    .body(Body::empty())
                    .expect("req"),
            )
            .await
            .expect("resp");
        assert_eq!(
            resp.status(),
            StatusCode::URI_TOO_LONG,
            "the URI precondition decides, not the unknown tenant"
        );

        let big = vec![b'a'; *crate::bounds::MAX_BODY_BYTES + 1];
        let resp = app
            .oneshot(
                Request::post("/ngsi-ld/v1/entities")
                    .header("Content-Type", "application/json")
                    .header("NGSILD-Tenant", "ghost")
                    .header("Content-Length", big.len())
                    .body(Body::from(big))
                    .expect("req"),
            )
            .await
            .expect("resp");
        assert_eq!(resp.status(), StatusCode::PAYLOAD_TOO_LARGE);
    }

    #[tokio::test]
    async fn bounds_wall_rejects_spec_shaped() {
        // Every cap answers with the spec error, before any parse.
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
                        ("x".repeat(*bounds::MAX_BODY_BYTES + 1)).len(),
                    )
                    .body(Body::from("x".repeat(*bounds::MAX_BODY_BYTES + 1)))
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
        let big: Vec<serde_json::Value> = (0..*bounds::MAX_BATCH_ITEMS + 1)
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
    async fn preadoptions_attr_get_value_options_head() {
        // 2.0 pre-adoptions: #14 GET .../attrs/{attrId},
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

    /// `information[].entities ×
    /// (propertyNames + relationshipNames)` was expanded into an in-memory Vec
    /// before any SQL ran, with no cardinality cap — a 4 MiB body produced on
    /// the order of 10^10 objects and OOM-killed the process. Capped at the
    /// validation boundary so it is a 400, not a dead pod: the request's
    /// content is what is refused, and 5.9.2.4 raises BadRequestData for it.
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
            StatusCode::BAD_REQUEST,
            "an oversized registration body is BadRequestData, not a query \
             the client is told to narrow"
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
        // Unknown members of Subscription/Registration documents are
        // stored and echoed, never rejected or stripped — a member added by a
        // future spec version flows through a broker that predates it.
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

    /// 5.5.10 / 6.3.14: the `snap-` tenant namespace is the broker's own —
    /// the snapshot code mints `snap-index` (the synth-tenant reverse index)
    /// and `snap-<uuid>` (one per snapshot). A client NGSILD-Tenant inside
    /// that namespace would let the caller read and delete other tenants'
    /// snapshot bookkeeping, so it is an invalid tenant value: 400
    /// BadRequestData, and nothing reaches the store.
    #[tokio::test]
    async fn internal_tenant_namespace_is_reserved() {
        let state = AppState::new("antares-test".into());
        let app = router(state.clone());
        let idx = antares_model::TenantId::new("snap-index").expect("tenant");

        // create (implicit tenant creation) must not mint the internal tenant
        let entity = serde_json::json!({"id": "urn:ngsi-ld:B:si", "type": "Building"});
        let body = entity.to_string();
        let resp = app
            .clone()
            .oneshot(
                Request::post("/ngsi-ld/v1/entities")
                    .header("Content-Type", "application/json")
                    .header("Content-Length", body.len())
                    .header("NGSILD-Tenant", "snap-index")
                    .body(Body::from(body))
                    .expect("req"),
            )
            .await
            .expect("resp");
        assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
        assert_eq!(
            body_json(resp).await["type"],
            "https://uri.etsi.org/ngsi-ld/errors/BadRequestData"
        );
        assert!(
            !state.store.tenant_exists(&idx).expect("store"),
            "refused request must not create the internal tenant"
        );
        assert!(state
            .store
            .list(&idx, antares_store::Kind::Entity)
            .expect("store")
            .is_empty());

        // reads of the index are refused too, whatever the resource
        for path in [
            "/ngsi-ld/v1/entities?type=Building",
            "/ngsi-ld/v1/subscriptions",
            "/ngsi-ld/v1/snapshots",
        ] {
            let resp = app
                .clone()
                .oneshot(
                    Request::get(path)
                        .header("NGSILD-Tenant", "snap-index")
                        .body(Body::empty())
                        .expect("req"),
                )
                .await
                .expect("resp");
            assert_eq!(resp.status(), StatusCode::BAD_REQUEST, "{path}");
        }
        // a per-snapshot synthetic tenant is equally off-limits, and so is the
        // distributed-subscription inbound index, whose own record claims it
        // is reserved while nothing enforced it
        for tenant in ["snap-0123456789abcdef", "distsub-index"] {
            let resp = app
                .clone()
                .oneshot(
                    Request::get("/ngsi-ld/v1/entities?type=Building")
                        .header("NGSILD-Tenant", tenant)
                        .body(Body::empty())
                        .expect("req"),
                )
                .await
                .expect("resp");
            assert_eq!(resp.status(), StatusCode::BAD_REQUEST, "{tenant}");
        }
    }

    /// The reserved namespace is the `snap-` prefix only: a tenant that
    /// merely contains "snap" is an ordinary 5.5.10 tenant.
    #[tokio::test]
    async fn tenant_containing_snap_is_not_reserved() {
        let app = app();
        let entity = serde_json::json!({"id": "urn:ngsi-ld:B:st", "type": "Building"});
        let body = entity.to_string();
        let resp = app
            .clone()
            .oneshot(
                Request::post("/ngsi-ld/v1/entities")
                    .header("Content-Type", "application/json")
                    .header("Content-Length", body.len())
                    .header("NGSILD-Tenant", "snapshots-team")
                    .body(Body::from(body))
                    .expect("req"),
            )
            .await
            .expect("resp");
        assert_eq!(resp.status(), StatusCode::CREATED);
        let resp = app
            .oneshot(
                Request::get("/ngsi-ld/v1/entities?type=Building")
                    .header("NGSILD-Tenant", "snapshots-team")
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
            Some("snapshots-team")
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

    /// 4.3.6.1 on batch distribution: "all constraints specified in the
    /// registration shall be respected" — a registration scoped to entity
    /// type A must not swallow a batch item of type B. The B item is purely
    /// local: it gets created; only the A item earns the Conflict part
    /// (read-only registration), so the batch is a 207.
    #[tokio::test]
    async fn batch_items_outside_a_registrations_types_stay_local() {
        crate::allow_private();
        let app = app();
        let reg = serde_json::json!({
            "id": "urn:ngsi-ld:ContextSourceRegistration:type-scope",
            "type": "ContextSourceRegistration",
            "mode": "redirect",
            "operations": ["retrieveEntity", "queryEntity"],
            "information": [{"entities": [{"type": "ScopedType"}]}],
            "endpoint": "http://127.0.0.1:1",
        });
        let resp = app
            .clone()
            .oneshot(
                Request::post("/ngsi-ld/v1/csourceRegistrations")
                    .header("Content-Type", "application/json")
                    .header("Content-Length", reg.to_string().len())
                    .body(Body::from(reg.to_string()))
                    .expect("req"),
            )
            .await
            .expect("resp");
        assert_eq!(resp.status(), StatusCode::CREATED);

        let batch = serde_json::json!([
            {"id": "urn:ngsi-ld:ScopedType:remote", "type": "ScopedType",
             "name": {"type": "Property", "value": "x"}},
            {"id": "urn:ngsi-ld:Other:local", "type": "OtherType",
             "name": {"type": "Property", "value": "y"}},
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
        assert_eq!(resp.status(), StatusCode::MULTI_STATUS, "one conflict part");

        // the out-of-scope item was created locally…
        let resp = app
            .clone()
            .oneshot(
                Request::get("/ngsi-ld/v1/entities/urn:ngsi-ld:Other:local?local=true")
                    .body(Body::empty())
                    .expect("req"),
            )
            .await
            .expect("resp");
        assert_eq!(resp.status(), StatusCode::OK, "non-matching item is local");
        // …and the in-scope one was refused everywhere (redirect, read-only)
        let resp = app
            .clone()
            .oneshot(
                Request::get("/ngsi-ld/v1/entities/urn:ngsi-ld:ScopedType:remote?local=true")
                    .body(Body::empty())
                    .expect("req"),
            )
            .await
            .expect("resp");
        assert_eq!(resp.status(), StatusCode::NOT_FOUND);
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

    #[tokio::test]
    async fn purge_rejects_linked_entity_paths() {
        // 5.6.21.4: "If projection attributes are present and indicate the
        // use of Linked Entity retrieval" and "If the filter conditions
        // specified by the query includes Linked Entity attributes" →
        // BadRequestData. `owner{name}` is the 4.9 LinkedEntityRelation form.
        let app = app();
        create(&app, "urn:ngsi-ld:Building:lep1", "Building").await;
        assert_eq!(
            purge(&app, "q=owner%7Bname%7D%3D%3D%22x%22").await,
            StatusCode::BAD_REQUEST
        );
        assert_eq!(
            purge(&app, "attrs=owner%7Bname%7D").await,
            StatusCode::BAD_REQUEST
        );
        // the entity must survive the rejected purges
        let resp = app
            .clone()
            .oneshot(
                Request::get("/ngsi-ld/v1/entities/urn:ngsi-ld:Building:lep1")
                    .body(Body::empty())
                    .expect("req"),
            )
            .await
            .expect("resp");
        assert_eq!(resp.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn purge_validates_csf_syntax() {
        // 5.6.21.4: "if the query, geoquery or context source filter are not
        // syntactically valid ... an error of type BadRequestData shall be
        // raised". csf is a 4.9 query over Context Source properties.
        let app = app();
        assert_eq!(
            purge(&app, "type=Building&csf=%29%29bad%28%28").await,
            StatusCode::BAD_REQUEST
        );
        // a well-formed csf is accepted
        assert_ne!(
            purge(&app, "type=Building&csf=endpoint%3D%3D%22x%22").await,
            StatusCode::BAD_REQUEST
        );
    }

    // ---- 5.7.1.4 Retrieve Entity ------------------------------------------

    async fn get_status(app: &Router, uri: &str, accept: Option<&str>) -> StatusCode {
        let mut req = Request::get(uri);
        if let Some(a) = accept {
            req = req.header("Accept", a);
        }
        app.clone()
            .oneshot(req.body(Body::empty()).expect("req"))
            .await
            .expect("resp")
            .status()
    }

    #[tokio::test]
    async fn retrieve_honors_the_type_selector() {
        // 5.7.1.4: ResourceNotFound when no entity "whose id (URI), and
        // where specified type, is equivalent" exists — ?type narrows the
        // retrieve target (4.17); a matching or wildcard selector passes.
        let app = app();
        create(&app, "urn:ngsi-ld:Building:rt1", "Building").await;
        let uri = "/ngsi-ld/v1/entities/urn:ngsi-ld:Building:rt1";
        assert_eq!(
            get_status(&app, &format!("{uri}?type=Vehicle"), None).await,
            StatusCode::NOT_FOUND
        );
        assert_eq!(
            get_status(&app, &format!("{uri}?type=Building"), None).await,
            StatusCode::OK
        );
        assert_eq!(
            get_status(&app, &format!("{uri}?type=%2A"), None).await,
            StatusCode::OK
        );
    }

    #[tokio::test]
    async fn retrieve_geometry_property_needs_geojson_accept() {
        // 5.7.1.4: "If geometryProperty parameter is present and the Accept
        // Header is not set to application/geo+json ... BadRequestData".
        let app = app();
        create(&app, "urn:ngsi-ld:Building:rg1", "Building").await;
        let uri = "/ngsi-ld/v1/entities/urn:ngsi-ld:Building:rg1?geometryProperty=location";
        assert_eq!(get_status(&app, uri, None).await, StatusCode::BAD_REQUEST);
        assert_eq!(
            get_status(&app, uri, Some("application/json")).await,
            StatusCode::BAD_REQUEST
        );
        assert_eq!(
            get_status(&app, uri, Some("application/geo+json")).await,
            StatusCode::OK
        );
    }

    #[tokio::test]
    async fn retrieve_linked_projection_requires_join_and_depth() {
        // 5.7.1.4: projection attributes that "indicate the use of Linked
        // Entity retrieval" without join, or that project deeper than
        // joinLevel, are BadRequestData.
        let app = app();
        create(&app, "urn:ngsi-ld:Building:rl1", "Building").await;
        let uri = "/ngsi-ld/v1/entities/urn:ngsi-ld:Building:rl1";
        // {…} selection without join
        assert_eq!(
            get_status(&app, &format!("{uri}?pick=owner%7Bname%7D"), None).await,
            StatusCode::BAD_REQUEST
        );
        // join=@none is "Linked Entity retrieval not specified"
        assert_eq!(
            get_status(
                &app,
                &format!("{uri}?pick=owner%7Bname%7D&join=%40none"),
                None
            )
            .await,
            StatusCode::BAD_REQUEST
        );
        // depth 2 projection over joinLevel=1
        assert_eq!(
            get_status(
                &app,
                &format!("{uri}?pick=owner%7Bworks%7Bname%7D%7D&join=inline&joinLevel=1"),
                None
            )
            .await,
            StatusCode::BAD_REQUEST
        );
        // well-formed: depth 1 within joinLevel=1, plain member kept
        assert_eq!(
            get_status(
                &app,
                &format!("{uri}?pick=id,type,name,owner%7Bname%7D&join=inline&joinLevel=1"),
                None
            )
            .await,
            StatusCode::OK
        );
    }

    // ---- 5.7.2.4 Query Entities validation --------------------------------

    #[tokio::test]
    async fn query_requires_a_non_system_attribute_in_attrs_and_q() {
        // 5.7.2.4 b/c: the Attribute list / query only qualifies the
        // too-wide guard when it includes "at least one non-system
        // Attribute" — system names alone are still a too-wide 400.
        let app = app();
        assert_eq!(
            get_status(&app, "/ngsi-ld/v1/entities?attrs=createdAt", None).await,
            StatusCode::BAD_REQUEST
        );
        assert_eq!(
            get_status(
                &app,
                "/ngsi-ld/v1/entities?q=modifiedAt%3E%222020-01-01T00:00:00Z%22",
                None
            )
            .await,
            StatusCode::BAD_REQUEST
        );
        assert_ne!(
            get_status(&app, "/ngsi-ld/v1/entities?attrs=name", None).await,
            StatusCode::BAD_REQUEST
        );
    }

    #[tokio::test]
    async fn query_linked_entity_guards() {
        // 5.7.2.4: projection or filter conditions that use Linked Entity
        // paths require join, and their depth may not exceed joinLevel
        // ("too deep query"); csf must be syntactically valid.
        let app = app();
        create(&app, "urn:ngsi-ld:Building:ql1", "Building").await;
        let base = "/ngsi-ld/v1/entities?type=Building";
        // {…} projection without join
        assert_eq!(
            get_status(&app, &format!("{base}&pick=owner%7Bname%7D"), None).await,
            StatusCode::BAD_REQUEST
        );
        // linked q term without join
        assert_eq!(
            get_status(
                &app,
                &format!("{base}&q=owner%7Bname%7D%3D%3D%22x%22"),
                None
            )
            .await,
            StatusCode::BAD_REQUEST
        );
        // linked q term two hops deep over joinLevel=1
        assert_eq!(
            get_status(
                &app,
                &format!(
                    "{base}&q=owner%7Bworks%7Bname%7D%7D%3D%3D%22x%22&join=inline&joinLevel=1"
                ),
                None
            )
            .await,
            StatusCode::BAD_REQUEST
        );
        // invalid csf
        assert_eq!(
            get_status(&app, &format!("{base}&csf=%29%29bad%28%28"), None).await,
            StatusCode::BAD_REQUEST
        );
        // well-formed linked q within depth is accepted
        assert_eq!(
            get_status(
                &app,
                &format!("{base}&q=owner%7Bname%7D%3D%3D%22x%22&join=inline&joinLevel=1"),
                None
            )
            .await,
            StatusCode::OK
        );
    }

    #[tokio::test]
    async fn temporal_retrieve_rejects_linked_projection() {
        // 5.7.3.4: "If projection attributes are present and indicate the
        // use of Linked Entity retrieval, an error of type BadRequestData
        // shall be raised" — unconditional, temporal has no join.
        let app = app();
        assert_eq!(
            get_status(
                &app,
                "/ngsi-ld/v1/temporal/entities/urn:ngsi-ld:Building:tl1?pick=owner%7Bname%7D",
                None
            )
            .await,
            StatusCode::BAD_REQUEST
        );
    }

    #[tokio::test]
    async fn temporal_query_validation_edges() {
        // 5.7.4.4: non-system rule for attrs/q; Linked Entity projection or
        // filter is an unconditional 400; invalid id URI 400; csf syntax
        // 400; orderBy may only name "id" on the temporal query.
        let app = app();
        let w = "timerel=after&timeAt=2020-01-01T00:00:00Z";
        let cases: &[(&str, StatusCode, &str)] = &[
            (
                "attrs=createdAt",
                StatusCode::BAD_REQUEST,
                "system attrs alone are too wide",
            ),
            (
                "type=Building&pick=owner%7Bname%7D",
                StatusCode::BAD_REQUEST,
                "linked projection",
            ),
            (
                "type=Building&q=owner%7Bname%7D%3D%3D%22x%22",
                StatusCode::BAD_REQUEST,
                "linked filter",
            ),
            (
                "type=Building&id=not%20a%20uri",
                StatusCode::BAD_REQUEST,
                "invalid id URI",
            ),
            (
                "type=Building&csf=%29%29bad%28%28",
                StatusCode::BAD_REQUEST,
                "invalid csf",
            ),
            (
                "type=Building&orderBy=name",
                StatusCode::BAD_REQUEST,
                "orderBy other than id",
            ),
            (
                "type=Building&orderBy=id",
                StatusCode::OK,
                "orderBy id is legal",
            ),
        ];
        for (qs, want, why) in cases {
            let uri = format!("/ngsi-ld/v1/temporal/entities?{w}&{qs}");
            assert_eq!(get_status(&app, &uri, None).await, *want, "{why}");
        }
    }

    #[tokio::test]
    async fn temporal_query_values_filter_respects_the_window() {
        // 5.7.4.4: "the values filter query shall be checked against all
        // the Attribute instances resulting from the initial filtering
        // performed by the temporal query" — an instance OUTSIDE the
        // interval must not satisfy q.
        let app = app();
        let body = serde_json::json!({
            "id": "urn:ngsi-ld:Vehicle:tw1", "type": "Vehicle",
            "speed": [
                {"type": "Property", "value": 5,
                 "observedAt": "2026-01-01T00:00:00Z"},
                {"type": "Property", "value": 99,
                 "observedAt": "2026-03-01T00:00:00Z"}
            ]
        })
        .to_string();
        let resp = app
            .clone()
            .oneshot(
                Request::post("/ngsi-ld/v1/temporal/entities")
                    .header("Content-Type", "application/json")
                    .header("Content-Length", body.len())
                    .body(Body::from(body))
                    .expect("req"),
            )
            .await
            .expect("resp");
        assert!(resp.status().is_success(), "seed: {}", resp.status());
        // window covers ONLY the value-5 instance; q asks for 99
        let uri = "/ngsi-ld/v1/temporal/entities?type=Vehicle&timerel=between&timeAt=2025-12-01T00:00:00Z&endTimeAt=2026-02-01T00:00:00Z&q=speed%3D%3D99";
        let resp = app
            .clone()
            .oneshot(Request::get(uri).body(Body::empty()).expect("req"))
            .await
            .expect("resp");
        let status = resp.status();
        let docs = body_json(resp).await;
        assert!(status.is_success(), "query: {status}");
        assert_eq!(
            docs.as_array().map(Vec::len),
            Some(0),
            "out-of-window instance must not satisfy q: {docs}"
        );
    }

    // ---- 5.9.2.4 Create Context Source Registration -----------------------

    async fn post_json(app: &Router, uri: &str, body: serde_json::Value) -> StatusCode {
        let body = body.to_string();
        app.clone()
            .oneshot(
                Request::post(uri)
                    .header("Content-Type", "application/json")
                    .header("Content-Length", body.len())
                    .body(Body::from(body))
                    .expect("req"),
            )
            .await
            .expect("resp")
            .status()
    }

    #[tokio::test]
    async fn csr_create_mode_restrictions() {
        // 5.9.2.4: auxiliary registrations may only offer retrieveOps /
        // retrieveEntity / queryEntity (or a combination); an exclusive
        // registration conflicts with an existing entity carrying any of
        // its Attributes; a redirect registration conflicts with any
        // existing matching entity.
        let app = app();
        let reg = |id: &str, mode: &str, ops: serde_json::Value, ent: serde_json::Value| {
            serde_json::json!({
                "id": format!("urn:ngsi-ld:ContextSourceRegistration:{id}"),
                "type": "ContextSourceRegistration",
                "mode": mode,
                "operations": ops,
                "information": [ent],
                "endpoint": "http://source.example.com"
            })
        };
        let uri = "/ngsi-ld/v1/csourceRegistrations";

        // auxiliary with a write op → 400; retrieve/query combination → 201
        assert_eq!(
            post_json(
                &app,
                uri,
                reg(
                    "aux-bad",
                    "auxiliary",
                    serde_json::json!(["updateEntity"]),
                    serde_json::json!({"entities": [{"type": "Building"}]})
                )
            )
            .await,
            StatusCode::BAD_REQUEST
        );
        assert_eq!(
            post_json(
                &app,
                uri,
                reg(
                    "aux-ok",
                    "auxiliary",
                    serde_json::json!(["retrieveOps", "queryEntity"]),
                    serde_json::json!({"entities": [{"type": "Building"}]})
                )
            )
            .await,
            StatusCode::CREATED
        );

        // exclusive vs existing entity carrying the registered attribute
        create(&app, "urn:ngsi-ld:Building:csr1", "Building").await; // has "name"
        assert_eq!(
            post_json(
                &app,
                uri,
                reg(
                    "exc-conflict",
                    "exclusive",
                    serde_json::json!(["retrieveOps"]),
                    serde_json::json!({
                        "entities": [{"type": "Building", "id": "urn:ngsi-ld:Building:csr1"}],
                        "propertyNames": ["name"]
                    })
                )
            )
            .await,
            StatusCode::CONFLICT
        );
        // exclusive over an attr the entity does NOT have is fine
        assert_eq!(
            post_json(
                &app,
                uri,
                reg(
                    "exc-ok",
                    "exclusive",
                    serde_json::json!(["retrieveOps"]),
                    serde_json::json!({
                        "entities": [{"type": "Building", "id": "urn:ngsi-ld:Building:csr1"}],
                        "propertyNames": ["capacity"]
                    })
                )
            )
            .await,
            StatusCode::CREATED
        );

        // redirect vs any existing matching entity
        assert_eq!(
            post_json(
                &app,
                uri,
                reg(
                    "red-conflict",
                    "redirect",
                    serde_json::json!(["retrieveOps"]),
                    serde_json::json!({
                        "entities": [{"type": "Building", "id": "urn:ngsi-ld:Building:csr1"}]
                    })
                )
            )
            .await,
            StatusCode::CONFLICT
        );
        // redirect for an untouched id is fine
        assert_eq!(
            post_json(
                &app,
                uri,
                reg(
                    "red-ok",
                    "redirect",
                    serde_json::json!(["retrieveOps"]),
                    serde_json::json!({
                        "entities": [{"type": "Building", "id": "urn:ngsi-ld:Building:other"}]
                    })
                )
            )
            .await,
            StatusCode::CREATED
        );
    }

    #[tokio::test]
    async fn csr_expires_at_deletes_the_registration() {
        // 5.9.2.4: "If expiresAt is a date and time in the future,
        // implementations shall delete the Registration when this point in
        // time is reached" — after expiry the registration is gone from
        // retrieve and query.
        let app = app();
        let soon = (chrono::Utc::now() + chrono::Duration::milliseconds(1100))
            .to_rfc3339_opts(chrono::SecondsFormat::Millis, true);
        let reg = serde_json::json!({
            "id": "urn:ngsi-ld:ContextSourceRegistration:expiring",
            "type": "ContextSourceRegistration",
            "expiresAt": soon,
            "information": [{"entities": [{"type": "Building"}]}],
            "endpoint": "http://source.example.com"
        });
        assert_eq!(
            post_json(&app, "/ngsi-ld/v1/csourceRegistrations", reg).await,
            StatusCode::CREATED
        );
        let uri = "/ngsi-ld/v1/csourceRegistrations/urn:ngsi-ld:ContextSourceRegistration:expiring";
        assert_eq!(get_status(&app, uri, None).await, StatusCode::OK);
        tokio::time::sleep(std::time::Duration::from_millis(1400)).await;
        assert_eq!(
            get_status(&app, uri, None).await,
            StatusCode::NOT_FOUND,
            "expired registration must be gone"
        );
        let resp = app
            .clone()
            .oneshot(
                Request::get("/ngsi-ld/v1/csourceRegistrations?type=Building")
                    .body(Body::empty())
                    .expect("req"),
            )
            .await
            .expect("resp");
        let docs = body_json(resp).await;
        assert!(
            !docs.to_string().contains("expiring"),
            "expired registration must not be listed: {docs}"
        );
    }

    #[tokio::test]
    async fn csr_update_reapplies_mode_restrictions() {
        // 5.9.3.4: the exclusive/redirect entity-conflict rules and the
        // auxiliary ops restriction apply to the post-merge document.
        let app = app();
        create(&app, "urn:ngsi-ld:Building:csru1", "Building").await;
        let reg = serde_json::json!({
            "id": "urn:ngsi-ld:ContextSourceRegistration:upd1",
            "type": "ContextSourceRegistration",
            "mode": "inclusive",
            "information": [{"entities": [{"type": "Building", "id": "urn:ngsi-ld:Building:csru1"}]}],
            "endpoint": "http://source.example.com"
        });
        assert_eq!(
            post_json(&app, "/ngsi-ld/v1/csourceRegistrations", reg).await,
            StatusCode::CREATED
        );
        // flipping the mode to redirect now collides with the entity
        let patch = serde_json::json!({"mode": "redirect"}).to_string();
        let resp = app
            .clone()
            .oneshot(
                Request::patch(
                    "/ngsi-ld/v1/csourceRegistrations/urn:ngsi-ld:ContextSourceRegistration:upd1",
                )
                .header("Content-Type", "application/json")
                .header("Content-Length", patch.len())
                .body(Body::from(patch))
                .expect("req"),
            )
            .await
            .expect("resp");
        assert_eq!(resp.status(), StatusCode::CONFLICT);
        // flipping to auxiliary while operations carry writes is 400
        let patch =
            serde_json::json!({"mode": "auxiliary", "operations": ["updateEntity"]}).to_string();
        let resp = app
            .clone()
            .oneshot(
                Request::patch(
                    "/ngsi-ld/v1/csourceRegistrations/urn:ngsi-ld:ContextSourceRegistration:upd1",
                )
                .header("Content-Type", "application/json")
                .header("Content-Length", patch.len())
                .body(Body::from(patch))
                .expect("req"),
            )
            .await
            .expect("resp");
        assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn csr_query_csf_scope_and_geo_filters() {
        // 5.10.2.4: the context source filter (csf) matches the
        // registration's own Context Source Properties; the Scope query
        // matches its scope; the geoquery matches its location.
        let app = app();
        let reg_a = serde_json::json!({
            "id": "urn:ngsi-ld:ContextSourceRegistration:disc-a",
            "type": "ContextSourceRegistration",
            "information": [{"entities": [{"type": "Building"}]}],
            "endpoint": "http://a.example.com",
            "scope": "/Madrid/Centro",
            "location": {"type": "Point", "coordinates": [8.68, 49.41]}
        });
        let reg_b = serde_json::json!({
            "id": "urn:ngsi-ld:ContextSourceRegistration:disc-b",
            "type": "ContextSourceRegistration",
            "information": [{"entities": [{"type": "Building"}]}],
            "endpoint": "http://b.example.com",
            "scope": "/Berlin"
        });
        for r in [reg_a, reg_b] {
            assert_eq!(
                post_json(&app, "/ngsi-ld/v1/csourceRegistrations", r).await,
                StatusCode::CREATED
            );
        }
        let list = |q: String| {
            let app = app.clone();
            async move {
                let resp = app
                    .oneshot(
                        Request::get(format!(
                            "/ngsi-ld/v1/csourceRegistrations?type=Building&{q}"
                        ))
                        .body(Body::empty())
                        .expect("req"),
                    )
                    .await
                    .expect("resp");
                assert_eq!(resp.status(), StatusCode::OK, "query {q}");
                body_json(resp).await.to_string()
            }
        };
        // csf on a Context Source Property
        let got = list("csf=endpoint%3D%3D%22http%3A%2F%2Fa.example.com%22".into()).await;
        assert!(
            got.contains("disc-a") && !got.contains("disc-b"),
            "csf: {got}"
        );
        // Scope query against the registration scope
        let got = list("scopeQ=%2FMadrid%2F%23".into()).await;
        assert!(
            got.contains("disc-a") && !got.contains("disc-b"),
            "scopeQ: {got}"
        );
        // geoquery against the registration location
        let got = list(
            "georel=near%3BmaxDistance%3D%3D2000&geometry=Point&coordinates=%5B8.68%2C49.41%5D"
                .into(),
        )
        .await;
        assert!(
            got.contains("disc-a") && !got.contains("disc-b"),
            "geo: {got}"
        );
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
        // orders without it.
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
    /// ld+json, never for plain application/json.
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

    /// 4.5.9 p.63/65: in the simplified temporal representation
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

    /// 4.5.19.1 Table -1 + 5.7.4.4 p.211: string-valued Properties
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

    /// 5.2.9, after Table 5.2.9-1: the Table 5.2.9-2 members "are read-only
    /// and shall be automatically generated by NGSI-LD implementations. In
    /// the event that they are provided (in update or create operations)
    /// NGSI-LD implementations shall ignore them." A tolerant reader that
    /// stores an unknown member verbatim would let a client dictate its own
    /// forward history, and every one of these is served back.
    #[tokio::test]
    async fn table_5_2_9_2_members_are_ignored_on_create_and_update() {
        use serde_json::json;
        const READ_ONLY: [&str; 5] = [
            "timesSent",
            "timesFailed",
            "lastSuccess",
            "lastFailure",
            "status",
        ];
        let id = "urn:ngsi-ld:ContextSourceRegistration:529-readonly";
        let st = AppState::new("t5292".into());
        let claimed = json!({
            "timesSent": 9_000,
            "timesFailed": 0,
            "lastSuccess": "2035-01-01T00:00:00Z",
            "lastFailure": "2035-01-01T00:00:00Z",
            "status": "ok",
        });

        let mut doc = json!({
            "id": id,
            "type": "ContextSourceRegistration",
            "information": [{"entities": [{"type": "Building"}]}],
            "endpoint": "http://cs.example.org:1026",
        });
        for (k, v) in claimed.as_object().expect("obj") {
            doc[k] = v.clone();
        }
        let body = doc.to_string();
        let res = router(st.clone())
            .oneshot(
                Request::post("/ngsi-ld/v1/csourceRegistrations")
                    .header("Content-Type", "application/json")
                    .header("Content-Length", body.len())
                    .body(Body::from(body))
                    .expect("req"),
            )
            .await
            .expect("resp");
        assert_eq!(res.status(), StatusCode::CREATED, "create");

        let served = |st: AppState| async move {
            let res = router(st)
                .oneshot(
                    Request::get(format!("/ngsi-ld/v1/csourceRegistrations/{id}"))
                        .body(Body::empty())
                        .expect("req"),
                )
                .await
                .expect("resp");
            assert_eq!(res.status(), StatusCode::OK, "retrieve");
            let bytes = axum::body::to_bytes(res.into_body(), usize::MAX)
                .await
                .expect("body");
            serde_json::from_slice::<serde_json::Value>(&bytes).expect("json")
        };

        let got = served(st.clone()).await;
        for m in READ_ONLY {
            assert!(
                got.get(m).is_none(),
                "create must ignore the read-only member {m}: {got}"
            );
        }

        // 5.9.3 update: the same members, through the merge patch.
        let patch = claimed.to_string();
        let res = router(st.clone())
            .oneshot(
                Request::patch(format!("/ngsi-ld/v1/csourceRegistrations/{id}"))
                    .header("Content-Type", "application/json")
                    .header("Content-Length", patch.len())
                    .body(Body::from(patch))
                    .expect("req"),
            )
            .await
            .expect("resp");
        assert_eq!(res.status(), StatusCode::NO_CONTENT, "update");

        let got = served(st).await;
        for m in READ_ONLY {
            assert!(
                got.get(m).is_none(),
                "update must ignore the read-only member {m}: {got}"
            );
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
                "timesFailed": 999,
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
            "timesFailed",
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
        // 4.23.3 EXAMPLE 7: a named ICU collation is honoured (200), and a
        // non-string member stays a loud 400
        let (st, body) = send(
            &app,
            "POST",
            q,
            Some(with_ordering(json!({"orderBy": ["id;asc"],
                "collation": "de-u-co-phonebk"}))),
        )
        .await;
        assert_eq!(st, StatusCode::OK, "named collation is honoured: {body}");
        let (st, body) = send(
            &app,
            "POST",
            q,
            Some(with_ordering(
                json!({"orderBy": ["id;asc"], "collation": 5}),
            )),
        )
        .await;
        assert_eq!(st, StatusCode::BAD_REQUEST, "{body}");
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

#[cfg(test)]
mod clause_6_3_11 {
    use super::*;
    use axum::body::Body;
    use axum::http::{Request, StatusCode};
    use axum::response::Response;
    use http_body_util::BodyExt;
    use tower::ServiceExt;

    async fn body_json(resp: Response) -> serde_json::Value {
        let bytes = resp.into_body().collect().await.expect("body").to_bytes();
        serde_json::from_slice(&bytes).expect("json body")
    }

    /// 6.3.11 Table 6.3.11-1: `expiresAt` is included in response payloads
    /// only when `options=sysAttrs` — entity level, attribute level and
    /// temporal instances alike (same gate as createdAt/modifiedAt).
    #[tokio::test]
    async fn clause_6_3_11_expires_at_gated_by_sysattrs() {
        let mut st = AppState::new("antares-test".into());
        crate::wire(&mut st);
        let app = router(st);
        let entity = serde_json::json!({
            "id": "urn:ngsi-ld:Building:exp6311", "type": "Building",
            "expiresAt": "2100-01-01T00:00:00Z",
            "name": {"type": "Property", "value": "x",
                     "observedAt": "2026-08-13T00:00:00Z",
                     "expiresAt": "2100-01-01T00:00:00Z"}
        });
        let body = entity.to_string();
        let resp = app
            .clone()
            .oneshot(
                Request::post("/ngsi-ld/v1/entities")
                    .header("Content-Type", "application/json")
                    .header("Content-Length", body.len())
                    .body(Body::from(body))
                    .expect("req"),
            )
            .await
            .expect("resp");
        assert_eq!(resp.status(), StatusCode::CREATED);

        let plain = body_json(
            app.clone()
                .oneshot(
                    Request::get("/ngsi-ld/v1/entities/urn:ngsi-ld:Building:exp6311")
                        .body(Body::empty())
                        .expect("req"),
                )
                .await
                .expect("resp"),
        )
        .await;
        assert!(
            plain.get("expiresAt").is_none(),
            "entity expiresAt must be sysAttrs-gated (6.3.11): {plain}"
        );
        assert!(
            plain["name"].get("expiresAt").is_none(),
            "attribute expiresAt must be sysAttrs-gated (6.3.11): {plain}"
        );

        let sys = body_json(
            app.clone()
                .oneshot(
                    Request::get(
                        "/ngsi-ld/v1/entities/urn:ngsi-ld:Building:exp6311?options=sysAttrs",
                    )
                    .body(Body::empty())
                    .expect("req"),
                )
                .await
                .expect("resp"),
        )
        .await;
        assert_eq!(sys["expiresAt"], "2100-01-01T00:00:00Z");
        assert_eq!(sys["name"]["expiresAt"], "2100-01-01T00:00:00Z");

        let inst_of = |body: &serde_json::Value| -> serde_json::Value {
            let a = &body["name"];
            if a.is_array() {
                a[0].clone()
            } else {
                a.clone()
            }
        };
        let t_plain = body_json(
            app.clone()
                .oneshot(
                    Request::get(
                        "/ngsi-ld/v1/temporal/entities/urn:ngsi-ld:Building:exp6311?timerel=after&timeAt=2020-01-01T00:00:00Z",
                    )
                    .body(Body::empty())
                    .expect("req"),
                )
                .await
                .expect("resp"),
        )
        .await;
        assert!(
            inst_of(&t_plain).get("expiresAt").is_none(),
            "temporal instance expiresAt must be sysAttrs-gated (6.3.11): {t_plain}"
        );
        let t_sys = body_json(
            app.clone()
                .oneshot(
                    Request::get(
                        "/ngsi-ld/v1/temporal/entities/urn:ngsi-ld:Building:exp6311?timerel=after&timeAt=2020-01-01T00:00:00Z&options=sysAttrs",
                    )
                    .body(Body::empty())
                    .expect("req"),
                )
                .await
                .expect("resp"),
        )
        .await;
        assert_eq!(inst_of(&t_sys)["expiresAt"], "2100-01-01T00:00:00Z");
    }
}

/// The programmatic egress override, not `ANTARES_EGRESS_ALLOW_PRIVATE`: a
/// sibling test reading the environment while another rewrote it saw the
/// policy missing and refused the loopback forward. An atomic store carries
/// the same switch with no write for a reader to land in the middle of.
#[cfg(test)]
pub(crate) fn allow_private() {
    antares_jsonld::allow_private_egress(true);
}
