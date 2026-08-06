//! N1: the broker in a browser tab.
//!
//! Everything above the socket is unchanged — the same axum router, the same
//! handlers, the same memory store through the §A seam. The ONE thing that
//! cannot cross is the TCP listener (browsers have no inbound sockets), so a
//! Service Worker feeds requests in instead (N3) and `handle` drives the
//! router directly with `tower::Service::call`, exactly as the native
//! `main.rs` accept loop does per connection.
//!
//! What the browser build is NOT (N8): no NATS, no MQTT, no Postgres, no
//! roles. `bus=local` and the memory store are the only shapes that exist
//! here, which is why this crate turns `antares-api`'s default features off.

use axum::body::Body;
use http_body_util::BodyExt;
use tower::Service;

/// One broker instance: the composed router plus the state it owns.
pub struct Broker {
    router: axum::Router,
}

impl Default for Broker {
    fn default() -> Self {
        Self::new()
    }
}

impl Broker {
    /// Build the router over an in-memory store. Mirrors the native wiring
    /// minus the pieces that need a socket or a pool.
    pub fn new() -> Self {
        Self::with_store(antares_sql::store::Store::default(), "memory")
    }

    /// The same wiring over an externally-constructed store — the OPFS-backed
    /// store (N4) enters here; `mode` is what `/q/health` reports (A4).
    pub fn with_store(store: antares_sql::store::Store, mode: &str) -> Self {
        Self::with_store_alias(store, mode, None)
    }

    /// `host_alias` names this instance in Via chains — must be distinct per
    /// instance in a federation, or loop detection 508s every forward.
    pub fn with_store_alias(
        store: antares_sql::store::Store,
        mode: &str,
        host_alias: Option<String>,
    ) -> Self {
        let store = antares_sql::store::any::AnyStore::Mem(store);
        let mut state = antares_api::AppState::with_store(
            host_alias.unwrap_or_else(|| "antares-wasm".to_owned()),
            std::sync::Arc::new(store),
            mode.to_owned(),
        );
        // Same in-process matcher/notifier path as bus=local (§9.2): the
        // store's change hook feeds it, no bus process exists to talk to.
        antares_api::notify::wire(&mut state);
        Self {
            router: antares_api::router(state),
        }
    }

    /// Serve ONE request. The signature is the seam every front end reduces
    /// to: the Service Worker (N3), the in-page API, and the Node shim (N7a)
    /// all funnel here.
    pub async fn handle(&mut self, req: http::Request<Vec<u8>>) -> http::Response<Vec<u8>> {
        let (mut parts, body) = req.into_parts();
        // The native binary routes under NormalizePathLayer::trim_trailing_slash
        // (6.3 URLs arrive both with and without a trailing '/'); this seam is
        // the wasm equivalent — same trim, applied before the router sees it.
        if let Some(pq) = parts.uri.path_and_query() {
            let path = pq.path();
            let trimmed = path.trim_end_matches('/');
            if trimmed.len() != path.len() && !trimmed.is_empty() {
                let new = match pq.query() {
                    Some(q) => format!("{trimmed}?{q}"),
                    None => trimmed.to_owned(),
                };
                if let Ok(uri) = new.parse() {
                    parts.uri = uri;
                }
            }
        }
        let req = http::Request::from_parts(parts, Body::from(body));
        let resp = match self.router.call(req).await {
            Ok(r) => r,
            // The router is Infallible; keep the arm honest rather than
            // unwrapping (workspace lints deny unwrap outside tests).
            Err(_) => {
                return http::Response::builder()
                    .status(500)
                    .body(Vec::new())
                    .unwrap_or_default()
            }
        };
        let (parts, body) = resp.into_parts();
        let bytes = body
            .collect()
            .await
            .map(|c| c.to_bytes().to_vec())
            .unwrap_or_default();
        http::Response::from_parts(parts, bytes)
    }
}

#[cfg(target_arch = "wasm32")]
mod browser;
#[cfg(target_arch = "wasm32")]
mod opfs;
#[cfg(target_arch = "wasm32")]
pub use browser::*;
