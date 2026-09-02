// SPDX-License-Identifier: EUPL-1.2
//! Federation response byte cap (the input bounds wall applied to the 5.7.2.4
//! forwarded-query read path). A registered Context Source answering with a
//! payload above ANTARES_MAX_FED_RESPONSE_BYTES is treated exactly like one
//! whose "payload of the response was invalid" (Table 6.3.17-1, warning 111):
//! the part is skipped, the request still succeeds, NGSILD-Warning is set.
//!
//! The cap is set to 2 KiB for this whole test binary (env read once).

use antares_api::AppState;
use axum::body::Body;
use axum::http::Request;
use std::io::{Read, Write};
use tower::ServiceExt;

const REMOTE_ID: &str = "urn:ngsi-ld:Vehicle:remote-big";

/// One-shot mock Context Source replying `reply` verbatim to every request.
fn mock_replying(reply: String) -> u16 {
    let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("bind");
    let port = listener.local_addr().expect("addr").port();
    std::thread::spawn(move || {
        for stream in listener.incoming() {
            let Ok(mut s) = stream else { continue };
            let mut buf = [0u8; 8192];
            let _ = s.read(&mut buf);
            let _ = s.write_all(reply.as_bytes());
        }
    });
    port
}

fn entity_array(padding: usize) -> String {
    serde_json::json!([{
        "id": REMOTE_ID,
        "type": "Vehicle",
        "note": {"type": "Property", "value": "x".repeat(padding)},
    }])
    .to_string()
}

fn reply_with_length(body: &str) -> String {
    format!(
        "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\n\
         Content-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    )
}

/// No Content-Length: the body is EOF-delimited, so the cap can only be
/// enforced while reading, not from the declared length.
fn reply_without_length(body: &str) -> String {
    format!(
        "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\n\
         Connection: close\r\n\r\n{body}"
    )
}

fn state() -> AppState {
    antares_jsonld::allow_private_egress(true);
    std::env::set_var("ANTARES_MAX_FED_RESPONSE_BYTES", "2048");
    AppState::new("antares1".into())
}

async fn register_query_source(st: &AppState, port: u16) {
    let body = serde_json::json!({
        "id": format!("urn:ngsi-ld:ContextSourceRegistration:cap-{port}"),
        "type": "ContextSourceRegistration",
        "mode": "inclusive",
        "operations": ["queryEntity"],
        "information": [{"entities": [{"type": "Vehicle"}]}],
        "endpoint": format!("http://127.0.0.1:{port}"),
    })
    .to_string();
    let req = Request::builder()
        .method("POST")
        .uri("/ngsi-ld/v1/csourceRegistrations")
        .header("Content-Type", "application/json")
        .header("Content-Length", body.len())
        .body(Body::from(body))
        .expect("request");
    let res = antares_api::router(st.clone())
        .oneshot(req)
        .await
        .expect("response");
    assert_eq!(res.status(), 201, "registration create");
}

async fn query_vehicles(st: &AppState) -> (axum::http::response::Parts, String) {
    let req = Request::builder()
        .method("GET")
        .uri("/ngsi-ld/v1/entities?type=Vehicle")
        .body(Body::empty())
        .expect("request");
    let res = antares_api::router(st.clone())
        .oneshot(req)
        .await
        .expect("response");
    let (parts, body) = res.into_parts();
    let bytes = axum::body::to_bytes(body, usize::MAX).await.expect("body");
    (parts, String::from_utf8_lossy(&bytes).into_owned())
}

/// Control: a small remote payload flows through this harness — proving that
/// when the oversized tests below see no remote entity, the cap (and not a
/// broken fixture) removed it.
#[tokio::test(flavor = "multi_thread")]
async fn small_remote_response_is_merged() {
    let st = state();
    let port = mock_replying(reply_with_length(&entity_array(16)));
    register_query_source(&st, port).await;

    let (parts, body) = query_vehicles(&st).await;
    assert_eq!(parts.status, 200);
    assert!(
        body.contains(REMOTE_ID),
        "control remote entity must appear: {body}"
    );
}

/// 6.3.17: an over-cap response with a declared Content-Length is skipped
/// before the body is read; the part fails with warning 111 and the remote
/// entity must NOT appear in the result.
#[tokio::test(flavor = "multi_thread")]
async fn oversized_declared_response_is_skipped_with_warning() {
    let st = state();
    let port = mock_replying(reply_with_length(&entity_array(8 * 1024)));
    register_query_source(&st, port).await;

    let (parts, body) = query_vehicles(&st).await;
    assert_eq!(parts.status, 200, "the request itself still succeeds");
    assert!(
        !body.contains(REMOTE_ID),
        "over-cap remote payload must not be merged: {body}"
    );
    let warn = parts
        .headers
        .get("NGSILD-Warning")
        .and_then(|v| v.to_str().ok())
        .unwrap_or_default();
    assert!(
        warn.starts_with("111 "),
        "warning 111 expected, got {warn:?}"
    );
}

/// Same, without Content-Length (EOF-delimited body): the cap must hold while
/// reading, not only from the declared length.
#[tokio::test(flavor = "multi_thread")]
async fn oversized_undeclared_response_is_skipped_with_warning() {
    let st = state();
    let port = mock_replying(reply_without_length(&entity_array(8 * 1024)));
    register_query_source(&st, port).await;

    let (parts, body) = query_vehicles(&st).await;
    assert_eq!(parts.status, 200);
    assert!(
        !body.contains(REMOTE_ID),
        "over-cap remote payload must not be merged: {body}"
    );
    let warn = parts
        .headers
        .get("NGSILD-Warning")
        .and_then(|v| v.to_str().ok())
        .unwrap_or_default();
    assert!(
        warn.starts_with("111 "),
        "warning 111 expected, got {warn:?}"
    );
}

/// 6.3.17 across a cascade (4.3.6.4): a registered Context Source that is
/// itself a broker reports its own abnormal parts with `NGSILD-Warning`. Those
/// values travel on to the client with the aggregated response — a warning
/// dropped at the first hop hides a failure two hops away behind a 200.
#[tokio::test(flavor = "multi_thread")]
async fn peer_warnings_reach_the_client() {
    let st = state();
    let body = entity_array(16);
    let reply = format!(
        "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\n\
         NGSILD-Warning: 299 downstream \"an error response was received \
         from the registration endpoint\"\r\n\
         Content-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    );
    let port = mock_replying(reply);
    register_query_source(&st, port).await;

    let (parts, out) = query_vehicles(&st).await;
    assert_eq!(parts.status, 200);
    assert!(
        out.contains(REMOTE_ID),
        "a usable payload is still merged: {out}"
    );
    let warns: Vec<String> = parts
        .headers
        .get_all("NGSILD-Warning")
        .iter()
        .filter_map(|v| v.to_str().ok().map(str::to_owned))
        .collect();
    assert!(
        warns.iter().any(|w| w.contains("downstream")),
        "the peer's own warning must reach the client: {warns:?}"
    );
}
