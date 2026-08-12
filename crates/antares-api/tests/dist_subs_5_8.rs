//! 5.8.1.4 / 5.8.2.4 / 5.8.5.4 — the CONSUMER half of distributed
//! subscriptions: an entity Subscription (localOnly != true) creates an
//! internal Context Source Registration Subscription (5.11.2); a matching
//! registration supporting createSubscription receives a reduced copy of
//! the Subscription whose notification endpoint is the local broker;
//! inbound notifications are remapped to the original subscriptionId and
//! forwarded to the original subscriber; deleting the Subscription forwards
//! the delete (5.11.6).

use antares_api::AppState;
use axum::body::Body;
use axum::http::{Request, StatusCode};
use serde_json::{json, Value};
use std::sync::{Arc, Mutex};
use tower::ServiceExt;

async fn send(
    st: &AppState,
    method: &str,
    path: &str,
    body: Option<String>,
) -> (StatusCode, Value) {
    let b = Request::builder().method(method).uri(path);
    let req = match body {
        Some(body) => b
            .header("Content-Type", "application/json")
            .header("Content-Length", body.len())
            .body(Body::from(body)),
        None => b.body(Body::empty()),
    }
    .expect("request");
    let res = antares_api::router(st.clone())
        .oneshot(req)
        .await
        .expect("response");
    let status = res.status();
    let bytes = axum::body::to_bytes(res.into_body(), usize::MAX)
        .await
        .expect("body");
    let body = if bytes.is_empty() {
        Value::Null
    } else {
        serde_json::from_slice(&bytes).unwrap_or(Value::Null)
    };
    (status, body)
}

/// Recording remote broker: stores "METHOD PATH\n\nBODY" per request,
/// answers 201.
fn recording_mock() -> (u16, Arc<Mutex<Vec<String>>>) {
    let seen: Arc<Mutex<Vec<String>>> = Arc::default();
    let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("bind");
    let port = listener.local_addr().expect("addr").port();
    let sink = seen.clone();
    std::thread::spawn(move || {
        for stream in listener.incoming() {
            let Ok(mut s) = stream else { continue };
            let sink = sink.clone();
            std::thread::spawn(move || {
                use std::io::{Read, Write};
                let mut buf = Vec::new();
                let mut chunk = [0u8; 8192];
                while !buf.windows(4).any(|w| w == b"\r\n\r\n") {
                    match s.read(&mut chunk) {
                        Ok(0) => break,
                        Ok(n) => buf.extend_from_slice(&chunk[..n]),
                        Err(_) => break,
                    }
                }
                let header_end = buf
                    .windows(4)
                    .position(|w| w == b"\r\n\r\n")
                    .map(|p| p + 4)
                    .unwrap_or(buf.len());
                let headers = String::from_utf8_lossy(&buf[..header_end]).to_string();
                let want: usize = headers
                    .lines()
                    .find_map(|l| {
                        l.to_ascii_lowercase()
                            .strip_prefix("content-length:")
                            .map(|v| v.trim().parse().unwrap_or(0))
                    })
                    .unwrap_or(0);
                while buf.len() - header_end < want {
                    match s.read(&mut chunk) {
                        Ok(0) => break,
                        Ok(n) => buf.extend_from_slice(&chunk[..n]),
                        Err(_) => break,
                    }
                }
                let first = headers.lines().next().unwrap_or("").to_owned();
                let body = String::from_utf8_lossy(&buf[header_end..]).to_string();
                sink.lock()
                    .expect("sink")
                    .push(format!("{first}\n\n{body}"));
                let _ = s.write_all(
                    b"HTTP/1.1 201 Created\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
                );
            });
        }
    });
    (port, seen)
}

async fn wait_for<F: Fn() -> bool>(what: &str, f: F) {
    for _ in 0..100 {
        if f() {
            return;
        }
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    }
    panic!("timed out waiting for {what}");
}

#[tokio::test(flavor = "multi_thread")]
async fn clause_5_8_1_4_consumer_half_end_to_end() {
    std::env::set_var("ANTARES_EGRESS_ALLOW_PRIVATE", "true");
    std::env::set_var("ANTARES_PUBLIC_URL", "http://127.0.0.1:9999");
    let mut st = AppState::new("antares-distsub".into());
    antares_api::notify::wire(&mut st);

    let (remote_port, remote_seen) = recording_mock();

    // a registration supporting subscription forwarding (federationOps)
    let reg = json!({
        "id": "urn:ngsi-ld:ContextSourceRegistration:ds1",
        "type": "ContextSourceRegistration",
        "information": [{"entities": [{"type": "Vehicle"}]}],
        "operations": ["federationOps"],
        "endpoint": format!("http://127.0.0.1:{remote_port}"),
    });
    let (status, body) = send(
        &st,
        "POST",
        "/ngsi-ld/v1/csourceRegistrations",
        Some(reg.to_string()),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{body}");

    // the original subscriber's endpoint (never actually delivered to here)
    let sub = json!({
        "id": "urn:ngsi-ld:Subscription:ds-own",
        "type": "Subscription",
        "entities": [{"type": "Vehicle"}],
        "q": "speed>50",
        "notification": {"endpoint": {"uri": "http://127.0.0.1:9998/original"}},
    });
    let (status, body) = send(
        &st,
        "POST",
        "/ngsi-ld/v1/subscriptions",
        Some(sub.to_string()),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{body}");

    // 5.8.1.4: an internal CSR subscription exists (5.11.2)
    let (status, body) = send(&st, "GET", "/ngsi-ld/v1/csourceSubscriptions", None).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert!(
        body.as_array().is_some_and(|a| !a.is_empty()),
        "a Context Source Registration Subscription shall be created: {body}"
    );

    // newlyMatching → reduced copy forwarded to the remote broker
    wait_for("the forwarded remote subscription", || {
        remote_seen
            .lock()
            .expect("seen")
            .iter()
            .any(|r| r.starts_with("POST /ngsi-ld/v1/subscriptions"))
    })
    .await;
    let remote_sub: Value = {
        let seen = remote_seen.lock().expect("seen");
        let r = seen
            .iter()
            .find(|r| r.starts_with("POST /ngsi-ld/v1/subscriptions"))
            .expect("post")
            .clone();
        serde_json::from_str(r.split("\n\n").nth(1).expect("body")).expect("json")
    };
    // the copy's endpoint is the LOCAL broker, not the original subscriber
    let ep = remote_sub["notification"]["endpoint"]["uri"]
        .as_str()
        .expect("endpoint");
    assert!(ep.starts_with("http://127.0.0.1:9999"), "{remote_sub}");
    assert!(
        !ep.contains("9998"),
        "must not leak the subscriber: {remote_sub}"
    );
    let remote_id = remote_sub["id"].as_str().expect("remote id").to_owned();
    assert_ne!(remote_id, "urn:ngsi-ld:Subscription:ds-own");

    // an inbound remote notification is remapped to the OWN subscriptionId
    // and forwarded to the original subscriber
    let (orig_port, orig_seen) = recording_mock();
    let (status, _) = send(
        &st,
        "PATCH",
        "/ngsi-ld/v1/subscriptions/urn:ngsi-ld:Subscription:ds-own",
        Some(
            json!({"notification": {"endpoint": {"uri": format!("http://127.0.0.1:{orig_port}/original")}}})
                .to_string(),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    // 5.8.2.4: the update is forwarded to the mapped remote (5.11.3)
    wait_for("the forwarded remote update", || {
        remote_seen
            .lock()
            .expect("seen")
            .iter()
            .any(|r| r.starts_with(&format!("PATCH /ngsi-ld/v1/subscriptions/{remote_id}")))
    })
    .await;

    let inbound = json!({
        "id": "urn:ngsi-ld:Notification:remote1",
        "type": "Notification",
        "subscriptionId": remote_id,
        "notifiedAt": "2026-08-12T12:00:00Z",
        "data": [{"id": "urn:ngsi-ld:Vehicle:remote1", "type": "Vehicle",
                  "speed": {"type": "Property", "value": 99}}],
    });
    let (status, body) = send(
        &st,
        "POST",
        "/ngsi-ld/ex/remote-notify",
        Some(inbound.to_string()),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    wait_for(
        "the remapped notification at the original subscriber",
        || {
            orig_seen.lock().expect("seen").iter().any(|r| {
                r.contains("urn:ngsi-ld:Subscription:ds-own")
                    && r.contains("urn:ngsi-ld:Vehicle:remote1")
                    && !r.contains(&remote_id)
            })
        },
    )
    .await;

    // an unknown remote subscriptionId is 404, never forwarded
    let (status, _) = send(
        &st,
        "POST",
        "/ngsi-ld/ex/remote-notify",
        Some(json!({"type": "Notification", "subscriptionId": "urn:nope", "data": []}).to_string()),
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);

    // 5.8.5.4: deleting the Subscription forwards the delete (5.11.6)
    let (status, _) = send(
        &st,
        "DELETE",
        "/ngsi-ld/v1/subscriptions/urn:ngsi-ld:Subscription:ds-own",
        None,
    )
    .await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    wait_for("the forwarded remote delete", || {
        remote_seen
            .lock()
            .expect("seen")
            .iter()
            .any(|r| r.starts_with(&format!("DELETE /ngsi-ld/v1/subscriptions/{remote_id}")))
    })
    .await;
    // and the internal CSR subscription is gone
    let (status, body) = send(&st, "GET", "/ngsi-ld/v1/csourceSubscriptions", None).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body.as_array().map(Vec::len), Some(0), "{body}");
}

/// 5.8.6: with a csf on the Subscription, an inbound notification whose
/// origin Context Source does not match the filter is NOT forwarded.
#[tokio::test(flavor = "multi_thread")]
async fn clause_5_8_6_origin_csf_gates_inbound_notifications() {
    std::env::set_var("ANTARES_EGRESS_ALLOW_PRIVATE", "true");
    let mut st = AppState::new("antares-distsub3".into());
    antares_api::notify::wire(&mut st);
    let (remote_port, remote_seen) = recording_mock();
    let reg = json!({
        "id": "urn:ngsi-ld:ContextSourceRegistration:ds3",
        "type": "ContextSourceRegistration",
        "information": [{"entities": [{"type": "Vehicle"}]}],
        "operations": ["federationOps"],
        "endpoint": format!("http://127.0.0.1:{remote_port}"),
        "sourceType": {"type": "Property", "value": "archive"},
    });
    let (status, _) = send(
        &st,
        "POST",
        "/ngsi-ld/v1/csourceRegistrations",
        Some(reg.to_string()),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);

    let (orig_port, orig_seen) = recording_mock();
    let sub = json!({
        "id": "urn:ngsi-ld:Subscription:ds3-own",
        "type": "Subscription",
        "entities": [{"type": "Vehicle"}],
        "csf": "sourceType==\"sensor\"",
        "notification": {"endpoint": {"uri": format!("http://127.0.0.1:{orig_port}/notify")}},
    });
    let (status, body) = send(
        &st,
        "POST",
        "/ngsi-ld/v1/subscriptions",
        Some(sub.to_string()),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{body}");
    wait_for("the forwarded remote subscription", || {
        remote_seen
            .lock()
            .expect("seen")
            .iter()
            .any(|r| r.starts_with("POST /ngsi-ld/v1/subscriptions"))
    })
    .await;
    let remote_id: String = {
        let seen = remote_seen.lock().expect("seen");
        let r = seen
            .iter()
            .find(|r| r.starts_with("POST /ngsi-ld/v1/subscriptions"))
            .expect("post")
            .clone();
        serde_json::from_str::<Value>(r.split("\n\n").nth(1).expect("body")).expect("json")["id"]
            .as_str()
            .expect("id")
            .to_owned()
    };

    // the origin is an "archive" source; csf wants "sensor" → NOT forwarded
    let inbound = json!({
        "type": "Notification",
        "subscriptionId": remote_id,
        "data": [{"id": "urn:ngsi-ld:Vehicle:gated", "type": "Vehicle"}],
    });
    let (status, _) = send(
        &st,
        "POST",
        "/ngsi-ld/ex/remote-notify",
        Some(inbound.to_string()),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    tokio::time::sleep(std::time::Duration::from_millis(600)).await;
    assert!(
        orig_seen.lock().expect("seen").is_empty(),
        "csf must gate the archive-origin notification out"
    );
}

/// localOnly=true opts OUT of the distributed-subscription machinery: no
/// CSR subscription, nothing forwarded.
#[tokio::test(flavor = "multi_thread")]
async fn clause_5_8_1_4_local_only_subscription_stays_local() {
    std::env::set_var("ANTARES_EGRESS_ALLOW_PRIVATE", "true");
    let mut st = AppState::new("antares-distsub2".into());
    antares_api::notify::wire(&mut st);
    let (remote_port, remote_seen) = recording_mock();
    let reg = json!({
        "id": "urn:ngsi-ld:ContextSourceRegistration:ds2",
        "type": "ContextSourceRegistration",
        "information": [{"entities": [{"type": "Vehicle"}]}],
        "operations": ["federationOps"],
        "endpoint": format!("http://127.0.0.1:{remote_port}"),
    });
    let (status, _) = send(
        &st,
        "POST",
        "/ngsi-ld/v1/csourceRegistrations",
        Some(reg.to_string()),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);

    let sub = json!({
        "type": "Subscription",
        "entities": [{"type": "Vehicle"}],
        "localOnly": true,
        "notification": {"endpoint": {"uri": "http://127.0.0.1:9998/original"}},
    });
    let (status, body) = send(
        &st,
        "POST",
        "/ngsi-ld/v1/subscriptions",
        Some(sub.to_string()),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{body}");

    tokio::time::sleep(std::time::Duration::from_millis(600)).await;
    assert!(
        remote_seen.lock().expect("seen").is_empty(),
        "localOnly must not forward"
    );
    let (_, body) = send(&st, "GET", "/ngsi-ld/v1/csourceSubscriptions", None).await;
    assert_eq!(body.as_array().map(Vec::len), Some(0), "{body}");
}
