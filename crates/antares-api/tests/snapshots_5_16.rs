// SPDX-License-Identifier: EUPL-1.2
//! 5.16 Snapshots (optional API group, resources 6.36–6.38, scoping
//! 6.3.22): Create/Clone/Retrieve status/Update status/Delete/Purge, the
//! 5.2.41 Snapshot data type gates, background query execution into an
//! isolated copy, NGSILD-Snapshot-scoped Core/Temporal operations, and the
//! 5.3.4 SnapshotNotification.
#![allow(clippy::unwrap_used)]

use antares_api::AppState;
use axum::body::Body;
use axum::http::{Request, StatusCode};
use serde_json::{json, Value};
use tower::ServiceExt;

/// set_var once: a sibling test reading the env while another rewrites it
/// saw the policy missing and refused the loopback forward (TSan flake).
fn allow_private() {
    static ONCE: std::sync::Once = std::sync::Once::new();
    ONCE.call_once(|| std::env::set_var("ANTARES_EGRESS_ALLOW_PRIVATE", "true"));
}

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

async fn send(
    st: &AppState,
    method: &str,
    path: &str,
    body: Option<String>,
) -> (StatusCode, Value) {
    let (s, _, b) = send_h(st, method, path, body, &[]).await;
    (s, b)
}

async fn create_vehicle(st: &AppState, id: &str, speed: i64) {
    let body = json!({"id": id, "type": "Vehicle",
        "speed": {"type": "Property", "value": speed}})
    .to_string();
    let (status, b) = send(st, "POST", "/ngsi-ld/v1/entities", Some(body)).await;
    assert_eq!(status, StatusCode::CREATED, "{b}");
}

/// Poll the snapshot until it leaves "preparing" (the fill runs in the
/// background per 5.16.1.4).
async fn wait_ready(st: &AppState, loc: &str) -> Value {
    for _ in 0..100 {
        let (status, body) = send(st, "GET", loc, None).await;
        assert_eq!(status, StatusCode::OK, "{body}");
        if body["snapshotStatus"] != "preparing" {
            return body;
        }
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    }
    panic!("snapshot never left preparing");
}

fn state() -> AppState {
    let mut st = AppState::new("antares-snap".into());
    antares_api::notify::wire(&mut st);
    st
}

/// 5.16.1.4 + 6.36.3.1: create → 201 + Location; status preparing → success;
/// 5.2.41 output members (priority default 5, expiresAt, per-query details).
#[tokio::test(flavor = "multi_thread")]
async fn clause_5_16_1_create_executes_queries_into_the_snapshot() {
    let st = state();
    create_vehicle(&st, "urn:ngsi-ld:Vehicle:s1", 80).await;
    create_vehicle(&st, "urn:ngsi-ld:Vehicle:s2", 30).await;

    let snap = json!({"type": "Snapshot",
        "snapshotQueries": [{"type": "Query", "entities": [{"type": "Vehicle"}], "q": "speed>50"}]})
    .to_string();
    let (status, headers, body) =
        send_h(&st, "POST", "/ngsi-ld/v1/snapshots", Some(snap), &[]).await;
    assert_eq!(status, StatusCode::CREATED, "{body}");
    let loc = headers
        .get("Location")
        .and_then(|v| v.to_str().ok())
        .expect("Location header")
        .to_owned();
    assert!(loc.contains("/ngsi-ld/v1/snapshots/"), "{loc}");

    let ready = wait_ready(&st, &loc).await;
    assert_eq!(ready["snapshotStatus"], "success", "{ready}");
    assert_eq!(ready["type"], "Snapshot");
    assert_eq!(ready["snapshotPriority"], 5, "default priority: {ready}");
    assert!(ready["expiresAt"].is_string(), "{ready}");
    assert_eq!(
        ready["snapshotQueriesDetails"][0]["resultStatus"], "success",
        "{ready}"
    );

    // 6.3.22: the snapshot header scopes the query to the frozen copy —
    // only the matching entity (s1) is inside, and the header is echoed
    let sid = ready["id"].as_str().expect("id");
    let (status, headers, list) = send_h(
        &st,
        "GET",
        "/ngsi-ld/v1/entities?type=Vehicle",
        None,
        &[("NGSILD-Snapshot", sid)],
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{list}");
    assert_eq!(
        headers.get("NGSILD-Snapshot").and_then(|v| v.to_str().ok()),
        Some(sid),
        "6.3.22: header echoed"
    );
    let ids: Vec<&str> = list
        .as_array()
        .expect("array")
        .iter()
        .filter_map(|d| d["id"].as_str())
        .collect();
    assert_eq!(ids, vec!["urn:ngsi-ld:Vehicle:s1"], "{list}");

    // the snapshot is FROZEN: mutating the live entity does not leak in
    let patch = json!({"speed": {"type": "Property", "value": 99}}).to_string();
    let (status, _) = send(
        &st,
        "PATCH",
        "/ngsi-ld/v1/entities/urn:ngsi-ld:Vehicle:s1/attrs",
        Some(patch),
    )
    .await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    let (_, _, frozen) = send_h(
        &st,
        "GET",
        "/ngsi-ld/v1/entities/urn:ngsi-ld:Vehicle:s1",
        None,
        &[("NGSILD-Snapshot", sid)],
    )
    .await;
    assert_eq!(frozen["speed"]["value"], 80, "frozen copy: {frozen}");
}

/// 5.16.1.4: a query with no results → snapshotStatus "empty" with an
/// "empty" ExecutionResultDetails entry.
#[tokio::test(flavor = "multi_thread")]
async fn clause_5_16_1_empty_result_yields_empty_status() {
    let st = state();
    let snap = json!({"type": "Snapshot",
        "snapshotQueries": [{"type": "Query", "entities": [{"type": "Nothing"}]}]})
    .to_string();
    let (status, headers, body) =
        send_h(&st, "POST", "/ngsi-ld/v1/snapshots", Some(snap), &[]).await;
    assert_eq!(status, StatusCode::CREATED, "{body}");
    let loc = headers
        .get("Location")
        .unwrap()
        .to_str()
        .unwrap()
        .to_owned();
    let ready = wait_ready(&st, &loc).await;
    assert_eq!(ready["snapshotStatus"], "empty", "{ready}");
    assert_eq!(
        ready["snapshotQueriesDetails"][0]["resultStatus"], "empty",
        "{ready}"
    );
}

/// 5.2.41 gates: at least one of snapshotQueries/snapshotTemporalQueries;
/// snapshotQueries entries must NOT carry temporalQ; priority is 1..=10.
#[tokio::test(flavor = "multi_thread")]
async fn clause_5_2_41_create_validation() {
    let st = state();
    for bad in [
        json!({"type": "Snapshot"}),
        json!({"type": "Snapshot", "snapshotQueries": [
            {"type": "Query", "entities": [{"type": "V"}],
             "temporalQ": {"timerel": "after", "timeAt": "2020-01-01T00:00:00Z"}}]}),
        json!({"type": "Snapshot", "snapshotPriority": 11,
            "snapshotQueries": [{"type": "Query", "entities": [{"type": "V"}]}]}),
        json!({"type": "NotASnapshot",
            "snapshotQueries": [{"type": "Query", "entities": [{"type": "V"}]}]}),
    ] {
        let (status, body) =
            send(&st, "POST", "/ngsi-ld/v1/snapshots", Some(bad.to_string())).await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "{bad} -> {body}");
        assert_eq!(
            body["type"], "https://uri.etsi.org/ngsi-ld/errors/BadRequestData",
            "{body}"
        );
    }
}

/// 5.16.4: PATCH updates lifetime/priority/endpoint; the read-only
/// snapshotQueries member in a fragment is 400; unknown snapshot 404.
#[tokio::test(flavor = "multi_thread")]
async fn clause_5_16_4_update_status() {
    let st = state();
    create_vehicle(&st, "urn:ngsi-ld:Vehicle:u1", 80).await;
    let snap = json!({"type": "Snapshot",
        "snapshotQueries": [{"type": "Query", "entities": [{"type": "Vehicle"}]}]})
    .to_string();
    let (_, headers, _) = send_h(&st, "POST", "/ngsi-ld/v1/snapshots", Some(snap), &[]).await;
    let loc = headers
        .get("Location")
        .unwrap()
        .to_str()
        .unwrap()
        .to_owned();
    wait_ready(&st, &loc).await;

    let (status, body) = send(
        &st,
        "PATCH",
        &loc,
        Some(json!({"snapshotPriority": 9}).to_string()),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body["snapshotPriority"], 9, "{body}");

    let (status, body) = send(
        &st,
        "PATCH",
        &loc,
        Some(json!({"snapshotQueries": []}).to_string()),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "read-only member: {body}");

    let (status, _) = send(
        &st,
        "PATCH",
        "/ngsi-ld/v1/snapshots/urn:ngsi-ld:snapshot:nope",
        Some(json!({"snapshotPriority": 2}).to_string()),
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}

/// 5.16.2: the clone copies the data; the original's deletion does not
/// touch the clone. Clone bodies must not carry snapshotQueries.
#[tokio::test(flavor = "multi_thread")]
async fn clause_5_16_2_clone_and_5_16_5_delete() {
    let st = state();
    create_vehicle(&st, "urn:ngsi-ld:Vehicle:c1", 80).await;
    let snap = json!({"type": "Snapshot",
        "snapshotQueries": [{"type": "Query", "entities": [{"type": "Vehicle"}]}]})
    .to_string();
    let (_, headers, _) = send_h(&st, "POST", "/ngsi-ld/v1/snapshots", Some(snap), &[]).await;
    let loc = headers
        .get("Location")
        .unwrap()
        .to_str()
        .unwrap()
        .to_owned();
    let ready = wait_ready(&st, &loc).await;
    let sid = ready["id"].as_str().expect("id").to_owned();

    // a clone body with queries is 400
    let (status, _) = send(
        &st,
        "POST",
        &format!("{loc}/clone"),
        Some(
            json!({"type": "Snapshot",
                "snapshotQueries": [{"type": "Query", "entities": [{"type": "V"}]}]})
            .to_string(),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);

    let (status, cheaders, body) = send_h(
        &st,
        "POST",
        &format!("{loc}/clone"),
        Some(json!({"type": "Snapshot"}).to_string()),
        &[],
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{body}");
    let cloc = cheaders
        .get("Location")
        .unwrap()
        .to_str()
        .unwrap()
        .to_owned();
    let clone_ready = wait_ready(&st, &cloc).await;
    assert_eq!(clone_ready["snapshotStatus"], "success", "{clone_ready}");
    let cid = clone_ready["id"].as_str().expect("id").to_owned();
    assert_ne!(cid, sid);

    // delete the ORIGINAL — the clone still serves its frozen copy
    let (status, _) = send(&st, "DELETE", &loc, None).await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    let (status, _) = send(&st, "GET", &loc, None).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    let (status, _, list) = send_h(
        &st,
        "GET",
        "/ngsi-ld/v1/entities?type=Vehicle",
        None,
        &[("NGSILD-Snapshot", &cid)],
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{list}");
    assert_eq!(list.as_array().map(Vec::len), Some(1), "{list}");
    // operations against the deleted original are 404
    let (status, _, _) = send_h(
        &st,
        "GET",
        "/ngsi-ld/v1/entities?type=Vehicle",
        None,
        &[("NGSILD-Snapshot", &sid)],
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}

/// 5.16.7 + 6.36.3.2: purge with a q over Snapshot members deletes only the
/// matching snapshots.
#[tokio::test(flavor = "multi_thread")]
async fn clause_5_16_7_purge_by_query() {
    let st = state();
    create_vehicle(&st, "urn:ngsi-ld:Vehicle:p1", 80).await;
    let mk = |prio: i64| {
        json!({"type": "Snapshot", "snapshotPriority": prio,
            "snapshotQueries": [{"type": "Query", "entities": [{"type": "Vehicle"}]}]})
        .to_string()
    };
    let (_, h1, _) = send_h(&st, "POST", "/ngsi-ld/v1/snapshots", Some(mk(2)), &[]).await;
    let low = h1.get("Location").unwrap().to_str().unwrap().to_owned();
    let (_, h2, _) = send_h(&st, "POST", "/ngsi-ld/v1/snapshots", Some(mk(8)), &[]).await;
    let high = h2.get("Location").unwrap().to_str().unwrap().to_owned();
    wait_ready(&st, &low).await;
    wait_ready(&st, &high).await;

    let (status, body) = send(
        &st,
        "DELETE",
        "/ngsi-ld/v1/snapshots?q=snapshotPriority%3C5",
        None,
    )
    .await;
    assert_eq!(status, StatusCode::NO_CONTENT, "{body}");
    let (status, _) = send(&st, "GET", &low, None).await;
    assert_eq!(status, StatusCode::NOT_FOUND, "low priority purged");
    let (status, _) = send(&st, "GET", &high, None).await;
    assert_eq!(status, StatusCode::OK, "high priority survives");

    // a purge without a q is 400 (5.16.7.4)
    let (status, _) = send(&st, "DELETE", "/ngsi-ld/v1/snapshots", None).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
}

/// 5.16.6 / 5.3.4: with an endpoint set, a SnapshotNotification arrives
/// after the fill, carrying snapshotId + status.
#[tokio::test(flavor = "multi_thread")]
async fn clause_5_16_6_snapshot_notification() {
    allow_private();
    let st = state();
    create_vehicle(&st, "urn:ngsi-ld:Vehicle:n1", 80).await;

    let (tx, mut rx) = tokio::sync::mpsc::channel::<Value>(4);
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind");
    let addr = listener.local_addr().expect("addr");
    let app = axum::Router::new().route(
        "/snapnotify",
        axum::routing::post(move |body: axum::body::Bytes| {
            let tx = tx.clone();
            async move {
                let v: Value = serde_json::from_slice(&body).unwrap_or(Value::Null);
                let _ = tx.send(v).await;
                StatusCode::OK
            }
        }),
    );
    tokio::spawn(async move {
        axum::serve(listener, app).await.expect("serve");
    });

    let snap = json!({"type": "Snapshot",
        "endpoint": format!("http://{addr}/snapnotify"),
        "snapshotQueries": [{"type": "Query", "entities": [{"type": "Vehicle"}]}],
        "snapshotTemporalQueries": [{"type": "Query", "entities": [{"type": "Vehicle"}],
            "temporalQ": {"timerel": "after", "timeAt": "2000-01-01T00:00:00Z",
                          "timeproperty": "createdAt"}}]})
    .to_string();
    let (status, headers, body) =
        send_h(&st, "POST", "/ngsi-ld/v1/snapshots", Some(snap), &[]).await;
    assert_eq!(status, StatusCode::CREATED, "{body}");
    let loc = headers
        .get("Location")
        .unwrap()
        .to_str()
        .unwrap()
        .to_owned();
    let ready = wait_ready(&st, &loc).await;

    let n = tokio::time::timeout(
        std::time::Duration::from_secs(5 * antares_api::state::slow_factor()),
        rx.recv(),
    )
    .await
    .expect("SnapshotNotification within 5s")
    .expect("one notification");
    assert_eq!(n["type"], "SnapshotNotification", "{n}");
    assert_eq!(n["snapshotId"], ready["id"], "{n}");
    assert_eq!(n["snapshotStatus"], "success", "{n}");
    assert!(n["notifiedAt"].is_string(), "{n}");
    assert!(n["expiresAt"].is_string(), "{n}");
    assert_eq!(
        n["snapshotQueriesDetails"][0]["resultStatus"], "success",
        "{n}"
    );
    // 5.3.4 key naming: temporalSnapshotQueriesDetails, NOT the 5.2.41
    // Snapshot-member key — and no snapshotReady member exists.
    assert_eq!(
        n["temporalSnapshotQueriesDetails"][0]["resultStatus"], "success",
        "{n}"
    );
    assert!(n["snapshotTemporalQueriesDetails"].is_null(), "{n}");
    assert!(n["snapshotReady"].is_null(), "{n}");
}

/// 5.16.1.4 temporal: snapshotTemporalQueries feed the snapshot's Temporal
/// API view (NGSILD-Snapshot on GET /temporal/entities).
#[tokio::test(flavor = "multi_thread")]
async fn clause_5_16_1_temporal_queries_fill_the_temporal_view() {
    let st = state();
    create_vehicle(&st, "urn:ngsi-ld:Vehicle:t1", 80).await;

    let snap = json!({"type": "Snapshot",
        "snapshotTemporalQueries": [{"type": "Query", "entities": [{"type": "Vehicle"}],
            "temporalQ": {"timerel": "after", "timeAt": "2000-01-01T00:00:00Z",
                          "timeproperty": "createdAt"}}]})
    .to_string();
    let (status, headers, body) =
        send_h(&st, "POST", "/ngsi-ld/v1/snapshots", Some(snap), &[]).await;
    assert_eq!(status, StatusCode::CREATED, "{body}");
    let loc = headers
        .get("Location")
        .unwrap()
        .to_str()
        .unwrap()
        .to_owned();
    let ready = wait_ready(&st, &loc).await;
    assert_eq!(ready["snapshotStatus"], "success", "{ready}");
    let sid = ready["id"].as_str().expect("id");

    let (status, _, list) = send_h(
        &st,
        "GET",
        "/ngsi-ld/v1/temporal/entities?type=Vehicle&timerel=after&timeAt=2000-01-01T00:00:00Z&timeproperty=createdAt",
        None,
        &[("NGSILD-Snapshot", sid)],
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{list}");
    let ids: Vec<&str> = list
        .as_array()
        .expect("array")
        .iter()
        .filter_map(|d| d["id"].as_str())
        .collect();
    assert_eq!(ids, vec!["urn:ngsi-ld:Vehicle:t1"], "{list}");
}

/// 5.16.1.4: snapshot queries follow 5.7.2.4 — the DISTRIBUTED query
/// behaviour. Entities served by a registered Context Source are part of
/// the snapshot alongside local ones.
#[tokio::test(flavor = "multi_thread")]
async fn clause_5_16_1_federated_fill() {
    allow_private();
    let st = state();
    create_vehicle(&st, "urn:ngsi-ld:Vehicle:fedlocal", 70).await;

    // a context source serving one remote Vehicle on the query path
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind");
    let addr = listener.local_addr().expect("addr");
    let app = axum::Router::new().route(
        "/ngsi-ld/v1/entities",
        axum::routing::get(|| async {
            axum::Json(
                json!([{"id": "urn:ngsi-ld:Vehicle:fedremote", "type": "Vehicle",
                "speed": {"type": "Property", "value": 80}}]),
            )
        }),
    );
    tokio::spawn(async move {
        axum::serve(listener, app).await.expect("serve");
    });
    let reg = json!({
        "id": "urn:ngsi-ld:ContextSourceRegistration:snapfed",
        "type": "ContextSourceRegistration",
        "information": [{"entities": [{"type": "Vehicle"}]}],
        "operations": ["queryEntity"],
        "endpoint": format!("http://{addr}"),
    });
    let (status, body) = send(
        &st,
        "POST",
        "/ngsi-ld/v1/csourceRegistrations",
        Some(reg.to_string()),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{body}");

    let snap = json!({"type": "Snapshot",
        "snapshotQueries": [{"type": "Query", "entities": [{"type": "Vehicle"}], "q": "speed>50"}]})
    .to_string();
    let (_, h, _) = send_h(&st, "POST", "/ngsi-ld/v1/snapshots", Some(snap), &[]).await;
    let loc = h.get("Location").unwrap().to_str().unwrap().to_owned();
    let ready = wait_ready(&st, &loc).await;
    assert_eq!(ready["snapshotStatus"], "success", "{ready}");
    let sid = ready["id"].as_str().expect("id");

    let (status, _, list) = send_h(
        &st,
        "GET",
        "/ngsi-ld/v1/entities?type=Vehicle",
        None,
        &[("NGSILD-Snapshot", sid)],
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{list}");
    let mut ids: Vec<&str> = list
        .as_array()
        .expect("array")
        .iter()
        .filter_map(|d| d["id"].as_str())
        .collect();
    ids.sort_unstable();
    assert_eq!(
        ids,
        vec![
            "urn:ngsi-ld:Vehicle:fedlocal",
            "urn:ngsi-ld:Vehicle:fedremote"
        ],
        "5.7.2.4: the remote entity is part of the snapshot: {list}"
    );
    // the local q still applies: no third entity, no duplicates
    assert_eq!(ids.len(), 2, "{list}");
}

/// 5.16.1.4: "If the size of the respective results require pagination,
/// all pages are to be retrieved completely" — the temporal fill must page
/// past the broker's max_limit.
#[tokio::test(flavor = "multi_thread")]
async fn clause_5_16_1_temporal_fill_pages_past_max_limit() {
    let mut st = state();
    st.max_limit = 2;
    for i in 1..=3 {
        create_vehicle(&st, &format!("urn:ngsi-ld:Vehicle:page{i}"), 60 + i).await;
    }
    let snap = json!({"type": "Snapshot",
        "snapshotTemporalQueries": [{"type": "Query", "entities": [{"type": "Vehicle"}],
            "temporalQ": {"timerel": "after", "timeAt": "2000-01-01T00:00:00Z",
                          "timeproperty": "createdAt"}}]})
    .to_string();
    let (_, h, _) = send_h(&st, "POST", "/ngsi-ld/v1/snapshots", Some(snap), &[]).await;
    let loc = h.get("Location").unwrap().to_str().unwrap().to_owned();
    let ready = wait_ready(&st, &loc).await;
    assert_eq!(ready["snapshotStatus"], "success", "{ready}");
    let sid = ready["id"].as_str().expect("id");

    // the verification itself must page (max_limit is 2)
    let mut ids = std::collections::BTreeSet::new();
    for offset in [0, 2] {
        let (status, _, list) = send_h(
            &st,
            "GET",
            &format!("/ngsi-ld/v1/temporal/entities?type=Vehicle&timerel=after&timeAt=2000-01-01T00:00:00Z&timeproperty=createdAt&limit=2&offset={offset}"),
            None,
            &[("NGSILD-Snapshot", sid)],
        )
        .await;
        assert_eq!(status, StatusCode::OK, "{list}");
        for d in list.as_array().expect("array") {
            ids.insert(d["id"].as_str().expect("id").to_owned());
        }
    }
    assert_eq!(
        ids.len(),
        3,
        "all pages retrieved into the snapshot: {ids:?}"
    );
}

/// 5.5.15: under resource pressure (the per-tenant cap) snapshots are
/// evicted lowest-snapshotPriority-first; higher-priority snapshots
/// survive.
#[tokio::test(flavor = "multi_thread")]
async fn clause_5_5_15_priority_eviction_over_cap() {
    let mut st = state();
    st.snapshot_cap = 2;
    let mk = |prio: i64| {
        json!({"type": "Snapshot", "snapshotPriority": prio,
            "snapshotQueries": [{"type": "Query", "entities": [{"type": "Vehicle"}]}]})
        .to_string()
    };
    let (_, h_hi, _) = send_h(&st, "POST", "/ngsi-ld/v1/snapshots", Some(mk(8)), &[]).await;
    let hi = h_hi.get("Location").unwrap().to_str().unwrap().to_owned();
    let (_, h_lo, _) = send_h(&st, "POST", "/ngsi-ld/v1/snapshots", Some(mk(2)), &[]).await;
    let lo = h_lo.get("Location").unwrap().to_str().unwrap().to_owned();
    let (status, _, _) = send_h(&st, "POST", "/ngsi-ld/v1/snapshots", Some(mk(5)), &[]).await;
    assert_eq!(status, StatusCode::CREATED);
    // the lowest priority (2) was evicted; 8 and 5 survive
    let (status, _) = send(&st, "GET", &lo, None).await;
    assert_eq!(status, StatusCode::NOT_FOUND, "priority 2 evicted");
    let (status, _) = send(&st, "GET", &hi, None).await;
    assert_eq!(status, StatusCode::OK, "priority 8 must survive");
}

/// 5.2.41 Table 5.2.41-2: lastUsedAt is initialized at creation time and
/// refreshed when the snapshot is used via NGSILD-Snapshot (5.5.15); plain
/// status retrieval is not "use".
#[tokio::test(flavor = "multi_thread")]
async fn clause_5_2_41_last_used_at() {
    let st = state();
    create_vehicle(&st, "urn:ngsi-ld:Vehicle:l1", 80).await;
    let snap = json!({"type": "Snapshot",
        "snapshotQueries": [{"type": "Query", "entities": [{"type": "Vehicle"}]}]})
    .to_string();
    let (_, headers, _) = send_h(&st, "POST", "/ngsi-ld/v1/snapshots", Some(snap), &[]).await;
    let loc = headers
        .get("Location")
        .unwrap()
        .to_str()
        .unwrap()
        .to_owned();
    let ready = wait_ready(&st, &loc).await;
    let first = ready["lastUsedAt"]
        .as_str()
        .expect("lastUsedAt initialized at creation")
        .to_owned();
    let sid = ready["id"].as_str().expect("id").to_owned();
    tokio::time::sleep(std::time::Duration::from_millis(5)).await;
    let (status, _, _) = send_h(
        &st,
        "GET",
        "/ngsi-ld/v1/entities?type=Vehicle",
        None,
        &[("NGSILD-Snapshot", &sid)],
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let (_, after_use) = send(&st, "GET", &loc, None).await;
    let used = after_use["lastUsedAt"]
        .as_str()
        .expect("lastUsedAt")
        .to_owned();
    assert!(used > first, "use refreshes lastUsedAt: {first} -> {used}");
}

/// 5.16.7.4: the purge q is restricted to members of the Snapshot data
/// type — any other attribute is BadRequestData and nothing is purged.
#[tokio::test(flavor = "multi_thread")]
async fn clause_5_16_7_purge_q_restricted_to_snapshot_members() {
    let st = state();
    let snap = json!({"type": "Snapshot",
        "snapshotQueries": [{"type": "Query", "entities": [{"type": "Vehicle"}]}]})
    .to_string();
    let (_, h, _) = send_h(&st, "POST", "/ngsi-ld/v1/snapshots", Some(snap), &[]).await;
    let loc = h.get("Location").unwrap().to_str().unwrap().to_owned();
    let (status, body) = send(&st, "DELETE", "/ngsi-ld/v1/snapshots?q=speed%3E50", None).await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
    assert_eq!(
        body["type"], "https://uri.etsi.org/ngsi-ld/errors/BadRequestData",
        "{body}"
    );
    let (status, _) = send(&st, "GET", &loc, None).await;
    assert_eq!(status, StatusCode::OK, "nothing purged");
}

/// 6.3.22: notifications resulting from a subscription created under an
/// NGSILD-Snapshot scope carry the NGSILD-Snapshot header — and the
/// internal synthetic tenant never leaks as NGSILD-Tenant.
#[tokio::test(flavor = "multi_thread")]
async fn clause_6_3_22_snapshot_scoped_subscription_notification_header() {
    allow_private();
    let st = state();
    create_vehicle(&st, "urn:ngsi-ld:Vehicle:sn1", 80).await;
    let snap = json!({"type": "Snapshot",
        "snapshotQueries": [{"type": "Query", "entities": [{"type": "Vehicle"}]}]})
    .to_string();
    let (_, h, _) = send_h(&st, "POST", "/ngsi-ld/v1/snapshots", Some(snap), &[]).await;
    let loc = h.get("Location").unwrap().to_str().unwrap().to_owned();
    let ready = wait_ready(&st, &loc).await;
    let sid = ready["id"].as_str().expect("id").to_owned();

    let (tx, mut rx) = tokio::sync::mpsc::channel::<(axum::http::HeaderMap, Value)>(4);
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind");
    let addr = listener.local_addr().expect("addr");
    let app = axum::Router::new().route(
        "/notify",
        axum::routing::post(
            move |headers: axum::http::HeaderMap, body: axum::body::Bytes| {
                let tx = tx.clone();
                async move {
                    let v: Value = serde_json::from_slice(&body).unwrap_or(Value::Null);
                    let _ = tx.send((headers, v)).await;
                    StatusCode::OK
                }
            },
        ),
    );
    tokio::spawn(async move {
        axum::serve(listener, app).await.expect("serve");
    });

    let sub = json!({"id": "urn:ngsi-ld:Subscription:snap1", "type": "Subscription",
        "entities": [{"type": "Vehicle"}],
        "notification": {"endpoint": {"uri": format!("http://{addr}/notify")}}})
    .to_string();
    let (status, _, body) = send_h(
        &st,
        "POST",
        "/ngsi-ld/v1/subscriptions",
        Some(sub),
        &[("NGSILD-Snapshot", &sid)],
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{body}");

    // a write INSIDE the snapshot triggers the snapshot-scoped subscription
    let patch = json!({"speed": {"type": "Property", "value": 10}}).to_string();
    let (status, _, _) = send_h(
        &st,
        "PATCH",
        "/ngsi-ld/v1/entities/urn:ngsi-ld:Vehicle:sn1/attrs",
        Some(patch),
        &[("NGSILD-Snapshot", &sid)],
    )
    .await;
    assert_eq!(status, StatusCode::NO_CONTENT);

    let (nh, nb) = tokio::time::timeout(
        std::time::Duration::from_secs(5 * antares_api::state::slow_factor()),
        rx.recv(),
    )
    .await
    .expect("notification within 5s")
    .expect("one notification");
    assert_eq!(
        nh.get("NGSILD-Snapshot").and_then(|v| v.to_str().ok()),
        Some(sid.as_str()),
        "6.3.22: NGSILD-Snapshot on the notification"
    );
    let leaked = nh
        .get("NGSILD-Tenant")
        .and_then(|v| v.to_str().ok())
        .is_some_and(|t| t.starts_with("snap-"));
    assert!(!leaked, "synthetic tenant must not leak into NGSILD-Tenant");
    assert_eq!(nb["data"][0]["id"], "urn:ngsi-ld:Vehicle:sn1", "{nb}");

    let _ = loc;
}

/// 5.5.15: Snapshots are orthogonal to Tenants — a snapshot created on a
/// tenant needs BOTH headers, and is invisible from any other tenant.
#[tokio::test(flavor = "multi_thread")]
async fn clause_5_5_15_snapshot_is_tenant_scoped() {
    let st = state();
    let tenant = &[("NGSILD-Tenant", "acme")];
    let body = json!({"id": "urn:ngsi-ld:Vehicle:ten1", "type": "Vehicle",
        "speed": {"type": "Property", "value": 80}})
    .to_string();
    let (status, _, b) = send_h(&st, "POST", "/ngsi-ld/v1/entities", Some(body), tenant).await;
    assert_eq!(status, StatusCode::CREATED, "{b}");

    let snap = json!({"type": "Snapshot",
        "snapshotQueries": [{"type": "Query", "entities": [{"type": "Vehicle"}]}]})
    .to_string();
    let (status, h, b) = send_h(&st, "POST", "/ngsi-ld/v1/snapshots", Some(snap), tenant).await;
    assert_eq!(status, StatusCode::CREATED, "{b}");
    let loc = h.get("Location").unwrap().to_str().unwrap().to_owned();
    for _ in 0..100 {
        let (_, _, s) = send_h(&st, "GET", &loc, None, tenant).await;
        if s["snapshotStatus"] != "preparing" {
            assert_eq!(s["snapshotStatus"], "success", "{s}");
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    }
    let (_, _, ready) = send_h(&st, "GET", &loc, None, tenant).await;
    let sid = ready["id"].as_str().expect("id").to_owned();

    // both headers → the snapshot serves the tenant's frozen copy
    let (status, _, list) = send_h(
        &st,
        "GET",
        "/ngsi-ld/v1/entities?type=Vehicle",
        None,
        &[("NGSILD-Tenant", "acme"), ("NGSILD-Snapshot", &sid)],
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{list}");
    assert_eq!(list.as_array().map(Vec::len), Some(1), "{list}");

    // the snapshot id is INVISIBLE without its tenant (default tenant here)
    let (status, _, _) = send_h(
        &st,
        "GET",
        "/ngsi-ld/v1/entities?type=Vehicle",
        None,
        &[("NGSILD-Snapshot", &sid)],
    )
    .await;
    assert_eq!(
        status,
        StatusCode::NOT_FOUND,
        "other tenant must not see it"
    );
    let (status, _, _) = send_h(&st, "GET", &loc, None, &[]).await;
    assert_eq!(
        status,
        StatusCode::NOT_FOUND,
        "status invisible cross-tenant"
    );
}

/// 6.3.22: an unknown NGSILD-Snapshot reference is ResourceNotFound.
#[tokio::test(flavor = "multi_thread")]
async fn clause_6_3_22_unknown_snapshot_is_404() {
    let st = state();
    let (status, _, body) = send_h(
        &st,
        "GET",
        "/ngsi-ld/v1/entities?type=Vehicle",
        None,
        &[("NGSILD-Snapshot", "urn:ngsi-ld:snapshot:missing")],
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND, "{body}");
}

/// How many snap-index reverse-lookup markers point at `sid`. The markers
/// share Kind::Snapshot storage with the snapshot documents themselves.
fn index_markers(st: &AppState, sid: &str) -> usize {
    let idx = antares_model::TenantId::new("snap-index").expect("tenant");
    st.store
        .list(&idx, antares_sql::store::Kind::Snapshot)
        .unwrap_or_default()
        .iter()
        .filter(|d| d["snapshot"] == sid)
        .count()
}

async fn make_snapshot(st: &AppState, tenant: &[(&str, &str)], prio: i64) -> String {
    let snap = json!({"type": "Snapshot", "snapshotPriority": prio,
        "snapshotQueries": [{"type": "Query", "entities": [{"type": "Vehicle"}]}]})
    .to_string();
    let (status, h, b) = send_h(st, "POST", "/ngsi-ld/v1/snapshots", Some(snap), tenant).await;
    assert_eq!(status, StatusCode::CREATED, "{b}");
    h.get("Location").unwrap().to_str().unwrap().to_owned()
}

/// 5.5.15 resource-pressure eviction is about Snapshots — 5.2.41 fixes
/// their type. The reverse-lookup markers share Kind::Snapshot storage but
/// are not Snapshots: counting them doubles the apparent registry size, and
/// since they carry no snapshotPriority (default 5) and no expiresAt they
/// take victim slots, so snapshots still inside the cap get deleted.
#[tokio::test(flavor = "multi_thread")]
async fn clause_5_5_15_eviction_counts_snapshots_only() {
    let mut st = state();
    st.snapshot_cap = 2;
    // priorities below the markers' default 5 so an unguarded sort puts the
    // snapshots ahead of the markers in the victim order. The markers live
    // under the broker's own snap-index tenant whoever owns the snapshot, and
    // 6.3.14 refuses that tenant to clients, so this drives the default one —
    // a named tenant would have to be created first (5.5.10 answers
    // NonexistentTenant on every non-create operation).
    let t: &[(&str, &str)] = &[];
    let lo = make_snapshot(&st, t, 2).await;
    let mid = make_snapshot(&st, t, 3).await;

    // two snapshots against a cap of two: nothing is over the cap
    for loc in [&lo, &mid] {
        let (status, _, body) = send_h(&st, "GET", loc, None, t).await;
        assert_eq!(
            status,
            StatusCode::OK,
            "5.5.15: only Snapshots count towards the cap: {body}"
        );
    }
    // and the marker itself is not a victim either
    let (_, _, meta) = send_h(&st, "GET", &lo, None, t).await;
    let sid = meta["id"].as_str().expect("id").to_owned();
    assert_eq!(
        index_markers(&st, &sid),
        1,
        "the reverse index survives the eviction pass"
    );
}

/// 5.16.7.4: a purge deletes the Snapshots matching the q. The snap-index
/// markers are not Snapshots (5.2.41 type), so a q that only they can
/// satisfy purges nothing.
#[tokio::test(flavor = "multi_thread")]
async fn clause_5_16_7_purge_spares_the_snapshot_index() {
    let st = state();
    let loc = make_snapshot(&st, &[], 5).await;
    let ready = wait_ready(&st, &loc).await;
    let sid = ready["id"].as_str().expect("id").to_owned();
    assert_eq!(index_markers(&st, &sid), 1, "marker written at creation");

    // Every snapshot carries snapshotPriority (defaulted at creation), so only
    // the markers satisfy this q. The purge runs on the owner tenant because
    // 6.3.14 refuses the broker's own snap- namespace to clients; the markers
    // are out of reach of any purge a client can actually issue.
    let (status, _, body) = send_h(
        &st,
        "DELETE",
        "/ngsi-ld/v1/snapshots?q=%21snapshotPriority",
        None,
        &[],
    )
    .await;
    assert_eq!(status, StatusCode::NO_CONTENT, "{body:?}");
    assert_eq!(
        index_markers(&st, &sid),
        1,
        "5.16.7.4: the reverse index is not purgeable as a snapshot"
    );
    let (status, _) = send(&st, "GET", &loc, None).await;
    assert_eq!(status, StatusCode::OK, "nothing purged on the owner tenant");
}

/// 5.5.15: the resource-pressure cap applies to every new snapshot, so a
/// clone (5.16.2) cannot be used to grow the registry past it — the
/// lowest-snapshotPriority snapshot is evicted exactly as on a create.
#[tokio::test(flavor = "multi_thread")]
async fn clause_5_5_15_clone_respects_the_cap() {
    let mut st = state();
    st.snapshot_cap = 2;
    let mk = |prio: i64| {
        json!({"type": "Snapshot", "snapshotPriority": prio,
            "snapshotQueries": [{"type": "Query", "entities": [{"type": "Vehicle"}]}]})
        .to_string()
    };
    let (_, h_hi, _) = send_h(&st, "POST", "/ngsi-ld/v1/snapshots", Some(mk(8)), &[]).await;
    let hi = h_hi.get("Location").unwrap().to_str().unwrap().to_owned();
    let (_, h_lo, _) = send_h(&st, "POST", "/ngsi-ld/v1/snapshots", Some(mk(2)), &[]).await;
    let lo = h_lo.get("Location").unwrap().to_str().unwrap().to_owned();
    wait_ready(&st, &hi).await;
    wait_ready(&st, &lo).await;

    let (status, ch, body) = send_h(
        &st,
        "POST",
        &format!("{hi}/clone"),
        Some(json!({"type": "Snapshot"}).to_string()),
        &[],
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{body}");
    let cloc = ch.get("Location").unwrap().to_str().unwrap().to_owned();

    let (status, _) = send(&st, "GET", &lo, None).await;
    assert_eq!(
        status,
        StatusCode::NOT_FOUND,
        "5.5.15: the clone is over the cap, priority 2 is evicted"
    );
    let (status, _) = send(&st, "GET", &hi, None).await;
    assert_eq!(status, StatusCode::OK, "priority 8 must survive");
    let (status, _) = send(&st, "GET", &cloc, None).await;
    assert_eq!(status, StatusCode::OK, "the clone is never its own victim");
}
