// SPDX-License-Identifier: EUPL-1.2
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

/// set_var once: a sibling test reading the env while another rewrites it
/// saw the policy missing and refused the loopback forward (TSan flake).
fn allow_private() {
    static ONCE: std::sync::Once = std::sync::Once::new();
    ONCE.call_once(|| std::env::set_var("ANTARES_EGRESS_ALLOW_PRIVATE", "true"));
}

async fn send(
    st: &AppState,
    method: &str,
    path: &str,
    body: Option<String>,
) -> (StatusCode, Value) {
    send_via(st, method, path, body, None).await
}

/// Same as [`send`], with the JSON-LD media type so the body's own
/// `@context` is the one the request is expanded and stored under (5.5.7).
async fn send_ld(st: &AppState, method: &str, path: &str, body: String) -> (StatusCode, Value) {
    let req = Request::builder()
        .method(method)
        .uri(path)
        .header("Content-Type", "application/ld+json")
        .header("Content-Length", body.len())
        .body(Body::from(body))
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

/// Same as [`send`], with an inbound `Via` header — the shape of a request
/// arriving from a peer broker (6.3.17/6.3.18).
async fn send_via(
    st: &AppState,
    method: &str,
    path: &str,
    body: Option<String>,
    via: Option<&str>,
) -> (StatusCode, Value) {
    let mut b = Request::builder().method(method).uri(path);
    if let Some(v) = via {
        b = b.header("Via", v);
    }
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
    recording_mock_opts(0, false)
}

/// Same, but each record keeps the FULL request head (all header lines), so
/// a test can assert on the `Via` chain a forward travelled with.
fn recording_mock_head() -> (u16, Arc<Mutex<Vec<String>>>) {
    recording_mock_opts(0, true)
}

/// Same, but holds the 201 for `delay_ms` after recording the request —
/// models an httpctrl-style mock (ETSI TP 5814_01) whose reply waits for
/// the test's assertions.
fn recording_mock_with_delay(delay_ms: u64) -> (u16, Arc<Mutex<Vec<String>>>) {
    recording_mock_opts(delay_ms, false)
}

fn recording_mock_opts(delay_ms: u64, record_head: bool) -> (u16, Arc<Mutex<Vec<String>>>) {
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
                let head = if record_head { headers.clone() } else { first };
                sink.lock().expect("sink").push(format!("{head}\n\n{body}"));
                if delay_ms > 0 {
                    std::thread::sleep(std::time::Duration::from_millis(delay_ms));
                }
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
    allow_private();
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

    // 5.8.1.4: an internal CSR subscription exists (5.11.2). It is broker
    // plumbing, so it is read where it lives and not off the 5.11 endpoint.
    let mapping = st
        .store
        .get(
            &antares_model::TenantId::default(),
            antares_store::Kind::DistSub,
            "urn:ngsi-ld:Subscription:ds-own",
        )
        .expect("mapping read")
        .expect("the distributed half stores a mapping");
    assert!(
        mapping["csr_sub"].is_string(),
        "a Context Source Registration Subscription shall be created: {mapping}"
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

/// 5.8.6 + 5.11.2: the csf decides which Context Sources take part. A source
/// the filter excludes is never subscribed at all, and a source that STOPS
/// matching the filter after its copy exists stops reaching the subscriber.
#[tokio::test(flavor = "multi_thread")]
async fn clause_5_8_6_origin_csf_gates_inbound_notifications() {
    allow_private();
    let mut st = AppState::new("antares-distsub3".into());
    antares_api::notify::wire(&mut st);
    let (sensor_port, sensor_seen) = recording_mock();
    let (archive_port, archive_seen) = recording_mock();
    for (name, port, source_type) in [
        ("ds3-sensor", sensor_port, "sensor"),
        ("ds3-archive", archive_port, "archive"),
    ] {
        let reg = json!({
            "id": format!("urn:ngsi-ld:ContextSourceRegistration:{name}"),
            "type": "ContextSourceRegistration",
            "information": [{"entities": [{"type": "Vehicle"}]}],
            "operations": ["federationOps"],
            "endpoint": format!("http://127.0.0.1:{port}"),
            "sourceType": {"type": "Property", "value": source_type},
        });
        let (status, body) = send(
            &st,
            "POST",
            "/ngsi-ld/v1/csourceRegistrations",
            Some(reg.to_string()),
        )
        .await;
        assert_eq!(status, StatusCode::CREATED, "{body}");
    }

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
    let posted = |seen: &Arc<Mutex<Vec<String>>>| {
        seen.lock()
            .expect("seen")
            .iter()
            .any(|r| r.starts_with("POST /ngsi-ld/v1/subscriptions"))
    };
    wait_for("the forwarded remote subscription", || posted(&sensor_seen)).await;
    assert!(
        !posted(&archive_seen),
        "a source the csf excludes must never be subscribed: {:?}",
        archive_seen.lock().expect("seen")
    );
    let remote_id: String = {
        let seen = sensor_seen.lock().expect("seen");
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

    // positive control: while the origin matches the filter, its notification
    // reaches the subscriber — without this the negative below is vacuous
    let inbound = |entity: &str| {
        json!({
            "type": "Notification",
            "subscriptionId": remote_id,
            "data": [{"id": entity, "type": "Vehicle"}],
        })
        .to_string()
    };
    let (status, _) = send(
        &st,
        "POST",
        "/ngsi-ld/ex/remote-notify",
        Some(inbound("urn:ngsi-ld:Vehicle:matching")),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    wait_for("the subscriber notification", || {
        !orig_seen.lock().expect("seen").is_empty()
    })
    .await;
    let delivered = orig_seen.lock().expect("seen").len();

    // the origin stops matching the csf
    let (status, body) = send(
        &st,
        "PATCH",
        "/ngsi-ld/v1/csourceRegistrations/urn:ngsi-ld:ContextSourceRegistration:ds3-sensor",
        Some(json!({"sourceType": {"type": "Property", "value": "archive"}}).to_string()),
    )
    .await;
    assert_eq!(status, StatusCode::NO_CONTENT, "{body}");

    // 5.11.2: the source no longer belongs to the subscription, so its copy
    // is deleted at the source and the mapping with it
    wait_for("the remote copy to be deleted", || {
        sensor_seen
            .lock()
            .expect("seen")
            .iter()
            .any(|r| r.starts_with("DELETE /ngsi-ld/v1/subscriptions"))
    })
    .await;
    let (status, _) = send(
        &st,
        "POST",
        "/ngsi-ld/ex/remote-notify",
        Some(inbound("urn:ngsi-ld:Vehicle:gated")),
    )
    .await;
    assert_eq!(
        status,
        StatusCode::NOT_FOUND,
        "a notification for a mapping that no longer exists is refused"
    );
    tokio::time::sleep(std::time::Duration::from_millis(600)).await;
    assert_eq!(
        orig_seen.lock().expect("seen").len(),
        delivered,
        "an origin that no longer matches the csf must reach the subscriber no more"
    );
}

/// 5.8.6 splitEntities=true: the Entities of an inbound Notification shall
/// be retrieved from all OTHER Context Sources (never the origin), merged
/// with the notified fragments, and re-filtered by the local Subscription's
/// conditions; the reduced remote copy carries no q (5.8.1.4).
#[tokio::test(flavor = "multi_thread")]
async fn clause_5_8_6_split_entities_inbound_merge() {
    allow_private();
    std::env::set_var("ANTARES_PUBLIC_URL", "http://127.0.0.1:9999");
    let mut st = AppState::new("antares-split".into());
    antares_api::notify::wire(&mut st);

    // origin: receives the reduced subscription copy, notifies fragments
    let (remote_port, remote_seen) = recording_mock();
    let reg = json!({
        "id": "urn:ngsi-ld:ContextSourceRegistration:split-origin",
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

    // a SECOND context source serving the brandName part of the entities
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind");
    let brand_addr = listener.local_addr().expect("addr");
    let app = axum::Router::new().route(
        "/ngsi-ld/v1/entities/{id}",
        axum::routing::get(
            |axum::extract::Path(id): axum::extract::Path<String>| async move {
                axum::Json(json!({"id": id, "type": "Vehicle",
                "brandName": {"type": "Property", "value": "Tesla"}}))
            },
        ),
    );
    tokio::spawn(async move {
        axum::serve(listener, app).await.expect("serve");
    });
    let reg2 = json!({
        "id": "urn:ngsi-ld:ContextSourceRegistration:split-brand",
        "type": "ContextSourceRegistration",
        "information": [{"entities": [{"type": "Vehicle"}]}],
        "operations": ["retrieveEntity"],
        "endpoint": format!("http://{brand_addr}"),
    });
    let (status, _) = send(
        &st,
        "POST",
        "/ngsi-ld/v1/csourceRegistrations",
        Some(reg2.to_string()),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);

    let (orig_port, orig_seen) = recording_mock();
    let sub = json!({
        "id": "urn:ngsi-ld:Subscription:ds-split",
        "type": "Subscription",
        "entities": [{"type": "Vehicle"}],
        "q": "speed>50;brandName==\"Tesla\"",
        "splitEntities": true,
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
    let remote_sub: Value = {
        let seen = remote_seen.lock().expect("seen");
        let r = seen
            .iter()
            .find(|r| r.starts_with("POST /ngsi-ld/v1/subscriptions"))
            .expect("post")
            .clone();
        serde_json::from_str(r.split("\n\n").nth(1).expect("body")).expect("json")
    };
    // 5.8.1.4: with splitEntities the conditions stay LOCAL — no q pushed down
    assert!(
        remote_sub.get("q").is_none(),
        "the reduced copy must not carry q: {remote_sub}"
    );
    let remote_id = remote_sub["id"].as_str().expect("remote id").to_owned();

    // inbound fragments: split1 matches q only after the brandName merge;
    // split2 fails q (speed 10) even merged and must be dropped
    let inbound = json!({
        "type": "Notification",
        "subscriptionId": remote_id,
        "data": [
            {"id": "urn:ngsi-ld:Vehicle:split1", "type": "Vehicle",
             "speed": {"type": "Property", "value": 99}},
            {"id": "urn:ngsi-ld:Vehicle:split2", "type": "Vehicle",
             "speed": {"type": "Property", "value": 10}},
        ],
    });
    let (status, _) = send(
        &st,
        "POST",
        "/ngsi-ld/ex/remote-notify",
        Some(inbound.to_string()),
    )
    .await;
    assert_eq!(status, StatusCode::OK);

    wait_for("the merged notification at the original subscriber", || {
        orig_seen
            .lock()
            .expect("seen")
            .iter()
            .any(|r| r.contains("urn:ngsi-ld:Vehicle:split1") && r.contains("Tesla"))
    })
    .await;
    let delivered = orig_seen
        .lock()
        .expect("seen")
        .iter()
        .find(|r| r.contains("urn:ngsi-ld:Vehicle:split1"))
        .expect("notification")
        .clone();
    // negative controls: the non-matching entity is dropped; the origin is
    // never re-queried for entity data
    assert!(
        !delivered.contains("urn:ngsi-ld:Vehicle:split2"),
        "split2 fails q after merge and must be removed: {delivered}"
    );
    assert!(
        !remote_seen
            .lock()
            .expect("seen")
            .iter()
            .any(|r| r.starts_with("GET /ngsi-ld/v1/entities/")),
        "the origin Context Source must be excluded from the merge retrieval"
    );
}

/// 5.8.5.4: the delete of a Subscription whose reduced copy is still in
/// flight (the Context Source has RECEIVED the create but not yet answered)
/// must still forward the delete — the remote-subscription mapping is
/// broker-generated, so it exists independently of the create's response.
/// This is the ETSI 5814_01_01 pg/timescale flake: the httpctrl mock holds
/// its 201 until the test's assertions ran, and the test deletes right after.
#[tokio::test(flavor = "multi_thread")]
async fn clause_5_8_5_4_delete_races_slow_create_forward() {
    allow_private();
    std::env::set_var("ANTARES_PUBLIC_URL", "http://127.0.0.1:9999");
    let mut st = AppState::new("antares-distsub4".into());
    antares_api::notify::wire(&mut st);
    let (remote_port, remote_seen) = recording_mock_with_delay(800);
    let reg = json!({
        "id": "urn:ngsi-ld:ContextSourceRegistration:ds4",
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
        "id": "urn:ngsi-ld:Subscription:ds-race",
        "type": "Subscription",
        "entities": [{"type": "Vehicle"}],
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

    // the create-forward has ARRIVED at the Context Source (response pending)
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
            .expect("remote id")
            .to_owned()
    };

    // delete while the create's 201 is still held back
    let (status, _) = send(
        &st,
        "DELETE",
        "/ngsi-ld/v1/subscriptions/urn:ngsi-ld:Subscription:ds-race",
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
    // the delete targets the REMOTE id — the own id never leaks to the source
    assert!(
        !remote_seen
            .lock()
            .expect("seen")
            .iter()
            .any(|r| r.lines().next().is_some_and(|l| l.contains("ds-race"))),
        "forwarded requests must use the broker-generated remote id"
    );
}

/// localOnly=true opts OUT of the distributed-subscription machinery: no
/// CSR subscription, nothing forwarded.
#[tokio::test(flavor = "multi_thread")]
async fn clause_5_8_1_4_local_only_subscription_stays_local() {
    allow_private();
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

/// 5.2.33 / 5.8.1.4 / 5.8.6: the reduced remote copy may be broader than
/// the original Subscription (it carries the REGISTRATION's entity scope) —
/// inbound notification entities are re-filtered against the ORIGINAL
/// Subscription's entities selector before forwarding: an id the selector's
/// idPattern does not match reaches the subscriber in NO payload.
/// 4.3.6.4: "It is necessary to include a binding-specific mechanism to
/// request operations only on the registered endpoint itself to avoid
/// cascades of an excessive lengths, duplicates or loops", and Table 5.2.9-1
/// `localOnly`: "distributed operations associated to this Context Source
/// Registration will act only on data held directly by the registered
/// Context Source itself". A forwarded Subscription create is one of those
/// operations, so the copy has to carry the 6.3.18 `local` parameter — the
/// reduced copy cannot say it in the body, because the body's own
/// `localOnly` is stripped before forwarding. Without it the peer creates a
/// DISTRIBUTED subscription and fans out again: the cascade the clause
/// exists to stop, bought by a registration that asked for the opposite.
#[tokio::test(flavor = "multi_thread")]
async fn clause_4_3_6_4_local_only_registration_bounds_the_forwarded_subscription() {
    allow_private();
    std::env::set_var("ANTARES_PUBLIC_URL", "http://127.0.0.1:9999");
    let mut st = AppState::new("antares-lo-reg".into());
    antares_api::notify::wire(&mut st);

    let (remote_port, remote_seen) = recording_mock();

    let reg = json!({
        "id": "urn:ngsi-ld:ContextSourceRegistration:lo-reg",
        "type": "ContextSourceRegistration",
        "information": [{"entities": [{"type": "Vehicle"}]}],
        "operations": ["federationOps"],
        "localOnly": true,
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

    let sub = json!({
        "id": "urn:ngsi-ld:Subscription:lo-reg-own",
        "type": "Subscription",
        "entities": [{"type": "Vehicle"}],
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

    wait_for("the forwarded remote subscription", || {
        remote_seen
            .lock()
            .expect("seen")
            .iter()
            .any(|r| r.starts_with("POST /ngsi-ld/v1/subscriptions"))
    })
    .await;
    let line = {
        let seen = remote_seen.lock().expect("seen");
        seen.iter()
            .find(|r| r.starts_with("POST /ngsi-ld/v1/subscriptions"))
            .expect("post")
            .lines()
            .next()
            .unwrap_or_default()
            .to_owned()
    };
    assert!(
        line.contains("local=true"),
        "a localOnly registration must not buy a cascading remote subscription: {line}"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn clause_5_2_33_inbound_notification_refiltered_by_selector() {
    allow_private();
    std::env::set_var("ANTARES_PUBLIC_URL", "http://127.0.0.1:9999");
    let mut st = AppState::new("antares-distsub-idr".into());
    antares_api::notify::wire(&mut st);

    let (remote_port, remote_seen) = recording_mock();
    let reg = json!({
        "id": "urn:ngsi-ld:ContextSourceRegistration:ds-idr",
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

    let (orig_port, orig_seen) = recording_mock();
    let sub = json!({
        "id": "urn:ngsi-ld:Subscription:ds-idr",
        "type": "Subscription",
        "entities": [{"type": "Vehicle",
                      "idPattern": "^urn:ngsi-ld:Vehicle:sk_bb:.*$"}],
        "notification": {"endpoint":
            {"uri": format!("http://127.0.0.1:{orig_port}/original")}},
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
            .expect("remote id")
            .to_owned()
    };

    // mixed inbound: one matching, one foreign-razidlo id
    let inbound = json!({
        "id": "urn:ngsi-ld:Notification:idr1",
        "type": "Notification",
        "subscriptionId": remote_id,
        "notifiedAt": "2026-08-15T12:00:00Z",
        "data": [
            {"id": "urn:ngsi-ld:Vehicle:sk_bb:1", "type": "Vehicle",
             "speed": {"type": "Property", "value": 1}},
            {"id": "urn:ngsi-ld:Vehicle:sk_po:1", "type": "Vehicle",
             "speed": {"type": "Property", "value": 2}},
        ],
    });
    let (status, body) = send(
        &st,
        "POST",
        "/ngsi-ld/ex/remote-notify",
        Some(inbound.to_string()),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    wait_for("the filtered notification at the subscriber", || {
        orig_seen
            .lock()
            .expect("seen")
            .iter()
            .any(|r| r.contains("urn:ngsi-ld:Vehicle:sk_bb:1"))
    })
    .await;
    assert!(
        !orig_seen
            .lock()
            .expect("seen")
            .iter()
            .any(|r| r.contains("urn:ngsi-ld:Vehicle:sk_po:1")),
        "an id outside the selector pattern must reach the subscriber in NO payload"
    );

    // an inbound carrying ONLY foreign ids is acknowledged but never forwarded
    let before = orig_seen.lock().expect("seen").len();
    let inbound = json!({
        "id": "urn:ngsi-ld:Notification:idr2",
        "type": "Notification",
        "subscriptionId": remote_id,
        "notifiedAt": "2026-08-15T12:01:00Z",
        "data": [{"id": "urn:ngsi-ld:Vehicle:sk_po:2", "type": "Vehicle",
                  "speed": {"type": "Property", "value": 3}}],
    });
    let (status, _) = send(
        &st,
        "POST",
        "/ngsi-ld/ex/remote-notify",
        Some(inbound.to_string()),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    tokio::time::sleep(std::time::Duration::from_millis(1200)).await;
    assert_eq!(
        orig_seen.lock().expect("seen").len(),
        before,
        "a notification with no selector-matching entity is not forwarded"
    );
}

/// 6.3.17: "the Context Broker shall add itself to the Via header" — the
/// forwarded Subscription copy (5.8.1.4) travels with the inbound chain
/// EXTENDED by this broker's alias, so downstream brokers can detect loops.
#[tokio::test(flavor = "multi_thread")]
async fn clause_6_3_18_forwarded_copy_extends_the_via_chain() {
    allow_private();
    std::env::set_var("ANTARES_PUBLIC_URL", "http://127.0.0.1:9999");
    let mut st = AppState::new("antares-via-ext".into());
    antares_api::notify::wire(&mut st);
    let (remote_port, remote_seen) = recording_mock_head();
    let reg = json!({
        "id": "urn:ngsi-ld:ContextSourceRegistration:viaext",
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
    // the Subscription itself arrives as a forwarded copy: one upstream hop
    let sub = json!({
        "id": "urn:ngsi-ld:Subscription:viaext",
        "type": "Subscription",
        "entities": [{"type": "Vehicle"}],
        "notification": {"endpoint": {"uri": "http://127.0.0.1:9998/original"}},
    });
    let (status, body) = send_via(
        &st,
        "POST",
        "/ngsi-ld/v1/subscriptions",
        Some(sub.to_string()),
        Some("1.1 peer-upstream"),
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
    let head = {
        let seen = remote_seen.lock().expect("seen");
        seen.iter()
            .find(|r| r.starts_with("POST /ngsi-ld/v1/subscriptions"))
            .expect("post")
            .to_ascii_lowercase()
    };
    let via_line = head
        .lines()
        .find(|l| l.starts_with("via:"))
        .expect("the forwarded copy must carry a Via header")
        .to_owned();
    let up = via_line.find("peer-upstream");
    let own = via_line.find("antares-via-ext");
    assert!(
        up.is_some() && own.is_some() && up < own,
        "the chain must keep the upstream hop and append this broker: {via_line}"
    );
}

/// 6.3.18: the Via header exists "to avoid infinite loops". A forwarded
/// Subscription copy whose chain already names THIS broker has looped back
/// (two mutually registered brokers, 5.8.1.4): it is stored and serves
/// locally, but produces NO further forwarded copy — otherwise each round
/// trip creates two more Subscriptions without bound.
#[tokio::test(flavor = "multi_thread")]
async fn clause_6_3_18_a_looped_copy_is_not_reforwarded() {
    allow_private();
    std::env::set_var("ANTARES_PUBLIC_URL", "http://127.0.0.1:9999");
    let mut st = AppState::new("antares-via-loop".into());
    antares_api::notify::wire(&mut st);
    let (remote_port, remote_seen) = recording_mock();
    let reg = json!({
        "id": "urn:ngsi-ld:ContextSourceRegistration:vialoop",
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
    // the chain already names this broker: the copy has come full circle
    let looped = json!({
        "id": "urn:ngsi-ld:Subscription:vialoop",
        "type": "Subscription",
        "entities": [{"type": "Vehicle"}],
        "notification": {"endpoint": {"uri": "http://127.0.0.1:9998/original"}},
    });
    let (status, body) = send_via(
        &st,
        "POST",
        "/ngsi-ld/v1/subscriptions",
        Some(looped.to_string()),
        Some("1.1 antares-via-loop"),
    )
    .await;
    assert_eq!(
        status,
        StatusCode::CREATED,
        "the looped copy still serves locally: {body}"
    );
    // positive control second: a fresh Subscription with no chain forwards,
    // which also brackets the wait — by the time the control's copy arrives,
    // a copy of the looped one would have arrived too
    let fresh = json!({
        "id": "urn:ngsi-ld:Subscription:vialoop-fresh",
        "type": "Subscription",
        "entities": [{"type": "Vehicle"}],
        "notification": {"endpoint": {"uri": "http://127.0.0.1:9998/original"}},
    });
    let (status, body) = send(
        &st,
        "POST",
        "/ngsi-ld/v1/subscriptions",
        Some(fresh.to_string()),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{body}");
    wait_for("the control subscription's forwarded copy", || {
        remote_seen
            .lock()
            .expect("seen")
            .iter()
            .any(|r| r.starts_with("POST /ngsi-ld/v1/subscriptions"))
    })
    .await;
    let posts: Vec<String> = remote_seen
        .lock()
        .expect("seen")
        .iter()
        .filter(|r| r.starts_with("POST /ngsi-ld/v1/subscriptions"))
        .cloned()
        .collect();
    assert!(
        !posts.iter().any(|r| r.contains("Vehicle")) || posts.len() == 1,
        "only the control may forward: {posts:?}"
    );
    assert!(
        !posts.iter().any(|r| {
            r.split("\n\n").nth(1).is_some_and(|b| {
                serde_json::from_str::<Value>(b).is_ok_and(|v| {
                    v["notification"]["endpoint"]["uri"]
                        .as_str()
                        .is_some_and(|u| u.contains("9999"))
                        && posts.len() > 1
                })
            })
        }),
        "the looped copy must not re-forward: {posts:?}"
    );
    assert_eq!(posts.len(), 1, "exactly the control's copy: {posts:?}");
}

/// Table 6.3.18-2: the Via listing "is used when determining matching
/// registrations" — a registration whose contextSourceAlias is already in
/// the chain the Subscription travelled through receives no copy, while a
/// registration naming a different source still does.
#[tokio::test(flavor = "multi_thread")]
async fn clause_6_3_18_registration_already_in_the_via_chain_receives_no_copy() {
    allow_private();
    std::env::set_var("ANTARES_PUBLIC_URL", "http://127.0.0.1:9999");
    let mut st = AppState::new("antares-via-reg".into());
    antares_api::notify::wire(&mut st);
    let (origin_port, origin_seen) = recording_mock();
    let (other_port, other_seen) = recording_mock_head();
    for (id, port, alias) in [
        (
            "urn:ngsi-ld:ContextSourceRegistration:viareg-origin",
            origin_port,
            Some("peer-origin"),
        ),
        (
            "urn:ngsi-ld:ContextSourceRegistration:viareg-other",
            other_port,
            None,
        ),
    ] {
        let mut reg = json!({
            "id": id,
            "type": "ContextSourceRegistration",
            "information": [{"entities": [{"type": "Vehicle"}]}],
            "operations": ["federationOps"],
            "endpoint": format!("http://127.0.0.1:{port}"),
        });
        if let Some(a) = alias {
            reg["contextSourceAlias"] = json!(a);
        }
        let (status, body) = send(
            &st,
            "POST",
            "/ngsi-ld/v1/csourceRegistrations",
            Some(reg.to_string()),
        )
        .await;
        assert_eq!(status, StatusCode::CREATED, "{body}");
    }
    let sub = json!({
        "id": "urn:ngsi-ld:Subscription:viareg",
        "type": "Subscription",
        "entities": [{"type": "Vehicle"}],
        "notification": {"endpoint": {"uri": "http://127.0.0.1:9998/original"}},
    });
    let (status, body) = send_via(
        &st,
        "POST",
        "/ngsi-ld/v1/subscriptions",
        Some(sub.to_string()),
        Some("1.1 peer-origin"),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{body}");
    wait_for("the other registration's forwarded copy", || {
        other_seen
            .lock()
            .expect("seen")
            .iter()
            .any(|r| r.starts_with("POST /ngsi-ld/v1/subscriptions"))
    })
    .await;
    assert!(
        !origin_seen
            .lock()
            .expect("seen")
            .iter()
            .any(|r| r.starts_with("POST /ngsi-ld/v1/subscriptions")),
        "the source the copy came through must not receive it back"
    );
    // and the copy the other registration received extends the chain
    let head = {
        let seen = other_seen.lock().expect("seen");
        seen.iter()
            .find(|r| r.starts_with("POST /ngsi-ld/v1/subscriptions"))
            .expect("post")
            .to_ascii_lowercase()
    };
    let via_line = head
        .lines()
        .find(|l| l.starts_with("via:"))
        .expect("the forwarded copy must carry a Via header")
        .to_owned();
    assert!(
        via_line.contains("peer-origin") && via_line.contains("antares-via-reg"),
        "the chain keeps the inbound hop and appends this broker: {via_line}"
    );
}

/// 5.8.6 gates an inbound Notification on the origin Context Source matching
/// the Subscription's csf, and 5.8.1.4 fixes the terms that filter is written
/// in: "the @context to be used for … this Subscription shall be the one
/// specified in the jsonldContext field". 5.11.2.4 already reads the csf in
/// that @context when it decides which sources the Subscription is forwarded
/// to. Reading it in the core context on the way back in makes the two
/// disagree: the broker subscribes to a source and then drops everything it
/// sends, with a 200 and no warning.
#[tokio::test(flavor = "multi_thread")]
async fn clause_5_8_6_the_inbound_csf_is_read_in_the_subscriptions_own_context() {
    allow_private();
    let mut st = AppState::new("antares-distsub-csfctx".into());
    antares_api::notify::wire(&mut st);
    let (sensor_port, sensor_seen) = recording_mock();
    // The Context Source Property as the source itself spells it.
    let (status, body) = send(
        &st,
        "POST",
        "/ngsi-ld/v1/csourceRegistrations",
        Some(
            json!({
                "id": "urn:ngsi-ld:ContextSourceRegistration:csfctx-sensor",
                "type": "ContextSourceRegistration",
                "information": [{"entities": [{"type": "Vehicle"}]}],
                "operations": ["federationOps"],
                "endpoint": format!("http://127.0.0.1:{sensor_port}"),
                "srcType": {"type": "Property", "value": "sensor"},
            })
            .to_string(),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{body}");

    // The subscriber spells the same Property its own way; its @context maps
    // both spellings onto one IRI, which is what makes the filter match.
    let (orig_port, orig_seen) = recording_mock();
    let (status, body) = send_ld(
        &st,
        "POST",
        "/ngsi-ld/v1/subscriptions",
        json!({
            "@context": {
                "sourceType": "https://uri.etsi.org/ngsi-ld/default-context/srcType",
            },
            "id": "urn:ngsi-ld:Subscription:csfctx-own",
            "type": "Subscription",
            "entities": [{"type": "Vehicle"}],
            "csf": "sourceType==\"sensor\"",
            "notification": {"endpoint": {"uri": format!("http://127.0.0.1:{orig_port}/notify")}},
        })
        .to_string(),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{body}");

    // 5.11.2.4 read the csf in that @context and decided this source belongs
    // to the Subscription: its copy is at the source.
    wait_for("the forwarded remote subscription", || {
        sensor_seen
            .lock()
            .expect("seen")
            .iter()
            .any(|r| r.starts_with("POST /ngsi-ld/v1/subscriptions"))
    })
    .await;
    let remote_id: String = {
        let seen = sensor_seen.lock().expect("seen");
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

    let (status, _) = send(
        &st,
        "POST",
        "/ngsi-ld/ex/remote-notify",
        Some(
            json!({
                "type": "Notification",
                "subscriptionId": remote_id,
                "data": [{"id": "urn:ngsi-ld:Vehicle:csfctx", "type": "Vehicle"}],
            })
            .to_string(),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    wait_for("the subscriber notification", || {
        !orig_seen.lock().expect("seen").is_empty()
    })
    .await;
}

/// 5.5.4: `"urn:ngsi-ld:null"` as a first level member value is BadRequestData
/// "with the exception of NGSI-LD Fragments … or to represent deleted
/// Properties in concise representation as part of notifications". A Context
/// Source therefore notifies a deletion as a concise NGSI-LD Null, and the
/// 5.8.6 splitEntities merge must carry that Entity through to the original
/// subscriber instead of refusing it as an invalid payload.
#[tokio::test(flavor = "multi_thread")]
async fn clause_5_8_6_split_merge_keeps_a_notified_deletion() {
    allow_private();
    std::env::set_var("ANTARES_PUBLIC_URL", "http://127.0.0.1:9999");
    let mut st = AppState::new("antares-split-null".into());
    antares_api::notify::wire(&mut st);

    let (remote_port, remote_seen) = recording_mock();
    let reg = json!({
        "id": "urn:ngsi-ld:ContextSourceRegistration:split-null-origin",
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

    let (orig_port, orig_seen) = recording_mock();
    let sub = json!({
        "id": "urn:ngsi-ld:Subscription:ds-split-null",
        "type": "Subscription",
        "entities": [{"type": "Vehicle"}],
        "splitEntities": true,
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
    let remote_id = {
        let seen = remote_seen.lock().expect("seen");
        let r = seen
            .iter()
            .find(|r| r.starts_with("POST /ngsi-ld/v1/subscriptions"))
            .expect("post")
            .clone();
        let v: Value = serde_json::from_str(r.split("\n\n").nth(1).expect("body")).expect("json");
        v["id"].as_str().expect("remote id").to_owned()
    };

    // 4.5.7: the Context Source reports the deletion of `speed` and a live
    // value for `brandName` on the same Entity, both concise.
    let inbound = json!({
        "type": "Notification",
        "subscriptionId": remote_id,
        "data": [{
            "id": "urn:ngsi-ld:Vehicle:split-null",
            "type": "Vehicle",
            "speed": "urn:ngsi-ld:null",
            "isParked": {"object": "urn:ngsi-ld:null"},
            "label": {"languageMap": {"@none": "urn:ngsi-ld:null"}},
            "brandName": "Tesla",
        }],
    });
    let (status, _) = send(
        &st,
        "POST",
        "/ngsi-ld/ex/remote-notify",
        Some(inbound.to_string()),
    )
    .await;
    assert_eq!(status, StatusCode::OK);

    wait_for("the notification at the original subscriber", || {
        orig_seen
            .lock()
            .expect("seen")
            .iter()
            .any(|r| r.contains("urn:ngsi-ld:Vehicle:split-null"))
    })
    .await;
    let delivered = orig_seen
        .lock()
        .expect("seen")
        .iter()
        .find(|r| r.contains("urn:ngsi-ld:Vehicle:split-null"))
        .expect("notification")
        .clone();
    let notif: Value =
        serde_json::from_str(delivered.split("\n\n").nth(1).expect("body")).expect("json");
    let ent = &notif["data"][0];
    // 4.5.7 / 5.5.4: all three deleted forms reach the subscriber, each in
    // the encoding its Attribute type mandates.
    assert_eq!(
        ent["speed"],
        json!({"type": "Property", "value": "urn:ngsi-ld:null"}),
        "the deleted Property must reach the subscriber: {notif}"
    );
    assert_eq!(
        ent["isParked"],
        json!({"type": "Relationship", "object": "urn:ngsi-ld:null"}),
        "the deleted Relationship must reach the subscriber: {notif}"
    );
    assert_eq!(
        ent["label"],
        json!({"type": "LanguageProperty", "languageMap": {"@none": "urn:ngsi-ld:null"}}),
        "a deleted Language Property is the map form, never a bare string: {notif}"
    );
    assert_eq!(
        ent["brandName"]["value"],
        json!("Tesla"),
        "the live attribute on the same Entity must survive: {notif}"
    );
    // nothing else rode along: no internal member, no attribute the Context
    // Source never notified.
    let mut keys: Vec<&str> = ent
        .as_object()
        .expect("entity")
        .keys()
        .map(String::as_str)
        .collect();
    keys.sort_unstable();
    assert_eq!(
        keys,
        ["brandName", "id", "isParked", "label", "speed", "type"],
        "the merged Entity carries exactly what was notified: {notif}"
    );
}

/// 5.11.5.4 lists "all the existing Context Source Registration
/// Subscriptions" — the ones a Context Source Subscriber created through
/// 5.11.2. The Registration Subscription 5.8.1.4 creates for a distributed
/// entity Subscription is not one of those: no client asked for it, and it
/// carries the internal `urn:antares:distsub:` endpoint naming the tenant
/// and the owning Subscription. On the 5.11 endpoints it is a client
/// resource like any other, so a subscriber can read it, patch isActive to
/// false, or DELETE it — silently disabling the distributed half of a
/// Subscription that keeps reporting status "active", and reading the
/// Subscription ids of every other subscriber in the tenant out of the
/// listing.
#[tokio::test(flavor = "multi_thread")]
async fn clause_5_11_5_the_internal_registration_subscription_is_not_a_client_resource() {
    allow_private();
    let st = AppState::new("antares-internal-csr".into());
    antares_api::notify::wire(&mut st.clone());
    let own = "urn:ngsi-ld:Subscription:internal-csr-owner";
    let (status, _) = send(
        &st,
        "POST",
        "/ngsi-ld/v1/subscriptions",
        Some(
            json!({
                "id": own,
                "type": "Subscription",
                "entities": [{"type": "Building"}],
                "notification": {"endpoint": {"uri": "http://127.0.0.1:9/notify"}},
            })
            .to_string(),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);

    let t = antares_model::TenantId::default();
    let csr = st
        .store
        .get(&t, antares_store::Kind::DistSub, own)
        .expect("mapping read")
        .expect("the distributed half stores a mapping")
        .get("csr_sub")
        .and_then(Value::as_str)
        .expect("the mapping names the internal Registration Subscription")
        .to_owned();

    let (status, body) = send(&st, "GET", "/ngsi-ld/v1/csourceSubscriptions", None).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        body,
        json!([]),
        "the client created no Context Source Registration Subscription: {body}"
    );
    let text = body.to_string();
    assert!(
        !text.contains("urn:antares:") && !text.contains(own),
        "the internal endpoint and the owning Subscription id must not leak: {text}"
    );

    let path = format!("/ngsi-ld/v1/csourceSubscriptions/{csr}");
    for (method, payload) in [
        ("GET", None),
        ("PATCH", Some(json!({"isActive": false}).to_string())),
        ("DELETE", None),
    ] {
        let (status, _) = send(&st, method, &path, payload).await;
        assert_eq!(
            status,
            StatusCode::NOT_FOUND,
            "{method} on the internal Registration Subscription must be 404"
        );
    }

    assert!(
        st.store
            .get(&t, antares_store::Kind::DistSub, &csr)
            .expect("read")
            .is_some(),
        "the distributed half survives every client request aimed at it"
    );
}

/// Same as [`send`], under one tenant (4.14).
async fn send_tenant(
    st: &AppState,
    method: &str,
    path: &str,
    body: Option<String>,
    tenant: &str,
) -> (StatusCode, Value) {
    let mut b = Request::builder()
        .method(method)
        .uri(path)
        .header("NGSILD-Tenant", tenant);
    if body.is_some() {
        b = b.header("Content-Type", "application/json");
    }
    let req = match body {
        Some(body) => b
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

/// 5.8.1.4 stores "the mapping of the received subscriptionId with the own
/// Subscription identifier … to enable forwarding received notifications to
/// the original subscriber", and 4.14 keeps every tenant's data apart. The
/// endpoint the forwarded copies notify is one URL for every tenant, so the
/// stored mapping is the ONLY thing that may decide which subscriber a
/// notification reaches: a peer that sends the tenant header of a different
/// tenant — or none — must not move one tenant's notification into another
/// tenant's subscription.
#[tokio::test(flavor = "multi_thread")]
async fn clause_5_8_1_4_an_inbound_notification_is_routed_by_the_mapping_alone() {
    allow_private();
    let mut st = AppState::new("antares-ds-tenants".into());
    antares_api::notify::wire(&mut st);
    let (remote_port, remote_seen) = recording_mock();
    let (alpha_port, alpha_seen) = recording_mock();
    let (beta_port, beta_seen) = recording_mock();

    for (tenant, etype, cb) in [
        ("alpha", "Vehicle", alpha_port),
        ("beta", "Building", beta_port),
    ] {
        let (status, body) = send_tenant(
            &st,
            "POST",
            "/ngsi-ld/v1/csourceRegistrations",
            Some(
                json!({
                    "id": format!("urn:ngsi-ld:ContextSourceRegistration:{tenant}"),
                    "type": "ContextSourceRegistration",
                    "information": [{"entities": [{"type": etype}]}],
                    "operations": ["federationOps"],
                    "endpoint": format!("http://127.0.0.1:{remote_port}"),
                })
                .to_string(),
            ),
            tenant,
        )
        .await;
        assert_eq!(status, StatusCode::CREATED, "{body}");
        let (status, body) = send_tenant(
            &st,
            "POST",
            "/ngsi-ld/v1/subscriptions",
            Some(
                json!({
                    "id": format!("urn:ngsi-ld:Subscription:{tenant}"),
                    "type": "Subscription",
                    "entities": [{"type": etype}],
                    "notification": {"endpoint": {"uri": format!("http://127.0.0.1:{cb}/cb")}},
                })
                .to_string(),
            ),
            tenant,
        )
        .await;
        assert_eq!(status, StatusCode::CREATED, "{body}");
    }

    wait_for("both forwarded copies", || {
        remote_seen.lock().expect("seen").len() >= 2
    })
    .await;
    // each copy carries the entity type of the registration it was reduced
    // to, which is what tells the two tenants' remote ids apart
    let alpha_remote = {
        let seen = remote_seen.lock().expect("seen");
        let raw = seen
            .iter()
            .find(|r| r.contains("Vehicle"))
            .expect("the copy forwarded for alpha")
            .clone();
        let body: Value = serde_json::from_str(raw.split_once("\n\n").expect("body").1)
            .expect("the forwarded copy is JSON");
        body["id"].as_str().expect("remote id").to_owned()
    };

    // the peer answers with alpha's subscriptionId while claiming to be beta
    let (status, body) = send_tenant(
        &st,
        "POST",
        "/ngsi-ld/ex/remote-notify",
        Some(
            json!({
                "type": "Notification",
                "subscriptionId": alpha_remote,
                "data": [{"id": "urn:ngsi-ld:Vehicle:slip", "type": "Vehicle"}],
            })
            .to_string(),
        ),
        "beta",
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");

    wait_for("the notification alpha subscribed to", || {
        !alpha_seen.lock().expect("seen").is_empty()
    })
    .await;
    assert!(
        alpha_seen
            .lock()
            .expect("seen")
            .iter()
            .any(|r| r.contains("urn:ngsi-ld:Vehicle:slip")),
        "the mapping names alpha, so alpha is the subscriber notified"
    );
    let beta = beta_seen.lock().expect("seen").clone();
    assert!(
        beta.is_empty(),
        "a forged tenant header must not move a notification between tenants: {beta:?}"
    );
}
