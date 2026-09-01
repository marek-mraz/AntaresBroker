// SPDX-License-Identifier: EUPL-1.2
//! Outbound safety for the request-path egress classes:
//! notification delivery and federation forwarding. The third class,
//! @context fetching, enforces the same policy inside `antares-jsonld`
//! (that is where the fetch happens) — this module governs the two that
//! leave from `antares-api`.
//!
//! Per-destination circuit breakers matter at federation scale: a dead
//! peer must not spend its full timeout on every request.

use std::collections::HashMap;
use std::sync::Mutex;
use std::time::Duration;
// Clock rule: std Instant panics on wasm32; web-time is the std re-export
// natively and performance.now() in the browser.
#[cfg(not(target_arch = "wasm32"))]
use std::time::Instant;
#[cfg(target_arch = "wasm32")]
use web_time::Instant;

/// Ceiling on tracked destinations and registrations. Both maps are keyed by
/// client-supplied strings, so they need a bound: at the ceiling the least
/// recently recorded entry is dropped, which costs at most a forgotten failure
/// count for a destination nobody has touched in a while.
const MAX_TRACKED: usize = 4096;
/// Consecutive failures before a destination is tripped.
pub(crate) const TRIP_AFTER: u32 = 5;
/// How long a tripped destination stays open-circuit before one probe.
const COOLDOWN: Duration = Duration::from_secs(30);

#[derive(Default)]
struct Breaker {
    failures: u32,
    tripped_at: Option<Instant>,
    /// When this entry was last written — the eviction order at the ceiling.
    touched_at: Option<Instant>,
}

/// Drop the least recently written entry once the map is at its ceiling, so a
/// new key always has room. Called before inserting, never on lookup.
///
/// The ceiling is shared by every tenant, so the eviction stays inside the
/// tenant that is filling it (both maps are keyed `tenant\u{1f}rest`): one
/// tenant pointing subscriptions at thousands of dead hosts would otherwise
/// drop another tenant's tripped breaker, and that tenant's notifications go
/// back to spending a full timeout on a destination already known dead. A key
/// whose tenant holds no entry yet takes the globally oldest one, so a tenant
/// arriving at a full map still gets in.
// ponytail: a linear scan per eviction, which happens only at the ceiling on
// a new key; a per-tenant LRU list if a profile ever shows it.
fn evict_oldest<V>(map: &mut HashMap<String, V>, key: &str, stamp: impl Fn(&V) -> Option<Instant>) {
    let prefix = format!("{}\u{1f}", key.split('\u{1f}').next().unwrap_or(""));
    while map.len() >= MAX_TRACKED {
        let oldest = |mine: bool| {
            map.iter()
                .filter(|(k, _)| k.starts_with(&prefix) == mine)
                .min_by_key(|(_, v)| stamp(v))
                .map(|(k, _)| k.clone())
        };
        let Some(victim) = oldest(true).or_else(|| oldest(false)) else {
            return;
        };
        map.remove(&victim);
    }
}

/// Egress gate shared by the notification and federation paths.
pub struct Egress {
    policy: antares_jsonld::EgressPolicy,
    breakers: Mutex<HashMap<String, Breaker>>,
    /// 5.2.34 cooldown: instant of the last failed forward per registration
    /// id — only consulted for registrations that DECLARE management.cooldown.
    reg_failures: Mutex<HashMap<String, Instant>>,
}

impl Default for Egress {
    fn default() -> Self {
        Self::new(antares_jsonld::EgressPolicy::from_env())
    }
}

/// A URI as it may be repeated back to a caller. Every reason string this
/// module returns is interpolated into a log line beside a URI the caller
/// already redacted, so it carries the same redaction: 5.2.9 puts no limit on
/// a registered endpoint's URI and reqwest sends its userinfo as credentials.
fn redacted(url: &str) -> String {
    antares_notifier::redact_userinfo(url)
}

/// The 5.2.34 cooldown key. The registration id is client-chosen PER TENANT
/// (5.5.10), so the bare id would let one tenant's failing registration put
/// another tenant's same-id registration into timeout. The unit separator
/// cannot appear in either part (TenantId and EntityId both refuse C0
/// controls).
pub fn reg_key(tenant: &str, reg_id: &str) -> String {
    format!("{tenant}\u{1f}{reg_id}")
}

impl Egress {
    pub fn new(policy: antares_jsonld::EgressPolicy) -> Self {
        Self {
            policy,
            breakers: Mutex::new(HashMap::new()),
            reg_failures: Mutex::new(HashMap::new()),
        }
    }

    /// 5.2.34 cooldown: "If requests are received before the cooldown
    /// period has expired, a timeout error response for the registration is
    /// automatically returned." True while the per-registration window is
    /// still open.
    pub fn reg_in_cooldown(&self, reg_key: &str, cooldown_ms: u64) -> bool {
        self.reg_failures
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .get(reg_key)
            .is_some_and(|t| t.elapsed() < Duration::from_millis(cooldown_ms))
    }

    /// 5.2.34 cooldown bookkeeping: a failed forward stamps the window, a
    /// successful one clears it.
    pub fn reg_record(&self, reg_key: &str, ok: bool) {
        let mut m = self
            .reg_failures
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if ok {
            m.remove(reg_key);
        } else {
            if !m.contains_key(reg_key) {
                evict_oldest(&mut m, reg_key, |t: &Instant| Some(*t));
            }
            m.insert(reg_key.to_owned(), Instant::now());
        }
    }

    /// scheme allowlist + private-range deny. `Err` is a reason
    /// string for the caller\'s log/207 detail.
    pub async fn check_url(&self, url: &str) -> Result<(), String> {
        let scheme = reqwest::Url::parse(url)
            .map(|u| u.scheme().to_owned())
            .map_err(|e| format!("bad URL {}: {e}", redacted(url)))?;
        match scheme.as_str() {
            "http" | "https" => {}
            other => return Err(format!("scheme {other:?} is not allowed for egress")),
        }
        self.check_destination(url).await
    }

    /// The host policy for the destination of any notification binding. The
    /// scheme belongs to the sink (6.3.8, clause 7, or one a deployment
    /// registered); the host and port belong here. A URI with no host names
    /// no destination that can be cleared, so it is refused.
    ///
    /// This is the verdict on the destination as WRITTEN. A destination
    /// written as a name, under the default `ANTARES_EGRESS_ALLOW_PRIVATE`,
    /// is not resolved here — the addresses a name stands for are judged by
    /// the transport that dials them, and a binding that opens its own
    /// socket owes that filter (`EgressPolicy::ip_is_metadata` and
    /// `ip_is_private` over the resolved answer, as `checked_addr` does for
    /// MQTT and `PolicyResolver` for every reqwest client).
    pub async fn check_destination(&self, url: &str) -> Result<(), String> {
        let parsed =
            reqwest::Url::parse(url).map_err(|e| format!("bad URL {}: {e}", redacted(url)))?;
        let host = parsed
            .host_str()
            .filter(|h| !h.is_empty())
            .ok_or_else(|| format!("endpoint {} names no host", redacted(url)))?
            .to_owned();
        let port = parsed.port_or_known_default().unwrap_or(443);
        self.policy.check_host(&host, port).await
    }

    /// The breaker key: one destination, within one tenant. 4.14 puts the
    /// tenant in it — "the NGSI-LD API operations for managing, retrieving
    /// and subscribing to entity information, but also any context source
    /// related operations only apply to the information of the specified
    /// `Tenant` in isolation and never have any effect on the information of
    /// other `Tenants`". Tenants share destinations (one consumer host, one
    /// MQTT broker), so a destination-only key lets one tenant's failing
    /// endpoint suppress another tenant's notifications to the same
    /// host:port, and the victim sees no evidence: a suppressed delivery
    /// deliberately does not move `timesSent`, `lastNotification` or
    /// `status`. Same reasoning, and the same separator, as `reg_key`.
    ///
    /// Userinfo, path and topic stay out: they are the credentials and the
    /// destination WITHIN a peer, and it is the peer that goes unresponsive.
    fn key(tenant: &str, url: &str) -> String {
        let dest = reqwest::Url::parse(url)
            .ok()
            .map(|u| {
                format!(
                    "{}://{}:{}",
                    u.scheme(),
                    u.host_str().unwrap_or_default(),
                    u.port_or_known_default().unwrap_or(0)
                )
            })
            .unwrap_or_else(|| url.to_owned());
        format!("{tenant}\u{1f}{dest}")
    }

    /// Is this destination currently open-circuit FOR THIS TENANT? A tripped
    /// destination admits ONE probe per cooldown window (half-open).
    pub fn is_open(&self, tenant: &str, url: &str) -> bool {
        let mut map = self
            .breakers
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let Some(b) = map.get_mut(&Self::key(tenant, url)) else {
            return false;
        };
        match b.tripped_at {
            Some(t) if t.elapsed() >= COOLDOWN => {
                b.tripped_at = Some(Instant::now()); // this call IS the probe
                false
            }
            Some(_) => true,
            None => false,
        }
    }

    pub fn record_success(&self, tenant: &str, url: &str) {
        self.breakers
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .remove(&Self::key(tenant, url));
    }

    pub fn record_failure(&self, tenant: &str, url: &str) {
        let mut map = self
            .breakers
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let k = Self::key(tenant, url);
        if !map.contains_key(&k) {
            evict_oldest(&mut map, &k, |b: &Breaker| b.touched_at);
        }
        let b = map.entry(k).or_default();
        b.failures += 1;
        b.touched_at = Some(Instant::now());
        if b.failures >= TRIP_AFTER {
            b.tripped_at = Some(Instant::now());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The reason string is interpolated into a caller's log line next to a
    /// URI that caller redacted (`notify.rs`, `federation.rs`), so it may not
    /// smuggle back what the redaction removed: 5.2.9 allows any URI as a
    /// registered endpoint and reqwest sends its userinfo as basic auth.
    #[tokio::test]
    async fn a_refused_url_never_repeats_its_userinfo() {
        let e = Egress::new(antares_jsonld::EgressPolicy {
            allow_private: false,
        });
        for url in [
            "http://alice:s3cret@[not-an-ip]/x",
            "http://alice:s3cret@/x",
        ] {
            let err = e.check_url(url).await.expect_err("refused");
            assert!(!err.contains("s3cret"), "userinfo in the reason: {err}");
        }
    }

    #[tokio::test]
    async fn scheme_allowlist_and_private_deny() {
        let e = Egress::new(antares_jsonld::EgressPolicy {
            allow_private: false,
        });
        assert!(e.check_url("file:///etc/passwd").await.is_err());
        assert!(e.check_url("http://127.0.0.1:9090/x").await.is_err());
        assert!(e
            .check_url("http://169.254.169.254/latest/meta-data")
            .await
            .is_err());
        let allow = Egress::new(antares_jsonld::EgressPolicy {
            allow_private: true,
        });
        assert!(allow.check_url("http://127.0.0.1:9090/x").await.is_ok());
        assert!(
            allow.check_url("mqtt://localhost:1883/t").await.is_err(),
            "@context fetches and federation forwards are HTTP; a notification \
             binding's own scheme goes through check_destination"
        );
        // A binding's own scheme is the sink's business, but its host is
        // still the policy's: a plugin binding cannot reach a denied host,
        // and an endpoint with no host is refused outright.
        assert!(allow
            .check_destination("wss://localhost:9000/n")
            .await
            .is_ok());
        assert!(e.check_destination("wss://127.0.0.1:9000/n").await.is_err());
        assert!(e
            .check_destination("wss://169.254.169.254/latest")
            .await
            .is_err());
        assert!(allow.check_destination("file:///etc/passwd").await.is_err());
        assert!(allow.check_destination("memory://").await.is_err());
    }

    /// The metadata denial does not depend on `ANTARES_EGRESS_ALLOW_PRIVATE`
    /// — but this check judges the destination as WRITTEN. A literal
    /// metadata address is refused in every spelling with private egress
    /// allowed; a host written as a NAME is not resolved here under that
    /// switch, and the classifier below is what the transports apply to the
    /// addresses the name turns out to stand for.
    #[tokio::test]
    async fn a_literal_metadata_address_is_refused_with_private_egress_allowed() {
        let allow = Egress::new(antares_jsonld::EgressPolicy {
            allow_private: true,
        });
        for u in [
            "http://169.254.169.254/latest/meta-data",
            "http://100.100.100.200/latest",
            "http://[fd00:ec2::254]/latest",
            "http://[::ffff:169.254.169.254]/latest",
            "http://[64:ff9b::a9fe:a9fe]/latest",
            // 6to4 (RFC 3056): 2002:169.254.169.254::
            "http://[2002:a9fe:a9fe::]/latest",
        ] {
            assert!(
                allow.check_url(u).await.is_err(),
                "{u} reached the instance-metadata range"
            );
            assert!(
                allow.check_destination(u).await.is_err(),
                "{u} reached the instance-metadata range as a binding endpoint"
            );
        }
        // The same addresses, as an answer a NAME resolves to: this is the
        // classifier `PolicyResolver` and the MQTT connect run before they
        // dial, and it is what covers the case the check above cannot see.
        for ip in [
            "169.254.169.254",
            "100.100.100.200",
            "fd00:ec2::254",
            "::ffff:169.254.169.254",
            "64:ff9b::a9fe:a9fe",
            "2002:a9fe:a9fe::",
            "2002:a9fe:a9fe:1:2:3:4:5",
        ] {
            assert!(
                antares_jsonld::EgressPolicy::ip_is_metadata(ip.parse().expect("address")),
                "{ip} not classified as instance metadata"
            );
        }
    }

    /// 5.2.34 + 5.5.10: the cooldown a failing registration earns belongs to
    /// ITS tenant. Another tenant's registration under the same client-chosen
    /// id keeps being contacted.
    #[test]
    fn cooldown_is_scoped_to_the_tenant_that_earned_it() {
        let e = Egress::default();
        let id = "urn:ngsi-ld:ContextSourceRegistration:shared";
        e.reg_record(&reg_key("tenant-a", id), false);
        assert!(
            e.reg_in_cooldown(&reg_key("tenant-a", id), 60_000),
            "the failing tenant's registration is in its window"
        );
        assert!(
            !e.reg_in_cooldown(&reg_key("tenant-b", id), 60_000),
            "one tenant's failing registration must not put another tenant's \
             same-id registration into timeout"
        );
        // and a success clears only its own tenant's stamp
        e.reg_record(&reg_key("tenant-b", id), false);
        e.reg_record(&reg_key("tenant-a", id), true);
        assert!(!e.reg_in_cooldown(&reg_key("tenant-a", id), 60_000));
        assert!(e.reg_in_cooldown(&reg_key("tenant-b", id), 60_000));
    }

    #[test]
    fn breaker_trips_after_consecutive_failures() {
        let e = Egress::default();
        let t = "tenant-a";
        let url = "http://dead.example:9090/notify";
        for _ in 0..(TRIP_AFTER - 1) {
            e.record_failure(t, url);
            assert!(!e.is_open(t, url), "not tripped before the threshold");
        }
        e.record_failure(t, url);
        assert!(e.is_open(t, url), "tripped at the threshold");
        // per-destination, not global
        assert!(!e.is_open(t, "http://healthy.example:9090/notify"));
        e.record_success(t, url);
        assert!(!e.is_open(t, url), "success clears the breaker");
    }

    /// 4.14 + 5.5.10: the breaker a failing endpoint earns belongs to the
    /// tenant whose delivery earned it. Tenants share destinations, and
    /// whether a destination answers inside the deadline is a property of the
    /// pair, not of the host: the same host is a timeout for a subscription
    /// at the 6.3.8 100 ms floor and healthy for one that allows 5 s.
    #[test]
    fn a_tripped_destination_is_tripped_only_for_the_tenant_that_tripped_it() {
        let e = Egress::default();
        let url = "http://shared-consumer.example:8080/notify";
        for _ in 0..TRIP_AFTER {
            e.record_failure("tenant-a", url);
        }
        assert!(e.is_open("tenant-a", url), "the failing tenant is tripped");
        assert!(
            !e.is_open("tenant-b", url),
            "one tenant's failing endpoint must not suppress another \
             tenant's notifications to the same host:port"
        );
        // and clearing one tenant's breaker leaves the other's alone
        for _ in 0..TRIP_AFTER {
            e.record_failure("tenant-b", url);
        }
        e.record_success("tenant-a", url);
        assert!(!e.is_open("tenant-a", url));
        assert!(e.is_open("tenant-b", url));
    }

    /// The ceiling is shared; the isolation must not be. A tenant churning
    /// destinations past it evicts its OWN oldest entry — another tenant's
    /// tripped breaker survives, or every later notification to that
    /// tenant's dead endpoint goes back to spending a full timeout.
    #[test]
    fn filling_the_ceiling_leaves_another_tenants_breaker_tripped() {
        let e = Egress::default();
        let victim = "http://dead-peer.example:9090/notify";
        for _ in 0..TRIP_AFTER {
            e.record_failure("b", victim);
        }
        assert!(e.is_open("b", victim), "the breaker starts tripped");
        for i in 0..(MAX_TRACKED + 500) {
            e.record_failure("a", &format!("http://churn-{i}.example:9090/notify"));
        }
        assert!(
            e.is_open("b", victim),
            "one tenant's churn cleared another tenant's breaker"
        );
    }

    /// Both maps are keyed by client-supplied strings (notification endpoints,
    /// registration ids), so neither may grow without a ceiling: a client that
    /// points subscriptions at thousands of dead hosts must not be able to
    /// spend the broker's memory one entry at a time.
    #[test]
    fn destination_maps_stay_bounded_under_distinct_keys() {
        let e = Egress::default();
        for i in 0..(MAX_TRACKED + 500) {
            e.record_failure("t", &format!("http://dead-{i}.example:9090/notify"));
            e.reg_record(&format!("urn:ngsi-ld:CSR:{i}"), false);
        }
        assert!(
            e.breakers.lock().expect("breaker lock").len() <= MAX_TRACKED,
            "breaker map grew past the ceiling"
        );
        assert!(
            e.reg_failures.lock().expect("reg_failures lock").len() <= MAX_TRACKED,
            "registration cooldown map grew past the ceiling"
        );
        // The ceiling must not cost correctness for a live destination: the
        // most recently recorded failure is still tracked.
        let live = format!("http://dead-{}.example:9090/notify", MAX_TRACKED + 499);
        for _ in 0..TRIP_AFTER {
            e.record_failure("t", &live);
        }
        assert!(e.is_open("t", &live), "recent destination still trips");
    }
}
