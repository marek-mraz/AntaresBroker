// SPDX-License-Identifier: EUPL-1.2
//! MQTT notification binding e2e (CIM 009 clause 7.2): the
//! real binary delivers a `{metadata, body}` message to a live MQTT server.
//! Gated on ANTARES_TEST_MQTT_URL (e.g. `mqtt://127.0.0.1:1883`) — CI installs
//! mosquitto and sets it; without it the test is skipped, not green-lied.
//! std-only harness + rumqttc's sync client (no async test rig needed).
//! A build without the `mqtt` feature rejects the endpoint scheme at
//! subscription creation (422), so the test only exists with it.
#![cfg(all(unix, feature = "mqtt"))]

use std::io::{Read, Write};
use std::net::TcpStream;
use std::process::Command;
use std::time::{Duration, Instant};

fn http(port: u16, request: &str) -> String {
    let mut s = TcpStream::connect(("127.0.0.1", port)).expect("connect");
    s.write_all(request.as_bytes()).expect("write");
    let mut out = String::new();
    s.read_to_string(&mut out).expect("read");
    out
}

fn post(port: u16, path: &str, body: &str) -> String {
    http(
        port,
        &format!(
            "POST {path} HTTP/1.1\r\nHost: x\r\nContent-Type: application/ld+json\r\n\
             Content-Length: {}\r\nConnection: close\r\n\r\n{body}",
            body.len()
        ),
    )
}

fn wait_healthy(port: u16) {
    let deadline = Instant::now() + Duration::from_secs(15);
    loop {
        if let Ok(mut s) = TcpStream::connect(("127.0.0.1", port)) {
            let req = "GET /q/health HTTP/1.1\r\nHost: x\r\nConnection: close\r\n\r\n";
            if s.write_all(req.as_bytes()).is_ok() {
                let mut out = String::new();
                let _ = s.read_to_string(&mut out);
                if out.contains("200") {
                    return;
                }
            }
        }
        assert!(Instant::now() < deadline, "broker never got healthy");
        std::thread::sleep(Duration::from_millis(100));
    }
}

const CTX: &str =
    "\"@context\": [\"https://uri.etsi.org/ngsi-ld/v1/ngsi-ld-core-context-v1.8.jsonld\"]";

#[test]
fn mqtt_notification_carries_metadata_and_body() {
    let Ok(mqtt_url) = std::env::var("ANTARES_TEST_MQTT_URL") else {
        eprintln!(
            "skipped: set ANTARES_TEST_MQTT_URL=mqtt://127.0.0.1:1883 (needs a live MQTT server)"
        );
        return;
    };
    let hostport = mqtt_url
        .strip_prefix("mqtt://")
        .expect("ANTARES_TEST_MQTT_URL must be mqtt://host:port");
    let (host, port_s) = hostport.split_once(':').expect("host:port");
    let mqtt_port: u16 = port_s.parse().expect("port");

    let topic = format!("antares-e2e-{}", std::process::id());
    let http_port = 19195_u16;

    // Subscriber FIRST (QoS 1 so the publish is buffered for us).
    let mut opts = rumqttc::MqttOptions::new(
        format!("antares-e2e-sub-{}", std::process::id()),
        host,
        mqtt_port,
    );
    opts.set_keep_alive(Duration::from_secs(10));
    let (client, mut conn) = rumqttc::Client::new(opts, 16);
    client
        .subscribe(&topic, rumqttc::QoS::AtLeastOnce)
        .expect("subscribe");
    // Drive the connection until the SUBACK: subscribe() only queues the
    // packet, and a notification published before the subscription is live
    // on the server is silently lost (no persistent session buffers it).
    for event in conn.iter() {
        if matches!(
            event.expect("mqtt connect"),
            rumqttc::Event::Incoming(rumqttc::Packet::SubAck(_))
        ) {
            break;
        }
    }

    let mut broker = Command::new(env!("CARGO_BIN_EXE_antares"))
        .env_remove("ANTARES_TEST_DATABASE_URL")
        .env_remove("ANTARES_TEST_MQTT_URL")
        .env("ANTARES_HTTP_PORT", http_port.to_string())
        .env("ANTARES_EGRESS_ALLOW_PRIVATE", "true")
        .spawn()
        .expect("spawn antares");
    wait_healthy(http_port);

    // Subscription with an MQTT endpoint + receiverInfo (Table 7.2-2).
    let sub = format!(
        r#"{{"id": "urn:ngsi-ld:Subscription:mqtt-e2e", "type": "Subscription",
             "entities": [{{"type": "Vehicle"}}],
             "notification": {{
               "endpoint": {{
                 "uri": "{mqtt_url}/{topic}",
                 "notifierInfo": [{{"key": "MQTT-Version", "value": "mqtt3.1.1"}},
                                   {{"key": "MQTT-QoS", "value": "1"}}],
                 "receiverInfo": [{{"key": "My-Key", "value": "my-value"}}]
               }}
             }},
             {CTX}}}"#
    );
    let resp = post(http_port, "/ngsi-ld/v1/subscriptions/", &sub);
    assert!(resp.contains("201"), "subscription create failed: {resp}");

    let entity = format!(
        r#"{{"id": "urn:ngsi-ld:Vehicle:mqtt-e2e", "type": "Vehicle",
             "speed": {{"type": "Property", "value": 42}}, {CTX}}}"#
    );
    let resp = post(http_port, "/ngsi-ld/v1/entities/", &entity);
    assert!(resp.contains("201"), "entity create failed: {resp}");

    // Pump the MQTT connection until the notification lands (≤15 s).
    let deadline = Instant::now() + Duration::from_secs(15);
    let mut payload: Option<Vec<u8>> = None;
    for event in conn.iter() {
        if let Ok(rumqttc::Event::Incoming(rumqttc::Packet::Publish(p))) = event {
            payload = Some(p.payload.to_vec());
            break;
        }
        if Instant::now() > deadline {
            break;
        }
    }
    let _ = broker.kill();
    let _ = broker.wait();

    let payload = payload.expect("no MQTT notification arrived within 15 s");
    let msg: serde_json::Value = serde_json::from_slice(&payload).expect("payload is JSON");
    // 7.2: two elements, metadata + body.
    let meta = msg.get("metadata").expect("metadata element");
    assert_eq!(
        meta.get("Content-Type").and_then(|v| v.as_str()),
        Some("application/json"),
        "default MIME type is application/json (Table 7.2-2)"
    );
    assert!(
        meta.get("Link")
            .and_then(|v| v.as_str())
            .unwrap_or_default()
            .contains("json-ld#context"),
        "Link to @context required when Content-Type is application/json"
    );
    assert_eq!(
        meta.get("My-Key").and_then(|v| v.as_str()),
        Some("my-value"),
        "receiverInfo pairs are copied into metadata"
    );
    let body = msg.get("body").expect("body element");
    assert_eq!(
        body.get("type").and_then(|v| v.as_str()),
        Some("Notification")
    );
    assert_eq!(
        body.pointer("/data/0/id").and_then(|v| v.as_str()),
        Some("urn:ngsi-ld:Vehicle:mqtt-e2e"),
        "body is the 5.3.1 notification"
    );
}
