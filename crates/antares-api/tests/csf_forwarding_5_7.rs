// SPDX-License-Identifier: EUPL-1.2
//! csf (Context Source Filter, 4.9 grammar) applied to Context Source
//! matching on the QUERY paths: 5.7.2.4 / 5.7.4.4 / 5.6.21.4 — with a csf
//! present, only registrations whose Context Source Properties match the
//! filter are considered for forwarding. (Discovery, 5.10.2.4, already
//! applies it — this is the forwarding half.)

use antares_api::AppState;
use axum::body::Body;
use axum::http::{Request, StatusCode};
use serde_json::{json, Value};
use tower::ServiceExt;

async fn send(st: &AppState, req: Request<Body>) -> (StatusCode, Value) {
    let res = antares_api::router(st.clone())
        .oneshot(req)
        .await
        .expect("response");
    let status = res.status();
    // 6.3.17: a source that failed shows up here, not in the body — keep
    // the reason in the assertion messages, where a flake can be read.
    let warnings: Vec<String> = res
        .headers()
        .get_all("NGSILD-Warning")
        .iter()
        .filter_map(|v| v.to_str().ok().map(str::to_owned))
        .collect();
    let bytes = axum::body::to_bytes(res.into_body(), usize::MAX)
        .await
        .expect("body");
    let mut body = if bytes.is_empty() {
        Value::Null
    } else {
        serde_json::from_slice(&bytes).unwrap_or(Value::Null)
    };
    if !warnings.is_empty() {
        body = json!({"body": body, "NGSILD-Warning": warnings});
    }
    (status, body)
}

async fn get(st: &AppState, uri: &str) -> (StatusCode, Value) {
    send(
        st,
        Request::builder()
            .method("GET")
            .uri(uri)
            .body(Body::empty())
            .expect("request"),
    )
    .await
}

async fn post(st: &AppState, uri: &str, body: String) -> (StatusCode, Value) {
    send(
        st,
        Request::builder()
            .method("POST")
            .uri(uri)
            .header("Content-Type", "application/json")
            .header("Content-Length", body.len())
            .body(Body::from(body))
            .expect("request"),
    )
    .await
}

/// set_var once: a sibling test reading the env while this one rewrites it
/// saw the policy missing and refused the loopback forward (TSan flake).
fn allow_private() {
    static ONCE: std::sync::Once = std::sync::Once::new();
    ONCE.call_once(|| std::env::set_var("ANTARES_EGRESS_ALLOW_PRIVATE", "true"));
}

/// Canned Context Source answering every request with `reply` (raw HTTP).
fn mock(reply: String) -> u16 {
    let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("bind");
    let port = listener.local_addr().expect("addr").port();
    std::thread::spawn(move || {
        for stream in listener.incoming() {
            let Ok(mut s) = stream else { continue };
            use std::io::{Read, Write};
            let mut buf = [0u8; 8192];
            let _ = s.read(&mut buf);
            let _ = s.write_all(reply.as_bytes());
        }
    });
    port
}

fn entity_reply(id: &str) -> String {
    let body = json!([{ "id": id, "type": "Vehicle" }]).to_string();
    format!(
        "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\r\n{body}",
        body.len()
    )
}

/// Register a Vehicle source with a Context Source Property `sourceType`.
async fn register(st: &AppState, reg_id: &str, port: u16, source_type: &str) {
    let reg = json!({
        "id": reg_id,
        "type": "ContextSourceRegistration",
        "information": [{"entities": [{"type": "Vehicle"}]}],
        "endpoint": format!("http://127.0.0.1:{port}"),
        "sourceType": {"type": "Property", "value": source_type},
    })
    .to_string();
    let (status, body) = post(st, "/ngsi-ld/v1/csourceRegistrations", reg).await;
    assert_eq!(status, StatusCode::CREATED, "{body}");
}

/// 5.7.2.4: with csf present only matching sources are contacted — the
/// non-matching source's entity must NOT appear; without csf both do.
#[tokio::test(flavor = "multi_thread")]
async fn clause_5_7_2_4_csf_filters_forwarding_targets() {
    allow_private();
    let st = AppState::new("antares-csf".into());
    let port_a = mock(entity_reply("urn:ngsi-ld:Vehicle:fromA"));
    let port_b = mock(entity_reply("urn:ngsi-ld:Vehicle:fromB"));
    register(
        &st,
        "urn:ngsi-ld:ContextSourceRegistration:csfA",
        port_a,
        "sensor",
    )
    .await;
    register(
        &st,
        "urn:ngsi-ld:ContextSourceRegistration:csfB",
        port_b,
        "archive",
    )
    .await;

    // sanity: no csf → both sources contribute
    let (status, body) = get(&st, "/ngsi-ld/v1/entities?type=Vehicle").await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let ids: Vec<&str> = body
        .get("body")
        .unwrap_or(&body)
        .as_array()
        .expect("array")
        .iter()
        .filter_map(|d| d["id"].as_str())
        .collect();
    assert!(ids.contains(&"urn:ngsi-ld:Vehicle:fromA"), "{body}");
    assert!(ids.contains(&"urn:ngsi-ld:Vehicle:fromB"), "{body}");

    // csf narrows to the sensor source; the archive entity must NOT appear
    let (status, body) = get(
        &st,
        "/ngsi-ld/v1/entities?type=Vehicle&csf=sourceType%3D%3D%22sensor%22",
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let ids: Vec<&str> = body
        .get("body")
        .unwrap_or(&body)
        .as_array()
        .expect("array")
        .iter()
        .filter_map(|d| d["id"].as_str())
        .collect();
    assert!(ids.contains(&"urn:ngsi-ld:Vehicle:fromA"), "{body}");
    assert!(
        !ids.contains(&"urn:ngsi-ld:Vehicle:fromB"),
        "csf must gate the archive source out: {body}"
    );
}

/// 5.7.2.4: a csf no registration satisfies → nothing is forwarded, only
/// local data answers.
#[tokio::test(flavor = "multi_thread")]
async fn clause_5_7_2_4_csf_matching_nothing_stays_local() {
    allow_private();
    let st = AppState::new("antares-csf2".into());
    let port_a = mock(entity_reply("urn:ngsi-ld:Vehicle:fromA2"));
    register(
        &st,
        "urn:ngsi-ld:ContextSourceRegistration:csfA2",
        port_a,
        "sensor",
    )
    .await;

    let local = json!({"id": "urn:ngsi-ld:Vehicle:local2", "type": "Vehicle"}).to_string();
    let (status, _) = post(&st, "/ngsi-ld/v1/entities", local).await;
    assert_eq!(status, StatusCode::CREATED);

    let (status, body) = get(
        &st,
        "/ngsi-ld/v1/entities?type=Vehicle&csf=sourceType%3D%3D%22nonexistent%22",
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let ids: Vec<&str> = body
        .get("body")
        .unwrap_or(&body)
        .as_array()
        .expect("array")
        .iter()
        .filter_map(|d| d["id"].as_str())
        .collect();
    assert_eq!(ids, vec!["urn:ngsi-ld:Vehicle:local2"], "{body}");
}
