// SPDX-License-Identifier: EUPL-1.2
//! Notification delivery.
//!
//! A sink serves one family of `endpoint.uri` schemes (6.3.8, and 7.2 for
//! the optional MQTT binding): it validates its own endpoints at
//! subscription creation and delivers the prepared notification. The
//! registry keys sinks by scheme and is the only way one is chosen — a
//! scheme it does not hold is rejected at creation, never delivered through
//! another binding. Sinks: http/reqwest, mqtt/rumqttc behind the `mqtt`
//! feature.
//!
//! The egress policy (allowlist, private-range deny, per-destination
//! breakers) runs in the caller before `deliver`, so a sink registered from
//! outside this workspace cannot step around it.
#![cfg_attr(not(test), warn(clippy::expect_used))]

use antares_model::NgsiError;
use serde_json::Value;
use std::future::Future;
use std::pin::Pin;
use std::time::Duration;

pub mod http;
#[cfg(feature = "mqtt")]
pub mod mqtt;

pub use http::HttpSink;

/// One prepared notification, deliverable any number of times: a retry or a
/// dead-letter replay renders the identical message from the same parts.
/// The parts are transport-neutral — 6.3.8 turns them into HTTP headers,
/// Table 7.2-2 into the MQTT message's `metadata` object.
#[derive(Clone, Debug, PartialEq)]
pub struct Outbound {
    /// The Notification (5.3.1) to deliver.
    pub body: Value,
    /// `endpoint.accept` (Table 5.2.15-1): the MIME type of `body`.
    pub accept: String,
    /// The JSON-LD `@context` Link value belonging to `body` (6.3.8).
    pub link: String,
    /// `endpoint.receiverInfo` (Table 5.2.15-1) followed by the tenant and
    /// snapshot markers the binding has to convey (6.3.22).
    pub receiver_info: Vec<(String, String)>,
    /// `endpoint.notifierInfo` (Table 5.2.15-1): the parameters the binding
    /// needs to set up its channel, e.g. Table 7.2-1's MQTT-QoS. Opaque to
    /// every sink but the one whose scheme the endpoint names.
    pub notifier_info: Vec<(String, String)>,
}

impl Outbound {
    /// A dead letter back into deliverable form. Letters written before the
    /// bindings moved behind the registry carry a rendered HTTP header list, or
    /// an already-wrapped clause 7 message, instead of the endpoint members they
    /// were rendered from; both read back, so an upgrade does not strand the
    /// letters an operator has not replayed yet.
    pub fn from_dead_letter(letter: &Value) -> Result<Self, String> {
        let pairs = |v: &Value| -> Vec<(String, String)> {
            serde_json::from_value::<Vec<(String, String)>>(v.clone()).unwrap_or_default()
        };
        if letter.get("accept").and_then(Value::as_str).is_some() {
            return Ok(Self {
                body: letter["payload"].clone(),
                accept: letter["accept"].as_str().unwrap_or_default().to_owned(),
                link: letter["link"].as_str().unwrap_or_default().to_owned(),
                receiver_info: pairs(&letter["receiverInfo"]),
                notifier_info: pairs(&letter["notifierInfo"]),
            });
        }
        // Pre-registry HTTP: Content-Type and Link ARE the accept and link they
        // were rendered from; every other header came from receiverInfo.
        if let Some(headers) = letter.get("headers").filter(|h| !h.is_null()) {
            let headers = serde_json::from_value::<Vec<(String, String)>>(headers.clone())
                .map_err(|e| format!("dead letter headers unreadable: {e}"))?;
            let take = |name: &str| {
                headers
                    .iter()
                    .find(|(k, _)| k.eq_ignore_ascii_case(name))
                    .map(|(_, v)| v.clone())
                    .unwrap_or_default()
            };
            return Ok(Self {
                body: letter["payload"].clone(),
                accept: take("Content-Type"),
                link: take("Link"),
                receiver_info: headers
                    .iter()
                    .filter(|(k, _)| {
                        !k.eq_ignore_ascii_case("Content-Type") && !k.eq_ignore_ascii_case("Link")
                    })
                    .cloned()
                    .collect(),
                notifier_info: Vec::new(),
            });
        }
        // Pre-registry MQTT: the payload is the 7.2 message, so the notification
        // and its metadata come back out of the wrapper.
        let meta = &letter["payload"]["metadata"];
        if let Some(meta) = meta.as_object() {
            let mut notifier_info = Vec::new();
            if let Some(q) = letter["mqtt"]["qos"].as_u64() {
                notifier_info.push(("MQTT-QoS".to_owned(), q.to_string()));
            }
            if let Some(v5) = letter["mqtt"]["v5"].as_bool() {
                let v = if v5 { "mqtt5.0" } else { "mqtt3.1.1" };
                notifier_info.push(("MQTT-Version".to_owned(), v.to_owned()));
            }
            return Ok(Self {
                body: letter["payload"]["body"].clone(),
                accept: meta
                    .get("Content-Type")
                    .and_then(Value::as_str)
                    .unwrap_or("application/json")
                    .to_owned(),
                link: meta
                    .get("Link")
                    .and_then(Value::as_str)
                    .unwrap_or_default()
                    .to_owned(),
                receiver_info: meta
                    .iter()
                    .filter(|(k, _)| k.as_str() != "Content-Type" && k.as_str() != "Link")
                    .filter_map(|(k, v)| Some((k.clone(), v.as_str()?.to_owned())))
                    .collect(),
                notifier_info,
            });
        }
        Err("dead letter carries no deliverable notification".to_owned())
    }

    /// `notifier_info` in the borrowed pair form the sinks parse.
    pub fn notifier_pairs(&self) -> Vec<(&str, &str)> {
        self.notifier_info
            .iter()
            .map(|(k, v)| (k.as_str(), v.as_str()))
            .collect()
    }
}

/// Why one delivery attempt did not land. `timed_out` is the only class the
/// caller's circuit breaker counts: an endpoint that answers — with any
/// status — is alive.
#[derive(Clone, Debug, PartialEq)]
pub struct DeliveryError {
    /// The attempt ran out of time rather than being answered.
    pub timed_out: bool,
    /// Failure text for the log, the subscription status and the dead
    /// letter. Never carries endpoint credentials.
    pub message: String,
}

impl DeliveryError {
    /// The endpoint answered, or refused, within the deadline.
    pub fn failed(message: impl Into<String>) -> Self {
        Self {
            timed_out: false,
            message: message.into(),
        }
    }

    /// The deadline passed with no answer.
    pub fn timeout(message: impl Into<String>) -> Self {
        Self {
            timed_out: true,
            message: message.into(),
        }
    }
}

/// The future one `deliver` returns. `Send` on every target: the un-Send
/// piece of a browser fetch is fenced inside `antares_jsonld::http_interaction`.
pub type DeliveryFuture<'a> = Pin<Box<dyn Future<Output = Result<(), DeliveryError>> + Send + 'a>>;

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

/// Strip the authority's userinfo from an endpoint URI. 7.2 allows
/// credentials there (`mqtt[s]://[<username>][:<password>]@<host>…`), and a
/// rejected or failed endpoint travels back to the client as the `detail`
/// member of the ProblemDetails body (5.5.3) and into the delivery logs —
/// neither may carry the subscription's password. Everything after the
/// authority's last `@` is kept; an `@` in the path or topic is data.
pub fn redact_userinfo(uri: &str) -> String {
    if let Some(scheme_end) = uri.find("//") {
        let rest = &uri[scheme_end + 2..];
        let authority_end = rest.find(['/', '?', '#']).unwrap_or(rest.len());
        if let Some(at) = rest[..authority_end].rfind('@') {
            return format!("{}{}", &uri[..scheme_end + 2], &rest[at + 1..]);
        }
    }
    uri.to_owned()
}

/// A delivery binding for one URI scheme family.
pub trait NotificationSink: Send + Sync {
    /// Schemes this sink serves, e.g. `["http", "https"]`.
    fn schemes(&self) -> &'static [&'static str];

    /// 5.8.1.4: the endpoint's own syntax and parameters, checked at
    /// subscription creation rather than at first delivery. `notifier_info`
    /// is `endpoint.notifierInfo` (Table 5.2.15-1) as key/value pairs. An
    /// endpoint that does not meet the sink's requirements is
    /// BadRequestData; the message names the URI with any userinfo
    /// credentials stripped, since it travels back in `detail` (5.5.3).
    fn parse_endpoint(&self, uri: &str, notifier_info: &[(&str, &str)]) -> Result<(), NgsiError>;

    /// Does an endpoint of this binding name a network destination? The
    /// caller runs the egress guard — host and port policy, private-range
    /// and metadata-address deny, per-destination circuit breaker — against
    /// every endpoint that does, before `deliver` and never inside it, so
    /// one guard covers every binding whatever scheme it serves. The
    /// default is the safe answer: a sink is policed unless it declares
    /// that it opens no socket, and a release binary registers no sink that
    /// declares otherwise.
    fn network(&self) -> bool {
        true
    }

    /// One attempt on the wire. `timeout` is `endpoint.timeout`
    /// (Table 5.2.15-1) already clamped by the caller.
    fn deliver<'a>(
        &'a self,
        uri: &'a str,
        out: &'a Outbound,
        timeout: Duration,
    ) -> DeliveryFuture<'a>;
}

/// Scheme → sink registry; populated by the composition root. Choosing a
/// binding goes through here and nowhere else, so an endpoint scheme with no
/// sink can never fall through to the HTTP binding.
#[derive(Default)]
pub struct SinkRegistry {
    sinks: Vec<Box<dyn NotificationSink>>,
}

impl SinkRegistry {
    /// Add a binding. A scheme already served keeps its first sink.
    pub fn register(&mut self, sink: Box<dyn NotificationSink>) {
        self.sinks.push(sink);
    }

    /// The scheme of an endpoint URI, lowercased per IETF RFC 3986 §3.1.
    pub fn scheme_of(uri: &str) -> String {
        uri.split(':').next().unwrap_or("").to_ascii_lowercase()
    }

    /// The sink serving `scheme`, if any.
    pub fn sink_for(&self, scheme: &str) -> Option<&dyn NotificationSink> {
        // Linear scan is fine at <5 sinks; switch to a map when sinks multiply.
        self.sinks
            .iter()
            .find(|s| s.schemes().contains(&scheme))
            .map(AsRef::as_ref)
    }

    /// The sink serving an endpoint URI, by its scheme.
    pub fn sink_for_uri(&self, uri: &str) -> Option<&dyn NotificationSink> {
        self.sink_for(&Self::scheme_of(uri))
    }

    /// 5.8.1.4 reject-at-creation: the sink for this endpoint, having
    /// accepted the endpoint's own syntax. An endpoint whose scheme this
    /// deployment cannot deliver to is input data that does not meet the
    /// requirements of the operation — BadRequestData (Table 5.5.2-1, 400
    /// per Table 6.3.2-1). Not OperationNotSupported: Create Subscription
    /// is supported, this endpoint value is not.
    pub fn require(&self, uri: &str, notifier_info: &[(&str, &str)]) -> Result<(), NgsiError> {
        let scheme = Self::scheme_of(uri);
        let sink = self.sink_for(&scheme).ok_or_else(|| {
            NgsiError::BadRequestData(format!(
                "no notification binding registered for endpoint scheme {scheme:?} (6.3.8)"
            ))
        })?;
        sink.parse_endpoint(uri, notifier_info)
    }

    /// Schemes this deployment can deliver to, for `/q/health` and the
    /// startup banner.
    pub fn schemes(&self) -> Vec<&'static str> {
        let mut v: Vec<&'static str> = self
            .sinks
            .iter()
            .flat_map(|s| s.schemes())
            .copied()
            .collect();
        v.sort_unstable();
        v.dedup();
        v
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    struct FakeHttp;
    impl NotificationSink for FakeHttp {
        fn schemes(&self) -> &'static [&'static str] {
            &["http", "https"]
        }
        fn parse_endpoint(&self, uri: &str, _ni: &[(&str, &str)]) -> Result<(), NgsiError> {
            uri.contains("://")
                .then_some(())
                .ok_or_else(|| NgsiError::BadRequestData(format!("no authority in {uri:?}")))
        }
        fn deliver<'a>(
            &'a self,
            _uri: &'a str,
            _o: &'a Outbound,
            _t: Duration,
        ) -> DeliveryFuture<'a> {
            Box::pin(async { Ok(()) })
        }
    }

    fn registry() -> SinkRegistry {
        let mut reg = SinkRegistry::default();
        reg.register(Box::new(FakeHttp));
        reg
    }

    /// 5.8.1.4 + Table 5.5.2-1: an endpoint scheme no sink serves is input
    /// data that does not meet the operation's requirements — BadRequestData
    /// (400), not OperationNotSupported. Create Subscription is supported.
    #[test]
    fn unknown_scheme_is_bad_request_data() {
        let reg = registry();
        assert!(reg.require("http://h/n", &[]).is_ok());
        assert!(reg.require("https://h/n", &[]).is_ok());
        let err = reg
            .require("ws://h/n", &[])
            .expect_err("ws has no sink in v1");
        assert_eq!(err.status(), 400);
        assert!(matches!(err, NgsiError::BadRequestData(_)), "{err:?}");
        assert!(format!("{err}").contains("ws"), "{err}");
    }

    /// The scheme comparison is case-insensitive (IETF RFC 3986 §3.1),
    /// so `HTTP://…` is not silently unroutable.
    #[test]
    fn scheme_matching_ignores_case() {
        assert!(registry().require("HTTP://h/n", &[]).is_ok());
        assert_eq!(SinkRegistry::scheme_of("MQTTS://h/t"), "mqtts");
    }

    /// The sink's own endpoint check runs at creation, and its error is the
    /// one the client sees.
    #[test]
    fn the_sinks_own_rejection_is_returned() {
        let err = registry()
            .require("http:/malformed", &[])
            .expect_err("no authority");
        assert_eq!(err.status(), 400);
        assert!(format!("{err}").contains("no authority"), "{err}");
    }

    /// Every binding this workspace ships opens a socket, so every endpoint
    /// it delivers to passes the caller's egress policy. A sink that
    /// declares otherwise is in-process only and belongs to a test.
    #[test]
    fn every_shipped_sink_is_policed() {
        let mut reg = SinkRegistry::default();
        reg.register(Box::new(crate::HttpSink::new(
            antares_jsonld::HttpClient::default(),
        )));
        #[cfg(feature = "mqtt")]
        reg.register(Box::new(crate::mqtt::MqttSink::default()));
        assert!(!reg.sinks.is_empty());
        for s in &reg.sinks {
            assert!(
                s.network(),
                "shipped sink {:?} skips the egress policy",
                s.schemes()
            );
        }
    }

    /// A dead letter reads back into the same notification whichever broker
    /// wrote it: the current shape, and the two shapes written before the
    /// bindings moved behind the registry.
    #[test]
    fn dead_letters_of_every_shape_read_back() {
        let body = json!({"type": "Notification", "subscriptionId": "urn:s:1"});
        let link = "<https://ctx>; rel=\"http://www.w3.org/ns/json-ld#context\"";

        let current = json!({"uri": "http://h/n", "payload": body,
            "accept": "application/ld+json", "link": link,
            "receiverInfo": [["Authorization", "Bearer t"]],
            "notifierInfo": []});
        let o = Outbound::from_dead_letter(&current).expect("current shape");
        assert_eq!(o.body, body);
        assert_eq!(o.accept, "application/ld+json");
        assert_eq!(
            o.receiver_info,
            [("Authorization".into(), "Bearer t".into())]
        );

        let legacy_http = json!({"uri": "http://h/n", "binding": "http", "payload": body,
            "headers": [["Content-Type", "application/json"], ["Link", link],
                        ["Authorization", "Bearer t"]]});
        let o = Outbound::from_dead_letter(&legacy_http).expect("legacy http shape");
        assert_eq!(o.body, body);
        assert_eq!(o.accept, "application/json");
        assert_eq!(o.link, link);
        assert_eq!(
            o.receiver_info,
            [("Authorization".into(), "Bearer t".into())],
            "Content-Type and Link are the accept and link, not receiverInfo"
        );

        let legacy_mqtt = json!({"uri": "mqtt://h/t", "binding": "mqtt",
            "mqtt": {"qos": 2, "v5": false},
            "payload": {"metadata": {"Content-Type": "application/json", "Link": link,
                                     "NGSILD-Tenant": "acme"},
                        "body": body}});
        let o = Outbound::from_dead_letter(&legacy_mqtt).expect("legacy mqtt shape");
        assert_eq!(
            o.body, body,
            "the notification comes back out of the wrapper"
        );
        assert_eq!(o.accept, "application/json");
        assert_eq!(o.receiver_info, [("NGSILD-Tenant".into(), "acme".into())]);
        assert_eq!(
            o.notifier_info,
            [
                ("MQTT-QoS".to_owned(), "2".to_owned()),
                ("MQTT-Version".to_owned(), "mqtt3.1.1".to_owned())
            ]
        );

        assert!(Outbound::from_dead_letter(&json!({"uri": "http://h/n"})).is_err());
    }

    /// Endpoint URIs may carry credentials (mqtt[s]://user:pass@host, 7.1);
    /// log lines must never leak them.
    #[test]
    fn log_redaction_strips_uri_userinfo() {
        let red = redact_userinfo("mqtts://alice:s3cret@broker:8883/topic");
        assert_eq!(red, "mqtts://broker:8883/topic");
        assert!(!red.contains("s3cret"));
        assert!(!red.contains("alice"));
        assert_eq!(
            redact_userinfo("http://host:9090/notify"),
            "http://host:9090/notify"
        );
        // an '@' beyond the authority is path data, not userinfo
        assert_eq!(redact_userinfo("http://h/p@x"), "http://h/p@x");
    }

    /// An unregistered scheme resolves to no sink at delivery time either —
    /// there is no fall-through to the first registered binding.
    #[test]
    fn delivery_lookup_never_falls_through() {
        let reg = registry();
        assert!(reg.sink_for_uri("http://h/n").is_some());
        assert!(reg.sink_for_uri("memory://box").is_none());
        assert!(reg.sink_for("").is_none());
        assert_eq!(reg.schemes(), vec!["http", "https"]);
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
