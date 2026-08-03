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
    pub host_alias: String,
    /// Default page size for queries without an explicit limit.
    pub default_limit: usize,
    /// Hard ceiling on limit (TooManyResults guard).
    pub max_limit: usize,
}

impl AppState {
    pub fn new(host_alias: String) -> Self {
        Self {
            store: Arc::new(Store::default()),
            loader: Arc::new(Loader::new()),
            started: Instant::now(),
            host_alias,
            default_limit: 1000,
            max_limit: 1000,
        }
    }
}

/// Server-managed timestamp, ISO 8601 UTC with milliseconds.
pub fn now_iso() -> String {
    chrono::Utc::now()
        .to_rfc3339_opts(chrono::SecondsFormat::Millis, true)
}
