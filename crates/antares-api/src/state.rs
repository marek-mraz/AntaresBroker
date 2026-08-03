//! Shared application state.

use antares_jsonld::Loader;
use antares_sql::store::Store;
use std::sync::Arc;
use std::time::Instant;

#[derive(Clone)]
pub struct AppState {
    pub store: Arc<Store>,
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
}

impl AppState {
    pub fn new(host_alias: String) -> Self {
        Self {
            store: Arc::new(Store::default()),
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
        }
    }
}

/// Server-managed timestamp, ISO 8601 UTC with milliseconds.
pub fn now_iso() -> String {
    chrono::Utc::now()
        .to_rfc3339_opts(chrono::SecondsFormat::Millis, true)
}
