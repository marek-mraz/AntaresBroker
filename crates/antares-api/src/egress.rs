//! Outbound safety for the request-path egress classes (tasks.md I4; §16.4):
//! notification delivery and federation forwarding. The third class,
//! @context fetching, enforces the same policy inside `antares-jsonld`
//! (that is where the fetch happens) — this module governs the two that
//! leave from `antares-api`.
//!
//! Per-destination circuit breakers close the U1 lesson at federation scale
//! (§16.7): a dead peer must not spend its full timeout on every request.

use std::collections::HashMap;
use std::sync::Mutex;
use std::time::Duration;
// N2 clock rule: std Instant panics on wasm32; web-time is the std re-export
// natively and performance.now() in the browser.
#[cfg(not(target_arch = "wasm32"))]
use std::time::Instant;
#[cfg(target_arch = "wasm32")]
use web_time::Instant;

/// Consecutive failures before a destination is tripped.
const TRIP_AFTER: u32 = 5;
/// How long a tripped destination stays open-circuit before one probe.
const COOLDOWN: Duration = Duration::from_secs(30);

#[derive(Default)]
struct Breaker {
    failures: u32,
    tripped_at: Option<Instant>,
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
    pub fn reg_in_cooldown(&self, reg_id: &str, cooldown_ms: u64) -> bool {
        self.reg_failures
            .lock()
            .expect("reg_failures lock")
            .get(reg_id)
            .is_some_and(|t| t.elapsed() < Duration::from_millis(cooldown_ms))
    }

    /// 5.2.34 cooldown bookkeeping: a failed forward stamps the window, a
    /// successful one clears it.
    pub fn reg_record(&self, reg_id: &str, ok: bool) {
        let mut m = self.reg_failures.lock().expect("reg_failures lock");
        if ok {
            m.remove(reg_id);
        } else {
            m.insert(reg_id.to_owned(), Instant::now());
        }
    }

    /// scheme allowlist + private-range deny (§16.4). `Err` is a reason
    /// string for the caller\'s log/207 detail.
    pub async fn check_url(&self, url: &str) -> Result<(), String> {
        let parsed = reqwest::Url::parse(url).map_err(|e| format!("bad URL {url}: {e}"))?;
        match parsed.scheme() {
            "http" | "https" | "mqtt" | "mqtts" => {}
            other => return Err(format!("scheme {other:?} is not allowed for egress")),
        }
        let host = parsed.host_str().unwrap_or_default().to_owned();
        let port = parsed.port_or_known_default().unwrap_or(443);
        self.policy.check_host(&host, port).await
    }

    fn key(url: &str) -> String {
        reqwest::Url::parse(url)
            .ok()
            .map(|u| {
                format!(
                    "{}://{}:{}",
                    u.scheme(),
                    u.host_str().unwrap_or_default(),
                    u.port_or_known_default().unwrap_or(0)
                )
            })
            .unwrap_or_else(|| url.to_owned())
    }

    /// Is this destination currently open-circuit? A tripped destination
    /// admits ONE probe per cooldown window (half-open).
    pub fn is_open(&self, url: &str) -> bool {
        let mut map = self.breakers.lock().expect("breaker lock");
        let Some(b) = map.get_mut(&Self::key(url)) else {
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

    pub fn record_success(&self, url: &str) {
        self.breakers
            .lock()
            .expect("breaker lock")
            .remove(&Self::key(url));
    }

    pub fn record_failure(&self, url: &str) {
        let mut map = self.breakers.lock().expect("breaker lock");
        let b = map.entry(Self::key(url)).or_default();
        b.failures += 1;
        if b.failures >= TRIP_AFTER {
            b.tripped_at = Some(Instant::now());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
        assert!(allow.check_url("mqtt://localhost:1883/t").await.is_ok());
    }

    #[test]
    fn breaker_trips_after_consecutive_failures() {
        let e = Egress::default();
        let url = "http://dead.example:9090/notify";
        for _ in 0..(TRIP_AFTER - 1) {
            e.record_failure(url);
            assert!(!e.is_open(url), "not tripped before the threshold");
        }
        e.record_failure(url);
        assert!(e.is_open(url), "tripped at the threshold");
        // per-destination, not global
        assert!(!e.is_open("http://healthy.example:9090/notify"));
        e.record_success(url);
        assert!(!e.is_open(url), "success clears the breaker");
    }
}
