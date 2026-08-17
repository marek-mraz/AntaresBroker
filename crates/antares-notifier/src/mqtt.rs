//! MQTT notification binding — CIM 009 clause 7 (feature `mqtt`).
//!
//! 7.2: a subscription whose `notification.endpoint.uri` uses the mqtt(s)
//! scheme gets its notifications as MQTT publishes. The message is a JSON
//! object `{"metadata": {...}, "body": <Notification per 5.3.1>}`; protocol
//! parameters ride in `notifier_info` (Table 7.2-1), receiver metadata in
//! `receiver_info` (Table 7.2-2).

use antares_model::NgsiError;
use serde_json::{Map, Value};
use std::collections::HashMap;
use std::sync::Mutex;
use std::time::{Duration, Instant};

fn bad(m: String) -> NgsiError {
    NgsiError::BadRequestData(m)
}

/// Strip the authority's userinfo from an endpoint URI. 7.2 allows
/// credentials there (`mqtt[s]://[<username>][:<password>]@<host>…`), while
/// a rejected endpoint travels back to the client as the `detail` member of
/// the ProblemDetails body (5.5.3) and into the delivery logs — neither may
/// carry the subscription's password. Everything after the last `@` of the
/// authority is kept; an `@` in the topic is path data, not userinfo.
fn redacted(uri: &str) -> String {
    let Some((scheme, rest)) = uri.split_once("://") else {
        return uri.to_owned();
    };
    match rest.split_once('/') {
        Some((authority, path)) => {
            let host = authority.rsplit_once('@').map_or(authority, |(_, h)| h);
            format!("{scheme}://{host}/{path}")
        }
        None => {
            let host = rest.rsplit_once('@').map_or(rest, |(_, h)| h);
            format!("{scheme}://{host}")
        }
    }
}

/// Parsed `mqtt[s]://[user][:pass]@host[:port]/topic[/subtopic]*` (7.2).
#[derive(Debug, Clone, PartialEq)]
pub struct MqttEndpoint {
    pub secure: bool,
    pub username: Option<String>,
    pub password: Option<String>,
    pub host: String,
    pub port: u16,
    pub topic: String,
}

impl MqttEndpoint {
    /// 7.2 endpoint URI syntax. A URI that does not meet it fails the
    /// 5.2.15 restrictions, so the caller raises BadRequestData — 400 per
    /// Table 6.3.2-1. The message names only the redacted URI: the
    /// credentials 7.2 permits in the userinfo never reach the response body.
    pub fn parse(uri: &str) -> Result<Self, NgsiError> {
        let safe = redacted(uri);
        let (secure, rest) = if let Some(r) = uri.strip_prefix("mqtts://") {
            (true, r)
        } else if let Some(r) = uri.strip_prefix("mqtt://") {
            (false, r)
        } else {
            return Err(bad(format!("not an mqtt(s) endpoint URI: {safe:?}")));
        };
        let (authority, topic) = rest
            .split_once('/')
            .ok_or_else(|| bad(format!("mqtt endpoint {safe:?} has no topic")))?;
        if topic.is_empty() {
            return Err(bad(format!("mqtt endpoint {safe:?} has no topic")));
        }
        let (userinfo, hostport) = match authority.rsplit_once('@') {
            Some((u, h)) => (Some(u), h),
            None => (None, authority),
        };
        let (username, password) = match userinfo {
            None => (None, None),
            Some(u) => match u.split_once(':') {
                Some((user, pass)) => (Some(user.to_owned()), Some(pass.to_owned())),
                None => (Some(u.to_owned()), None),
            },
        };
        // Deliberately no IPv6-literal hosts — the binding's URI convention
        // (i.19) and the ETSI suite use hostnames; add bracket parsing when a
        // deployment needs it.
        let (host, port) = match hostport.split_once(':') {
            Some((h, p)) => (
                h.to_owned(),
                p.parse::<u16>()
                    .map_err(|_| bad(format!("invalid mqtt port in {safe:?}")))?,
            ),
            None => (hostport.to_owned(), if secure { 8883 } else { 1883 }),
        };
        if host.is_empty() {
            return Err(bad(format!("mqtt endpoint {safe:?} has no host")));
        }
        Ok(Self {
            secure,
            username,
            password,
            host,
            port,
            topic: topic.to_owned(),
        })
    }
}

/// Table 7.2-1 protocol parameters from `notification.endpoint.notifierInfo`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct MqttParams {
    pub qos: u8,
    pub v5: bool,
}

impl Default for MqttParams {
    fn default() -> Self {
        Self { qos: 0, v5: true } // defaults per Table 7.2-1: QoS 0, mqtt5.0
    }
}

impl MqttParams {
    pub fn from_notifier_info<'a>(
        pairs: impl IntoIterator<Item = (&'a str, &'a str)>,
    ) -> Result<Self, NgsiError> {
        let mut p = Self::default();
        for (k, v) in pairs {
            match k {
                "MQTT-QoS" => {
                    p.qos = match v {
                        "0" => 0,
                        "1" => 1,
                        "2" => 2,
                        _ => return Err(bad(format!("MQTT-QoS must be 0, 1 or 2 (got {v:?})"))),
                    }
                }
                "MQTT-Version" => {
                    p.v5 = match v {
                        "mqtt5.0" => true,
                        "mqtt3.1.1" => false,
                        _ => {
                            return Err(bad(format!(
                                "MQTT-Version must be mqtt3.1.1 or mqtt5.0 (got {v:?})"
                            )))
                        }
                    }
                }
                _ => {} // unknown notifierInfo keys are not ours to police
            }
        }
        Ok(p)
    }
}

/// The 7.2 message: `{"metadata": {...}, "body": notification}`.
/// `link` is the HTTP-Link-header-formatted @context reference; per Table
/// 7.2-2 it is included only when the Content-Type is application/json
/// (with ld+json the @context travels in the body).
pub fn build_message(
    body: &Value,
    content_type: &str,
    link: Option<&str>,
    receiver_info: &[(String, String)],
) -> Value {
    let mut metadata = Map::new();
    metadata.insert("Content-Type".into(), Value::String(content_type.into()));
    if content_type == "application/json" {
        if let Some(l) = link {
            metadata.insert("Link".into(), Value::String(l.to_owned()));
        }
    }
    for (k, v) in receiver_info {
        metadata.insert(k.clone(), Value::String(v.clone()));
    }
    let mut msg = Map::new();
    msg.insert("metadata".into(), Value::Object(metadata));
    msg.insert("body".into(), body.clone());
    Value::Object(msg)
}

/// The key a pooled MQTT session is shared under. Everything that changes
/// WHO the session is authenticated as (or how) must participate — two
/// subscriptions whose endpoints differ only in password must never reuse
/// one another's authenticated session. Keys are map keys only: never log
/// them.
fn pool_key(ep: &MqttEndpoint, params: MqttParams) -> String {
    // The password participates via a one-way hash so the plaintext never
    // sits in a map key. DefaultHasher collisions are acceptable here — a
    // collision only merges two pool slots, it does not skip broker-side
    // authentication.
    use std::hash::{Hash, Hasher};
    let mut h = std::collections::hash_map::DefaultHasher::new();
    ep.password.hash(&mut h);
    format!(
        "{}:{:016x}:{}@{}:{}/v{}",
        ep.username.as_deref().unwrap_or(""),
        h.finish(),
        ep.secure,
        ep.host,
        ep.port,
        if params.v5 { 5 } else { 3 }
    )
}

/// The mqtts trust store, built ONCE per process. Loading the platform
/// certificate store on every connect is wasted work, and rumqttc's
/// `TlsConfiguration::default()` panics when the store is unreadable — a
/// bad cert bundle must fail the one delivery, not the broker.
fn shared_tls_config() -> Result<rumqttc::TlsConfiguration, NgsiError> {
    static TLS: std::sync::OnceLock<Option<rumqttc::TlsConfiguration>> = std::sync::OnceLock::new();
    TLS.get_or_init(|| {
        // `TlsConfiguration::default()` is the only rumqttc constructor
        // that loads the platform trust store, and it panics on failure —
        // contain that so a broken cert bundle degrades to failed mqtts
        // deliveries instead of killing the process. The failure is cached:
        // a store unreadable at first use will not become readable later.
        std::panic::catch_unwind(rumqttc::TlsConfiguration::default)
            .map_err(|_| tracing::error!("mqtts: loading the platform certificate store failed"))
            .ok()
    })
    .clone()
    .ok_or_else(|| NgsiError::InternalError("mqtts trust store unavailable".into()))
}

/// Is egress to private/loopback ranges allowed? The MQTT destination is
/// client-supplied (`notification.endpoint.uri`, 7.2), so it is governed by
/// the same deployment switch as the HTTP callbacks and @context fetches:
/// private egress is allowed by default (dev boxes, compose stacks and the
/// conformance mocks all live there) and
/// `ANTARES_EGRESS_ALLOW_PRIVATE=false` turns the deny on for
/// internet-exposed deployments. Read once per process.
fn allow_private_egress() -> bool {
    static ALLOW: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *ALLOW.get_or_init(|| {
        allow_private_from(
            std::env::var("ANTARES_EGRESS_ALLOW_PRIVATE")
                .ok()
                .as_deref(),
        )
    })
}

/// Read the switch tolerantly, as the HTTP side does: a security control
/// that understands one spelling hands the operator the opposite of the
/// intent when the value is `FALSE` or carries stray whitespace.
fn allow_private_from(v: Option<&str>) -> bool {
    v.is_none_or(|v| {
        let v = v.trim();
        !(v.eq_ignore_ascii_case("false") || v == "0")
    })
}

/// The cloud instance-metadata endpoints — IPv4 link-local (169.254.0.0/16,
/// RFC 3927), its IPv6 spellings and the IMDS-over-IPv6 ULA `fd00:ec2::254`.
/// Refused whatever the private-egress switch says: a subscription that
/// points its notifications at the instance credentials is the classic
/// credential-theft SSRF, and no real MQTT broker lives there.
fn ip_is_metadata(ip: std::net::IpAddr) -> bool {
    match ip {
        std::net::IpAddr::V4(v4) => v4.is_link_local(),
        std::net::IpAddr::V6(v6) => {
            v6.segments() == [0xfd00, 0x0ec2, 0, 0, 0, 0, 0, 0x254]
                || v6
                    .to_ipv4_mapped()
                    .or_else(|| v6.to_ipv4())
                    .is_some_and(|v4| v4.is_link_local())
        }
    }
}

fn ip_is_private(ip: std::net::IpAddr) -> bool {
    match ip {
        std::net::IpAddr::V4(v4) => {
            v4.is_loopback()
                || v4.is_private()
                || v4.is_link_local()
                || v4.is_unspecified()
                || v4.is_broadcast()
        }
        std::net::IpAddr::V6(v6) => {
            // an IPv4-mapped address (::ffff:a.b.c.d) is the v4 target in v6
            // spelling — judge it as its v4 self, or ::ffff:127.0.0.1 slips
            // past the v6 checks
            if let Some(v4) = v6.to_ipv4_mapped() {
                return ip_is_private(std::net::IpAddr::V4(v4));
            }
            v6.is_loopback()
                || v6.is_unspecified()
                // fc00::/7 unique-local + fe80::/10 link-local
                || (v6.segments()[0] & 0xfe00) == 0xfc00
                || (v6.segments()[0] & 0xffc0) == 0xfe80
        }
    }
}

/// Resolve the endpoint host ONCE and return the address to dial, or a
/// denial. Resolving for the check and then handing the NAME to the MQTT
/// client would leave a window in which the answer changes (DNS rebinding):
/// the connector would dial an address the policy never saw. The address
/// this returns is the address dialled, so check and connect see the same
/// answer by construction. A host that cannot be resolved is a DENIAL — a
/// destination the policy could not judge is never dialled — and the
/// resolver runs under the sink's own deadline.
async fn checked_addr(
    host: &str,
    port: u16,
    allow_private: bool,
    dns_timeout: Duration,
) -> Result<std::net::SocketAddr, NgsiError> {
    let denied = || {
        NgsiError::InternalError(format!(
            "mqtt egress to {host}:{port} denied (instance metadata or private range)"
        ))
    };
    let addrs: Vec<std::net::SocketAddr> = match host
        .trim_matches(['[', ']'])
        .parse::<std::net::IpAddr>()
    {
        // an IP literal needs no resolver, and is judged by the same rules
        Ok(ip) => vec![std::net::SocketAddr::new(ip, port)],
        Err(_) => tokio::time::timeout(dns_timeout, tokio::net::lookup_host((host, port)))
            .await
            .map_err(|_| {
                NgsiError::InternalError(format!("mqtt egress: resolving {host} timed out"))
            })?
            .map_err(|e| NgsiError::InternalError(format!("mqtt egress: resolving {host}: {e}")))?
            .collect(),
    };
    addrs
        .into_iter()
        .find(|a| !ip_is_metadata(a.ip()) && (allow_private || !ip_is_private(a.ip())))
        .ok_or_else(denied)
}

/// What to hand rumqttc as the broker address. Plain MQTT dials the checked
/// ADDRESS, which pins the resolution the policy judged (and makes the
/// event loop's own re-dial after a dropped connection reuse it instead of
/// resolving the name again, unchecked). mqtts keeps the host NAME: rumqttc
/// verifies the server certificate against the string it is given, so an
/// address there would demand an IP SAN and break certificate verification
/// against every ordinary broker certificate. For mqtts the check above
/// still gates the connect, and the certificate name check is what stops a
/// changed answer from impersonating the endpoint.
fn dial_host(ep: &MqttEndpoint, addr: std::net::SocketAddr) -> String {
    if ep.secure {
        ep.host.clone()
    } else {
        addr.ip().to_string()
    }
}

/// One pooled connection: the client plus its event-loop pump task.
enum Client {
    V3(rumqttc::AsyncClient),
    V5(rumqttc::v5::AsyncClient),
}

struct Conn {
    client: Client,
    pump: tokio::task::JoinHandle<()>,
    last_used: Instant,
}

impl Drop for Conn {
    fn drop(&mut self) {
        self.pump.abort();
    }
}

/// MQTT delivery with a bounded per-endpoint connection pool (bounded
/// WITH eviction; timeouts fixed at construction).
pub struct MqttSink {
    pool: Mutex<HashMap<String, Conn>>,
    cap: usize,
    timeout: Duration,
}

impl Default for MqttSink {
    fn default() -> Self {
        Self::new(32, Duration::from_secs(5))
    }
}

impl MqttSink {
    pub fn new(cap: usize, timeout: Duration) -> Self {
        Self {
            pool: Mutex::new(HashMap::new()),
            cap,
            timeout,
        }
    }

    /// Deliver one notification message. `message` is the 7.2 wrapper from
    /// [`build_message`], serialized by the caller once per subscription.
    pub async fn deliver(
        &self,
        ep: &MqttEndpoint,
        params: MqttParams,
        message: &[u8],
    ) -> Result<(), NgsiError> {
        let key = pool_key(ep, params);
        // one retry with a fresh connection: a pooled client whose broker
        // restarted fails the first publish; a dead broker fails both.
        for attempt in 0..2 {
            let conn = match self.checkout(&key) {
                Some(c) => c,
                None => self.connect(ep, params).await?,
            };
            let published = tokio::time::timeout(
                self.timeout,
                Self::publish(&conn.client, &ep.topic, params.qos, message),
            )
            .await;
            match published {
                Ok(Ok(())) if !conn.pump.is_finished() => {
                    self.checkin(key, conn);
                    return Ok(());
                }
                _ if attempt == 0 => continue, // drop conn, retry fresh
                Ok(Ok(())) => {
                    return Err(NgsiError::InternalError(
                        "mqtt connection lost during publish".into(),
                    ))
                }
                Ok(Err(e)) => return Err(NgsiError::InternalError(format!("mqtt publish: {e}"))),
                Err(_) => {
                    return Err(NgsiError::InternalError(format!(
                        "mqtt publish to {}:{} timed out",
                        ep.host, ep.port
                    )))
                }
            }
        }
        unreachable!("loop returns on second attempt");
    }

    async fn publish(client: &Client, topic: &str, qos: u8, payload: &[u8]) -> Result<(), String> {
        match client {
            Client::V3(c) => {
                let qos = rumqttc::qos(qos).map_err(|e| e.to_string())?;
                c.publish(topic, qos, false, payload.to_vec())
                    .await
                    .map_err(|e| e.to_string())
            }
            Client::V5(c) => {
                let qos =
                    rumqttc::v5::mqttbytes::qos(qos).ok_or_else(|| format!("invalid QoS {qos}"))?;
                c.publish(topic, qos, false, payload.to_vec())
                    .await
                    .map_err(|e| e.to_string())
            }
        }
    }

    fn checkout(&self, key: &str) -> Option<Conn> {
        self.pool.lock().expect("mqtt pool lock").remove(key)
    }

    fn checkin(&self, key: String, mut conn: Conn) {
        conn.last_used = Instant::now();
        let mut pool = self.pool.lock().expect("mqtt pool lock");
        pool.retain(|_, c| !c.pump.is_finished());
        pool.insert(key, conn);
        // bounded with eviction: drop the least-recently-used overflow.
        while pool.len() > self.cap {
            if let Some(oldest) = pool
                .iter()
                .min_by_key(|(_, c)| c.last_used)
                .map(|(k, _)| k.clone())
            {
                pool.remove(&oldest);
            }
        }
    }

    /// Connect and wait for ConnAck (a dead broker must fail delivery, not
    /// queue forever), then hand the event loop to a pump task.
    async fn connect(&self, ep: &MqttEndpoint, params: MqttParams) -> Result<Conn, NgsiError> {
        // A monotonic counter, not a timestamp: `Instant::now().elapsed()` is
        // ~0 for every caller, so two connects in one process would claim the
        // same client id and the broker would kick the older session.
        static SEQ: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        let id = format!(
            "antares-{}-{}",
            std::process::id(),
            SEQ.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
        );
        let refused = |e: String| {
            NgsiError::InternalError(format!("mqtt connect {}:{}: {e}", ep.host, ep.port))
        };
        // Egress policy first: resolve once, judge the answer, dial what was
        // judged. Nothing below opens a socket to an unchecked destination.
        let addr = checked_addr(&ep.host, ep.port, allow_private_egress(), self.timeout).await?;
        let dial = dial_host(ep, addr);
        if params.v5 {
            let mut opts = rumqttc::v5::MqttOptions::new(id, dial, addr.port());
            opts.set_keep_alive(Duration::from_secs(30));
            if let Some(u) = &ep.username {
                opts.set_credentials(u, ep.password.as_deref().unwrap_or(""));
            }
            if ep.secure {
                opts.set_transport(rumqttc::Transport::Tls(shared_tls_config()?));
            }
            let (client, mut eventloop) = rumqttc::v5::AsyncClient::new(opts, 16);
            tokio::time::timeout(self.timeout, async {
                loop {
                    match eventloop.poll().await {
                        Ok(rumqttc::v5::Event::Incoming(
                            rumqttc::v5::mqttbytes::v5::Packet::ConnAck(_),
                        )) => return Ok(()),
                        Ok(_) => {}
                        Err(e) => return Err(e.to_string()),
                    }
                }
            })
            .await
            .map_err(|_| refused("connect timeout".into()))?
            .map_err(refused)?;
            let pump = tokio::spawn(async move { while eventloop.poll().await.is_ok() {} });
            Ok(Conn {
                client: Client::V5(client),
                pump,
                last_used: Instant::now(),
            })
        } else {
            let mut opts = rumqttc::MqttOptions::new(id, dial, addr.port());
            opts.set_keep_alive(Duration::from_secs(30));
            if let Some(u) = &ep.username {
                opts.set_credentials(u, ep.password.as_deref().unwrap_or(""));
            }
            if ep.secure {
                opts.set_transport(rumqttc::Transport::Tls(shared_tls_config()?));
            }
            let (client, mut eventloop) = rumqttc::AsyncClient::new(opts, 16);
            tokio::time::timeout(self.timeout, async {
                loop {
                    match eventloop.poll().await {
                        Ok(rumqttc::Event::Incoming(rumqttc::Packet::ConnAck(_))) => return Ok(()),
                        Ok(_) => {}
                        Err(e) => return Err(e.to_string()),
                    }
                }
            })
            .await
            .map_err(|_| refused("connect timeout".into()))?
            .map_err(refused)?;
            let pump = tokio::spawn(async move { while eventloop.poll().await.is_ok() {} });
            Ok(Conn {
                client: Client::V3(client),
                pump,
                last_used: Instant::now(),
            })
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn parses_endpoint_variants() {
        let e = MqttEndpoint::parse("mqtt://host/topic").expect("plain");
        assert_eq!(
            e,
            MqttEndpoint {
                secure: false,
                username: None,
                password: None,
                host: "host".into(),
                port: 1883,
                topic: "topic".into()
            }
        );
        let e = MqttEndpoint::parse("mqtt://host:8085/a/b/c").expect("port+subtopics");
        assert_eq!(e.port, 8085);
        assert_eq!(e.topic, "a/b/c");
        let e = MqttEndpoint::parse("mqtt://user@host/t").expect("user");
        assert_eq!(e.username.as_deref(), Some("user"));
        assert_eq!(e.password, None);
        let e = MqttEndpoint::parse("mqtt://u:p@host:9001/t").expect("user+pass+port");
        assert_eq!(e.username.as_deref(), Some("u"));
        assert_eq!(e.password.as_deref(), Some("p"));
        assert_eq!(e.port, 9001);
        let e = MqttEndpoint::parse("mqtts://host/t").expect("tls");
        assert!(e.secure);
        assert_eq!(e.port, 8883, "mqtts default port");
    }

    #[test]
    fn rejects_bad_endpoints() {
        for uri in [
            "http://host/topic",
            "mqtt://host",
            "mqtt://host/",
            "mqtt:///topic",
            "mqtt://host:notaport/t",
        ] {
            assert!(MqttEndpoint::parse(uri).is_err(), "{uri} must be rejected");
        }
    }

    /// 7.2 endpoint URIs may carry credentials in the userinfo
    /// (`mqtt[s]://<username>:<password>@host`). A parse failure is answered
    /// to the client as BadRequestData (5.8.1.4, 400 per Table 6.3.2-1) and
    /// the message becomes the 5.5.3 ProblemDetails `detail`, so no password
    /// may appear in it.
    #[test]
    fn parse_errors_redact_endpoint_userinfo() {
        for uri in [
            "mqtt://user:hunter2@host",            // no topic
            "mqtt://user:hunter2@host/",           // empty topic
            "mqtt://user:hunter2@host:notaport/t", // bad port
            "mqtts://user:hunter2@/t",             // no host
            "http://user:hunter2@host/t",          // wrong scheme
        ] {
            let NgsiError::BadRequestData(msg) =
                MqttEndpoint::parse(uri).expect_err(&format!("{uri} must be rejected"))
            else {
                panic!("{uri} must be BadRequestData (400, Table 6.3.2-1)");
            };
            assert!(
                !msg.contains("hunter2"),
                "the password leaked into the 400 detail: {msg}"
            );
            assert!(
                !msg.contains("user:"),
                "the userinfo leaked into the 400 detail: {msg}"
            );
            // the detail must still be useful: scheme and host survive
            assert!(
                msg.contains("host") || uri.contains("@/"),
                "the redacted detail lost the host: {msg}"
            );
        }
    }

    #[test]
    fn notifier_info_defaults_and_validation() {
        let p = MqttParams::from_notifier_info([]).expect("defaults");
        assert_eq!(p, MqttParams { qos: 0, v5: true });
        let p = MqttParams::from_notifier_info([("MQTT-QoS", "2"), ("MQTT-Version", "mqtt3.1.1")])
            .expect("explicit");
        assert_eq!(p, MqttParams { qos: 2, v5: false });
        assert!(MqttParams::from_notifier_info([("MQTT-QoS", "3")]).is_err());
        assert!(MqttParams::from_notifier_info([("MQTT-Version", "mqtt4")]).is_err());
    }

    /// The pool key must separate sessions by credentials: same user with
    /// two different passwords = two different authenticated principals,
    /// which must never share one session. The plaintext password itself
    /// must not appear in the key.
    #[test]
    fn pool_key_separates_credentials() {
        let p = MqttParams::default();
        let a = MqttEndpoint::parse("mqtt://u:secret-one@host/t").expect("a");
        let b = MqttEndpoint::parse("mqtt://u:secret-two@host/t").expect("b");
        let c = MqttEndpoint::parse("mqtt://u:secret-one@host/t").expect("c");
        assert_ne!(
            pool_key(&a, p),
            pool_key(&b, p),
            "different passwords must not share an authenticated session"
        );
        assert_eq!(
            pool_key(&a, p),
            pool_key(&c, p),
            "identical endpoints must keep pooling"
        );
        assert!(
            !pool_key(&a, p).contains("secret-one"),
            "the plaintext password must never appear in the key"
        );
        // no password vs some password are different principals too
        let none = MqttEndpoint::parse("mqtt://u@host/t").expect("none");
        assert_ne!(pool_key(&a, p), pool_key(&none, p));
    }

    /// The mqtts trust store is loaded once and the same shared rustls
    /// config is handed to every connect. (The failure path — an unreadable
    /// platform store — cannot be simulated in a unit test; the helper's
    /// error contract covers it.)
    #[test]
    fn tls_config_is_built_once_and_shared() {
        let a = shared_tls_config().expect("first load");
        let b = shared_tls_config().expect("second load");
        let (rumqttc::TlsConfiguration::Rustls(a), rumqttc::TlsConfiguration::Rustls(b)) = (a, b)
        else {
            panic!("expected the injected-rustls variant");
        };
        assert!(
            std::sync::Arc::ptr_eq(&a, &b),
            "each call built a fresh trust store instead of sharing one"
        );
    }

    /// The MQTT destination is client-supplied (`notification.endpoint.uri`,
    /// 7.2), so it is an egress target: the cloud instance-metadata range is
    /// refused before any socket is opened, whatever the private-egress
    /// switch says, and the refusal must not echo the endpoint credentials.
    #[tokio::test]
    async fn deliver_refuses_instance_metadata_endpoint() {
        let ep = MqttEndpoint::parse("mqtt://user:hunter2@169.254.169.254/t").expect("parse");
        let sink = MqttSink::new(2, Duration::from_millis(250));
        let err = sink
            .deliver(&ep, MqttParams::default(), b"{}")
            .await
            .expect_err("the metadata range must never be dialled");
        let msg = err.to_string();
        assert!(
            msg.contains("denied"),
            "expected an egress denial, got: {msg}"
        );
        assert!(
            !msg.contains("hunter2"),
            "the endpoint password leaked into the delivery error: {msg}"
        );
    }

    /// Egress classification, restated from the HTTP side's policy: the
    /// metadata range is refused unconditionally, private ranges only when
    /// the deployment switched private egress off, and IPv4-mapped IPv6
    /// spellings are judged as their IPv4 selves.
    #[tokio::test]
    async fn checked_addr_applies_the_egress_rules_to_resolved_addresses() {
        let d = Duration::from_secs(2);
        // metadata: denied with private egress ALLOWED (the default)
        for host in ["169.254.169.254", "::ffff:169.254.169.254", "fd00:ec2::254"] {
            let e = checked_addr(host, 1883, true, d)
                .await
                .expect_err("metadata range must be refused whatever the switch says");
            assert!(e.to_string().contains("denied"), "{host}: {e}");
        }
        // loopback and RFC 1918: allowed by default, refused when the
        // deployment turns private egress off
        for host in [
            "127.0.0.1",
            "::ffff:127.0.0.1",
            "10.1.2.3",
            "::1",
            "localhost",
        ] {
            let ok = checked_addr(host, 1883, true, d)
                .await
                .unwrap_or_else(|e| panic!("{host} must be reachable by default: {e}"));
            assert_eq!(ok.port(), 1883);
            assert!(
                checked_addr(host, 1883, false, d).await.is_err(),
                "{host} must be refused with private egress off"
            );
        }
        // a public literal clears the strict policy and comes back as the
        // ADDRESS to dial — the resolution the check judged, pinned
        let a = checked_addr("93.184.216.34", 8883, false, d)
            .await
            .expect("public address allowed");
        assert_eq!(a.to_string(), "93.184.216.34:8883");
        // a name that cannot be resolved is a denial, not a pass-through
        assert!(
            checked_addr("no-such-host.invalid", 1883, true, d)
                .await
                .is_err(),
            "an unresolvable destination must not be dialled"
        );
    }

    /// The address the policy judged is the address dialled — except for
    /// mqtts, where the certificate is verified against the host NAME.
    #[test]
    fn dial_host_pins_the_address_and_keeps_the_tls_name() {
        let plain = MqttEndpoint::parse("mqtt://broker.example/t").expect("plain");
        let addr = "203.0.113.7:1883".parse().expect("addr");
        assert_eq!(dial_host(&plain, addr), "203.0.113.7");
        let secure = MqttEndpoint::parse("mqtts://broker.example/t").expect("secure");
        assert_eq!(
            dial_host(&secure, addr),
            "broker.example",
            "mqtts must dial the name so the certificate name check still applies"
        );
        // IPv6 comes back unbracketed, which is what rumqttc resolves
        let v6 = "[2001:db8::1]:1883".parse().expect("v6 addr");
        assert_eq!(dial_host(&plain, v6), "2001:db8::1");
    }

    /// The private-egress switch is read exactly as the HTTP side reads it:
    /// allowed unless the value spells false.
    #[test]
    fn private_egress_switch_parses_tolerantly() {
        for v in [None, Some(""), Some("true"), Some("yes"), Some(" 1 ")] {
            assert!(allow_private_from(v), "{v:?} must allow private egress");
        }
        for v in ["false", "FALSE", " False ", "0", " 0 "] {
            assert!(
                !allow_private_from(Some(v)),
                "{v:?} must deny private egress"
            );
        }
    }

    #[test]
    fn message_wrapper_shape() {
        let body = json!({"id": "urn:n:1", "type": "Notification"});
        let m = build_message(
            &body,
            "application/json",
            Some("<https://ctx>; rel=\"http://www.w3.org/ns/json-ld#context\""),
            &[("MyKey".into(), "MyValue".into())],
        );
        assert_eq!(m["body"], body);
        assert_eq!(m["metadata"]["Content-Type"], "application/json");
        assert!(m["metadata"]["Link"]
            .as_str()
            .expect("link present")
            .contains("json-ld#context"));
        assert_eq!(m["metadata"]["MyKey"], "MyValue");

        // ld+json: @context is in the body, no Link in metadata (Table 7.2-2)
        let m = build_message(&body, "application/ld+json", Some("<x>"), &[]);
        assert_eq!(m["metadata"]["Content-Type"], "application/ld+json");
        assert!(m["metadata"].get("Link").is_none());
    }
}
