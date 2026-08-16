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
    pub fn parse(uri: &str) -> Result<Self, NgsiError> {
        let (secure, rest) = if let Some(r) = uri.strip_prefix("mqtts://") {
            (true, r)
        } else if let Some(r) = uri.strip_prefix("mqtt://") {
            (false, r)
        } else {
            return Err(bad(format!("not an mqtt(s) endpoint URI: {uri:?}")));
        };
        let (authority, topic) = rest
            .split_once('/')
            .ok_or_else(|| bad(format!("mqtt endpoint {uri:?} has no topic")))?;
        if topic.is_empty() {
            return Err(bad(format!("mqtt endpoint {uri:?} has no topic")));
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
                    .map_err(|_| bad(format!("invalid mqtt port in {uri:?}")))?,
            ),
            None => (hostport.to_owned(), if secure { 8883 } else { 1883 }),
        };
        if host.is_empty() {
            return Err(bad(format!("mqtt endpoint {uri:?} has no host")));
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
        let key = format!(
            "{}:{}@{}:{}/v{}",
            ep.username.as_deref().unwrap_or(""),
            ep.secure,
            ep.host,
            ep.port,
            if params.v5 { 5 } else { 3 }
        );
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
        let id = format!(
            "antares-{}-{}",
            std::process::id(),
            Instant::now().elapsed().as_nanos()
        );
        let refused = |e: String| {
            NgsiError::InternalError(format!("mqtt connect {}:{}: {e}", ep.host, ep.port))
        };
        if params.v5 {
            let mut opts = rumqttc::v5::MqttOptions::new(id, &ep.host, ep.port);
            opts.set_keep_alive(Duration::from_secs(30));
            if let Some(u) = &ep.username {
                opts.set_credentials(u, ep.password.as_deref().unwrap_or(""));
            }
            if ep.secure {
                opts.set_transport(rumqttc::Transport::Tls(rumqttc::TlsConfiguration::default()));
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
            let mut opts = rumqttc::MqttOptions::new(id, &ep.host, ep.port);
            opts.set_keep_alive(Duration::from_secs(30));
            if let Some(u) = &ep.username {
                opts.set_credentials(u, ep.password.as_deref().unwrap_or(""));
            }
            if ep.secure {
                opts.set_transport(rumqttc::Transport::Tls(rumqttc::TlsConfiguration::default()));
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
