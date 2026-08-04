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
        Self {
            store,
            store_mode,
            loader: Arc::new(Loader::new()),
            started: Instant::now(),
            started_at: now_iso(),
            host_alias,
            default_limit: 1000,
            max_limit: 1000,
            http: reqwest::Client::builder()
                .connect_timeout(std::time::Duration::from_secs(2))
                .timeout(std::time::Duration::from_secs(5))
                // the suite's notification receiver asserts header names
                // case-sensitively ("Link", "X-Additional-Key")
                .http1_title_case_headers()
                .build()
                .expect("http client"),
            fed_http: reqwest::Client::builder()
                .connect_timeout(std::time::Duration::from_secs(2))
                .timeout(std::time::Duration::from_secs(8))
                .http1_title_case_headers()
                .build()
                .expect("fed http client"),
            #[cfg(feature = "mqtt")]
            mqtt: Arc::new(antares_notifier::mqtt::MqttSink::default()),
            limits: Arc::new(crate::bounds::LimitStats::default()),
            mem_stats: None,
        }
    }
}

/// Server-managed timestamp, ISO 8601 UTC with milliseconds.
pub fn now_iso() -> String {
    chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true)
}
