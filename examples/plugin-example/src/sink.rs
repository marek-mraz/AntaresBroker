// SPDX-License-Identifier: EUPL-1.2
//! The delivery seam: a `memory://` notification binding that keeps what it
//! was given instead of opening a socket.

use antares_model::NgsiError;
use antares_notifier::{DeliveryError, DeliveryFuture, NotificationSink, Outbound};
use serde_json::Value;
use std::sync::{Mutex, RwLock};
use std::time::Duration;

/// Notifications delivered to `memory://…` endpoints, newest last. A test
/// (or a demo) reads them back instead of standing up an HTTP listener.
#[derive(Default)]
pub struct MemorySink {
    delivered: Mutex<Vec<(String, Value)>>,
    /// Endpoints that must fail, so the 5.8.6 failure bookkeeping can be
    /// exercised without an unreachable host.
    failing: RwLock<Vec<String>>,
}

impl MemorySink {
    /// A sink that accepts every `memory://` endpoint.
    pub fn new() -> Self {
        Self::default()
    }

    /// Make deliveries to `uri` fail from now on.
    pub fn fail(&self, uri: &str) {
        self.failing
            .write()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .push(uri.to_owned());
    }

    /// Every notification delivered so far, as (endpoint, body).
    pub fn delivered(&self) -> Vec<(String, Value)> {
        self.delivered
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clone()
    }
}

impl NotificationSink for MemorySink {
    fn schemes(&self) -> &'static [&'static str] {
        &["memory"]
    }

    /// The endpoint is `memory://<name>`; anything else is refused when the
    /// subscription is created, not when a notification is due.
    fn parse_endpoint(&self, uri: &str, _notifier_info: &[(&str, &str)]) -> Result<(), NgsiError> {
        match uri.strip_prefix("memory://") {
            Some(rest) if !rest.is_empty() => Ok(()),
            _ => Err(NgsiError::BadRequestData(format!(
                "memory endpoint must be memory://<name>, got {uri:?}"
            ))),
        }
    }

    /// No socket is opened, so the egress policy has nothing to police.
    fn network(&self) -> bool {
        false
    }

    fn deliver<'a>(
        &'a self,
        uri: &'a str,
        out: &'a Outbound,
        _timeout: Duration,
    ) -> DeliveryFuture<'a> {
        Box::pin(async move {
            if self
                .failing
                .read()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .iter()
                .any(|f| f == uri)
            {
                return Err(DeliveryError::failed(format!("{uri} is set to fail")));
            }
            self.delivered
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .push((uri.to_owned(), out.body.clone()));
            Ok(())
        })
    }
}
