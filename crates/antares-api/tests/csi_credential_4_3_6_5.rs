// SPDX-License-Identifier: EUPL-1.2
//! A Context Source's credential goes on the wire and nowhere else.
//!
//! 4.3.6.5: `contextSourceInfo` "contains, whatever extra information the
//! Context Broker shall convey when contacting the Context Source. This can
//! be information the Context Broker needs to successfully communicate with
//! the Context Source (e.g. Authorization material)". A deployment behind a
//! provider's proxy puts a bearer token there, and the broker is then
//! holding a credential it was given for one purpose: to open one
//! connection.
//!
//! Everything else the broker writes about that connection is read by
//! somebody who is not the Context Source — an operator reading logs, a
//! client reading a 6.3.17 warning or an error body, whoever can list dead
//! letters, whoever receives the traces. None of them may find the token
//! there. The clause mandates the forward; the rest is this broker's rule,
//! and it holds on the failure paths, which are the ones that print things.
#![cfg(feature = "test-kit")]
#![allow(clippy::unwrap_used)] // an unwrap here is the assertion

use antares_api::AppState;
use axum::body::Body;
use axum::http::{Request, StatusCode};
use http_body_util::BodyExt;
use serde_json::{json, Value};
use std::io::{Read, Write};
use std::sync::{Arc, Mutex, OnceLock};
use tower::ServiceExt;

/// Everything the process logged since the subscriber was installed.
type Log = Arc<Mutex<Vec<u8>>>;

#[derive(Clone)]
struct Capture(Log);

impl Write for Capture {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        if let Ok(mut v) = self.0.lock() {
            v.extend_from_slice(buf);
        }
        Ok(buf.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

impl<'a> tracing_subscriber::fmt::MakeWriter<'a> for Capture {
    type Writer = Capture;
    fn make_writer(&'a self) -> Capture {
        self.clone()
    }
}

/// The one subscriber this binary installs, capturing every event and every
/// span field at every level — a span field is what `tracing-opentelemetry`
/// turns into a trace attribute, so one buffer answers for the logs and for
/// the traces the exporter would send.
fn log() -> &'static Log {
    static LOG: OnceLock<Log> = OnceLock::new();
    LOG.get_or_init(|| {
        let buf: Log = Arc::default();
        let sub = tracing_subscriber::fmt()
            .with_writer(Capture(Arc::clone(&buf)))
            .with_max_level(tracing::Level::TRACE)
            .with_span_events(tracing_subscriber::fmt::format::FmtSpan::FULL)
            .with_ansi(false)
            .finish();
        // Another test binary in the same process would already own this;
        // there is no other, and a failure here would make the assertions
        // vacuous rather than wrong, so it is asserted.
        tracing::subscriber::set_global_default(sub).expect("no subscriber yet");
        buf
    })
}

fn logged() -> String {
    String::from_utf8_lossy(&log().lock().expect("lock").clone()).into_owned()
}

fn state() -> AppState {
    let _ = log();
    antares_jsonld::allow_private_egress(true);
    AppState::new("me".into())
}

/// Everything one peer was sent, request by request, as raw bytes.
type Wire = Arc<Mutex<Vec<String>>>;

/// A peer that answers with `reply` and records what it was asked.
fn peer(reply: String) -> (u16, Wire) {
    let seen: Wire = Arc::default();
    let recorder = Arc::clone(&seen);
    let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("bind");
    let port = listener.local_addr().expect("addr").port();
    std::thread::spawn(move || {
        for stream in listener.incoming() {
            let Ok(mut s) = stream else { continue };
            let mut buf = [0u8; 16384];
            let n = s.read(&mut buf).unwrap_or(0);
            if let Ok(mut v) = recorder.lock() {
                v.push(String::from_utf8_lossy(&buf[..n]).into_owned());
            }
            let _ = s.write_all(reply.as_bytes());
        }
    });
    (port, seen)
}

/// An HTTP response with a body and no extra headers.
fn reply(status: &str, body: &str) -> String {
    format!(
        "HTTP/1.1 {status}\r\nContent-Type: application/json\r\n\
         Content-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    )
}

/// A port nothing is listening on: the forward is refused before it is sent.
fn dead_port() -> u16 {
    let l = std::net::TcpListener::bind("127.0.0.1:0").expect("bind");
    l.local_addr().expect("addr").port()
}

async fn call(st: &AppState, method: &str, path: &str) -> (StatusCode, Vec<String>, Value) {
    let req = Request::builder()
        .method(method)
        .uri(path)
        .body(Body::empty())
        .expect("req");
    let resp = antares_api::router(st.clone())
        .oneshot(req)
        .await
        .expect("resp");
    let status = resp.status();
    let headers = resp
        .headers()
        .iter()
        .map(|(k, v)| format!("{k}: {}", String::from_utf8_lossy(v.as_bytes())))
        .collect();
    let bytes = resp.into_body().collect().await.expect("body").to_bytes();
    let doc = if bytes.is_empty() {
        Value::Null
    } else {
        serde_json::from_slice(&bytes).unwrap_or(Value::Null)
    };
    (status, headers, doc)
}

/// A Context Source over Vehicles whose `contextSourceInfo` carries the
/// credential the provider's proxy demands.
async fn register(st: &AppState, id: &str, port: u16, token: &str) {
    let payload = json!({
        "id": id,
        "type": "ContextSourceRegistration",
        "mode": "inclusive",
        "operations": ["queryEntity"],
        "information": [{"entities": [{"type": "Vehicle"}]}],
        "contextSourceInfo": [{"key": "Authorization", "value": token}],
        "endpoint": format!("http://127.0.0.1:{port}"),
    })
    .to_string();
    let req = Request::builder()
        .method("POST")
        .uri("/ngsi-ld/v1/csourceRegistrations")
        .header("Content-Type", "application/json")
        .header("Content-Length", payload.len())
        .body(Body::from(payload))
        .expect("req");
    let resp = antares_api::router(st.clone())
        .oneshot(req)
        .await
        .expect("resp");
    assert_eq!(resp.status(), StatusCode::CREATED);
}

/// 4.3.6.5: the credential is conveyed when contacting the Context Source.
/// Asserted first so the tests below cannot pass by the broker having
/// dropped the header altogether.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn the_credential_is_conveyed_on_the_forward() {
    let secret = "Bearer csi-travels-8c1f";
    let st = state();
    let (port, seen) = peer(reply("200 OK", "[]"));
    register(&st, "urn:ngsi-ld:ContextSourceRegistration:e3a", port, secret).await;

    let (code, _, body) = call(&st, "GET", "/ngsi-ld/v1/entities?type=Vehicle").await;
    assert_eq!(code, StatusCode::OK, "{body}");
    let asked = seen.lock().expect("lock").clone();
    assert!(
        asked.iter().any(|r| r.contains(secret)),
        "4.3.6.5 credential never reached the Context Source: {asked:?}"
    );
}

/// The failure paths are the ones that print: a source that answers 500
/// raises a 6.3.17 warning on the response, and a source that cannot be
/// reached at all is logged. Neither may carry the token, and neither may
/// leave it on an admin surface.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_failing_forward_writes_the_credential_nowhere() {
    let secret = "Bearer csi-stays-home-4d7e";
    let st = state();
    let (port, seen) = peer(reply("500 Internal Server Error", r#"{"detail":"upstream"}"#));
    register(&st, "urn:ngsi-ld:ContextSourceRegistration:e3b", port, secret).await;
    let dead = dead_port();
    register(&st, "urn:ngsi-ld:ContextSourceRegistration:e3c", dead, secret).await;

    let (code, headers, body) = call(&st, "GET", "/ngsi-ld/v1/entities?type=Vehicle").await;
    assert!(code.is_success() || code.is_server_error(), "{code}");
    assert!(
        seen.lock().expect("lock").iter().any(|r| r.contains(secret)),
        "the forward that was supposed to fail never carried the credential"
    );
    // 6.3.17: the source did answer, so the response says so
    assert!(
        headers.iter().any(|h| h.starts_with("ngsild-warning")),
        "no warning was raised for a failing source: {headers:?}"
    );
    for h in &headers {
        assert!(!h.contains(secret), "credential in a response header: {h}");
    }
    assert!(
        !body.to_string().contains(secret),
        "credential in the response body: {body}"
    );

    for path in [
        "/q/dead-letters",
        "/q/health",
        "/q/metrics",
        "/ngsi-ld/v1/entities?type=Vehicle&options=count",
    ] {
        let (_, headers, body) = call(&st, "GET", path).await;
        assert!(
            !body.to_string().contains(secret),
            "credential served by {path}: {body}"
        );
        for h in &headers {
            assert!(!h.contains(secret), "credential in a {path} header: {h}");
        }
    }

    // Non-vacuity: the subscriber has to have SEEN the failing forward,
    // or a silent buffer would assert nothing at all.
    let text = logged();
    assert!(
        text.contains(&format!("127.0.0.1:{dead}")),
        "the forward to the unreachable source was never traced, so this proves nothing"
    );
    assert!(
        !text.contains(secret),
        "credential in the log or a span field"
    );
}
