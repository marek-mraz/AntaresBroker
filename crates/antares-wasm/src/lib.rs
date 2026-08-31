// SPDX-License-Identifier: EUPL-1.2
//! The broker in a browser tab.
//!
//! Everything above the socket is unchanged — the same axum router, the same
//! handlers, the same memory store through the store seam. The ONE thing that
//! cannot cross is the TCP listener (browsers have no inbound sockets), so a
//! Service Worker feeds requests in instead and `handle` drives the
//! router directly with `tower::Service::call`, exactly as the native
//! `main.rs` accept loop does per connection.
//!
//! What the browser build is NOT: no NATS, no MQTT, no Postgres, no
//! roles. `bus=local` and the memory store are the only shapes that exist
//! here, which is why this crate turns `antares-api`'s default features off.
#![cfg_attr(not(test), warn(clippy::expect_used))]

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
    /// store enters here; `mode` is what `/q/health` reports.
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
        // wasm compositions only ever carry the Mem arm — anything but a
        // known Mem-arm mode name is a caller bug, defaulted to memory.
        let mut state = antares_api::AppState::with_store(
            host_alias.unwrap_or_else(|| "antares-wasm".to_owned()),
            std::sync::Arc::new(store),
            mode,
        );
        // Same in-process matcher/notifier path as bus=local: the
        // store's change hook feeds it, no bus process exists to talk to.
        antares_api::notify::wire(&mut state);
        // 5.8.1.4: distributed subscriptions hand this URL to the remote
        // broker as the notification callback. wasm32 has no process env
        // (the native default reads ANTARES_PUBLIC_URL + appends the port),
        // so the SAME variable comes off `globalThis` — set it before
        // construction, like ANTARES_SWEEP_SECS below. Absent → the portless
        // host-alias default, which no peer outside a browser can dial.
        #[cfg(target_arch = "wasm32")]
        if let Some(url) = js_sys::Reflect::get(&js_sys::global(), &"ANTARES_PUBLIC_URL".into())
            .ok()
            .and_then(|v| v.as_string())
            .filter(|s| !s.is_empty())
        {
            state.public_url = url;
        }
        // 4.22 GC: the native broker sweeps on a tokio interval (main.rs,
        // ANTARES_SWEEP_SECS). The browser has no env, so the SAME variable is
        // read off `globalThis.ANTARES_SWEEP_SECS` (seconds, set before
        // construction — the playground forwards a ?ANTARES_SWEEP_SECS= URL
        // param into both the worker and in-page contexts); absent → 60 s.
        // Without this loop the OPFS file grows without bound under ticking
        // transient attributes — reads filter expired instances but nothing
        // would ever delete them.
        #[cfg(target_arch = "wasm32")]
        {
            let store = state.store.clone();
            let sweep_ms = js_sys::Reflect::get(&js_sys::global(), &"ANTARES_SWEEP_SECS".into())
                .ok()
                .and_then(|v| v.as_f64())
                .filter(|s| *s > 0.0)
                .map(|s| (s * 1000.0) as u32)
                .unwrap_or(60_000);
            wasm_bindgen_futures::spawn_local(async move {
                loop {
                    gloo_timers::future::TimeoutFuture::new(sweep_ms).await;
                    store.sweep_expired();
                }
            });
        }
        Self {
            router: antares_api::router(state),
        }
    }

    /// Serve ONE request. The signature is the seam every front end reduces
    /// to: the Service Worker, the in-page API, and the Node shim
    /// all funnel here.
    ///
    /// `&self` on purpose: a federation forward to the loopback host re-enters
    /// this same instance WHILE an outer `handle` is suspended — `&mut`
    /// would make that a wasm-bindgen recursive-borrow error. Router clone is
    /// a cheap Arc bump and shares all state.
    pub async fn handle(&self, req: http::Request<Vec<u8>>) -> http::Response<Vec<u8>> {
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
        let mut router = self.router.clone();
        let resp = match router.call(req).await {
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
