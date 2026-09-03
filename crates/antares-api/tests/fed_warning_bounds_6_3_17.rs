// SPDX-License-Identifier: EUPL-1.2
//! How much of a Context Source's `NGSILD-Warning` list this broker relays.
//!
//! 6.3.17: "NGSILD-Warning HTTP headers shall also be used to indicate
//! instances of abnormal behaviour for distributed HTTP GET operations
//! performed over the resources /entities and /entities/{entity-id}", and
//! Table 6.3.17-1 defines four codes for it. A source's own values are
//! relayed because 4.3.6.4 puts the abnormality of a deeper hop on this
//! response — but the list is written by the source, and every value in it
//! becomes a header of an answer this broker sends to a client that never
//! addressed that source.
//!
//! Two properties hold that in place: what one source contributes is
//! bounded, and the warning this broker is REQUIRED to raise about a source
//! cannot be crowded out by another source's list.

use antares_api::AppState;
use axum::body::Body;
use axum::http::{Request, StatusCode};
use serde_json::json;
use std::io::{Read, Write};
use tower::ServiceExt;

/// A Context Source answering `[]` with `n` warnings of its own.
fn mock_source(n: usize, tag: &str) -> u16 {
    let mut reply = String::from("HTTP/1.1 200 OK\r\nContent-Type: application/json\r\n");
    for i in 0..n {
        reply.push_str(&format!(
            "NGSILD-Warning: 199 {tag}.example \"{tag} {i}\"\r\n"
        ));
    }
    reply.push_str("Content-Length: 2\r\nConnection: close\r\n\r\n[]");
    let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("bind");
    let port = listener.local_addr().expect("addr").port();
    std::thread::spawn(move || {
        for stream in listener.incoming() {
            let Ok(mut s) = stream else { continue };
            let mut buf = [0u8; 65536];
            let _ = s.read(&mut buf);
            let _ = s.write_all(reply.as_bytes());
        }
    });
    port
}

/// A port nothing listens on: the forward fails to connect, which Table
/// 6.3.17-1 classifies as 199 and the broker MUST report.
fn dead_port() -> u16 {
    let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("bind");
    let port = listener.local_addr().expect("addr").port();
    drop(listener);
    port
}

async fn register(st: &AppState, port: u16, tag: &str) {
    let body = json!({
        "id": format!("urn:ngsi-ld:ContextSourceRegistration:warn-{tag}-{port}"),
        "type": "ContextSourceRegistration",
        "mode": "inclusive",
        "operations": ["queryEntity"],
        "information": [{"entities": [{"type": "Vehicle"}]}],
        "endpoint": format!("http://127.0.0.1:{port}"),
    })
    .to_string();
    let res = antares_api::router(st.clone())
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/ngsi-ld/v1/csourceRegistrations")
                .header("Content-Type", "application/json")
                .header("Content-Length", body.len())
                .body(Body::from(body))
                .expect("request"),
        )
        .await
        .expect("response");
    assert_eq!(res.status(), StatusCode::CREATED, "registration create");
}

/// The distributed GET 6.3.17 names, and every warning value on its answer.
async fn query_warnings(st: &AppState) -> Vec<String> {
    let res = antares_api::router(st.clone())
        .oneshot(
            Request::builder()
                .method("GET")
                .uri("/ngsi-ld/v1/entities?type=Vehicle")
                .body(Body::empty())
                .expect("request"),
        )
        .await
        .expect("response");
    assert_eq!(res.status(), StatusCode::OK);
    res.headers()
        .get_all("NGSILD-Warning")
        .iter()
        .filter_map(|v| v.to_str().ok().map(str::to_owned))
        .collect()
}

/// One source cannot make the response grow faster than the fan-out itself
/// does. Its list is relayed up to the cap and truncated after it; the
/// values kept are the first ones, so a source's own outcome (which it
/// states before the ones it relays) is the part that survives.
#[tokio::test(flavor = "multi_thread")]
async fn one_source_cannot_flood_the_response_with_warnings() {
    antares_jsonld::allow_private_egress(true);
    let st = AppState::new("antares-warnb".into());
    let cap = antares_api::bounds::MAX_PEER_WARNINGS;
    register(&st, mock_source(cap + 40, "flood"), "flood").await;

    let got = query_warnings(&st).await;
    assert_eq!(
        got.len(),
        cap,
        "a source relayed {} warnings onto this broker's response: {got:?}",
        got.len()
    );
    assert!(
        got[0].contains("flood 0"),
        "the truncation kept the tail instead of the head: {got:?}"
    );
}

/// The clause makes the 199 about an unreachable source a SHALL. A second
/// source's list is a payload, and a payload does not get to suppress it.
#[tokio::test(flavor = "multi_thread")]
async fn a_flooding_source_cannot_crowd_out_the_brokers_own_warning() {
    antares_jsonld::allow_private_egress(true);
    let st = AppState::new("antares-warnb".into());
    let cap = antares_api::bounds::MAX_PEER_WARNINGS;
    register(&st, mock_source(cap + 40, "loud"), "loud").await;
    register(&st, dead_port(), "dead").await;

    let got = query_warnings(&st).await;
    assert!(
        got.iter().any(|w| w.contains("antares-warnb")),
        "this broker's own 199 about the unreachable source is missing: {got:?}"
    );
    assert_eq!(
        got.iter().filter(|w| w.contains("loud.example")).count(),
        cap,
        "the loud source stayed over its cap: {got:?}"
    );
}

/// A source under the cap is relayed whole — the bound is a ceiling, not a
/// quota, and a cascade of a few hops still reaches the client intact.
#[tokio::test(flavor = "multi_thread")]
async fn a_source_within_the_cap_is_relayed_whole() {
    antares_jsonld::allow_private_egress(true);
    let st = AppState::new("antares-warnb".into());
    let n = antares_api::bounds::MAX_PEER_WARNINGS - 1;
    register(&st, mock_source(n, "quiet"), "quiet").await;

    let got = query_warnings(&st).await;
    assert_eq!(got.len(), n, "a cascade's warnings were dropped: {got:?}");
}
