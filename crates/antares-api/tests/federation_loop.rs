// SPDX-License-Identifier: EUPL-1.2
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
    // The mock serves ONE request per connection, so it must say so: without
    // Connection: close the broker's pooled client may reuse the (dropped)
    // keep-alive connection for the next forward — a CI-timing flake where a
    // per-entity fallback forward dies on the dead socket and `hits` undercounts.
    let reply = reply.replacen("\r\n", "\r\nConnection: close\r\n", 1);
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
        // 5.9.2.4: auxiliary registrations are limited to retrieve/query ops
        "operations": if mode == "auxiliary" { ["retrieveOps"] } else { ["redirectionOps"] },
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
    // the mock source is loopback, denied by the egress policy by default
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
/// still asserts 508 — a known suite defect.)
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

/// RFC 7230 section 3.2.2: Via is a list header — the chain may be split across any
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
/// silent; the broker's own forward deadline (8 s at construction) must
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

/// 5.6.2.4: an exclusive/redirect registration matching the update but NOT
/// supporting it yields Conflict (never contacted); an inclusive one without
/// support is not forwarded and the local update proceeds.
#[tokio::test(flavor = "multi_thread")]
async fn clause_5_6_2_update_unsupported_by_registration() {
    let patch_attrs = || {
        let body = serde_json::json!({"speed": {"type": "Property", "value": 9}}).to_string();
        Request::builder()
            .method("PATCH")
            .uri(format!("/ngsi-ld/v1/entities/{ENTITY}/attrs"))
            .header("Content-Type", "application/json")
            .header("Content-Length", body.len())
            .body(Body::from(body))
            .expect("request")
    };
    let register_retrieve_only = |st: AppState, mode: &'static str, port: u16| async move {
        let doc = serde_json::json!({
            "id": format!("urn:ngsi-ld:ContextSourceRegistration:upd-{mode}"),
            "type": "ContextSourceRegistration",
            "mode": mode,
            "operations": ["retrieveOps"],
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

    // redirect without update support → Conflict, source untouched
    let (port, hits) = mock_source();
    let st = register_retrieve_only(state(), "redirect", port).await;
    let res = send(&st, patch_attrs()).await;
    let status = res.status();
    let body = String::from_utf8_lossy(
        &axum::body::to_bytes(res.into_body(), usize::MAX)
            .await
            .expect("body"),
    )
    .into_owned();
    assert_eq!(
        status,
        StatusCode::CONFLICT,
        "complete update failed: {body}"
    );
    assert_eq!(
        hits.load(Ordering::SeqCst),
        0,
        "unsupported op never forwarded"
    );

    // inclusive without update support → local update proceeds, no forward
    let (port, hits) = mock_source();
    let st = register_retrieve_only(state(), "inclusive", port).await;
    let body = serde_json::json!({"id": ENTITY, "type": "Vehicle",
        "speed": {"type": "Property", "value": 1}})
    .to_string();
    let req = Request::builder()
        .method("POST")
        .uri("/ngsi-ld/v1/entities?local=true")
        .header("Content-Type", "application/json")
        .header("Content-Length", body.len())
        .body(Body::from(body))
        .expect("request");
    assert_eq!(send(&st, req).await.status(), StatusCode::CREATED);
    let res = send(&st, patch_attrs()).await;
    assert_eq!(
        res.status(),
        StatusCode::NO_CONTENT,
        "local update proceeds"
    );
    assert_eq!(hits.load(Ordering::SeqCst), 0, "no forward without support");
}

/// 5.6.7.4: per matching CSR — batch op supported → forward the batch;
/// only the single Create Entity op supported → forward PER-ENTITY create
/// requests; neither supported on a proxy mode → Conflict; the source is
/// never contacted with an operation it does not support.
#[tokio::test(flavor = "multi_thread")]
async fn clause_5_6_7_batch_create_forwarding_fallbacks() {
    let batch_create = || {
        let body = serde_json::json!([
            {"id": "urn:ngsi-ld:Vehicle:b1", "type": "Vehicle",
             "speed": {"type": "Property", "value": 1}},
            {"id": "urn:ngsi-ld:Vehicle:b2", "type": "Vehicle",
             "speed": {"type": "Property", "value": 2}}
        ])
        .to_string();
        Request::builder()
            .method("POST")
            .uri("/ngsi-ld/v1/entityOperations/create")
            .header("Content-Type", "application/json")
            .header("Content-Length", body.len())
            .body(Body::from(body))
            .expect("request")
    };
    let register_with = |st: AppState, ops: serde_json::Value, port: u16| async move {
        let doc = serde_json::json!({
            "id": "urn:ngsi-ld:ContextSourceRegistration:batch-fb",
            "type": "ContextSourceRegistration",
            "mode": "redirect",
            "operations": ops,
            "information": [{"entities": [{"type": "Vehicle"}]}],
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

    // createEntity-only source: the batch falls back to per-entity creates
    let m = mock_replying("HTTP/1.1 201 Created\r\nContent-Length: 0\r\n\r\n");
    let st = register_with(state(), serde_json::json!(["createEntity"]), m.port).await;
    let res = send(&st, batch_create()).await;
    assert!(
        res.status().is_success() || res.status() == StatusCode::MULTI_STATUS,
        "fallback forwarding must not fail the batch: {}",
        res.status()
    );
    assert_eq!(
        m.hits.load(Ordering::SeqCst),
        2,
        "one Create Entity forward per entity"
    );
    // 5.6.1 Create Entity posts to the COLLECTION: an id segment would be
    // the Entity resource (6.5), which defines no POST — so the request line
    // is asserted whole, not by prefix.
    assert!(
        m.last_head
            .lock()
            .expect("lock")
            .starts_with("POST /ngsi-ld/v1/entities HTTP/1.1"),
        "fallback posts to the entity collection: {}",
        m.last_head.lock().expect("lock")
    );

    // retrieve-only proxy source: Conflict, never contacted
    let (port, hits) = mock_source();
    let st = register_with(state(), serde_json::json!(["retrieveOps"]), port).await;
    let res = send(&st, batch_create()).await;
    let body = String::from_utf8_lossy(
        &axum::body::to_bytes(res.into_body(), usize::MAX)
            .await
            .expect("body"),
    )
    .into_owned();
    assert!(
        body.contains("does not accept") || body.contains("Conflict"),
        "unsupported proxy source reports Conflict: {body}"
    );
    assert_eq!(hits.load(Ordering::SeqCst), 0, "never contacted");
}

/// 5.6.8.4 support ladder: no upsertBatch → per-entity Create Entity, and on
/// AlreadyExists fall back to Replace Entity (mode replace/unset) or Update
/// Attributes (mode update); a source offering only updateEntity gets the
/// per-entity PATCH directly under options=update.
#[tokio::test(flavor = "multi_thread")]
async fn clause_5_6_8_upsert_forwarding_fallbacks() {
    let upsert = |q: &str| {
        let body = serde_json::json!([
            {"id": ENTITY, "type": "Vehicle", "speed": {"type": "Property", "value": 1}}
        ])
        .to_string();
        Request::builder()
            .method("POST")
            .uri(format!("/ngsi-ld/v1/entityOperations/upsert{q}"))
            .header("Content-Type", "application/json")
            .header("Content-Length", body.len())
            .body(Body::from(body))
            .expect("request")
    };
    let register_with = |st: AppState, ops: serde_json::Value, port: u16| async move {
        let doc = serde_json::json!({
            "id": "urn:ngsi-ld:ContextSourceRegistration:ups-fb",
            "type": "ContextSourceRegistration",
            "mode": "redirect",
            "operations": ops,
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

    // createEntity+replaceEntity source, remote already has the entity
    // (mock answers 409): create → 409 → Replace Entity forward
    let m =
        mock_replying("HTTP/1.1 409 Conflict\r\nConnection: close\r\nContent-Length: 0\r\n\r\n");
    let st = register_with(
        state(),
        serde_json::json!(["createEntity", "replaceEntity"]),
        m.port,
    )
    .await;
    let _ = send(&st, upsert("")).await;
    assert_eq!(
        m.hits.load(Ordering::SeqCst),
        2,
        "create then replace fallback"
    );
    assert!(
        m.last_head
            .lock()
            .expect("lock")
            .starts_with(&format!("PUT /ngsi-ld/v1/entities/{ENTITY}")),
        "AlreadyExists falls back to Replace Entity: {}",
        m.last_head.lock().expect("lock")
    );

    // updateEntity-only source with options=update: direct per-entity PATCH
    let m = mock_replying("HTTP/1.1 204 No Content\r\nContent-Length: 0\r\n\r\n");
    let st = register_with(state(), serde_json::json!(["updateEntity"]), m.port).await;
    let _ = send(&st, upsert("?options=update")).await;
    assert_eq!(m.hits.load(Ordering::SeqCst), 1, "one update forward");
    assert!(
        m.last_head
            .lock()
            .expect("lock")
            .starts_with(&format!("PATCH /ngsi-ld/v1/entities/{ENTITY}/attrs")),
        "update mode uses Update Attributes: {}",
        m.last_head.lock().expect("lock")
    );
}

/// 5.6.8.5: a 201 Upsert response lists ONLY the newly created ids — a
/// forwarded upsertBatch answered with 204 means those entities were
/// UPDATED on the source, so an all-updated distributed upsert returns
/// 204 (matrix-9 D013_01/02: every remote batch success was tagged
/// "created" and the aggregate flipped to 201).
#[tokio::test(flavor = "multi_thread")]
async fn clause_5_6_8_remote_204_upsert_is_update_not_create() {
    let m =
        mock_replying("HTTP/1.1 204 No Content\r\nConnection: close\r\nContent-Length: 0\r\n\r\n");
    let st = state();
    let reg = serde_json::json!({
        "id": "urn:ngsi-ld:ContextSourceRegistration:ups-204",
        "type": "ContextSourceRegistration",
        "mode": "redirect",
        "operations": ["upsertBatch"],
        "information": [{"entities": [{"type": "Vehicle", "id": ENTITY}]}],
        "endpoint": format!("http://127.0.0.1:{}", m.port),
    })
    .to_string();
    let req = Request::builder()
        .method("POST")
        .uri("/ngsi-ld/v1/csourceRegistrations")
        .header("Content-Type", "application/json")
        .header("Content-Length", reg.len())
        .body(Body::from(reg))
        .expect("request");
    assert_eq!(send(&st, req).await.status(), StatusCode::CREATED);

    let body = serde_json::json!([
        {"id": ENTITY, "type": "Vehicle", "speed": {"type": "Property", "value": 7}}
    ])
    .to_string();
    let req = Request::builder()
        .method("POST")
        .uri("/ngsi-ld/v1/entityOperations/upsert")
        .header("Content-Type", "application/json")
        .header("Content-Length", body.len())
        .body(Body::from(body))
        .expect("request");
    let res = send(&st, req).await;
    assert_eq!(res.status(), StatusCode::NO_CONTENT, "all updated → 204");
    let bytes = axum::body::to_bytes(res.into_body(), usize::MAX)
        .await
        .expect("body");
    assert!(
        bytes.is_empty(),
        "204 carries no created-id list: {}",
        String::from_utf8_lossy(&bytes)
    );
    assert_eq!(m.hits.load(Ordering::SeqCst), 1, "one batch forward");
}

/// 5.6.21.4: a matched registration forwards the purge only when it
/// supports purgeEntity; an unsupported matched registration — any mode,
/// redirect included — contributes a Conflict and is never contacted.
#[tokio::test(flavor = "multi_thread")]
async fn clause_5_6_21_purge_forwarding_gate() {
    let purge_req = || {
        Request::builder()
            .method("DELETE")
            .uri("/ngsi-ld/v1/entities?type=Vehicle")
            .body(Body::empty())
            .expect("request")
    };
    let register_with = |st: AppState, ops: serde_json::Value, port: u16| async move {
        let doc = serde_json::json!({
            "id": "urn:ngsi-ld:ContextSourceRegistration:purge-gate",
            "type": "ContextSourceRegistration",
            "mode": "redirect",
            "operations": ops,
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

    // redirect source without purgeEntity: Conflict, never contacted
    let (port, hits) = mock_source();
    let st = register_with(state(), serde_json::json!(["retrieveOps"]), port).await;
    let res = send(&st, purge_req()).await;
    let body = String::from_utf8_lossy(
        &axum::body::to_bytes(res.into_body(), usize::MAX)
            .await
            .expect("body"),
    )
    .into_owned();
    assert!(
        body.contains("does not accept") || body.contains("Conflict"),
        "unsupported matched source reports Conflict: {body}"
    );
    assert_eq!(hits.load(Ordering::SeqCst), 0, "never contacted");

    // purgeEntity-supporting source: one DELETE /entities forward with the
    // narrowing query on the wire
    let m = mock_replying("HTTP/1.1 204 No Content\r\nContent-Length: 0\r\n\r\n");
    let st = register_with(state(), serde_json::json!(["purgeEntity"]), m.port).await;
    let _ = send(&st, purge_req()).await;
    assert_eq!(m.hits.load(Ordering::SeqCst), 1, "one purge forward");
    let head = m.last_head.lock().expect("lock").clone();
    assert!(
        head.starts_with("DELETE /ngsi-ld/v1/entities?") && head.contains("type=Vehicle"),
        "forward keeps the purge query: {head}"
    );
}

/// 5.7.3.4: matching registrations that support retrieveTemporal are
/// forwarded GET /temporal/entities/{id} and the remote instance data
/// merges into the Temporal Evolution; sources without the operation are
/// never contacted.
#[tokio::test(flavor = "multi_thread")]
async fn clause_5_7_3_temporal_retrieve_forwarding() {
    let register_with = |st: AppState, ops: serde_json::Value, port: u16| async move {
        let doc = serde_json::json!({
            "id": "urn:ngsi-ld:ContextSourceRegistration:trf",
            "type": "ContextSourceRegistration",
            "mode": "redirect",
            "operations": ops,
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
    let temporal_get = || {
        Request::builder()
            .uri(format!("/ngsi-ld/v1/temporal/entities/{ENTITY}"))
            .body(Body::empty())
            .expect("request")
    };

    // retrieveTemporal-supporting source: forwarded, remote data served
    let remote = serde_json::json!({
        "id": ENTITY, "type": "Vehicle",
        "speed": [{"type": "Property", "value": 5,
                   "observedAt": "2026-01-01T00:00:00Z"}]
    })
    .to_string();
    let reply: &'static str = Box::leak(
        format!(
            "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\r\n{}",
            remote.len(),
            remote
        )
        .into_boxed_str(),
    );
    let m = mock_replying(reply);
    let st = register_with(state(), serde_json::json!(["retrieveTemporal"]), m.port).await;
    let res = send(&st, temporal_get()).await;
    assert_eq!(res.status(), StatusCode::OK);
    let bytes = axum::body::to_bytes(res.into_body(), 1 << 20)
        .await
        .expect("body");
    let doc: serde_json::Value = serde_json::from_slice(&bytes).expect("json");
    let speed = doc["speed"].as_array().expect("speed instances");
    assert_eq!(speed.len(), 1, "remote instance served: {doc}");
    assert_eq!(speed[0]["value"], 5);
    assert_eq!(m.hits.load(Ordering::SeqCst), 1, "one temporal forward");
    assert!(
        m.last_head
            .lock()
            .expect("lock")
            .starts_with(&format!("GET /ngsi-ld/v1/temporal/entities/{ENTITY}")),
        "forward targets the temporal resource: {}",
        m.last_head.lock().expect("lock")
    );

    // source without retrieveTemporal: never contacted; nothing local → 404
    let (port, hits) = mock_source();
    let st = register_with(state(), serde_json::json!(["retrieveEntity"]), port).await;
    let res = send(&st, temporal_get()).await;
    assert_eq!(res.status(), StatusCode::NOT_FOUND);
    assert_eq!(hits.load(Ordering::SeqCst), 0, "never contacted");
}

/// 5.7.4.4: the temporal query forwards to matching registrations that
/// support queryTemporal (with the temporal window on the wire); sources
/// without the operation are never contacted.
#[tokio::test(flavor = "multi_thread")]
async fn clause_5_7_4_temporal_query_forwarding() {
    let register_with = |st: AppState, ops: serde_json::Value, port: u16| async move {
        let doc = serde_json::json!({
            "id": "urn:ngsi-ld:ContextSourceRegistration:tqf",
            "type": "ContextSourceRegistration",
            "mode": "redirect",
            "operations": ops,
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
    let temporal_query = || {
        Request::builder()
            .uri(
                "/ngsi-ld/v1/temporal/entities?type=Vehicle&timerel=after\
                 &timeAt=2020-01-01T00:00:00Z",
            )
            .body(Body::empty())
            .expect("request")
    };

    let remote = serde_json::json!([{
        "id": ENTITY, "type": "Vehicle",
        "speed": [{"type": "Property", "value": 7,
                   "observedAt": "2026-01-01T00:00:00Z"}]
    }])
    .to_string();
    let reply: &'static str = Box::leak(
        format!(
            "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\r\n{}",
            remote.len(),
            remote
        )
        .into_boxed_str(),
    );
    let m = mock_replying(reply);
    let st = register_with(state(), serde_json::json!(["queryTemporal"]), m.port).await;
    let res = send(&st, temporal_query()).await;
    assert_eq!(res.status(), StatusCode::OK);
    let bytes = axum::body::to_bytes(res.into_body(), 1 << 20)
        .await
        .expect("body");
    let docs: serde_json::Value = serde_json::from_slice(&bytes).expect("json");
    let arr = docs.as_array().expect("array");
    assert_eq!(arr.len(), 1, "remote temporal entity served: {docs}");
    assert_eq!(arr[0]["speed"][0]["value"], 7, "{docs}");
    assert_eq!(
        m.hits.load(Ordering::SeqCst),
        1,
        "one temporal query forward"
    );
    let head = m.last_head.lock().expect("lock").clone();
    assert!(
        head.starts_with("GET /ngsi-ld/v1/temporal/entities?") && head.contains("timerel=after"),
        "forward carries the temporal window: {head}"
    );

    // retrieveTemporal-only source: not eligible for the query fan-out
    let (port, hits) = mock_source();
    let st = register_with(state(), serde_json::json!(["retrieveTemporal"]), port).await;
    let res = send(&st, temporal_query()).await;
    assert_eq!(res.status(), StatusCode::OK);
    assert_eq!(hits.load(Ordering::SeqCst), 0, "never contacted");
}

/// 5.6.20.4 support ladder: no mergeBatch → per-entity Merge Entity
/// (PATCH /entities/{id}) forwards.
#[tokio::test(flavor = "multi_thread")]
async fn clause_5_6_20_batch_merge_fallback() {
    let m = mock_replying("HTTP/1.1 204 No Content\r\nContent-Length: 0\r\n\r\n");
    let st = state();
    let doc = serde_json::json!({
        "id": "urn:ngsi-ld:ContextSourceRegistration:bm-fb",
        "type": "ContextSourceRegistration",
        "mode": "redirect",
        "operations": ["mergeEntity"],
        "information": [{"entities": [{"type": "Vehicle", "id": ENTITY}]}],
        "endpoint": format!("http://127.0.0.1:{}", m.port),
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
    let body = serde_json::json!([
        {"id": ENTITY, "type": "Vehicle", "speed": {"type": "Property", "value": 3}}
    ])
    .to_string();
    let req = Request::builder()
        .method("POST")
        .uri("/ngsi-ld/v1/entityOperations/merge")
        .header("Content-Type", "application/json")
        .header("Content-Length", body.len())
        .body(Body::from(body))
        .expect("request");
    let _ = send(&st, req).await;
    assert_eq!(m.hits.load(Ordering::SeqCst), 1, "one Merge Entity forward");
    assert!(
        m.last_head
            .lock()
            .expect("lock")
            .starts_with(&format!("PATCH /ngsi-ld/v1/entities/{ENTITY} ")),
        "fallback uses the single-entity merge: {}",
        m.last_head.lock().expect("lock")
    );
}

/// 5.6.20.4 local behaviour: each entity merged per 5.6.17 (RFC 7396 —
/// value overwrite, null deletes the attribute); 204 with no body when all
/// merge, 207 with the S/E partition when some fail (5.6.20.5).
#[tokio::test(flavor = "multi_thread")]
async fn clause_5_6_20_local_merge_and_partition() {
    let st = state();
    let post = |uri: &'static str, body: String| {
        Request::builder()
            .method("POST")
            .uri(uri)
            .header("Content-Type", "application/json")
            .header("Content-Length", body.len())
            .body(Body::from(body))
            .expect("request")
    };
    let entity = serde_json::json!({
        "id": ENTITY, "type": "Vehicle",
        "speed": {"type": "Property", "value": 3},
        "color": {"type": "Property", "value": "red"}
    })
    .to_string();
    assert_eq!(
        send(&st, post("/ngsi-ld/v1/entities", entity))
            .await
            .status(),
        StatusCode::CREATED
    );

    // all merged → 204, no body; overwrite + null-deletion + append applied
    let batch = serde_json::json!([{
        "id": ENTITY, "type": "Vehicle",
        "speed": {"type": "Property", "value": 9},
        "color": "urn:ngsi-ld:null",
        "brand": {"type": "Property", "value": "Ajax"}
    }])
    .to_string();
    let res = send(&st, post("/ngsi-ld/v1/entityOperations/merge", batch)).await;
    assert_eq!(res.status(), StatusCode::NO_CONTENT);
    let bytes = axum::body::to_bytes(res.into_body(), 1 << 20)
        .await
        .expect("body");
    assert!(bytes.is_empty(), "204 carries no body: {bytes:?}");
    let req = Request::builder()
        .uri(format!("/ngsi-ld/v1/entities/{ENTITY}"))
        .body(Body::empty())
        .expect("request");
    let res = send(&st, req).await;
    let bytes = axum::body::to_bytes(res.into_body(), 1 << 20)
        .await
        .expect("body");
    let doc: serde_json::Value = serde_json::from_slice(&bytes).expect("json");
    assert_eq!(doc["speed"]["value"], 9, "value overwritten: {doc}");
    assert_eq!(doc["brand"]["value"], "Ajax", "attribute appended: {doc}");
    assert!(
        doc.get("color").is_none(),
        "null must delete the attribute, not store the sentinel: {doc}"
    );

    // one existing + one absent → 207 partition per 5.6.20.5
    let missing = "urn:ngsi-ld:Vehicle:absent-5620";
    let batch = serde_json::json!([
        {"id": ENTITY, "type": "Vehicle", "speed": {"type": "Property", "value": 11}},
        {"id": missing, "type": "Vehicle", "speed": {"type": "Property", "value": 1}}
    ])
    .to_string();
    let res = send(&st, post("/ngsi-ld/v1/entityOperations/merge", batch)).await;
    assert_eq!(res.status(), StatusCode::MULTI_STATUS);
    let bytes = axum::body::to_bytes(res.into_body(), 1 << 20)
        .await
        .expect("body");
    let doc: serde_json::Value = serde_json::from_slice(&bytes).expect("json");
    assert_eq!(
        doc["success"],
        serde_json::json!([ENTITY]),
        "S array: {doc}"
    );
    let errors = doc["errors"].as_array().expect("E array");
    assert_eq!(errors.len(), 1, "one BatchEntityError: {doc}");
    assert_eq!(errors[0]["entityId"], missing);
    assert_eq!(
        errors[0]["error"]["type"],
        "https://uri.etsi.org/ngsi-ld/errors/ResourceNotFound"
    );

    // 5.5.4 validation: null item and empty array are whole-request 400s
    let res = send(
        &st,
        post(
            "/ngsi-ld/v1/entityOperations/merge",
            format!(r#"[{{"id":"{ENTITY}","type":"Vehicle"}}, null]"#),
        ),
    )
    .await;
    assert_eq!(res.status(), StatusCode::BAD_REQUEST, "null item");
    let res = send(&st, post("/ngsi-ld/v1/entityOperations/merge", "[]".into())).await;
    assert_eq!(res.status(), StatusCode::BAD_REQUEST, "empty array");
}

/// 5.6.9.4 support ladder: no updateBatch → per-entity Update Attributes
/// (overwrite permitted) or per-entity Append Attributes with overwrite
/// disabled (options=noOverwrite).
#[tokio::test(flavor = "multi_thread")]
async fn clause_5_6_9_batch_update_forwarding_fallbacks() {
    let batch_update = |q: &str| {
        let body = serde_json::json!([
            {"id": ENTITY, "type": "Vehicle", "speed": {"type": "Property", "value": 5}}
        ])
        .to_string();
        Request::builder()
            .method("POST")
            .uri(format!("/ngsi-ld/v1/entityOperations/update{q}"))
            .header("Content-Type", "application/json")
            .header("Content-Length", body.len())
            .body(Body::from(body))
            .expect("request")
    };
    let register_with = |st: AppState, ops: serde_json::Value, port: u16| async move {
        let doc = serde_json::json!({
            "id": "urn:ngsi-ld:ContextSourceRegistration:bu-fb",
            "type": "ContextSourceRegistration",
            "mode": "redirect",
            "operations": ops,
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

    // updateEntity-only source: per-entity Update Attributes forward
    let m = mock_replying("HTTP/1.1 204 No Content\r\nContent-Length: 0\r\n\r\n");
    let st = register_with(state(), serde_json::json!(["updateEntity"]), m.port).await;
    let _ = send(&st, batch_update("")).await;
    assert_eq!(m.hits.load(Ordering::SeqCst), 1);
    assert!(
        m.last_head
            .lock()
            .expect("lock")
            .starts_with(&format!("PATCH /ngsi-ld/v1/entities/{ENTITY}/attrs")),
        "overwrite-permitted uses Update Attributes: {}",
        m.last_head.lock().expect("lock")
    );

    // appendAttrs-only source with noOverwrite: per-entity append forward
    let m = mock_replying("HTTP/1.1 204 No Content\r\nContent-Length: 0\r\n\r\n");
    let st = register_with(state(), serde_json::json!(["appendAttrs"]), m.port).await;
    let _ = send(&st, batch_update("?options=noOverwrite")).await;
    assert_eq!(m.hits.load(Ordering::SeqCst), 1);
    let head = m.last_head.lock().expect("lock").clone();
    assert!(
        head.starts_with(&format!("POST /ngsi-ld/v1/entities/{ENTITY}/attrs")),
        "noOverwrite uses Append Attributes: {head}"
    );
    assert!(
        head.contains("options=noOverwrite"),
        "append forwarded with overwrite disabled: {head}"
    );
}

/// 5.6.10.4 support ladder: no deleteBatch → per-entity Delete Entity
/// forwards; a proxy source supporting neither is Conflict and never
/// contacted. A null item in the id array fails the whole request (400).
#[tokio::test(flavor = "multi_thread")]
async fn clause_5_6_10_batch_delete_forwarding_fallbacks() {
    let batch_delete = |body: String| {
        Request::builder()
            .method("POST")
            .uri("/ngsi-ld/v1/entityOperations/delete")
            .header("Content-Type", "application/json")
            .header("Content-Length", body.len())
            .body(Body::from(body))
            .expect("request")
    };
    let register_with = |st: AppState, ops: serde_json::Value, port: u16| async move {
        let doc = serde_json::json!({
            "id": "urn:ngsi-ld:ContextSourceRegistration:bd-fb",
            "type": "ContextSourceRegistration",
            "mode": "redirect",
            "operations": ops,
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

    // deleteEntity-only source: per-entity DELETE forward
    let m = mock_replying("HTTP/1.1 204 No Content\r\nContent-Length: 0\r\n\r\n");
    let st = register_with(state(), serde_json::json!(["deleteEntity"]), m.port).await;
    let _ = send(&st, batch_delete(serde_json::json!([ENTITY]).to_string())).await;
    assert_eq!(
        m.hits.load(Ordering::SeqCst),
        1,
        "one Delete Entity forward"
    );
    assert!(
        m.last_head
            .lock()
            .expect("lock")
            .starts_with(&format!("DELETE /ngsi-ld/v1/entities/{ENTITY}")),
        "fallback uses the single-entity resource: {}",
        m.last_head.lock().expect("lock")
    );

    // retrieve-only proxy source: Conflict, never contacted
    let (port, hits) = mock_source();
    let st = register_with(state(), serde_json::json!(["retrieveOps"]), port).await;
    let res = send(&st, batch_delete(serde_json::json!([ENTITY]).to_string())).await;
    let body = String::from_utf8_lossy(
        &axum::body::to_bytes(res.into_body(), usize::MAX)
            .await
            .expect("body"),
    )
    .into_owned();
    assert!(
        body.contains("does not accept") || body.contains("Conflict"),
        "unsupported proxy source reports Conflict: {body}"
    );
    assert_eq!(hits.load(Ordering::SeqCst), 0, "never contacted");

    // null item → whole-request 400
    let st = state();
    let res = send(&st, batch_delete(format!(r#"["{ENTITY}", null]"#))).await;
    assert_eq!(res.status(), StatusCode::BAD_REQUEST);
}

/// 5.6.10.5 Output data: none (empty 204) if all Entities were deleted;
/// otherwise 207 with the S array (deleted ids) and the E array
/// (BatchEntityError per 5.2.17) as {"success": [...], "errors": [...]}.
#[tokio::test(flavor = "multi_thread")]
async fn clause_5_6_10_5_output_shape() {
    let st = state();
    let create = |body: String| {
        Request::builder()
            .method("POST")
            .uri("/ngsi-ld/v1/entities")
            .header("Content-Type", "application/json")
            .header("Content-Length", body.len())
            .body(Body::from(body))
            .expect("request")
    };
    let batch_delete = |body: String| {
        Request::builder()
            .method("POST")
            .uri("/ngsi-ld/v1/entityOperations/delete")
            .header("Content-Type", "application/json")
            .header("Content-Length", body.len())
            .body(Body::from(body))
            .expect("request")
    };
    let entity = serde_json::json!({"id": ENTITY, "type": "Vehicle"}).to_string();

    // all deleted → 204 and NO body at all
    assert_eq!(
        send(&st, create(entity.clone())).await.status(),
        StatusCode::CREATED
    );
    let res = send(&st, batch_delete(serde_json::json!([ENTITY]).to_string())).await;
    assert_eq!(res.status(), StatusCode::NO_CONTENT);
    let bytes = axum::body::to_bytes(res.into_body(), 1 << 20)
        .await
        .expect("body");
    assert!(bytes.is_empty(), "204 carries no body: {bytes:?}");

    // partial → 207 with both arrays, correctly partitioned
    assert_eq!(
        send(&st, create(entity)).await.status(),
        StatusCode::CREATED
    );
    let missing = "urn:ngsi-ld:Vehicle:absent-56105";
    let res = send(
        &st,
        batch_delete(serde_json::json!([ENTITY, missing]).to_string()),
    )
    .await;
    assert_eq!(res.status(), StatusCode::MULTI_STATUS);
    let bytes = axum::body::to_bytes(res.into_body(), 1 << 20)
        .await
        .expect("body");
    let doc: serde_json::Value = serde_json::from_slice(&bytes).expect("json");
    assert_eq!(
        doc["success"],
        serde_json::json!([ENTITY]),
        "S array holds exactly the deleted ids: {doc}"
    );
    let errors = doc["errors"].as_array().expect("E array");
    assert_eq!(errors.len(), 1, "one BatchEntityError: {doc}");
    assert_eq!(errors[0]["entityId"], missing);
    assert_eq!(
        errors[0]["error"]["type"],
        "https://uri.etsi.org/ngsi-ld/errors/ResourceNotFound"
    );
    // the failed id must NOT leak into S
    assert!(
        !doc["success"]
            .as_array()
            .expect("S array")
            .iter()
            .any(|v| v == missing),
        "missing id must not appear as a success: {doc}"
    );
}

/// 5.6.11.4: exclusive/redirect registrations forward the temporal upsert
/// when "Create or Update Temporal" is supported; unsupported proxy modes
/// are Conflict and never contacted.
#[tokio::test(flavor = "multi_thread")]
async fn clause_5_6_11_temporal_upsert_forwarding() {
    let upsert = || {
        let body = serde_json::json!({
            "id": ENTITY, "type": "Vehicle",
            "speed": [{"type": "Property", "value": 1,
                       "observedAt": "2026-01-01T00:00:00Z"}]
        })
        .to_string();
        Request::builder()
            .method("POST")
            .uri("/ngsi-ld/v1/temporal/entities")
            .header("Content-Type", "application/json")
            .header("Content-Length", body.len())
            .body(Body::from(body))
            .expect("request")
    };
    let register_with = |st: AppState, ops: serde_json::Value, port: u16| async move {
        let doc = serde_json::json!({
            "id": "urn:ngsi-ld:ContextSourceRegistration:tu-fb",
            "type": "ContextSourceRegistration",
            "mode": "redirect",
            "operations": ops,
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

    // upsertTemporal-supporting source: the upsert is forwarded
    let m = mock_replying("HTTP/1.1 204 No Content\r\nContent-Length: 0\r\n\r\n");
    let st = register_with(state(), serde_json::json!(["upsertTemporal"]), m.port).await;
    let _ = send(&st, upsert()).await;
    assert_eq!(
        m.hits.load(Ordering::SeqCst),
        1,
        "temporal upsert forwarded"
    );
    assert!(
        m.last_head
            .lock()
            .expect("lock")
            .starts_with("POST /ngsi-ld/v1/temporal/entities"),
        "forwarded to the temporal resource: {}",
        m.last_head.lock().expect("lock")
    );

    // retrieve-only proxy source: Conflict, never contacted
    let (port, hits) = mock_source();
    let st = register_with(state(), serde_json::json!(["retrieveOps"]), port).await;
    let res = send(&st, upsert()).await;
    let body = String::from_utf8_lossy(
        &axum::body::to_bytes(res.into_body(), usize::MAX)
            .await
            .expect("body"),
    )
    .into_owned();
    assert!(
        body.contains("does not accept") || body.contains("Conflict"),
        "unsupported proxy source reports Conflict: {body}"
    );
    assert_eq!(hits.load(Ordering::SeqCst), 0, "never contacted");
}

/// 5.6.12.4: "Add Attributes to Temporal Evolution" forwards to matching
/// registrations supporting appendAttrsTemporal; unsupported proxy modes
/// are Conflict and never contacted.
#[tokio::test(flavor = "multi_thread")]
async fn clause_5_6_12_temporal_add_attrs_forwarding() {
    let add = || {
        let body = serde_json::json!({
            "speed": [{"type": "Property", "value": 2,
                       "observedAt": "2026-01-03T00:00:00Z"}]
        })
        .to_string();
        Request::builder()
            .method("POST")
            .uri(format!("/ngsi-ld/v1/temporal/entities/{ENTITY}/attrs"))
            .header("Content-Type", "application/json")
            .header("Content-Length", body.len())
            .body(Body::from(body))
            .expect("request")
    };
    let register_with = |st: AppState, ops: serde_json::Value, port: u16| async move {
        let doc = serde_json::json!({
            "id": "urn:ngsi-ld:ContextSourceRegistration:ta-fb",
            "type": "ContextSourceRegistration",
            "mode": "redirect",
            "operations": ops,
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

    let m = mock_replying("HTTP/1.1 204 No Content\r\nContent-Length: 0\r\n\r\n");
    let st = register_with(state(), serde_json::json!(["appendAttrsTemporal"]), m.port).await;
    let _ = send(&st, add()).await;
    assert_eq!(
        m.hits.load(Ordering::SeqCst),
        1,
        "temporal add-attrs forwarded"
    );
    assert!(
        m.last_head.lock().expect("lock").starts_with(&format!(
            "POST /ngsi-ld/v1/temporal/entities/{ENTITY}/attrs"
        )),
        "forwarded to the temporal attrs resource: {}",
        m.last_head.lock().expect("lock")
    );

    let (port, hits) = mock_source();
    let st = register_with(state(), serde_json::json!(["retrieveOps"]), port).await;
    let res = send(&st, add()).await;
    let body = String::from_utf8_lossy(
        &axum::body::to_bytes(res.into_body(), usize::MAX)
            .await
            .expect("body"),
    )
    .into_owned();
    assert!(
        body.contains("does not accept") || body.contains("Conflict"),
        "unsupported proxy source reports Conflict: {body}"
    );
    assert_eq!(hits.load(Ordering::SeqCst), 0, "never contacted");
}

/// 5.6.13.4/5.6.14.4/5.6.15.4: temporal attribute delete / instance modify /
/// instance delete forward to registrations supporting the respective op;
/// unsupported proxy modes are Conflict and never contacted.
#[tokio::test(flavor = "multi_thread")]
async fn clause_5_6_13_to_15_temporal_attr_ops_forwarding() {
    let register_with = |st: AppState, ops: serde_json::Value, port: u16| async move {
        let doc = serde_json::json!({
            "id": "urn:ngsi-ld:ContextSourceRegistration:td-fb",
            "type": "ContextSourceRegistration",
            "mode": "redirect",
            "operations": ops,
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
    let del_attr = || {
        Request::builder()
            .method("DELETE")
            .uri(format!(
                "/ngsi-ld/v1/temporal/entities/{ENTITY}/attrs/speed"
            ))
            .body(Body::empty())
            .expect("request")
    };

    // supporting source: forwarded
    let m = mock_replying("HTTP/1.1 204 No Content\r\nContent-Length: 0\r\n\r\n");
    let st = register_with(state(), serde_json::json!(["deleteAttrsTemporal"]), m.port).await;
    let _ = send(&st, del_attr()).await;
    assert_eq!(
        m.hits.load(Ordering::SeqCst),
        1,
        "temporal attr delete forwarded"
    );
    assert!(
        m.last_head.lock().expect("lock").starts_with(&format!(
            "DELETE /ngsi-ld/v1/temporal/entities/{ENTITY}/attrs/"
        )),
        "forwarded to the temporal attr resource: {}",
        m.last_head.lock().expect("lock")
    );

    // 5.6.14: instance modify forwards as PATCH on the instance resource
    let m = mock_replying("HTTP/1.1 204 No Content\r\nContent-Length: 0\r\n\r\n");
    let st = register_with(
        state(),
        serde_json::json!(["updateAttrInstanceTemporal"]),
        m.port,
    )
    .await;
    let body = serde_json::json!({"value": 5}).to_string();
    let req = Request::builder()
        .method("PATCH")
        .uri(format!(
            "/ngsi-ld/v1/temporal/entities/{ENTITY}/attrs/speed/urn:ngsi-ld:Instance:1"
        ))
        .header("Content-Type", "application/json")
        .header("Content-Length", body.len())
        .body(Body::from(body))
        .expect("request");
    let _ = send(&st, req).await;
    assert_eq!(
        m.hits.load(Ordering::SeqCst),
        1,
        "instance modify forwarded"
    );
    assert!(
        m.last_head.lock().expect("lock").starts_with(&format!(
            "PATCH /ngsi-ld/v1/temporal/entities/{ENTITY}/attrs/speed/urn:ngsi-ld:Instance:1"
        )),
        "5.6.14 instance path: {}",
        m.last_head.lock().expect("lock")
    );

    // 5.6.15: instance delete forwards as DELETE on the instance resource
    let m = mock_replying("HTTP/1.1 204 No Content\r\nContent-Length: 0\r\n\r\n");
    let st = register_with(
        state(),
        serde_json::json!(["deleteAttrInstanceTemporal"]),
        m.port,
    )
    .await;
    let req = Request::builder()
        .method("DELETE")
        .uri(format!(
            "/ngsi-ld/v1/temporal/entities/{ENTITY}/attrs/speed/urn:ngsi-ld:Instance:1"
        ))
        .body(Body::empty())
        .expect("request");
    let _ = send(&st, req).await;
    assert_eq!(
        m.hits.load(Ordering::SeqCst),
        1,
        "instance delete forwarded"
    );
    assert!(
        m.last_head.lock().expect("lock").starts_with(&format!(
            "DELETE /ngsi-ld/v1/temporal/entities/{ENTITY}/attrs/speed/urn:ngsi-ld:Instance:1"
        )),
        "5.6.15 instance path: {}",
        m.last_head.lock().expect("lock")
    );

    // retrieve-only proxy: Conflict, never contacted
    let (port, hits) = mock_source();
    let st = register_with(state(), serde_json::json!(["retrieveOps"]), port).await;
    let res = send(&st, del_attr()).await;
    let body = String::from_utf8_lossy(
        &axum::body::to_bytes(res.into_body(), usize::MAX)
            .await
            .expect("body"),
    )
    .into_owned();
    assert!(
        body.contains("does not accept") || body.contains("Conflict"),
        "unsupported proxy source reports Conflict: {body}"
    );
    assert_eq!(hits.load(Ordering::SeqCst), 0, "never contacted");
}

/// 5.6.16.4: Delete Temporal Evolution forwards to registrations supporting
/// deleteTemporal; unsupported proxy modes are Conflict, never contacted.
#[tokio::test(flavor = "multi_thread")]
async fn clause_5_6_16_temporal_delete_forwarding() {
    let register_with = |st: AppState, ops: serde_json::Value, port: u16| async move {
        let doc = serde_json::json!({
            "id": "urn:ngsi-ld:ContextSourceRegistration:tdel-fb",
            "type": "ContextSourceRegistration",
            "mode": "redirect",
            "operations": ops,
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
    let del = || {
        Request::builder()
            .method("DELETE")
            .uri(format!("/ngsi-ld/v1/temporal/entities/{ENTITY}"))
            .body(Body::empty())
            .expect("request")
    };

    let m = mock_replying("HTTP/1.1 204 No Content\r\nContent-Length: 0\r\n\r\n");
    let st = register_with(state(), serde_json::json!(["deleteTemporal"]), m.port).await;
    let _ = send(&st, del()).await;
    assert_eq!(
        m.hits.load(Ordering::SeqCst),
        1,
        "temporal delete forwarded"
    );
    assert!(
        m.last_head
            .lock()
            .expect("lock")
            .starts_with(&format!("DELETE /ngsi-ld/v1/temporal/entities/{ENTITY} ")),
        "forwarded to the temporal entity resource: {}",
        m.last_head.lock().expect("lock")
    );

    let (port, hits) = mock_source();
    let st = register_with(state(), serde_json::json!(["retrieveOps"]), port).await;
    let res = send(&st, del()).await;
    let body = String::from_utf8_lossy(
        &axum::body::to_bytes(res.into_body(), usize::MAX)
            .await
            .expect("body"),
    )
    .into_owned();
    assert!(
        body.contains("does not accept") || body.contains("Conflict"),
        "unsupported proxy source reports Conflict: {body}"
    );
    assert_eq!(hits.load(Ordering::SeqCst), 0, "never contacted");
}

/// 5.6.17.4: a redirect registration without Merge Entity support is
/// Conflict (never contacted); inclusive-unsupported is skipped while the
/// local merge proceeds.
#[tokio::test(flavor = "multi_thread")]
async fn clause_5_6_17_merge_unsupported_by_registration() {
    let merge = || {
        let body = serde_json::json!({"speed": {"type": "Property", "value": 7}}).to_string();
        Request::builder()
            .method("PATCH")
            .uri(format!("/ngsi-ld/v1/entities/{ENTITY}"))
            .header("Content-Type", "application/json")
            .header("Content-Length", body.len())
            .body(Body::from(body))
            .expect("request")
    };
    let register_with = |st: AppState, mode: &'static str, port: u16| async move {
        let doc = serde_json::json!({
            "id": format!("urn:ngsi-ld:ContextSourceRegistration:mrg-{mode}"),
            "type": "ContextSourceRegistration",
            "mode": mode,
            "operations": ["retrieveOps"],
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

    let (port, hits) = mock_source();
    let st = register_with(state(), "redirect", port).await;
    let res = send(&st, merge()).await;
    assert_eq!(res.status(), StatusCode::CONFLICT, "complete merge failed");
    assert_eq!(hits.load(Ordering::SeqCst), 0, "never contacted");

    // inclusive without support: local merge proceeds, no forward
    let (port, hits) = mock_source();
    let st = register_with(state(), "inclusive", port).await;
    let body = serde_json::json!({"id": ENTITY, "type": "Vehicle",
        "speed": {"type": "Property", "value": 1}})
    .to_string();
    let req = Request::builder()
        .method("POST")
        .uri("/ngsi-ld/v1/entities?local=true")
        .header("Content-Type", "application/json")
        .header("Content-Length", body.len())
        .body(Body::from(body))
        .expect("request");
    assert_eq!(send(&st, req).await.status(), StatusCode::CREATED);
    let res = send(&st, merge()).await;
    assert_eq!(res.status(), StatusCode::NO_CONTENT, "local merge proceeds");
    assert_eq!(hits.load(Ordering::SeqCst), 0, "no forward without support");
}

/// 5.6.18.4: replace honours the ?type selector (mismatch → 404, entity
/// intact) and a redirect registration without Replace Entity support is
/// Conflict, never contacted.
#[tokio::test(flavor = "multi_thread")]
async fn clause_5_6_18_replace_type_selector_and_unsupported() {
    let put = |q: &str| {
        let body = serde_json::json!({"id": ENTITY, "type": "Vehicle",
            "speed": {"type": "Property", "value": 9}})
        .to_string();
        Request::builder()
            .method("PUT")
            .uri(format!("/ngsi-ld/v1/entities/{ENTITY}{q}"))
            .header("Content-Type", "application/json")
            .header("Content-Length", body.len())
            .body(Body::from(body))
            .expect("request")
    };
    // type selector: create locally, replace with wrong selector → 404
    let st = state();
    let body = serde_json::json!({"id": ENTITY, "type": "Vehicle",
        "speed": {"type": "Property", "value": 1}})
    .to_string();
    let req = Request::builder()
        .method("POST")
        .uri("/ngsi-ld/v1/entities")
        .header("Content-Type", "application/json")
        .header("Content-Length", body.len())
        .body(Body::from(body))
        .expect("request");
    assert_eq!(send(&st, req).await.status(), StatusCode::CREATED);
    let res = send(&st, put("?type=Building")).await;
    assert_eq!(res.status(), StatusCode::NOT_FOUND, "wrong selector is 404");
    let res = send(&st, put("?type=Vehicle")).await;
    assert_eq!(
        res.status(),
        StatusCode::NO_CONTENT,
        "matching selector replaces"
    );

    // redirect without replaceEntity: Conflict, never contacted
    let (port, hits) = mock_source();
    let st = state();
    let doc = serde_json::json!({
        "id": "urn:ngsi-ld:ContextSourceRegistration:rep-fb",
        "type": "ContextSourceRegistration",
        "mode": "redirect",
        "operations": ["retrieveOps"],
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
    let res = send(&st, put("")).await;
    assert_eq!(
        res.status(),
        StatusCode::CONFLICT,
        "complete replace failed"
    );
    assert_eq!(hits.load(Ordering::SeqCst), 0, "never contacted");
}

/// 5.6.13-5.6.16 answer through the same 6.3.17/6.3.18 loop check as every
/// other distributed write. A 508 is an error status: it tells the client the
/// operation did not take place, so the local Temporal Evolution has to be
/// exactly what it was before the request arrived.
#[tokio::test(flavor = "multi_thread")]
async fn a_refused_loop_leaves_the_temporal_evolution_untouched() {
    let st = state();
    let body = serde_json::json!({
        "id": ENTITY,
        "type": "Vehicle",
        "speed": [{"type": "Property", "value": 120, "observedAt": "2020-09-01T12:03:00Z"}],
    })
    .to_string();
    let res = send(
        &st,
        Request::builder()
            .method("POST")
            .uri("/ngsi-ld/v1/temporal/entities")
            .header("Content-Type", "application/json")
            .header("Content-Length", body.len())
            .body(Body::from(body))
            .expect("request"),
    )
    .await;
    assert_eq!(res.status(), StatusCode::CREATED, "temporal create");

    let read = || async {
        let res = send(
            &st,
            Request::builder()
                .method("GET")
                .uri(format!(
                    "/ngsi-ld/v1/temporal/entities/{ENTITY}\
                     ?timerel=before&timeAt=2030-01-01T00:00:00Z&options=sysAttrs"
                ))
                .body(Body::empty())
                .expect("request"),
        )
        .await;
        let bytes = axum::body::to_bytes(res.into_body(), usize::MAX)
            .await
            .expect("body");
        serde_json::from_slice::<serde_json::Value>(&bytes).unwrap_or(serde_json::Value::Null)
    };

    let before = read().await;
    let iid = before["speed"][0]["instanceId"]
        .as_str()
        .expect("instanceId")
        .to_owned();

    // A Via chain past the broker's hop ceiling is refused before any source
    // is consulted, so the 508 is reachable without a registration at all.
    let via = vec!["1.1 a"; 33].join(", ");
    let fragment = serde_json::json!({"type": "Property", "value": 129}).to_string();
    let cases = [
        (
            "DELETE",
            format!("/ngsi-ld/v1/temporal/entities/{ENTITY}"),
            None,
        ),
        (
            "DELETE",
            format!("/ngsi-ld/v1/temporal/entities/{ENTITY}/attrs/speed?deleteAll=true"),
            None,
        ),
        (
            "PATCH",
            format!("/ngsi-ld/v1/temporal/entities/{ENTITY}/attrs/speed/{iid}"),
            Some(fragment),
        ),
        (
            "DELETE",
            format!("/ngsi-ld/v1/temporal/entities/{ENTITY}/attrs/speed/{iid}"),
            None,
        ),
    ];
    for (method, uri, payload) in cases {
        let mut req = Request::builder()
            .method(method)
            .uri(&uri)
            .header("Via", &via);
        let sent = match payload {
            Some(p) => {
                req = req
                    .header("Content-Type", "application/json")
                    .header("Content-Length", p.len());
                Body::from(p)
            }
            None => Body::empty(),
        };
        let res = send(&st, req.body(sent).expect("request")).await;
        assert_eq!(res.status(), StatusCode::LOOP_DETECTED, "{method} {uri}");
        assert_eq!(
            read().await,
            before,
            "a refused {method} {uri} still changed the stored Temporal Evolution"
        );
    }
}

/// 5.6.21 Purge Entities answers through the same loop check, and it deletes
/// a whole page rather than one Entity — the same 508 that means "nothing
/// happened" was returned over an emptied store.
#[tokio::test(flavor = "multi_thread")]
async fn a_refused_loop_leaves_the_purged_entities_in_place() {
    let st = state();
    for i in 0..3 {
        let body =
            serde_json::json!({"id": format!("urn:ngsi-ld:Vehicle:p{i}"), "type": "Vehicle"})
                .to_string();
        let res = send(
            &st,
            Request::builder()
                .method("POST")
                .uri("/ngsi-ld/v1/entities")
                .header("Content-Type", "application/json")
                .header("Content-Length", body.len())
                .body(Body::from(body))
                .expect("request"),
        )
        .await;
        assert_eq!(res.status(), StatusCode::CREATED, "seed {i}");
    }
    let count = || async {
        let res = send(
            &st,
            Request::builder()
                .method("GET")
                .uri("/ngsi-ld/v1/entities?type=Vehicle")
                .body(Body::empty())
                .expect("request"),
        )
        .await;
        let bytes = axum::body::to_bytes(res.into_body(), usize::MAX)
            .await
            .expect("body");
        serde_json::from_slice::<serde_json::Value>(&bytes)
            .ok()
            .and_then(|v| v.as_array().map(Vec::len))
            .unwrap_or_default()
    };
    assert_eq!(count().await, 3, "seeded");

    let res = send(
        &st,
        Request::builder()
            .method("DELETE")
            .uri("/ngsi-ld/v1/entities?type=Vehicle")
            .header("Via", vec!["1.1 a"; 33].join(", "))
            .body(Body::empty())
            .expect("request"),
    )
    .await;
    assert_eq!(
        res.status(),
        StatusCode::LOOP_DETECTED,
        "purge under a loop"
    );
    assert_eq!(
        count().await,
        3,
        "a refused purge still emptied the Entity store"
    );
}
