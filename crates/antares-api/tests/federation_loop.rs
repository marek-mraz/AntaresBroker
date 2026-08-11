//! Loop detection over the Via header (6.3.17 / 6.3.18, Tables 5.2.9-1 and
//! 5.2.40-1), end to end through the router against a mock Context Source.
//!
//! The scenario is ETSI `D018_01`: register a source, create through it, read
//! the Via pseudonym the broker sent, then replay it on a DELETE. What the
//! spec makes of that replay depends on the registration:
//!
//! * a single **exclusive/redirect** source looping back is the one case
//!   6.3.17 p.278 answers with **508 Loop Detected**;
//! * an **inclusive** source is dropped from matching instead (Table 6.3.18-2)
//!   and the operation runs locally — 508 would fail an operation the broker
//!   can serve;
//! * the same chain in **another tenant** is not a loop at all: Table 5.2.40-1
//!   makes the alias tenant-specific, so `antares1` and `antares1~zvolen` are
//!   different Context Sources.

use antares_api::AppState;
use axum::body::Body;
use axum::http::{Request, StatusCode};
use std::io::{Read, Write};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use tower::ServiceExt;

const ALIAS: &str = "antares1";
const ENTITY: &str = "urn:ngsi-ld:Vehicle:d018";

/// A canned Context Source: replies `reply` to every request, counts hits,
/// and keeps the head of the last request it saw (for header assertions).
struct Mock {
    port: u16,
    hits: Arc<AtomicUsize>,
    last_head: Arc<std::sync::Mutex<String>>,
}

fn mock_replying(reply: &'static str) -> Mock {
    let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("bind");
    let port = listener.local_addr().expect("addr").port();
    let hits: Arc<AtomicUsize> = Arc::default();
    let last_head: Arc<std::sync::Mutex<String>> = Arc::default();
    let (seen, head) = (hits.clone(), last_head.clone());
    std::thread::spawn(move || {
        for stream in listener.incoming() {
            let Ok(mut s) = stream else { continue };
            seen.fetch_add(1, Ordering::SeqCst);
            let mut buf = [0u8; 8192];
            let n = s.read(&mut buf).unwrap_or(0);
            *head.lock().expect("lock") = String::from_utf8_lossy(&buf[..n])
                .split("\r\n\r\n")
                .next()
                .unwrap_or_default()
                .to_owned();
            let _ = s.write_all(reply.as_bytes());
        }
    });
    Mock {
        port,
        hits,
        last_head,
    }
}

/// A Context Source that answers every request `204` and counts the hits.
fn mock_source() -> (u16, Arc<AtomicUsize>) {
    let m = mock_replying("HTTP/1.1 204 No Content\r\nContent-Length: 0\r\n\r\n");
    (m.port, m.hits)
}

/// A Context Source that accepts, reads, and never answers — the 6.3.17
/// "fails to respond in time" source. The socket is held open so the
/// client's total timeout (8 s), not a connection reset, ends the wait.
fn mock_stalling() -> u16 {
    let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("bind");
    let port = listener.local_addr().expect("addr").port();
    std::thread::spawn(move || {
        let mut held = Vec::new();
        for stream in listener.incoming() {
            let Ok(mut s) = stream else { continue };
            let mut buf = [0u8; 8192];
            let _ = s.read(&mut buf);
            held.push(s); // keep the connection open, never reply
        }
    });
    port
}

async fn send(st: &AppState, req: Request<Body>) -> axum::http::Response<Body> {
    antares_api::router(st.clone())
        .oneshot(req)
        .await
        .expect("response")
}

/// Register one Context Source for `ENTITY` in `tenant`.
async fn register(st: &AppState, tenant: Option<&str>, mode: &str, port: u16, alias: Option<&str>) {
    let mut doc = serde_json::json!({
        "id": format!("urn:ngsi-ld:ContextSourceRegistration:{mode}-{}", alias.unwrap_or("none")),
        "type": "ContextSourceRegistration",
        "mode": mode,
        "operations": ["redirectionOps"],
        "information": [{"entities": [{"type": "Vehicle", "id": ENTITY}]}],
        "endpoint": format!("http://127.0.0.1:{port}"),
    });
    if let Some(a) = alias {
        doc["contextSourceAlias"] = serde_json::json!(a);
    }
    let body = doc.to_string();
    let mut req = Request::builder()
        .method("POST")
        .uri("/ngsi-ld/v1/csourceRegistrations")
        .header("Content-Type", "application/json")
        .header("Content-Length", body.len());
    if let Some(t) = tenant {
        req = req.header("NGSILD-Tenant", t);
    }
    let res = send(st, req.body(Body::from(body)).expect("request")).await;
    assert_eq!(res.status(), StatusCode::CREATED, "registration create");
}

fn delete_with_via(tenant: Option<&str>, via: &str) -> Request<Body> {
    let mut req = Request::builder()
        .method("DELETE")
        .uri(format!("/ngsi-ld/v1/entities/{ENTITY}"))
        .header("Via", via);
    if let Some(t) = tenant {
        req = req.header("NGSILD-Tenant", t);
    }
    req.body(Body::empty()).expect("request")
}

fn state() -> AppState {
    // the mock source is loopback, denied by default (§16.4)
    std::env::set_var("ANTARES_EGRESS_ALLOW_PRIVATE", "true");
    AppState::new(ALIAS.into())
}

/// 6.3.17 p.278: "In the case of an exclusive or redirect registration, where
/// all of the data is held outside of the Context Broker and held in a single
/// registered source … 508 Loop Detected — if the single registered source
/// and tenant is registered to redirect back on to the Context Broker."
#[tokio::test(flavor = "multi_thread")]
async fn single_redirect_source_looping_back_is_508() {
    let st = state();
    let (port, _) = mock_source();
    register(&st, None, "redirect", port, None).await;

    let res = send(&st, delete_with_via(None, "1.1 antares1")).await;
    assert_eq!(res.status(), StatusCode::LOOP_DETECTED);
}

/// The same loop through an **inclusive** registration: Table 6.3.18-2 makes
/// the Via listing amend matching, so the forward is dropped and the
/// operation proceeds locally. (ETSI D018_01 registers `mode=inclusive` and
/// still asserts 508 — logged in error.md as a suite defect.)
#[tokio::test(flavor = "multi_thread")]
async fn inclusive_source_looping_back_runs_locally() {
    let st = state();
    let (port, hits) = mock_source();
    register(&st, None, "inclusive", port, None).await;

    let res = send(&st, delete_with_via(None, "1.1 antares1")).await;
    assert_ne!(
        res.status(),
        StatusCode::LOOP_DETECTED,
        "508 is scoped to a single exclusive/redirect source"
    );
    assert_eq!(
        hits.load(Ordering::SeqCst),
        0,
        "the looping registration must not be forwarded to"
    );
}

/// Table 5.2.40-1: "In the multi-tenancy use case … this id shall be
/// identifying a specific Tenant within a registered Context Source." A chain
/// naming this broker's DEFAULT tenant says nothing about tenant `zvolen`, so
/// federation there proceeds — one alias for the whole process turned every
/// cross-tenant federation inside one broker into a phantom loop.
#[tokio::test(flavor = "multi_thread")]
async fn a_chain_naming_another_tenant_is_not_a_loop() {
    let st = state();
    let (port, hits) = mock_source();
    register(&st, Some("zvolen"), "redirect", port, None).await;

    let res = send(&st, delete_with_via(Some("zvolen"), "1.1 antares1")).await;
    assert_ne!(
        res.status(),
        StatusCode::LOOP_DETECTED,
        "antares1 (default tenant) is not antares1~zvolen"
    );
    assert_eq!(hits.load(Ordering::SeqCst), 1, "the forward must happen");

    // …and the tenant-qualified pseudonym IS detected as the loop it is
    let res = send(&st, delete_with_via(Some("zvolen"), "1.1 antares1~zvolen")).await;
    assert_eq!(res.status(), StatusCode::LOOP_DETECTED);
    assert_eq!(hits.load(Ordering::SeqCst), 1, "no second forward");
}

/// Table 6.3.18-2 + 5.2.9: a registration carrying the `contextSourceAlias`
/// of a source already in the chain is not a matching registration — the
/// request has been there. Nothing is forwarded, and it is not this broker's
/// own loop, so no 508 either.
#[tokio::test(flavor = "multi_thread")]
async fn a_registered_source_already_in_the_chain_is_skipped() {
    let st = state();
    let (port, hits) = mock_source();
    register(&st, None, "redirect", port, Some("peer1")).await;

    let res = send(&st, delete_with_via(None, "1.1 peer1")).await;
    assert_ne!(res.status(), StatusCode::LOOP_DETECTED);
    assert_eq!(
        hits.load(Ordering::SeqCst),
        0,
        "a source already in the Via chain must not be contacted again"
    );
}

/// RFC 7230 §3.2.2: Via is a list header — the chain may be split across any
/// number of `Via:` field lines and the forms are equivalent. A pseudonym in
/// the SECOND field is still a loop; reading only the first field would run
/// the cycle undetected.
#[tokio::test(flavor = "multi_thread")]
async fn via_split_across_header_fields_still_detects_the_loop() {
    let st = state();
    let (port, _) = mock_source();
    register(&st, None, "redirect", port, None).await;

    let req = Request::builder()
        .method("DELETE")
        .uri(format!("/ngsi-ld/v1/entities/{ENTITY}"))
        .header("Via", "1.1 upstream")
        .header("Via", "1.1 antares1") // second FIELD, same header
        .body(Body::empty())
        .expect("request");
    let res = send(&st, req).await;
    assert_eq!(res.status(), StatusCode::LOOP_DETECTED);
}

/// The outbound chain preserves EVERY inbound Via field before appending our
/// own pseudonym — truncating it would delete the hop history downstream
/// brokers use for their own loop detection (6.3.18 Table 6.3.18-2).
#[tokio::test(flavor = "multi_thread")]
async fn forwarding_preserves_all_inbound_via_fields() {
    let st = state();
    let m = mock_replying("HTTP/1.1 204 No Content\r\nContent-Length: 0\r\n\r\n");
    register(&st, None, "inclusive", m.port, None).await;

    let req = Request::builder()
        .method("DELETE")
        .uri(format!("/ngsi-ld/v1/entities/{ENTITY}"))
        .header("Via", "1.1 up1")
        .header("Via", "1.1 up2")
        .body(Body::empty())
        .expect("request");
    send(&st, req).await;

    assert_eq!(m.hits.load(Ordering::SeqCst), 1, "forward must happen");
    let head = m.last_head.lock().expect("lock").clone();
    assert!(
        head.contains("Via: 1.1 up1, 1.1 up2, 1.1 antares1"),
        "both inbound fields + our pseudonym, in order — got:\n{head}"
    );
}

/// Malformed Via elements must neither panic nor false-positive, and an
/// element carrying an RFC 7230 comment — `1.1 antares1 (proxy)` — is still
/// our pseudonym (received-by is the token before the comment).
#[tokio::test(flavor = "multi_thread")]
async fn malformed_via_elements_are_tolerated() {
    let st = state();
    let (port, hits) = mock_source();
    register(&st, None, "redirect", port, None).await;

    // junk chain, nothing matches → not a loop, forward proceeds
    let res = send(&st, delete_with_via(None, ", ,   1.1, ;;; (x)")).await;
    assert_ne!(res.status(), StatusCode::LOOP_DETECTED);
    assert_eq!(hits.load(Ordering::SeqCst), 1);

    // comment after the pseudonym is not part of the token
    let res = send(&st, delete_with_via(None, "1.1 antares1 (proxy)")).await;
    assert_eq!(res.status(), StatusCode::LOOP_DETECTED);
}

/// 6.3.17 p.278, single proxied source: "404 Not Found — if resources not
/// found within the single registered source." The source's 404 IS the
/// operation's 404.
#[tokio::test(flavor = "multi_thread")]
async fn single_proxy_source_404_passes_through() {
    let st = state();
    let m = mock_replying(
        "HTTP/1.1 404 Not Found\r\nContent-Type: application/json\r\nContent-Length: 2\r\n\r\n{}",
    );
    register(&st, None, "redirect", m.port, None).await;

    let req = Request::builder()
        .method("DELETE")
        .uri(format!("/ngsi-ld/v1/entities/{ENTITY}"))
        .body(Body::empty())
        .expect("request");
    let res = send(&st, req).await;
    assert_eq!(res.status(), StatusCode::NOT_FOUND);
}

/// 6.3.17 p.278, single proxied source: "502 Bad Gateway — if the single
/// forwarded request fails for any other reason such as the Context Broker
/// itself having insufficient access rights." A peer's 500 (or 401/403) is
/// not this operation's status — it surfaces as 502.
#[tokio::test(flavor = "multi_thread")]
async fn single_proxy_source_failure_surfaces_as_502() {
    let st = state();
    let m = mock_replying("HTTP/1.1 500 Internal Server Error\r\nContent-Length: 0\r\n\r\n");
    register(&st, None, "redirect", m.port, None).await;

    let req = Request::builder()
        .method("DELETE")
        .uri(format!("/ngsi-ld/v1/entities/{ENTITY}"))
        .body(Body::empty())
        .expect("request");
    let res = send(&st, req).await;
    assert_eq!(res.status(), StatusCode::BAD_GATEWAY);
}

/// 6.3.17 p.278, single proxied source: "504 Gateway Timeout — if the single
/// registered source fails to respond in time." The source accepts and goes
/// silent; the broker's own forward deadline (8 s at construction, U1) must
/// end the wait. Slowest test in the file — it IS the timeout.
#[tokio::test(flavor = "multi_thread")]
async fn single_proxy_source_timeout_is_504() {
    let st = state();
    let port = mock_stalling();
    register(&st, None, "redirect", port, None).await;

    let req = Request::builder()
        .method("DELETE")
        .uri(format!("/ngsi-ld/v1/entities/{ENTITY}"))
        .body(Body::empty())
        .expect("request");
    let res = send(&st, req).await;
    assert_eq!(res.status(), StatusCode::GATEWAY_TIMEOUT);
}

/// 6.3.17 p.278, inclusive registration: "when updating the state of the
/// distributed entity, an error response is returned from one or more
/// registered sources: 207 Multi Status." Local success + remote failure is
/// a partial result, never a rollback and never a plain error.
#[tokio::test(flavor = "multi_thread")]
async fn inclusive_partial_failure_is_207() {
    let st = state();
    // entity exists locally BEFORE any registration → created purely locally
    let body = serde_json::json!({"id": ENTITY, "type": "Vehicle"}).to_string();
    let req = Request::builder()
        .method("POST")
        .uri("/ngsi-ld/v1/entities")
        .header("Content-Type", "application/json")
        .header("Content-Length", body.len())
        .body(Body::from(body))
        .expect("request");
    assert_eq!(send(&st, req).await.status(), StatusCode::CREATED);

    let m = mock_replying("HTTP/1.1 500 Internal Server Error\r\nContent-Length: 0\r\n\r\n");
    register(&st, None, "inclusive", m.port, None).await;

    let req = Request::builder()
        .method("DELETE")
        .uri(format!("/ngsi-ld/v1/entities/{ENTITY}"))
        .body(Body::empty())
        .expect("request");
    let res = send(&st, req).await;
    assert_eq!(res.status(), StatusCode::MULTI_STATUS);
    let bytes = axum::body::to_bytes(res.into_body(), 1 << 20)
        .await
        .expect("body");
    let doc: serde_json::Value = serde_json::from_slice(&bytes).expect("json");
    assert!(
        !doc["success"].as_array().expect("success").is_empty(),
        "the local delete succeeded and must be reported: {doc}"
    );
    assert_eq!(
        doc["errors"][0]["error"]["status"], 500,
        "an inclusive source's error keeps its own status in the 207: {doc}"
    );
}

/// 4.3.6.2 p.41 + 6.3.17 p.278: "In the case of an auxiliary registration
/// HTTP unsafe methods are not supported" — auxiliary distributed operations
/// are limited to consumption (5.7). Writes never reach an auxiliary source.
#[tokio::test(flavor = "multi_thread")]
async fn auxiliary_source_never_receives_unsafe_methods() {
    let st = state();
    let (port, hits) = mock_source();
    register(&st, None, "auxiliary", port, None).await;

    let body = serde_json::json!({"id": ENTITY, "type": "Vehicle"}).to_string();
    let req = Request::builder()
        .method("POST")
        .uri("/ngsi-ld/v1/entities")
        .header("Content-Type", "application/json")
        .header("Content-Length", body.len())
        .body(Body::from(body))
        .expect("request");
    assert_eq!(send(&st, req).await.status(), StatusCode::CREATED);

    let req = Request::builder()
        .method("DELETE")
        .uri(format!("/ngsi-ld/v1/entities/{ENTITY}"))
        .body(Body::empty())
        .expect("request");
    assert_eq!(send(&st, req).await.status(), StatusCode::NO_CONTENT);
    assert_eq!(
        hits.load(Ordering::SeqCst),
        0,
        "create and delete are unsafe methods — the auxiliary source must see neither"
    );
}

/// 4.3.6.2 p.41: an auxiliary source "never overrides data held directly
/// within a Context Broker … only included if it is supplementary". Same
/// attribute → local wins; new attribute → supplemented.
#[tokio::test(flavor = "multi_thread")]
async fn auxiliary_data_never_overrides_local() {
    let st = state();
    let body = serde_json::json!({
        "id": ENTITY, "type": "Vehicle",
        "brandName": {"type": "Property", "value": "local-value"},
    })
    .to_string();
    let req = Request::builder()
        .method("POST")
        .uri("/ngsi-ld/v1/entities")
        .header("Content-Type", "application/json")
        .header("Content-Length", body.len())
        .body(Body::from(body))
        .expect("request");
    assert_eq!(send(&st, req).await.status(), StatusCode::CREATED);

    let remote = serde_json::json!({
        "id": ENTITY, "type": "Vehicle",
        "brandName": {"type": "Property", "value": "aux-value"},
        "color": {"type": "Property", "value": "blue"},
    })
    .to_string();
    let reply: &'static str = Box::leak(
        format!(
            "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\r\n{remote}",
            remote.len()
        )
        .into_boxed_str(),
    );
    let m = mock_replying(reply);
    register(&st, None, "auxiliary", m.port, None).await;

    let req = Request::builder()
        .method("GET")
        .uri(format!("/ngsi-ld/v1/entities/{ENTITY}"))
        .header("Accept", "application/json")
        .body(Body::empty())
        .expect("request");
    let res = send(&st, req).await;
    assert_eq!(res.status(), StatusCode::OK);
    let bytes = axum::body::to_bytes(res.into_body(), 1 << 20)
        .await
        .expect("body");
    let doc: serde_json::Value = serde_json::from_slice(&bytes).expect("json");
    assert_eq!(
        doc["brandName"]["value"], "local-value",
        "auxiliary data must not override the broker's own: {doc}"
    );
    assert_eq!(
        doc["color"]["value"], "blue",
        "supplementary auxiliary data must be included: {doc}"
    );
}

/// Table 6.3.18-1: "If local=true then no Context Source Registrations shall
/// be considered as matching" — and Table 6.4.3.2-1 makes `type=*` imply
/// local=true. Either way the registered source is never contacted.
#[tokio::test(flavor = "multi_thread")]
async fn local_true_and_type_wildcard_suppress_forwarding() {
    let st = state();
    let (port, hits) = mock_source();
    register(&st, None, "inclusive", port, None).await;

    for uri in [
        "/ngsi-ld/v1/entities?type=Vehicle&local=true".to_owned(),
        "/ngsi-ld/v1/entities?type=*".to_owned(),
        format!("/ngsi-ld/v1/entities/{ENTITY}?local=true"),
    ] {
        let req = Request::builder()
            .method("GET")
            .uri(&uri)
            .header("Accept", "application/json")
            .body(Body::empty())
            .expect("request");
        let res = send(&st, req).await;
        assert_ne!(res.status(), StatusCode::BAD_GATEWAY, "{uri}");
        assert_eq!(
            hits.load(Ordering::SeqCst),
            0,
            "{uri}: local scope must not contact the registered source"
        );
    }
}

/// Table 6.3.17-1: a registration loop on a distributed GET is abnormal
/// behaviour — "199 Miscellaneous Warning: … a registration loop has been
/// detected" — surfaced as NGSILD-Warning, with the response still served
/// from local data.
#[tokio::test(flavor = "multi_thread")]
async fn suppressed_loop_forward_warns_199_on_get() {
    let st = state();
    let body = serde_json::json!({"id": ENTITY, "type": "Vehicle"}).to_string();
    let req = Request::builder()
        .method("POST")
        .uri("/ngsi-ld/v1/entities")
        .header("Content-Type", "application/json")
        .header("Content-Length", body.len())
        .body(Body::from(body))
        .expect("request");
    assert_eq!(send(&st, req).await.status(), StatusCode::CREATED);

    let (port, hits) = mock_source();
    register(&st, None, "inclusive", port, None).await;

    let req = Request::builder()
        .method("GET")
        .uri(format!("/ngsi-ld/v1/entities/{ENTITY}"))
        .header("Accept", "application/json")
        .header("Via", "1.1 antares1")
        .body(Body::empty())
        .expect("request");
    let res = send(&st, req).await;
    assert_eq!(res.status(), StatusCode::OK, "local data still serves");
    let warning = res
        .headers()
        .get("NGSILD-Warning")
        .and_then(|v| v.to_str().ok())
        .unwrap_or_default()
        .to_owned();
    assert!(
        warning.starts_with("199 antares1 "),
        "Table 6.3.17-1 warn form `199 <alias> \"…\"`, got {warning:?}"
    );
    assert_eq!(hits.load(Ordering::SeqCst), 0, "the forward was suppressed");
}

/// 5.6.1.4: a redirect (or exclusive) registration matching the input but
/// NOT supporting Create Entity yields an error of type Conflict and is
/// never contacted; an inclusive registration without Create Entity support
/// is simply not forwarded — the local create proceeds.
#[tokio::test(flavor = "multi_thread")]
async fn clause_5_6_1_create_unsupported_by_registration() {
    let post_entity = || {
        let body = serde_json::json!({"id": ENTITY, "type": "Vehicle",
                "speed": {"type": "Property", "value": 1}})
        .to_string();
        Request::builder()
            .method("POST")
            .uri("/ngsi-ld/v1/entities")
            .header("Content-Type", "application/json")
            .header("Content-Length", body.len())
            .body(Body::from(body))
            .expect("request")
    };
    let register_ops = |st: AppState, mode: &'static str, port: u16| async move {
        let doc = serde_json::json!({
            "id": format!("urn:ngsi-ld:ContextSourceRegistration:ops-{mode}"),
            "type": "ContextSourceRegistration",
            "mode": mode,
            "operations": ["retrieveEntity"],
            "information": [{"entities": [{"type": "Vehicle", "id": ENTITY}]}],
            "endpoint": format!("http://127.0.0.1:{port}"),
        });
        let body = doc.to_string();
        let req = Request::builder()
            .method("POST")
            .uri("/ngsi-ld/v1/csourceRegistrations")
            .header("Content-Type", "application/json")
            .header("Content-Length", body.len())
            .body(Body::from(body))
            .expect("request");
        assert_eq!(send(&st, req).await.status(), StatusCode::CREATED);
        st
    };

    // redirect without createEntity: the complete create fails → Conflict,
    // and the registered source is never contacted
    let st = register_ops(state(), "redirect", {
        let (port, hits) = mock_source();
        REDIRECT_HITS.set(hits).ok();
        port
    })
    .await;
    let res = send(&st, post_entity()).await;
    assert_eq!(res.status(), StatusCode::CONFLICT);
    let body = String::from_utf8_lossy(
        &axum::body::to_bytes(res.into_body(), usize::MAX)
            .await
            .expect("body"),
    )
    .into_owned();
    assert!(
        body.contains("errors/Conflict"),
        "error type must be Conflict: {body}"
    );
    assert_eq!(
        REDIRECT_HITS.get().expect("hits").load(Ordering::SeqCst),
        0,
        "unsupported operation must not be forwarded"
    );

    // inclusive without createEntity: not forwarded, local create succeeds
    let (port, hits) = mock_source();
    let st = register_ops(state(), "inclusive", port).await;
    let res = send(&st, post_entity()).await;
    assert_eq!(res.status(), StatusCode::CREATED, "local create proceeds");
    assert_eq!(hits.load(Ordering::SeqCst), 0, "no forward without support");
}

static REDIRECT_HITS: std::sync::OnceLock<Arc<AtomicUsize>> = std::sync::OnceLock::new();
