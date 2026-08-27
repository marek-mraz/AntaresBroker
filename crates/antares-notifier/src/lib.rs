// SPDX-License-Identifier: EUPL-1.2
//! Notification delivery.
//!
//! The pluggability seam is fixed in v0: sinks register by `endpoint.uri`
//! scheme; a subscription naming an unregistered scheme is rejected at
//! creation with OperationNotSupported (422). Sinks: http/reqwest,
//! mqtt/rumqttc behind the `mqtt` feature, ws in `antares-ws`.

use antares_model::NgsiError;

#[cfg(feature = "mqtt")]
pub mod mqtt;

/// How one notification is delivered: how many attempts, how they are
/// spaced, and how long after the first attempt the last one may still
/// start. The default is a single attempt — exactly 5.8.6, which sends the
/// notification once and books the outcome. Retries are an operator choice:
/// they never move `timesSent` again (the notification is sent ONCE, the
/// attempts are transport), a retry that succeeds books `lastSuccess` and
/// `status` ok, an exhausted policy leaves a dead letter.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct DeliveryPolicy {
    /// Total attempts, first one included. 1 = never retry.
    pub attempts: u32,
    /// Delay before the first retry; doubles per retry up to `MAX_BACKOFF`.
    pub backoff: std::time::Duration,
    /// Fraction of the delay randomised in both directions (0.2 = ±20 %),
    /// so many subscriptions to one dead endpoint do not retry in lockstep.
    pub jitter: f32,
    /// A retry that would start later than this after the first attempt is
    /// not made.
    pub max_age: std::time::Duration,
}

impl Default for DeliveryPolicy {
    fn default() -> Self {
        Self {
            attempts: 1,
            backoff: std::time::Duration::from_secs(1),
            jitter: 0.2,
            max_age: std::time::Duration::from_secs(300),
        }
    }
}

impl DeliveryPolicy {
    /// Ceiling on one delay, whatever the doubling says.
    pub const MAX_BACKOFF: std::time::Duration = std::time::Duration::from_secs(60);

    pub fn with_max_age(self, max_age: std::time::Duration) -> Self {
        Self { max_age, ..self }
    }

    /// ANTARES_NOTIFY_ATTEMPTS / ANTARES_NOTIFY_BACKOFF_MS /
    /// ANTARES_NOTIFY_MAX_AGE_SECS, each optional; a value that is present
    /// but not a positive integer is a startup error, never a silent default.
    pub fn from_env() -> Result<Self, String> {
        let get = |k: &str| std::env::var(k).ok();
        Self::parse(
            get("ANTARES_NOTIFY_ATTEMPTS").as_deref(),
            get("ANTARES_NOTIFY_BACKOFF_MS").as_deref(),
            get("ANTARES_NOTIFY_MAX_AGE_SECS").as_deref(),
        )
    }

    pub fn parse(
        attempts: Option<&str>,
        backoff_ms: Option<&str>,
        max_age_secs: Option<&str>,
    ) -> Result<Self, String> {
        fn positive(name: &str, raw: Option<&str>) -> Result<Option<u64>, String> {
            match raw {
                None => Ok(None),
                Some(v) => v
                    .trim()
                    .parse::<u64>()
                    .ok()
                    .filter(|n| *n > 0)
                    .map(Some)
                    .ok_or_else(|| format!("{name} must be a positive integer, got {v:?}")),
            }
        }
        let d = Self::default();
        Ok(Self {
            attempts: positive("ANTARES_NOTIFY_ATTEMPTS", attempts)?
                .map_or(d.attempts, |n| u32::try_from(n).unwrap_or(u32::MAX)),
            backoff: positive("ANTARES_NOTIFY_BACKOFF_MS", backoff_ms)?
                .map_or(d.backoff, std::time::Duration::from_millis),
            jitter: d.jitter,
            max_age: positive("ANTARES_NOTIFY_MAX_AGE_SECS", max_age_secs)?
                .map_or(d.max_age, std::time::Duration::from_secs),
        })
    }

    /// The delay before the next attempt after `made` attempts, `elapsed`
    /// after the first one — `None` when the policy is exhausted or the
    /// retry would start past `max_age`.
    pub fn next_delay(
        &self,
        made: u32,
        elapsed: std::time::Duration,
    ) -> Option<std::time::Duration> {
        if made == 0 || made >= self.attempts {
            return None;
        }
        let doubled = self
            .backoff
            .checked_mul(1u32.checked_shl(made - 1).unwrap_or(u32::MAX))
            .unwrap_or(Self::MAX_BACKOFF)
            .min(Self::MAX_BACKOFF);
        // ±jitter, seeded from the clock's sub-second noise: cheap and
        // uncorrelated enough to spread retries; no RNG dependency.
        let noise = (std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.subsec_nanos())
            .unwrap_or(0)
            % 1000) as f32
            / 1000.0;
        let factor = 1.0 + self.jitter.clamp(0.0, 1.0) * (2.0 * noise - 1.0);
        let delay = doubled.mul_f32(factor.max(0.0));
        (elapsed + delay <= self.max_age).then_some(delay)
    }
}

/// A delivery binding for one URI scheme family.
pub trait NotificationSink: Send + Sync {
    /// Schemes this sink serves, e.g. `["http", "https"]`.
    fn schemes(&self) -> &'static [&'static str];
}

/// Scheme → sink registry; populated by the composition root.
#[derive(Default)]
pub struct SinkRegistry {
    sinks: Vec<Box<dyn NotificationSink>>,
}

impl SinkRegistry {
    pub fn register(&mut self, sink: Box<dyn NotificationSink>) {
        self.sinks.push(sink);
    }

    pub fn sink_for(&self, scheme: &str) -> Option<&dyn NotificationSink> {
        // Linear scan is fine at <5 sinks; switch to a map when sinks multiply.
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

#[cfg(test)]
mod policy_tests {
    use super::DeliveryPolicy;
    use std::time::Duration;

    fn policy(attempts: u32, backoff_ms: u64, max_age_secs: u64) -> DeliveryPolicy {
        DeliveryPolicy {
            attempts,
            backoff: Duration::from_millis(backoff_ms),
            jitter: 0.0,
            max_age: Duration::from_secs(max_age_secs),
        }
    }

    /// Drive `op` the way the delivery loop does: one attempt, then a
    /// retry after every delay the policy grants. Returns the outcome and
    /// how many calls were made.
    fn run<E>(p: DeliveryPolicy, mut op: impl FnMut(u32) -> Result<(), E>) -> (Result<(), E>, u32) {
        let mut made = 1;
        let mut elapsed = Duration::ZERO;
        let mut last = op(made);
        while last.is_err() {
            let Some(d) = p.next_delay(made, elapsed) else {
                break;
            };
            elapsed += d;
            made += 1;
            last = op(made);
        }
        (last, made)
    }

    #[test]
    fn default_is_a_single_attempt() {
        let p = DeliveryPolicy::default();
        assert_eq!(p.attempts, 1);
        assert_eq!(p.next_delay(1, Duration::ZERO), None);
        let (res, made) = run(p, |_| Err::<(), _>("down"));
        assert_eq!(made, 1);
        assert_eq!(res, Err("down"));
    }

    #[test]
    fn fails_twice_then_succeeds_is_three_calls_and_one_ok() {
        let (res, made) = run(
            policy(3, 100, 60),
            |n| if n < 3 { Err("down") } else { Ok(()) },
        );
        assert_eq!(made, 3);
        assert_eq!(res, Ok(()));
    }

    #[test]
    fn always_failing_stops_after_attempts_with_the_last_error() {
        let (res, made) = run(policy(3, 100, 60), Err::<(), _>);
        assert_eq!(made, 3);
        assert_eq!(res, Err(3), "the LAST error is returned");
    }

    #[test]
    fn max_age_cuts_the_schedule_short() {
        // 100 ms, 200 ms, 400 ms … but only 250 ms of age allowed: the
        // second retry would land at 300 ms, so it is never made.
        let (res, made) = run(
            policy(10, 100, 0).with_max_age(Duration::from_millis(250)),
            |_| Err::<(), _>("down"),
        );
        assert_eq!(made, 2);
        assert!(res.is_err());
    }

    #[test]
    fn backoff_doubles_per_retry_and_jitter_stays_within_bounds() {
        let p = policy(5, 100, 60);
        assert_eq!(
            p.next_delay(1, Duration::ZERO),
            Some(Duration::from_millis(100))
        );
        assert_eq!(
            p.next_delay(2, Duration::ZERO),
            Some(Duration::from_millis(200))
        );
        assert_eq!(
            p.next_delay(3, Duration::ZERO),
            Some(Duration::from_millis(400))
        );
        let j = DeliveryPolicy { jitter: 0.5, ..p };
        for _ in 0..50 {
            let d = j.next_delay(1, Duration::ZERO).expect("granted");
            assert!(
                d >= Duration::from_millis(50) && d <= Duration::from_millis(150),
                "{d:?}"
            );
        }
        let d = j.next_delay(30, Duration::ZERO);
        assert_eq!(d, None, "attempt 30 of 5 is over");
        let big = policy(40, 100, 3600);
        let d = big.next_delay(35, Duration::ZERO).expect("granted");
        assert!(
            d <= DeliveryPolicy::MAX_BACKOFF,
            "2^34 backoff must not overflow: {d:?}"
        );
    }

    #[test]
    fn env_parsing_rejects_garbage_and_zero_attempts() {
        let parse =
            |a: Option<&str>, b: Option<&str>, m: Option<&str>| DeliveryPolicy::parse(a, b, m);
        assert_eq!(parse(None, None, None), Ok(DeliveryPolicy::default()));
        let p = parse(Some("3"), Some("250"), Some("30")).expect("valid");
        assert_eq!(p.attempts, 3);
        assert_eq!(p.backoff, Duration::from_millis(250));
        assert_eq!(p.max_age, Duration::from_secs(30));
        for bad in [
            parse(Some("0"), None, None),
            parse(Some("-1"), None, None),
            parse(Some("three"), None, None),
            parse(None, Some("0"), None),
            parse(None, Some("1e3"), None),
            parse(None, None, Some("never")),
            parse(Some("2"), Some(""), None),
        ] {
            assert!(bad.is_err(), "{bad:?}");
        }
    }
}
