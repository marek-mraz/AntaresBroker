// SPDX-License-Identifier: EUPL-1.2
//! Shared application state.

use antares_jsonld::Loader;
#[cfg(feature = "test-kit")]
use antares_sql::store::{any::AnyStore, Store};
use antares_store::{CurrentStateDriver, TemporalDriver};
use std::sync::Arc;
// Clock rule: std Instant panics on wasm32.
#[cfg(not(target_arch = "wasm32"))]
use std::time::Instant;
#[cfg(target_arch = "wasm32")]
use web_time::Instant;

/// ANTARES_TEMPORAL_RECORD — the auto-recording gate on the write path.
/// Direct temporal-API writes (POST /temporal/entities…) are never gated:
/// this decides what the ENTITY endpoints leave behind as history.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TemporalRecord {
    /// `all` (default): every changed attribute instance.
    All,
    /// `observed`: only instances carrying `observedAt` — the spec's own
    /// measurement axis (4.5.7); metadata-shaped writes leave no history.
    Observed,
    /// `none`: nothing is auto-recorded; the temporal endpoints still serve
    /// what the temporal API was given directly (unlike ANTARES_TEMPORAL=none,
    /// which turns the temporal seam off entirely).
    None,
}

impl std::str::FromStr for TemporalRecord {
    type Err = String;
    fn from_str(s: &str) -> Result<Self, String> {
        match s {
            "all" => Ok(Self::All),
            "observed" => Ok(Self::Observed),
            "none" => Ok(Self::None),
            other => Err(format!(
                "ANTARES_TEMPORAL_RECORD: unknown mode {other} (all|observed|none)"
            )),
        }
    }
}

/// The handler `wire` installs for a notification that arrives on the
/// internal distributed-subscription endpoint (5.8.1.4).
pub type CsourceNotification = Arc<
    dyn for<'a> Fn(
            &'a AppState,
            &'a antares_model::TenantId,
            &'a str,
            Option<&'a str>,
            &'a [serde_json::Value],
        ) -> std::pin::Pin<Box<dyn std::future::Future<Output = ()> + Send + 'a>>
        + Send
        + Sync,
>;

/// The request headers an in-process call carries from its caller
/// ([`AppState::call`]): the two that select WHICH data the operation runs
/// against, and the one that says what its terms mean. Everything else is
/// the façade's own to set — an inner request is not the outer one.
static PROPAGATED: [axum::http::HeaderName; 3] = [
    axum::http::HeaderName::from_static("ngsild-tenant"),
    axum::http::HeaderName::from_static("ngsild-snapshot"),
    axum::http::HeaderName::from_static("link"),
];

#[derive(Clone)]
pub struct AppState {
    pub store: Arc<dyn CurrentStateDriver>,
    /// The temporal driver — by default the same backend instance as
    /// `store`; a deployment may load a different one (or none).
    pub temporal: Arc<dyn TemporalDriver>,
    /// What the current-state driver is CALLED, reported by `/q/health` (NOT
    /// by `/info/sourceIdentity`, which is a spec resource) — a name, not an
    /// enumeration: a driver from outside this workspace mounts the same way
    /// as one from inside, and nothing here branches on it.
    pub store_name: String,
    /// The temporal backend `/q/health` names: the store's own mode when one
    /// instance serves both seams, `None` when history is off (`NoTemporal`);
    /// a second backend overwrites it after construction.
    pub temporal_name: Option<String>,
    pub loader: Arc<Loader>,
    pub started: Instant,
    /// Startup timestamp (createdAt of the built-in core @context entry).
    pub started_at: String,
    pub host_alias: String,
    /// Default page size for queries without an explicit limit.
    pub default_limit: usize,
    /// Hard ceiling on limit (TooManyResults guard).
    pub max_limit: usize,
    /// One shared outbound client, timeouts set at construction.
    pub http: antares_jsonld::HttpClient,
    /// Federation-forwarding client — longer deadline: the ETSI mock replies
    /// to unstubbed forwards only when the robot side wakes (up to ~5 s).
    pub fed_http: antares_jsonld::HttpClient,
    /// Notification bindings by `endpoint.uri` scheme (6.3.8, 7.2). The
    /// only way a delivery transport is chosen; `with_sink` adds one.
    pub sinks: Arc<antares_notifier::SinkRegistry>,
    /// HTTP surfaces mounted outside the NGSI-LD API root, each under its
    /// own reserved prefix; `with_surface` adds one. The admin surface is
    /// here by default.
    pub surfaces: Arc<Vec<Box<dyn crate::ApiSurface>>>,
    /// The router [`AppState::call`] serves in-process requests through,
    /// built on first use. Empty until a façade actually calls: building it
    /// costs about 1.5 ms, which is worth memoizing per state and not worth
    /// paying in a host that never makes an in-process call.
    pub(crate) inbound: Arc<std::sync::OnceLock<axum::Router>>,
    /// Bounds-wall rejection counters (exported by /q/health).
    pub limits: Arc<crate::bounds::LimitStats>,
    /// Allocator stats provider (set by the broker; None in tests/wasm).
    pub mem_stats: Option<Arc<dyn Fn() -> serde_json::Value + Send + Sync>>,
    /// Bus state provider for /q/health (`bus: {mode, connected,
    /// reconnects}`) — installed by the nats wiring only, so the member is
    /// absent for bus=local.
    pub bus_stats: Option<Arc<dyn Fn() -> serde_json::Value + Send + Sync>>,
    /// One egress policy for notifications and federation forwards
    /// (scheme allowlist, private-range deny, per-destination breakers).
    pub egress: Arc<crate::egress::Egress>,
    /// The policy engine every operation is asked about (ADR-0020).
    /// `AllowAll` unless a deployment attached its own with
    /// [`AppState::with_policy`], so the broker behaves exactly as it did
    /// before the seam existed.
    pub policy: Arc<dyn crate::policy::PolicyEngine>,
    /// Set the moment SIGTERM arrives, BEFORE the listener stops
    /// accepting — `/q/health` then answers 503 DRAINING so the load balancer
    /// takes this instance out while its socket still works.
    pub draining: Arc<std::sync::atomic::AtomicBool>,
    /// Change batches the matcher queue accepted and has not finished
    /// delivering. A shutdown drain waits for zero before the pool closes.
    pub pending_changes: Arc<std::sync::atomic::AtomicUsize>,
    /// Called after every Subscription and Context Source Registration
    /// Subscription CUD, so the mirror the matcher reads follows the store.
    /// On the bus the wiring pushes the change into the KV mirror bucket;
    /// in local mode it applies to this process's own mirror.
    #[allow(clippy::type_complexity)] // a boxed callback, named where it is installed
    pub sub_sync: Option<
        Arc<
            dyn Fn(&antares_model::TenantId, antares_store::Kind, &str, Option<&serde_json::Value>)
                + Send
                + Sync,
        >,
    >,
    /// bus=nats: the KV-watched compiled-subscription mirror the matcher
    /// reads, so the hot path never touches Postgres. `None` in local
    /// mode (the matcher reads the store directly).
    pub sub_mirror: Option<Arc<crate::mirror::SubMirror>>,
    /// bus=local: takes the entity changes one request buffered (the history
    /// layer hands them over after the handler) so the matcher sees a batch
    /// request as one unit. `None` until the local pipeline is wired.
    pub change_flush: Option<Arc<dyn Fn(Vec<crate::mirror::Change>) + Send + Sync>>,
    /// bus=nats: called after every Registration CUD so the wiring can
    /// publish the delta on `ANTARES_REGISTRY`. `None` in local mode.
    #[allow(clippy::type_complexity)] // a boxed callback, named where it is installed
    pub reg_sync: Option<
        Arc<dyn Fn(&antares_model::TenantId, &str, Option<&serde_json::Value>) + Send + Sync>,
    >,
    /// bus=nats: the ONE per-process compiled registration mirror,
    /// delta-fed from `ANTARES_REGISTRY`; expiry stays filtered at the single
    /// yield point (`federation::matching_regs`). `None` in local mode.
    pub reg_mirror: Option<Arc<crate::mirror::DocMirror>>,
    /// 5.2.34 (bus=nats): shares a cooldown stamp with the other api pods —
    /// a per-process stamp re-dials a failed source from every pod behind
    /// the LB. Seconds-scale state: broadcast on the
    /// registry stream, deliberately not persisted. `None` in local mode.
    #[allow(clippy::type_complexity)] // a boxed callback, named where it is installed
    pub reg_fail_sync: Option<Arc<dyn Fn(&str, bool) + Send + Sync>>,
    /// Renders the Prometheus text format for /q/metrics. Installed by
    /// the broker (the only crate that knows an exporter exists);
    /// `None` = 404, the facade calls elsewhere stay no-ops.
    pub metrics_render: Option<Arc<dyn Fn() -> String + Send + Sync>>,
    /// 5.8.1.4 consumer half: a notification addressed to the internal
    /// `urn:antares:distsub:` endpoint never leaves the broker, it re-enters
    /// it. The delivery path hands it here instead of naming the module that
    /// owns distributed subscriptions; `wire` installs the handler. `None`
    /// drops such a notification, which is what a broker that never created
    /// an internal subscription should do with one.
    pub csource_notification: Option<CsourceNotification>,
    /// True only under bus=nats (set by the broker's wiring): multiple
    /// processes share the store, so interval-subscription firings must be
    /// claimed single-winner. bus=local keeps the direct path — a
    /// claim there would disturb the 046_12 bookkeeping ordering for
    /// nothing.
    pub nats: bool,
    /// History gate 2 (ANTARES_TEMPORAL_RECORD): which changed attribute
    /// instances the write path records into history. Default `All`, which
    /// the ETSI temporal suites assume.
    pub temporal_record: TemporalRecord,
    /// The base URL remote Context Sources reach THIS broker at — used as
    /// the notification endpoint of forwarded subscription copies
    /// (5.8.1.4). ANTARES_PUBLIC_URL, defaulting to
    /// http://{host_alias}:{ANTARES_HTTP_PORT} (portless when 80/unset).
    pub public_url: String,
    /// 5.5.15 resource-pressure signal: max snapshots per tenant — above it
    /// the lowest-snapshotPriority snapshots are evicted. Snapshot documents
    /// themselves live in the store (Kind::Snapshot) so persistent modes
    /// survive restarts.
    pub snapshot_cap: usize,
    /// How a notification is delivered: attempts, backoff, age ceiling.
    /// Default = one attempt (5.8.6 as written).
    pub delivery: antares_notifier::DeliveryPolicy,
}

impl AppState {
    /// The built-in store: in-memory, or — under the reserved harness
    /// variable `ANTARES_TEST_STORE=file` — a fresh on-disk redb store per
    /// state, so the same test binary proves the durable backend without a
    /// second copy of every test. The broker's own boot path never calls
    /// this; it composes from `ANTARES_STORE` in `with_drivers`.
    #[cfg(feature = "test-kit")]
    pub fn new(host_alias: String) -> Self {
        #[cfg(not(target_arch = "wasm32"))]
        if std::env::var("ANTARES_TEST_STORE").as_deref() == Ok("file") {
            let dir = std::env::temp_dir()
                .join("antares-test-store")
                .join(uuid::Uuid::new_v4().simple().to_string());
            // reachable only through the harness variable read above; a
            // harness that cannot open its own store has nothing to run
            #[allow(clippy::expect_used)]
            let store = Store::open_file(&dir).expect("open the redb test store");
            return Self::with_store(host_alias, Arc::new(AnyStore::Mem(store)), "file");
        }
        Self::with_store(
            host_alias,
            Arc::new(AnyStore::Mem(Store::default())),
            "memory",
        )
    }

    /// Convenience over the built-in backends: one `AnyStore` serves as
    /// both drivers.
    #[cfg(feature = "test-kit")]
    pub fn with_store(host_alias: String, store: Arc<AnyStore>, store_name: &str) -> Self {
        let temporal: Arc<dyn TemporalDriver> = store.clone();
        Self::with_drivers(host_alias, store, temporal, store_name)
    }

    pub fn with_drivers(
        host_alias: String,
        store: Arc<dyn CurrentStateDriver>,
        temporal: Arc<dyn TemporalDriver>,
        store_name: &str,
    ) -> Self {
        // One policy value, read once, shared by every outbound path —
        // the gate (scheme/breakers) and the clients (DNS pinning, redirect
        // cap) can never disagree about what is allowed.
        let egress_policy = antares_jsonld::EgressPolicy::from_env();
        // Cached-@context rows are the ONE source of truth for 5.13
        // existence — wire the write-through HERE so every composition
        // (native binary, wasm, tests) gets it; a composition that forgets
        // the writer is the "expiry checked in some paths" disease with
        // @contexts instead of expiry.
        let loader = Arc::new(Loader::new());
        {
            let store = store.clone();
            loader.set_cache_writer(Box::new(move |tenant, url, ctx_value| {
                if hosted_row_id(&*store, tenant, url).is_some() {
                    return; // broker-local (Hosted/Implicit) URLs are not Cached entries
                }
                let id = uuid::Uuid::new_v5(&uuid::Uuid::NAMESPACE_URL, url.as_bytes());
                // A refetch (staleness, delete+reload) must keep the row's
                // identity and hit counters — only the body is new.
                // The row this writes is `Cached` and belongs to no Tenant
                // (ADR-0021), but the call still acts for the Tenant whose
                // resolution triggered the fetch: that is what lets the guard
                // above see this Tenant's own Hosted rows.
                let prior = store.context_get(tenant, &id.to_string()).ok().flatten();
                let field = |k: &str| {
                    prior
                        .as_ref()
                        .and_then(|p| p[k].as_str())
                        .map(str::to_owned)
                };
                let created = field("createdAt").unwrap_or_else(now_iso);
                let doc = serde_json::json!({
                    "url": url,
                    "localId": id.to_string(),
                    "kind": "Cached",
                    "createdAt": created,
                    "numberOfHits": prior
                        .as_ref()
                        .and_then(|p| p["numberOfHits"].as_u64())
                        .unwrap_or(0),
                    "lastUsage": field("lastUsage"),
                    "body": {"@context": ctx_value},
                });
                if let Err(e) = store.context_put(tenant, &id.to_string(), doc) {
                    // a client-named @context URL may carry userinfo
                    tracing::warn!(
                        "@context write-through failed for {}: {e}",
                        antares_notifier::redact_userinfo(url)
                    );
                }
            }));
        }
        // 5.13.3.5: hit counters live in the SHARED row, not per instance
        // — behind a load balancer per-instance counters split-brain.
        // A bump that finds the row gone reports
        // a cross-instance delete; the loader then drops its warm copies so
        // the delete is honoured everywhere (5.13.5.4). Pinned core contexts
        // have no row and are never evicted.
        {
            let store = store.clone();
            loader.set_usage_bump(Box::new(move |tenant, url| {
                if Loader::is_pinned_core(url) {
                    return true;
                }
                // One read, not two: a bump runs on every counted use of a
                // non-pinned @context, and the hosted probe already carries
                // the row it found.
                let held = hosted_row(&*store, tenant, url);
                let (id, row) = match held {
                    Some((id, row)) => (id, Ok(Some(row))),
                    None => {
                        let id = uuid::Uuid::new_v5(&uuid::Uuid::NAMESPACE_URL, url.as_bytes())
                            .to_string();
                        let row = store.context_get(tenant, &id);
                        (id, row)
                    }
                };
                match row {
                    // the row is what the store handed back, and `Value`'s
                    // index panics on anything that is not an object; a row
                    // that cannot carry the counters is left uncounted
                    Ok(Some(mut doc)) if doc.is_object() => {
                        let hits = doc["numberOfHits"].as_u64().unwrap_or(0) + 1;
                        doc["numberOfHits"] = serde_json::json!(hits);
                        doc["lastUsage"] = serde_json::json!(now_iso());
                        if let Err(e) = store.context_put(tenant, &id, doc) {
                            tracing::warn!(
                                "@context hit bump failed for {}: {e}",
                                antares_notifier::redact_userinfo(url)
                            );
                        }
                        true
                    }
                    // a row that cannot carry the counters is left
                    // uncounted, and still exists: it must not be evicted
                    Ok(Some(_)) => true,
                    Ok(None) => false,
                    // a store hiccup must never evict a healthy cache entry
                    Err(_) => true,
                }
            }));
        }
        // 5.13.1: an @context this broker HOSTS is served from its row, not
        // fetched. Its warm copy in the loader is only a cache — a restart
        // reloads Cached rows alone, and the bounded cache may evict it at
        // any moment — and the URL under it is minted from the request Host
        // header, so a miss would otherwise send the broker to whatever
        // address a client put there for the term mappings of its own
        // Tenant's payloads. The row carries the owner, so 5.5.10 still
        // decides who may resolve it.
        {
            let store = store.clone();
            loader.set_local_lookup(Box::new(move |tenant, url| {
                let (_, row) = hosted_row(&*store, tenant, url)?;
                // Ownership is one rule for the whole broker (ADR-0021): a
                // Cached row is a copy of a public document and belongs to no
                // Tenant, everything else belongs to its `owner` — with the
                // DEFAULT Tenant for a row written before that member
                // existed, which is the Tenant that lists, serves and deletes
                // it.
                let owner = antares_store::context_row_owner(&row)
                    .and_then(|o| antares_model::TenantId::new(o).ok());
                Some((owner, row["body"]["@context"].clone()))
            }));
        }
        // 5.8.1.4: this URL is handed to peer brokers as the notification
        // endpoint for distributed subscriptions — the default must carry
        // the HTTP port or peers dial port 80 (ETSI-matrix ADV_02 shape).
        let public_url =
            std::env::var("ANTARES_PUBLIC_URL").unwrap_or_else(|_| {
                match std::env::var("ANTARES_HTTP_PORT") {
                    Ok(p) if p != "80" => format!("http://{host_alias}:{p}"),
                    _ => format!("http://{host_alias}"),
                }
            });
        let temporal_name = temporal.supported().then(|| store_name.to_owned());
        // 6.3.8 is the mandatory binding; clause 7's MQTT one is optional and
        // registers only when compiled in. Both go through the registry, so
        // notification delivery never names a transport.
        let http = outbound_client(
            egress_policy,
            std::time::Duration::from_secs(5 * slow_factor()),
        );
        let mut sinks = antares_notifier::SinkRegistry::default();
        sinks.register(Box::new(antares_notifier::HttpSink::new(http.clone())));
        #[cfg(feature = "mqtt")]
        sinks.register(Box::new(antares_notifier::mqtt::MqttSink::default()));
        Self {
            store,
            temporal,
            store_name: store_name.to_owned(),
            temporal_name,
            loader,
            started: Instant::now(),
            started_at: now_iso(),
            host_alias,
            default_limit: 1000,
            max_limit: 1000,
            http: http.clone(),
            fed_http: outbound_client(
                egress_policy,
                std::time::Duration::from_secs(8 * slow_factor()),
            ),
            sinks: Arc::new(sinks),
            inbound: Arc::new(std::sync::OnceLock::new()),
            surfaces: Arc::new(vec![Box::new(crate::Admin)]),
            limits: Arc::new(crate::bounds::LimitStats::default()),
            mem_stats: None,
            bus_stats: None,
            egress: Arc::new(crate::egress::Egress::new(egress_policy)),
            policy: Arc::new(crate::policy::AllowAll),
            draining: Arc::new(std::sync::atomic::AtomicBool::new(false)),
            pending_changes: Arc::default(),
            sub_sync: None,
            sub_mirror: None,
            change_flush: None,
            reg_sync: None,
            reg_fail_sync: None,
            reg_mirror: None,
            metrics_render: None,
            csource_notification: None,
            nats: false,
            temporal_record: TemporalRecord::All,
            public_url,
            snapshot_cap: 1024,
            delivery: antares_notifier::DeliveryPolicy::default(),
        }
    }

    /// Attach a policy engine (ADR-0020). One engine per broker, chosen at
    /// startup: an engine that could be swapped per request would make what
    /// a caller may see a function of when they asked. Call before the
    /// state is shared.
    #[must_use]
    pub fn with_policy(mut self, engine: Arc<dyn crate::policy::PolicyEngine>) -> Self {
        self.policy = engine;
        self
    }

    /// Register one more notification binding (6.3.8). A deployment adds a
    /// sink for a scheme this workspace does not ship; endpoints naming that
    /// scheme then validate and deliver through it. Call before the state is
    /// shared — a clone already handed out keeps the registry it was made
    /// with.
    ///
    /// Not behind `test-kit`, unlike the mirror accessors beside it: this is
    /// the sink seam itself (ADR-0016), the call `docs/src/extending.md`
    /// tells a deployment to make, and `examples/plugin-example` is the host
    /// that proves it from outside. A release library without it has no
    /// notification binding seam at all.
    #[must_use]
    pub fn with_sink(mut self, sink: Box<dyn antares_notifier::NotificationSink>) -> Self {
        match Arc::get_mut(&mut self.sinks) {
            Some(reg) => reg.register(sink),
            None => tracing::warn!("sink registry already shared; binding not registered"),
        }
        self
    }

    /// Mount one more HTTP surface (`/q`, `/x`, or below `/x`). A prefix
    /// outside those, or one that overlaps a surface already mounted, is an
    /// error the caller is expected to make fatal: a surface that could
    /// shadow a spec resource would make conformance a function of
    /// deployment configuration, and two surfaces on one prefix would leave
    /// the winner to route-matching order. Call before the state is shared.
    pub fn with_surface(mut self, s: Box<dyn crate::ApiSurface>) -> Result<Self, String> {
        crate::surface::check_prefix(s.prefix())?;
        if let Some(clash) = self
            .surfaces
            .iter()
            .find(|m| crate::surface::overlaps(m.prefix(), s.prefix()))
        {
            return Err(format!(
                "api surface {:?} claims {:?}, already served by {:?} at {:?}",
                s.name(),
                s.prefix(),
                clash.name(),
                clash.prefix()
            ));
        }
        match Arc::get_mut(&mut self.surfaces) {
            Some(v) => v.push(s),
            None => return Err("api surfaces already shared; register before serving".into()),
        }
        Ok(self)
    }

    /// Replace the mounted surfaces with a deployment's own selection —
    /// what a binary reads out of its configuration, rather than what the
    /// default mounting put there. Same prefix rules as `with_surface`, and
    /// a selection may leave admin out: `/q` is then not served at all.
    pub fn with_surfaces(mut self, list: Vec<Box<dyn crate::ApiSurface>>) -> Result<Self, String> {
        self.surfaces = Arc::new(Vec::new());
        for s in list {
            self = self.with_surface(s)?;
        }
        Ok(self)
    }

    /// Serve one request through this broker's own router, in process.
    ///
    /// This is the seam a façade for another standard (SensorThings, OGC
    /// API, WFS, OData) is built on: the façade is an [`crate::ApiSurface`]
    /// under `/x/<standard>` that translates its own request into an NGSI-LD
    /// one and calls this. There is no second data path — the inner request
    /// takes the same route as one off the socket, so negotiation, the
    /// bounds wall, tenancy, the policy seam, history and notifications all
    /// apply exactly once and exactly as they do for an NGSI-LD client.
    ///
    /// `caller` is the outer request's headers, and the ones that decide
    /// WHICH data an operation runs against are copied into the inner
    /// request when it does not set them itself:
    ///
    /// - `NGSILD-Tenant` (6.3.14) — a façade that forgot it would answer
    ///   every caller out of the default tenant, so it is not left to the
    ///   façade to remember;
    /// - `NGSILD-Snapshot` (6.3.22) — for the same reason: a façade called
    ///   inside a snapshot request must not quietly serve live data;
    /// - `Link` (6.3.5) — the `@context` the caller supplied, so a term
    ///   means the same thing on both sides of the translation;
    /// - every header `ANTARES_POLICY_SUBJECT_HEADERS` names, so the policy
    ///   engine is asked about the caller rather than about the façade.
    ///
    /// All values of a copied header are carried, never just the first: a
    /// repeated `NGSILD-Tenant` is `BadRequestData` (6.3.14), and a façade
    /// must not be the place where a repeat is laundered into a single valid
    /// value.
    ///
    /// `&self` on purpose — an inner call runs while the outer handler is
    /// suspended on it, and the router clone is an `Arc` bump over the same
    /// state. The router itself is built once per state, on the first call:
    /// building one costs about 1.5 ms, which no façade should pay per
    /// request. After that first call the state counts as shared, so
    /// `with_surface` and the other builders refuse — which is the rule
    /// they already state.
    pub async fn call(
        &self,
        caller: &axum::http::HeaderMap,
        req: axum::http::Request<axum::body::Body>,
    ) -> axum::response::Response {
        use axum::response::IntoResponse as _;
        let router = self.inbound.get_or_init(|| {
            // Every layer of a built router captured an `AppState` clone, so
            // the memo owns states. The clone it is built from must NOT be
            // able to reach THIS cell: a state that could would hold itself
            // alive for the life of the process and pin its store handle
            // with it — a file store would keep its lock after the host
            // dropped everything. A fresh cell means the memo of a nested
            // façade call hangs off this one and dies with it.
            let mut inner = self.clone();
            inner.inbound = Arc::new(std::sync::OnceLock::new());
            crate::router(inner)
        });
        let (mut parts, body) = req.into_parts();
        for name in &PROPAGATED {
            if parts.headers.contains_key(name) {
                continue;
            }
            for v in caller.get_all(name) {
                parts.headers.append(name.clone(), v.clone());
            }
        }
        for name in crate::policy::SUBJECT_HEADERS.iter() {
            let Ok(name) = axum::http::HeaderName::from_bytes(name.as_bytes()) else {
                continue;
            };
            if parts.headers.contains_key(&name) {
                continue;
            }
            for v in caller.get_all(&name) {
                parts.headers.append(name.clone(), v.clone());
            }
        }
        let req = axum::http::Request::from_parts(parts, body);
        match tower::ServiceExt::oneshot(router.clone(), req).await {
            Ok(r) => r,
            // `Router`'s error is `Infallible`; the arm stays honest rather
            // than unwrapping (the workspace denies unwrap outside tests).
            Err(_) => crate::negotiate::ApiError::from(antares_model::NgsiError::InternalError(
                "the in-process router failed".into(),
            ))
            .into_response(),
        }
    }

    /// Temporal auto-recording happens synchronously in the write path in
    /// EVERY bus mode (read-your-writes) — but only when the loaded
    /// temporal driver actually records anything.
    pub fn record_locally(&self) -> bool {
        self.temporal.supported()
    }

    /// Fire the subscription-sync hook for one written row. `kind` decides
    /// what the mirror does with it: a Subscription is indexed as a document,
    /// a Context Source Registration Subscription only wakes the interval
    /// sweep (5.11.7) — it is matched against registrations, not entities.
    pub(crate) fn sub_changed(
        &self,
        tenant: &antares_model::TenantId,
        kind: antares_store::Kind,
        id: &str,
        doc: Option<&serde_json::Value>,
    ) {
        if let Some(h) = &self.sub_sync {
            h(tenant, kind, id, doc);
        }
    }

    /// 5.2.34: stamp the per-registration cooldown locally AND on the other
    /// api pods (no-op half in local mode). Keyed per tenant — the id alone
    /// is client-chosen per tenant (5.5.10) and must not gate a neighbour.
    pub(crate) fn reg_cooldown_stamp(
        &self,
        tenant: &antares_model::TenantId,
        reg_id: &str,
        ok: bool,
    ) {
        let key = crate::egress::reg_key(tenant.as_str(), reg_id);
        self.egress.reg_record(&key, ok);
        if let Some(h) = &self.reg_fail_sync {
            // the broadcast carries the COMPOSED key; receiving pods stamp it
            // verbatim, so their lookups agree without re-deriving anything
            h(&key, ok);
        }
    }

    /// Fire the registration-delta hook (no-op in local mode).
    pub(crate) fn reg_changed(
        &self,
        tenant: &antares_model::TenantId,
        id: &str,
        doc: Option<&serde_json::Value>,
    ) {
        if let Some(h) = &self.reg_sync {
            h(tenant, id, doc);
        }
    }
}

/// The ONE outbound-client construction for this crate (timeouts at
/// construction). Title-case headers are an http1 knob and timeouts are
/// client-level knobs — both native-only; the browser's fetch supplies its
/// own transport on wasm32.
/// Outbound timeouts stretch 10× when the test binary runs under a
/// sanitizer (ANTARES_TEST_SANITIZER, set by the strict workflow):
/// ThreadSanitizer slows every thread, and 371 tests sharing one runner
/// pushed loopback forwards past the 2 s connect / 5 s total limits —
/// each run failing a different test with a 504. Production is unchanged.
pub fn slow_factor() -> u64 {
    antares_jsonld::slow_factor()
}

fn outbound_client(
    policy: antares_jsonld::EgressPolicy,
    total: std::time::Duration,
) -> antares_jsonld::HttpClient {
    let b = antares_jsonld::with_timeouts(
        antares_jsonld::client_builder(policy),
        std::time::Duration::from_secs(2 * slow_factor()),
        total,
    );
    // the suite's notification receiver asserts header names
    // case-sensitively ("Link", "X-Additional-Key")
    #[cfg(not(target_arch = "wasm32"))]
    let b = b.http1_title_case_headers();
    // reqwest fails to build only when the process cannot initialise its TLS
    // backend: no outbound request of any kind can be made after that
    #[allow(clippy::expect_used)]
    let c = b.build().expect("http client");
    antares_jsonld::wrap_client(c)
}

/// 5.13.1: an @context this broker HOSTS (Hosted or ImplicitlyCreated) is
/// identified by its stored row, never by the URL's shape — a peer broker or
/// an attacker can serve a document under the same resource path, and such a
/// URL is external to us (a Cached entry). The row records the URL it was
/// minted under, so that is what decides: a URL whose trailing segment names
/// a stored row but whose origin is somebody else's names a document this
/// broker does not host, and the stored row — another Tenant's, as often as
/// not — must not be read, counted or rewritten for it. Returns the local
/// row id when the URL is the one the row was minted under.
fn hosted_row(
    store: &dyn CurrentStateDriver,
    tenant: Option<&antares_model::TenantId>,
    url: &str,
) -> Option<(String, serde_json::Value)> {
    let (_, seg) = url.rsplit_once("/ngsi-ld/v1/jsonldContexts/")?;
    let seg = seg.split(['?', '#']).next().unwrap_or(seg);
    if seg.is_empty() || seg.contains('/') {
        return None;
    }
    let row = store
        .context_get(tenant, seg)
        .ok()
        .flatten()
        .filter(|row| row["url"].as_str() == Some(url))?;
    Some((seg.to_owned(), row))
}

fn hosted_row_id(
    store: &dyn CurrentStateDriver,
    tenant: Option<&antares_model::TenantId>,
    url: &str,
) -> Option<String> {
    hosted_row(store, tenant, url).map(|(id, _)| id)
}

/// Server-managed timestamp, ISO 8601 UTC with milliseconds.
pub fn now_iso() -> String {
    chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true)
}

#[cfg(test)]
mod jsonld_context_locality_5_13 {
    use super::*;

    /// Serve one @context document under `path` on a loopback port, counting
    /// fetches (the negative half: a warm copy must NOT be refetched).
    fn context_server(path: &str) -> (String, Arc<std::sync::atomic::AtomicUsize>) {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("bind");
        let port = listener.local_addr().expect("addr").port();
        let fetches = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let n = fetches.clone();
        std::thread::spawn(move || {
            for stream in listener.incoming() {
                let Ok(mut s) = stream else { continue };
                n.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                let body = r#"{"@context":{"peerTemp":"http://example.org/peerTemp"}}"#;
                let resp = format!(
                    "HTTP/1.1 200 OK\r\nContent-Type: application/ld+json\r\n\
                     Content-Length: {}\r\nConnection: close\r\n\r\n{body}",
                    body.len()
                );
                use std::io::{Read, Write};
                let mut buf = [0u8; 2048];
                let _ = s.read(&mut buf);
                let _ = s.write_all(resp.as_bytes());
            }
        });
        (format!("http://127.0.0.1:{port}{path}"), fetches)
    }

    fn fetch_count(c: &Arc<std::sync::atomic::AtomicUsize>) -> usize {
        c.load(std::sync::atomic::Ordering::SeqCst)
    }

    /// 5.13.1: a Cached @context is one this broker fetched from elsewhere.
    /// Which URLs this broker HOSTS is a property of the stored rows, not of
    /// the URL path, so a peer's @context served under the same resource path
    /// is persisted as Cached and never mistaken for a row deleted through
    /// another instance (5.13.5.4).
    #[tokio::test]
    async fn a_peer_context_url_under_the_local_path_is_cached() {
        let st = AppState::new("me".into());
        let (url, fetches) = context_server("/ngsi-ld/v1/jsonldContexts/peer-ctx");
        let user = serde_json::json!(url);
        st.loader.resolve(&user).await.expect("resolve");
        let id = uuid::Uuid::new_v5(&uuid::Uuid::NAMESPACE_URL, url.as_bytes()).to_string();
        let row = st
            .store
            .context_get(None, &id)
            .expect("store")
            .expect("the fetched @context is persisted as a Cached row");
        assert_eq!(row["kind"], "Cached");
        assert_eq!(row["url"], serde_json::json!(url));
        st.loader.resolve(&user).await.expect("resolve again");
        assert_eq!(
            fetch_count(&fetches),
            1,
            "a warm, still-existing @context must not be refetched"
        );
        assert_eq!(
            st.store
                .context_get(None, &id)
                .expect("store")
                .expect("row")["numberOfHits"],
            serde_json::json!(2),
            "both counted uses land on the shared row (5.13.3.5)"
        );
    }

    /// 5.13.3.5 counts uses of an @context this broker hosts on its own
    /// stored row — no second, Cached copy of the same document, and no
    /// fetch of its URL: the row holds the document (5.13.1 "Hosted").
    #[tokio::test]
    async fn a_hosted_context_is_counted_on_its_own_row() {
        let st = AppState::new("me".into());
        let owner = antares_model::TenantId::default();
        let (url, fetches) = context_server("/ngsi-ld/v1/jsonldContexts/local-1");
        // no `owner` member: a row written before it existed belongs to the
        // default Tenant, which is the one resolving below
        st.store
            .context_put(
                Some(&owner),
                "local-1",
                serde_json::json!({
                    "url": url,
                    "localId": "local-1",
                    "kind": "Hosted",
                    "createdAt": now_iso(),
                    "body": {"@context": {"peerTemp": "http://example.org/peerTemp"}},
                }),
            )
            .expect("seed hosted row");
        let user = serde_json::json!(url);
        st.loader.resolve_for(&owner, &user).await.expect("resolve");
        st.loader
            .resolve_for(&owner, &user)
            .await
            .expect("resolve again");
        let cached = uuid::Uuid::new_v5(&uuid::Uuid::NAMESPACE_URL, url.as_bytes()).to_string();
        assert!(
            st.store
                .context_get(None, &cached)
                .expect("store")
                .is_none(),
            "a hosted @context must not be duplicated as a Cached row"
        );
        assert_eq!(
            st.store
                .context_get(Some(&owner), "local-1")
                .expect("store")
                .expect("row")["numberOfHits"],
            serde_json::json!(2)
        );
        assert_eq!(
            fetch_count(&fetches),
            0,
            "a hosted @context is read from its row, never fetched over the network"
        );
    }

    /// 5.13.1: what this broker HOSTS comes from its own store, not from the
    /// network. The in-process copy is a cache, and a miss — a restart (only
    /// `Cached` rows are preloaded) or an eviction from the bounded document
    /// cache — must not turn into an outbound GET of a URL a client chose:
    /// the URL is minted from the request's `Host` header when
    /// `ANTARES_PUBLIC_URL` is unset, so a spoofed one would send the broker
    /// to the spoofer for the term mappings that expand that Tenant's
    /// payloads. 5.5.10 still holds through the store: the row's owner is
    /// the only Tenant it resolves for.
    #[tokio::test]
    async fn a_hosted_context_resolves_from_the_store_after_its_copy_is_gone() {
        let st = AppState::new("me".into());
        let alpha = antares_model::TenantId::new("alpha").expect("tenant");
        let beta = antares_model::TenantId::new("beta").expect("tenant");
        // a dead port: anything resolved can only have come from the row
        let url = "http://127.0.0.1:9/ngsi-ld/v1/jsonldContexts/hosted-1";
        st.store
            .context_put(
                Some(&alpha),
                "hosted-1",
                serde_json::json!({
                    "url": url,
                    "localId": "hosted-1",
                    "kind": "Hosted",
                    "createdAt": now_iso(),
                    "owner": "alpha",
                    "body": {"@context": {"secret": "https://alpha.example/secret"}},
                }),
            )
            .expect("seed hosted row");
        let user = serde_json::json!(url);
        let ctx = st
            .loader
            .resolve_for(&alpha, &user)
            .await
            .expect("the owning Tenant resolves its stored @context");
        assert_eq!(ctx.expand_key("secret"), "https://alpha.example/secret");
        let err = st
            .loader
            .resolve_for(&beta, &user)
            .await
            .expect_err("another Tenant may not resolve it (5.5.10)");
        assert!(
            matches!(err, antares_model::NgsiError::LdContextNotAvailable(_)),
            "got {err:?}"
        );
    }

    /// 5.13.1 + 5.5.10: a stored @context with no `owner` member is not
    /// ownerless. `contexts.rs` `row_visible` reads a row written before that
    /// member existed as the DEFAULT Tenant's — it is listed, served and
    /// deleted through that Tenant alone — so resolving one must answer the
    /// same way. Read as "belongs to no Tenant" it would expand every other
    /// Tenant's payloads with the default Tenant's private term mappings.
    #[tokio::test]
    async fn a_stored_context_without_an_owner_belongs_to_the_default_tenant() {
        let st = AppState::new("me".into());
        let default = antares_model::TenantId::new("default").expect("tenant");
        let other = antares_model::TenantId::new("beta").expect("tenant");
        let url = "http://127.0.0.1:9/ngsi-ld/v1/jsonldContexts/legacy-1";
        st.store
            .context_put(
                Some(&default),
                "legacy-1",
                serde_json::json!({
                    "url": url,
                    "localId": "legacy-1",
                    "kind": "Hosted",
                    "createdAt": now_iso(),
                    "body": {"@context": {"legacy": "https://legacy.example/term"}},
                }),
            )
            .expect("seed a row from before the owner member");
        let user = serde_json::json!(url);
        let ctx = st
            .loader
            .resolve_for(&default, &user)
            .await
            .expect("the Tenant the row belongs to resolves it");
        assert_eq!(ctx.expand_key("legacy"), "https://legacy.example/term");
        let err = st
            .loader
            .resolve_for(&other, &user)
            .await
            .expect_err("no other Tenant may resolve it (5.5.10)");
        assert!(
            matches!(err, antares_model::NgsiError::LdContextNotAvailable(_)),
            "got {err:?}"
        );
    }

    /// 5.13.1 again, with the local id of a row that exists: what this broker
    /// hosts is the row MINTED under that URL, so a document served from
    /// somewhere else under the same resource path — with the local id of a
    /// Hosted @context another Tenant added — is external. It becomes a
    /// Cached row of its own, and the Tenant's row is neither read for its
    /// mappings nor counted against.
    #[tokio::test]
    async fn a_peer_url_reusing_a_stored_local_id_leaves_that_row_alone() {
        let st = AppState::new("me".into());
        let (url, _) = context_server("/ngsi-ld/v1/jsonldContexts/alpha-1");
        let row = serde_json::json!({
            "url": "https://broker.example/ngsi-ld/v1/jsonldContexts/alpha-1",
            "localId": "alpha-1",
            "kind": "Hosted",
            "createdAt": now_iso(),
            "owner": "alpha",
            "body": {"@context": {"a": "http://example.org/a"}},
        });
        let alpha = antares_model::TenantId::new("alpha").expect("tenant");
        st.store
            .context_put(Some(&alpha), "alpha-1", row.clone())
            .expect("seed hosted row");
        st.loader
            .resolve_for(&antares_model::TenantId::default(), &serde_json::json!(url))
            .await
            .expect("resolve");
        assert_eq!(
            st.store
                .context_get(Some(&alpha), "alpha-1")
                .expect("store")
                .expect("row"),
            row,
            "another Tenant's row must be untouched by a peer URL"
        );
        let cached = uuid::Uuid::new_v5(&uuid::Uuid::NAMESPACE_URL, url.as_bytes()).to_string();
        assert_eq!(
            st.store
                .context_get(None, &cached)
                .expect("store")
                .expect("row")["kind"],
            serde_json::json!("Cached"),
            "the fetched document is this broker's Cached copy (5.13.1)"
        );
    }
}
