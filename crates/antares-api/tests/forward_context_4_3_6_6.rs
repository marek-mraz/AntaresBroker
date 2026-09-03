// SPDX-License-Identifier: EUPL-1.2
//! 4.3.6.6 `contextSourceInfo` `jsonldContext` on a forwarded write.
//!
//! "The Context Broker shall apply a compaction operation … over both
//! payload and query parameters using the JSON-LD Context supplied in the
//! value of the `jsonldContext` key-value pair, prior to distributing the
//! request to the context source endpoint AND FORWARDING WITH THIS JSON-LD
//! CONTEXT" — one rule, not two. A forward that advertises the registered
//! @context while carrying terms compacted against a different one hands the
//! Context Source a payload it will expand to the wrong Fully Qualified
//! Names (5.5.7), and the write lands on Attributes the client never named.

use antares_api::AppState;
use axum::body::Body;
use axum::http::Request;
use std::io::{Read, Write};
use std::sync::{Arc, Mutex};
use tower::ServiceExt;

const ENTITY: &str = "urn:ngsi-ld:Vehicle:ctxfwd";

/// The registered @context: it binds the term `speed` to a DIFFERENT IRI and
/// names the client's `speed` `velocity`. A payload compacted against it is
/// therefore not merely spelled differently — a payload that was NOT
/// compacted against it expands, at the Context Source, to
/// `http://example.org/other#speed`: another Attribute entirely.
fn context_server() -> u16 {
    let doc = r#"{"@context":{"speed":"http://example.org/other#speed","velocity":"https://uri.etsi.org/ngsi-ld/default-context/speed"}}"#;
    let reply = format!(
        "HTTP/1.1 200 OK\r\nContent-Type: application/ld+json\r\n\
         Content-Length: {}\r\nConnection: close\r\n\r\n{doc}",
        doc.len()
    );
    serve(reply)
}

/// Mock Context Source: records the head and body of the last request it saw
/// and answers 204.
struct Mock {
    port: u16,
    head: Arc<Mutex<String>>,
    body: Arc<Mutex<String>>,
}

fn mock_source() -> Mock {
    let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("bind");
    let port = listener.local_addr().expect("addr").port();
    let head: Arc<Mutex<String>> = Arc::default();
    let body: Arc<Mutex<String>> = Arc::default();
    let (h, b) = (head.clone(), body.clone());
    std::thread::spawn(move || {
        for stream in listener.incoming() {
            let Ok(mut s) = stream else { continue };
            let text = read_request(&mut s);
            let (rh, rb) = text.split_once("\r\n\r\n").unwrap_or((text.as_str(), ""));
            *h.lock().expect("lock") = rh.to_owned();
            *b.lock().expect("lock") = rb.to_owned();
            let _ = s.write_all(
                b"HTTP/1.1 204 No Content\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
            );
        }
    });
    Mock { port, head, body }
}

fn serve(reply: String) -> u16 {
    let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("bind");
    let port = listener.local_addr().expect("addr").port();
    std::thread::spawn(move || {
        for stream in listener.incoming() {
            let Ok(mut s) = stream else { continue };
            let _ = read_request(&mut s);
            let _ = s.write_all(reply.as_bytes());
        }
    });
    port
}

fn read_request(s: &mut std::net::TcpStream) -> String {
    let mut raw = Vec::new();
    let mut buf = [0u8; 8192];
    loop {
        let n = s.read(&mut buf).unwrap_or(0);
        if n == 0 {
            break;
        }
        raw.extend_from_slice(&buf[..n]);
        let text = String::from_utf8_lossy(&raw).into_owned();
        let Some((h, b)) = text.split_once("\r\n\r\n") else {
            continue;
        };
        let want: usize = h
            .lines()
            .find_map(|l| {
                let l = l.to_ascii_lowercase();
                l.strip_prefix("content-length:")
                    .and_then(|v| v.trim().parse().ok())
            })
            .unwrap_or(0);
        if b.len() >= want {
            break;
        }
    }
    String::from_utf8_lossy(&raw).into_owned()
}

fn state() -> AppState {
    antares_jsonld::allow_private_egress(true);
    AppState::new("antares-ctxfwd".into())
}

async fn send(st: &AppState, method: &str, uri: &str, body: Option<String>) -> (u16, String) {
    let mut req = Request::builder().method(method).uri(uri);
    let b = match body {
        Some(b) => {
            req = req
                .header("Content-Type", "application/json")
                .header("Content-Length", b.len());
            Body::from(b)
        }
        None => Body::empty(),
    };
    let res = antares_api::router(st.clone())
        .oneshot(req.body(b).expect("request"))
        .await
        .expect("response");
    let status = res.status().as_u16();
    let bytes = axum::body::to_bytes(res.into_body(), usize::MAX)
        .await
        .expect("body");
    (status, String::from_utf8_lossy(&bytes).into_owned())
}

async fn register(st: &AppState, port: u16, ctx_url: &str, ops: &[&str]) {
    let doc = serde_json::json!({
        "id": "urn:ngsi-ld:ContextSourceRegistration:ctxfwd",
        "type": "ContextSourceRegistration",
        "mode": "redirect",
        "operations": ops,
        "information": [{"entities": [{"type": "Vehicle", "id": ENTITY}]}],
        "endpoint": format!("http://127.0.0.1:{port}"),
        "contextSourceInfo": [{"key": "jsonldContext", "value": ctx_url}],
    });
    let (status, body) = send(
        st,
        "POST",
        "/ngsi-ld/v1/csourceRegistrations",
        Some(doc.to_string()),
    )
    .await;
    assert_eq!(status, 201, "registration create: {body}");
}

/// The Link header a forward advertised, if any.
fn link_of(head: &str) -> Option<String> {
    head.lines()
        .find(|l| l.to_ascii_lowercase().starts_with("link:"))
        .map(|l| l[5..].trim().to_owned())
}

/// 4.3.6.6 positive control: a payload the broker CAN recompact travels with
/// the registered @context and the registered context's terms.
#[tokio::test(flavor = "multi_thread")]
async fn a_recompacted_write_forwards_the_registered_context() {
    let st = state();
    let ctx = format!("http://127.0.0.1:{}/ctx.jsonld", context_server());
    let m = mock_source();
    register(&st, m.port, &ctx, &["createEntity"]).await;

    let ent = serde_json::json!({"id": ENTITY, "type": "Vehicle",
        "speed": {"type": "Property", "value": 10}});
    let (status, body) = send(&st, "POST", "/ngsi-ld/v1/entities", Some(ent.to_string())).await;
    assert!(status < 300, "create: {status} {body}");

    let head = m.head.lock().expect("lock").clone();
    let sent = m.body.lock().expect("lock").clone();
    assert!(
        link_of(&head).is_some_and(|l| l.contains(&ctx)),
        "the forward advertises the registered @context: {head}"
    );
    assert!(
        sent.contains("velocity") && !sent.contains("\"speed\""),
        "the payload is compacted against it: {sent}"
    );
}

/// 4.3.6.6: the compaction and the advertised @context are one rule. When the
/// payload cannot be recompacted, the forward must degrade to the @context
/// the terms ARE in — the same degradation the clause's own binding note
/// forces when the registered context cannot be loaded. A merge fragment
/// (5.6.17) carrying an NGSI-LD Null is such a payload.
#[tokio::test(flavor = "multi_thread")]
async fn a_forward_never_advertises_a_context_its_terms_are_not_in() {
    let st = state();
    let ctx = format!("http://127.0.0.1:{}/ctx.jsonld", context_server());
    let m = mock_source();
    register(&st, m.port, &ctx, &["mergeEntity"]).await;

    let frag = serde_json::json!({"speed": "urn:ngsi-ld:null"});
    let (status, body) = send(
        &st,
        "PATCH",
        &format!("/ngsi-ld/v1/entities/{ENTITY}"),
        Some(frag.to_string()),
    )
    .await;
    assert!(status < 400, "merge: {status} {body}");

    let head = m.head.lock().expect("lock").clone();
    let sent = m.body.lock().expect("lock").clone();
    assert!(!head.is_empty(), "the merge must reach the Context Source");
    let advertised_registered = link_of(&head).is_some_and(|l| l.contains(&ctx));
    let carries_registered_terms = sent.contains("velocity");
    assert_eq!(
        advertised_registered, carries_registered_terms,
        "the advertised @context and the payload's terms must agree \
         (4.3.6.6); head: {head}\nbody: {sent}"
    );
}

/// The request line of the forward the mock saw.
fn target_of(head: &str) -> String {
    head.lines()
        .next()
        .and_then(|l| l.split_whitespace().nth(1))
        .unwrap_or_default()
        .to_owned()
}

/// A 5.6.4 Attribute Fragment (`{"type": "Property", "value": …}`) is a body
/// shape the recompaction also sees, and it is NOT an Entity: read as one,
/// its `type` is an Entity Type and its `value` an Attribute name. It is read
/// with `expand_attr_fragment` and written back with `compact_instance`, so
/// its sub-Attributes are translated and its reserved members are not.
///
/// The Attribute the request names travels in the PATH (6.6), which the
/// Context Source expands with the @context the forward advertises: the
/// segment is translated with the payload, or the write lands on an
/// Attribute the client never named. Here the client's `speed` and the
/// registered context's `velocity` are the same Fully Qualified Name.
#[tokio::test(flavor = "multi_thread")]
async fn an_attribute_fragment_travels_as_a_fragment_and_its_path_is_translated() {
    let st = state();
    let ctx = format!("http://127.0.0.1:{}/ctx.jsonld", context_server());
    let m = mock_source();
    register(&st, m.port, &ctx, &["updateAttrs"]).await;

    // the sub-Attribute is `speed` again, so a fragment read as a fragment
    // translates it and a fragment read as an Entity could not
    let frag = serde_json::json!({
        "type": "Property",
        "value": 42,
        "speed": {"type": "Property", "value": 1},
    });
    let (status, body) = send(
        &st,
        "PATCH",
        &format!("/ngsi-ld/v1/entities/{ENTITY}/attrs/speed"),
        Some(frag.to_string()),
    )
    .await;
    assert!(status < 400, "partial update: {status} {body}");

    let head = m.head.lock().expect("lock").clone();
    let sent = m.body.lock().expect("lock").clone();
    assert!(!head.is_empty(), "the update must reach the Context Source");
    assert_eq!(
        link_of(&head),
        Some(format!(
            "<{ctx}>; rel=\"http://www.w3.org/ns/json-ld#context\"; type=\"application/ld+json\""
        )),
        "4.3.6.6 forwards WITH the registered @context: {head}"
    );
    assert!(
        target_of(&head).contains("/attrs/velocity"),
        "the path names the Attribute in the registered @context: {head}"
    );
    let sent: serde_json::Value = serde_json::from_str(&sent).expect("forwarded body is JSON");
    assert_eq!(
        sent,
        serde_json::json!({
            "type": "Property",
            "value": 42,
            "velocity": {"type": "Property", "value": 1},
        }),
        "the fragment stays a fragment and its sub-Attribute is translated: {sent}"
    );
}

/// 5.6.5 delete Attribute carries NO payload, so nothing about the body can
/// tell the broker which Attribute the request is about — only the path can.
/// A forward that switched the @context and left the segment alone would
/// delete a different Attribute at the Context Source, or none.
#[tokio::test(flavor = "multi_thread")]
async fn a_delete_of_a_named_attribute_translates_its_path() {
    let st = state();
    let ctx = format!("http://127.0.0.1:{}/ctx.jsonld", context_server());
    let m = mock_source();
    register(&st, m.port, &ctx, &["deleteAttrs"]).await;

    let (status, body) = send(
        &st,
        "DELETE",
        &format!("/ngsi-ld/v1/entities/{ENTITY}/attrs/speed"),
        None,
    )
    .await;
    assert!(status < 400, "delete attribute: {status} {body}");

    let head = m.head.lock().expect("lock").clone();
    assert!(!head.is_empty(), "the delete must reach the Context Source");
    let target = target_of(&head);
    assert!(
        target.contains("/attrs/velocity"),
        "the deleted Attribute is named in the registered @context: {head}"
    );
    assert!(
        !target.contains("/attrs/speed"),
        "`speed` under the registered @context is another Attribute entirely: {head}"
    );
}
