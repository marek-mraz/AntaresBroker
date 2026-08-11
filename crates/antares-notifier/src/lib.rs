//! Notification delivery (docs/deep-analysis.md §9.2/§9.3).
//!
//! The pluggability seam is fixed in v0: sinks register by `endpoint.uri`
//! scheme; a subscription naming an unregistered scheme is rejected at
//! creation with OperationNotSupported (422). Sinks land in phase 2
//! (http/reqwest, mqtt/rumqttc behind the `mqtt` feature, ws in `antares-ws`).

use antares_model::NgsiError;

#[cfg(feature = "mqtt")]
pub mod mqtt;

/// A delivery binding for one URI scheme family.
pub trait NotificationSink: Send + Sync {
    /// Schemes this sink serves, e.g. `["http", "https"]`.
    fn schemes(&self) -> &'static [&'static str];
}

/// Scheme → sink registry; populated by the composition root (§9.2).
#[derive(Default)]
pub struct SinkRegistry {
    sinks: Vec<Box<dyn NotificationSink>>,
}

impl SinkRegistry {
    pub fn register(&mut self, sink: Box<dyn NotificationSink>) {
        self.sinks.push(sink);
    }

    pub fn sink_for(&self, scheme: &str) -> Option<&dyn NotificationSink> {
        // ponytail: linear scan over <5 sinks; a map when sinks multiply.
        self.sinks
            .iter()
            .find(|s| s.schemes().contains(&scheme))
            .map(AsRef::as_ref)
    }

    /// Reject-at-creation check: an endpoint scheme this deployment cannot
    /// deliver to is OperationNotSupported (5.5.6).
    pub fn require(&self, scheme: &str) -> Result<(), NgsiError> {
        self.sink_for(scheme).map(|_| ()).ok_or_else(|| {
            NgsiError::OperationNotSupported(format!(
                "no notification binding registered for scheme {scheme:?}"
            ))
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct HttpSink;
    impl NotificationSink for HttpSink {
        fn schemes(&self) -> &'static [&'static str] {
            &["http", "https"]
        }
    }

    #[test]
    fn unknown_scheme_is_operation_not_supported() {
        let mut reg = SinkRegistry::default();
        reg.register(Box::new(HttpSink));
        assert!(reg.require("http").is_ok());
        assert!(reg.require("https").is_ok());
        let err = reg.require("ws").expect_err("ws not registered in v1");
        assert_eq!(err.status(), 422);
    }
}
