// SPDX-License-Identifier: EUPL-1.2
//! Durable state across restarts: snapshots (5.16), EntityMaps (5.14) and
//! distributed-subscription mappings (5.8.1.4) live in the store
//! (Kind::Snapshot / Kind::EntityMap / Kind::DistSub / Kind::DeadLetter) — reopening a
//! file-mode store serves them again (pg/timescale get the same via the
//! shared doc-kind path, exercised by CI).
#![allow(clippy::unwrap_used)]

use antares_api::AppState;
use antares_sql::store::any::AnyStore;
use antares_sql::store::Store;
use axum::body::Body;
use axum::http::{Request, StatusCode};
use serde_json::{json, Value};
use std::sync::Arc;
use tower::ServiceExt;

async fn send_h(
    st: &AppState,
    method: &str,
    path: &str,
    body: Option<String>,
    extra: &[(&str, &str)],
) -> (StatusCode, axum::http::HeaderMap, Value) {
    let mut b = Request::builder().method(method).uri(path);
    for (k, v) in extra {
        b = b.header(*k, *v);
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
    let headers = res.headers().clone();
    let bytes = axum::body::to_bytes(res.into_body(), usize::MAX)
        .await
        .expect("body");
    let body = if bytes.is_empty() {
        Value::Null
    } else {
        serde_json::from_slice(&bytes).unwrap_or(Value::Null)
    };
    (status, headers, body)
}

fn file_state(dir: &std::path::Path) -> AppState {
    let store = Store::open_file(dir).expect("open file store");
    let mut st = AppState::with_store(
        "antares-durable".into(),
        Arc::new(AnyStore::Mem(store)),
        antares_sql::StoreMode::File,
    );
    antares_api::notify::wire(&mut st);
    st
}

/// The notify change hook captures the AppState, which holds the store —
/// an Arc cycle that would keep the redb lock forever. A real broker exits
/// the process; the test simulates the restart by breaking the cycle and
/// waiting for the lock to free.
fn shutdown(st: AppState) {
    st.store.set_change_hook(Box::new(|_, _, _| {}));
    let store = st.store.clone();
    drop(st);
    for _ in 0..100 {
        if Arc::strong_count(&store) == 1 {
            return;
        }
        std::thread::sleep(std::time::Duration::from_millis(50));
    }
}

fn scratch_dir(label: &str) -> std::path::PathBuf {
    let d = std::env::temp_dir().join(format!("antares-durable-{label}-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&d).expect("mkdir");
    d
}

async fn wait_ready(st: &AppState, loc: &str) -> Value {
    for _ in 0..100 {
        let (status, _, body) = send_h(st, "GET", loc, None, &[]).await;
        assert_eq!(status, StatusCode::OK, "{body}");
        if body["snapshotStatus"] != "preparing" {
            return body;
        }
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    }
    panic!("snapshot never left preparing");
}

fn rt() -> tokio::runtime::Runtime {
    tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .enable_all()
        .build()
        .expect("runtime")
}

/// A snapshot (metadata AND frozen data) plus an EntityMap survive a
/// close-and-reopen of the file store. Plain #[test]: the first tokio
/// runtime (and the background sweepers holding the state) is DROPPED to
/// simulate the process exit, then a second runtime reopens the store.
#[test]
fn snapshot_and_entity_map_survive_restart() {
    let dir = scratch_dir("snap");
    let first = rt();
    let (sid, loc, map_id) = first.block_on(async {
        let st = file_state(&dir);
        let body = json!({"id": "urn:ngsi-ld:Vehicle:d1", "type": "Vehicle",
            "speed": {"type": "Property", "value": 80}})
        .to_string();
        let (status, _, b) = send_h(&st, "POST", "/ngsi-ld/v1/entities", Some(body), &[]).await;
        assert_eq!(status, StatusCode::CREATED, "{b}");

        let snap = json!({"type": "Snapshot",
            "snapshotQueries": [{"type": "Query", "entities": [{"type": "Vehicle"}]}]})
        .to_string();
        let (status, h, b) = send_h(&st, "POST", "/ngsi-ld/v1/snapshots", Some(snap), &[]).await;
        assert_eq!(status, StatusCode::CREATED, "{b}");
        let loc = h.get("Location").unwrap().to_str().unwrap().to_owned();
        let ready = wait_ready(&st, &loc).await;
        assert_eq!(ready["snapshotStatus"], "success", "{ready}");
        let sid = ready["id"].as_str().expect("id").to_owned();

        let (status, h, b) =
            send_h(&st, "GET", "/ngsi-ld/v1/entityMaps?type=Vehicle", None, &[]).await;
        assert_eq!(status, StatusCode::CREATED, "{b}");
        // the header carries the map's LOCATION (6.4.3.2-2) — id is the last segment
        let map_id = h
            .get("NGSILD-EntityMap")
            .and_then(|v| v.to_str().ok())
            .map(|r| r.rsplit('/').next().unwrap_or(r).to_owned())
            .or_else(|| b.get("id").and_then(Value::as_str).map(str::to_owned))
            .expect("entity map id");
        let out = (sid, loc, map_id);
        shutdown(st);
        out
    });
    drop(first);

    // "restart": a fresh AppState over the same directory
    let second = rt();
    second.block_on(async {
        let st2 = file_state(&dir);
        let (status, _, meta) = send_h(&st2, "GET", &loc, None, &[]).await;
        assert_eq!(status, StatusCode::OK, "snapshot metadata survives: {meta}");
        assert_eq!(meta["snapshotStatus"], "success", "{meta}");

        let (status, _, list) = send_h(
            &st2,
            "GET",
            "/ngsi-ld/v1/entities?type=Vehicle",
            None,
            &[("NGSILD-Snapshot", &sid)],
        )
        .await;
        assert_eq!(status, StatusCode::OK, "{list}");
        assert_eq!(
            list.as_array().map(Vec::len),
            Some(1),
            "frozen data survives: {list}"
        );

        let (status, _, m) = send_h(
            &st2,
            "GET",
            &format!("/ngsi-ld/v1/entityMaps/{map_id}"),
            None,
            &[],
        )
        .await;
        assert_eq!(status, StatusCode::OK, "entity map survives: {m}");
    });

    let _ = std::fs::remove_dir_all(&dir);
}

/// The distributed-subscription mappings survive a restart: an inbound
/// remote notification with the pre-restart remote id still resolves
/// (before the promotion this was a guaranteed 404).
#[test]
fn dist_sub_mapping_survives_restart() {
    std::env::set_var("ANTARES_EGRESS_ALLOW_PRIVATE", "true");
    let dir = scratch_dir("distsub");

    let first = rt();
    let remote_id = first.block_on(async {
        // remote broker mock: replies 201, records subscription bodies
        let (tx, mut rx) = tokio::sync::mpsc::channel::<Value>(4);
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind");
        let addr = listener.local_addr().expect("addr");
        let app = axum::Router::new().route(
            "/ngsi-ld/v1/subscriptions",
            axum::routing::post(move |body: axum::body::Bytes| {
                let tx = tx.clone();
                async move {
                    let v: Value = serde_json::from_slice(&body).unwrap_or(Value::Null);
                    let _ = tx.send(v).await;
                    (StatusCode::CREATED, "")
                }
            }),
        );
        tokio::spawn(async move {
            axum::serve(listener, app).await.expect("serve");
        });

        {
            let st = file_state(&dir);
            let reg = json!({
                "id": "urn:ngsi-ld:ContextSourceRegistration:durable",
                "type": "ContextSourceRegistration",
                "information": [{"entities": [{"type": "Vehicle"}]}],
                "operations": ["federationOps"],
                "endpoint": format!("http://{addr}"),
            });
            let (status, _, b) = send_h(
                &st,
                "POST",
                "/ngsi-ld/v1/csourceRegistrations",
                Some(reg.to_string()),
                &[],
            )
            .await;
            assert_eq!(status, StatusCode::CREATED, "{b}");
            let sub = json!({
                "id": "urn:ngsi-ld:Subscription:durable",
                "type": "Subscription",
                "entities": [{"type": "Vehicle"}],
                "notification": {"endpoint": {"uri": "http://127.0.0.1:9998/original"}},
            });
            let (status, _, b) = send_h(
                &st,
                "POST",
                "/ngsi-ld/v1/subscriptions",
                Some(sub.to_string()),
                &[],
            )
            .await;
            assert_eq!(status, StatusCode::CREATED, "{b}");
            let copy = tokio::time::timeout(std::time::Duration::from_secs(10), rx.recv())
                .await
                .expect("forwarded subscription within 10s")
                .expect("one copy");
            let rid = copy["id"].as_str().expect("remote id").to_owned();
            shutdown(st);
            rid
        }
    });
    drop(first);

    let second = rt();
    second.block_on(async {
        let st2 = file_state(&dir);
        let inbound = json!({
            "type": "Notification",
            "subscriptionId": remote_id,
            "data": [{"id": "urn:ngsi-ld:Vehicle:afterboot", "type": "Vehicle"}],
        });
        let (status, _, b) = send_h(
            &st2,
            "POST",
            "/ngsi-ld/ex/remote-notify",
            Some(inbound.to_string()),
            &[],
        )
        .await;
        assert_eq!(
            status,
            StatusCode::OK,
            "the mapping must survive the restart: {b}"
        );
        // negative: an id that was never mapped is still 404
        let (status, _, _) = send_h(
            &st2,
            "POST",
            "/ngsi-ld/ex/remote-notify",
            Some(
                json!({"type": "Notification", "subscriptionId": "urn:nope", "data": []})
                    .to_string(),
            ),
            &[],
        )
        .await;
        assert_eq!(status, StatusCode::NOT_FOUND);
    });

    let _ = std::fs::remove_dir_all(&dir);
}

/// A dead letter is durable state like the other doc kinds: a restart must
/// not lose the notification an operator still wants to replay.
#[test]
fn dead_letter_survives_restart() {
    let dir = scratch_dir("deadletter");
    let first = rt();
    first.block_on(async {
        let st = file_state(&dir);
        let t = antares_model::TenantId::new("acme").expect("tenant");
        assert!(st
            .store
            .create(
                &t,
                antares_store::Kind::DeadLetter,
                "urn:ngsi-ld:DeadLetter:1",
                json!({"id": "urn:ngsi-ld:DeadLetter:1", "type": "DeadLetter",
                       "subscriptionId": "urn:s:1", "uri": "http://127.0.0.1:9/n",
                       "binding": "http", "headers": [], "payload": {}, "attempts": 3,
                       "lastAt": "2026-01-01T00:00:00Z"}),
            )
            .expect("create"));
        shutdown(st);
    });
    drop(first);
    let second = rt();
    second.block_on(async {
        let st2 = file_state(&dir);
        let (status, _, b) = send_h(&st2, "GET", "/q/dead-letters?tenant=acme", None, &[]).await;
        assert_eq!(status, StatusCode::OK, "{b}");
        assert_eq!(b[0]["id"], "urn:ngsi-ld:DeadLetter:1", "{b}");
        assert_eq!(b[0]["attempts"], 3);
        let (status, _, b) = send_h(&st2, "GET", "/q/dead-letters?tenant=other", None, &[]).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(b, json!([]), "tenant scoping survives too");
    });
    let _ = std::fs::remove_dir_all(&dir);
}
