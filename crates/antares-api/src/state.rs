//! Shared application state.

use antares_jsonld::Loader;
use antares_sql::store::any::AnyStore;
use antares_sql::store::Store;
use std::sync::Arc;
use std::time::Instant;

#[derive(Clone)]
pub struct AppState {
    pub store: Arc<AnyStore>,
    /// Active store backend ("memory" | "file" | …) — reported by `/q/health`
    /// (A4; NOT in `/info/sourceIdentity`, which is a spec resource).
    pub store_mode: String,
    pub loader: Arc<Loader>,
    pub started: Instant,
    /// Startup timestamp (createdAt of the built-in core @context entry).
    pub started_at: String,
    pub host_alias: String,
    /// Default page size for queries without an explicit limit.
    pub default_limit: usize,
    /// Hard ceiling on limit (TooManyResults guard).
    pub max_limit: usize,
    /// One shared outbound client, timeouts at construction (U1 lesson).
    pub http: reqwest::Client,
    /// Federation-forwarding client — longer deadline: the ETSI mock replies
    /// to unstubbed forwards only when the robot side wakes (up to ~5 s).
    pub fed_http: reqwest::Client,
    /// Clause 7 MQTT delivery: bounded pooled sink (L5/U1 lessons).
    #[cfg(feature = "mqtt")]
    pub mqtt: Arc<antares_notifier::mqtt::MqttSink>,
    /// I2 bounds-wall rejection counters (exported by /q/health).
    pub limits: Arc<crate::bounds::LimitStats>,
    /// J7: allocator stats provider (set by the broker; None in tests/wasm).
    pub mem_stats: Option<Arc<dyn Fn() -> serde_json::Value + Send + Sync>>,
    /// I4: one egress policy for notifications and federation forwards
    /// (scheme allowlist, private-range deny, per-destination breakers).
    pub egress: Arc<crate::egress::Egress>,
    /// K1: set the moment SIGTERM arrives, BEFORE the listener stops
    /// accepting — `/q/health` then answers 503 DRAINING so the load balancer
    /// takes this instance out while its socket still works.
    pub draining: Arc<std::sync::atomic::AtomicBool>,
    /// F4 (bus=nats): called after every Subscription CUD so the wiring can
    /// push the change into the KV mirror bucket. `None` in local mode — this
    /// process's store already IS the truth every consumer reads.
    #[allow(clippy::type_complexity)]
    pub sub_sync: Option<
        Arc<dyn Fn(&antares_model::TenantId, &str, Option<&serde_json::Value>) + Send + Sync>,
    >,
    /// F4 (bus=nats): the KV-watched compiled-subscription mirror the matcher
    /// reads, so the hot path never touches Postgres (§6.4). `None` in local
    /// mode (the matcher reads the store directly).
    pub sub_mirror: Option<Arc<crate::notify::SubMirror>>,
    /// F5 (bus=nats): called after every Registration CUD so the wiring can
    /// publish the delta on `ANTARES_REGISTRY`. `None` in local mode.
    #[allow(clippy::type_complexity)]
    pub reg_sync: Option<
        Arc<dyn Fn(&antares_model::TenantId, &str, Option<&serde_json::Value>) + Send + Sync>,
    >,
    /// F5 (bus=nats): the ONE per-process compiled registration mirror,
    /// delta-fed from `ANTARES_REGISTRY`; expiry stays filtered at the single
    /// yield point (`federation::matching_regs`). `None` in local mode.
    pub reg_mirror: Option<Arc<crate::notify::DocMirror>>,
    /// Temporal auto-recording happens synchronously in the write path in
    /// EVERY bus mode (K8 lesson — read-your-writes; the F8 recorder
    /// consumer is gone). The flag stays as the tests' lever for exercising
    /// the no-local-recording shape.
    pub record_locally: bool,
    /// K12: renders the Prometheus text format for /q/metrics. Installed by
    /// the broker (the only crate that knows an exporter exists, §9.2);
    /// `None` = 404, the facade calls elsewhere stay no-ops.
    pub metrics_render: Option<Arc<dyn Fn() -> String + Send + Sync>>,
    /// True only under bus=nats (set by the broker's wiring): multiple
    /// processes share the store, so interval-subscription firings must be
    /// claimed single-winner (§3.1.6). bus=local keeps the direct path — a
    /// claim there would disturb the 046_12 bookkeeping ordering for
    /// nothing.
    pub nats: bool,
}

impl AppState {
    /// In-memory store (tests, `memory` mode).
    pub fn new(host_alias: String) -> Self {
        Self::with_store(
            host_alias,
            Arc::new(AnyStore::Mem(Store::default())),
            "memory".into(),
        )
    }

    pub fn with_store(host_alias: String, store: Arc<AnyStore>, store_mode: String) -> Self {
        // I4: one policy value, read once, shared by every outbound path —
        // the gate (scheme/breakers) and the clients (DNS pinning, redirect
        // cap) can never disagree about what is allowed (§16.4).
        let egress_policy = antares_jsonld::EgressPolicy::from_env();
        Self {
            store,
            store_mode,
            loader: Arc::new(Loader::new()),
            started: Instant::now(),
            started_at: now_iso(),
            host_alias,
            default_limit: 1000,
            max_limit: 1000,
            http: antares_jsonld::client_builder(egress_policy)
                .connect_timeout(std::time::Duration::from_secs(2))
                .timeout(std::time::Duration::from_secs(5))
                // the suite's notification receiver asserts header names
                // case-sensitively ("Link", "X-Additional-Key")
                .http1_title_case_headers()
                .build()
                .expect("http client"),
            fed_http: antares_jsonld::client_builder(egress_policy)
                .connect_timeout(std::time::Duration::from_secs(2))
                .timeout(std::time::Duration::from_secs(8))
                .http1_title_case_headers()
                .build()
                .expect("fed http client"),
            #[cfg(feature = "mqtt")]
            mqtt: Arc::new(antares_notifier::mqtt::MqttSink::default()),
            limits: Arc::new(crate::bounds::LimitStats::default()),
            mem_stats: None,
            egress: Arc::new(crate::egress::Egress::new(egress_policy)),
            draining: Arc::new(std::sync::atomic::AtomicBool::new(false)),
            sub_sync: None,
            sub_mirror: None,
            reg_sync: None,
            reg_mirror: None,
            record_locally: true,
            metrics_render: None,
            nats: false,
        }
    }

    /// Fire the F4 subscription-sync hook (no-op in local mode).
    pub fn sub_changed(
        &self,
        tenant: &antares_model::TenantId,
        id: &str,
        doc: Option<&serde_json::Value>,
    ) {
        if let Some(h) = &self.sub_sync {
            h(tenant, id, doc);
        }
    }

    /// Fire the F5 registration-delta hook (no-op in local mode).
    pub fn reg_changed(
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

/// Server-managed timestamp, ISO 8601 UTC with milliseconds.
pub fn now_iso() -> String {
    chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true)
}
