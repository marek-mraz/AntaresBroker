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
        let store = antares_sql::store::any::AnyStore::Mem(store);
        let mut state = antares_api::AppState::with_store(
            "antares-wasm".to_owned(),
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
        let (parts, body) = req.into_parts();
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
