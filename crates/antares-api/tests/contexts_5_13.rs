// SPDX-License-Identifier: EUPL-1.2
//! 5.13 Storing, Managing and Serving @contexts — wire-level tests through
//! the router: Add (5.13.2), List (5.13.3), Serve (5.13.4) and
//! Delete/Reload (5.13.5) against the memory store.

use antares_api::AppState;
use axum::body::Body;
use axum::http::{Request, StatusCode};
use serde_json::{json, Value};
use tower::ServiceExt;

fn state() -> AppState {
    AppState::new("antares-ctx".into())
}

async fn send(st: &AppState, req: Request<Body>) -> (StatusCode, axum::http::HeaderMap, Value) {
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

async fn post_ctx(st: &AppState, body: Value) -> (StatusCode, axum::http::HeaderMap, Value) {
    let body = body.to_string();
    send(
        st,
        Request::builder()
            .method("POST")
            .uri("/ngsi-ld/v1/jsonldContexts")
            .header("Content-Type", "application/json")
            .header("Content-Length", body.len())
            .body(Body::from(body))
            .expect("request"),
    )
    .await
}

async fn get(st: &AppState, uri: &str) -> (StatusCode, axum::http::HeaderMap, Value) {
    send(
        st,
        Request::builder()
            .method("GET")
            .uri(uri)
            .body(Body::empty())
            .expect("request"),
    )
    .await
}

async fn delete(st: &AppState, uri: &str) -> (StatusCode, axum::http::HeaderMap, Value) {
    send(
        st,
        Request::builder()
            .method("DELETE")
            .uri(uri)
            .body(Body::empty())
            .expect("request"),
    )
    .await
}

/// Add a valid hosted @context, returning its Location path.
async fn add_hosted(st: &AppState) -> String {
    let (status, headers, _) =
        post_ctx(st, json!({"@context": {"A1": "urn:ngsi-ld:attributes:A1"}})).await;
    assert_eq!(status, StatusCode::CREATED);
    headers
        .get("Location")
        .and_then(|l| l.to_str().ok())
        .expect("Location header")
        .to_owned()
}

/// 5.13.2.4: "The behaviour described in clause 5.5.4 about JSON and JSON-LD
/// validation shall be applied in case of invalid @context." A top-level
/// @context that is not a string/object/array-of-those is not valid JSON-LD.
#[tokio::test(flavor = "multi_thread")]
async fn clause_5_13_2_4_invalid_context_value_is_400() {
    let st = state();
    for bad in [
        json!({"@context": 42}),
        json!({"@context": true}),
        json!({"@context": ["https://example.org/ctx.jsonld", 7]}),
    ] {
        let (status, headers, body) = post_ctx(&st, bad.clone()).await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "payload {bad}");
        assert_eq!(
            body["type"], "https://uri.etsi.org/ngsi-ld/errors/BadRequestData",
            "payload {bad}: {body}"
        );
        // an invalid @context must NOT have been stored
        assert!(headers.get("Location").is_none(), "payload {bad}");
    }
    // negative control: nothing invalid leaked into the store
    let (_, _, list) = get(&st, "/ngsi-ld/v1/jsonldContexts?kind=Hosted").await;
    assert_eq!(list, json!([]));
}

/// 5.13.2.3/.4/.5: extra members outside @context are discarded, the entry
/// is flagged Hosted, and a locally unique URI comes back (Location).
#[tokio::test(flavor = "multi_thread")]
async fn clause_5_13_2_hosted_add_serve_roundtrip() {
    let st = state();
    let (status, headers, _) = post_ctx(
        &st,
        json!({"@context": {"A1": "urn:ngsi-ld:attributes:A1"}, "junk": "outside the @context subtree"}),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);
    let loc = headers
        .get("Location")
        .and_then(|l| l.to_str().ok())
        .expect("Location header")
        .to_owned();
    assert!(loc.starts_with("/ngsi-ld/v1/jsonldContexts/"), "{loc}");

    // 5.13.4.4: full content served for Hosted
    let (status, _, body) = get(&st, &loc).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        body["@context"]["A1"], "urn:ngsi-ld:attributes:A1",
        "{body}"
    );
    // 5.13.2.3: "all extra information located outside of the @context
    // subtree ... shall be discarded"
    assert!(body.get("junk").is_none(), "{body}");

    // 5.13.4.4 details=true: metadata per 5.13.3.5
    let (status, _, meta) = get(&st, &format!("{loc}?details=true")).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(meta["kind"], "Hosted", "{meta}");
    assert!(meta["localId"].is_string(), "{meta}");
    assert!(meta["URL"].is_string(), "{meta}");
    assert!(meta["createdAt"].is_string(), "{meta}");
    // metadata is not the content
    assert!(meta.get("@context").is_none(), "{meta}");
}

/// 5.13.3.3/.4: kind filter applied; unknown kind / bad details flag → 400.
#[tokio::test(flavor = "multi_thread")]
async fn clause_5_13_3_list_and_kind_filter() {
    let st = state();
    let loc = add_hosted(&st).await;
    let local_id = loc.rsplit('/').next().expect("id");

    // simple list: URLs (strings), containing the hosted entry
    let (status, _, list) = get(&st, "/ngsi-ld/v1/jsonldContexts").await;
    assert_eq!(status, StatusCode::OK);
    let urls: Vec<&str> = list
        .as_array()
        .expect("array")
        .iter()
        .filter_map(Value::as_str)
        .collect();
    assert!(
        urls.iter().any(|u| u.ends_with(local_id)),
        "hosted URL missing from {urls:?}"
    );

    // details list restricted to Hosted
    let (status, _, list) = get(&st, "/ngsi-ld/v1/jsonldContexts?kind=Hosted&details=true").await;
    assert_eq!(status, StatusCode::OK);
    let entry = &list.as_array().expect("array")[0];
    assert_eq!(entry["kind"], "Hosted");
    assert_eq!(entry["localId"], *local_id);

    // 5.13.3.4: the Hosted entry must NOT match a Cached filter
    let (status, _, list) = get(&st, "/ngsi-ld/v1/jsonldContexts?kind=Cached").await;
    assert_eq!(status, StatusCode::OK);
    assert!(
        !list
            .as_array()
            .expect("array")
            .iter()
            .filter_map(Value::as_str)
            .any(|u| u.ends_with(local_id)),
        "{list}"
    );

    // invalid kind / invalid details flag → 400
    for uri in [
        "/ngsi-ld/v1/jsonldContexts?kind=Bogus",
        "/ngsi-ld/v1/jsonldContexts?details=banana",
    ] {
        let (status, _, body) = get(&st, uri).await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "{uri}");
        assert_eq!(
            body["type"], "https://uri.etsi.org/ngsi-ld/errors/BadRequestData",
            "{uri}: {body}"
        );
    }
}

/// 5.13.4.4: unknown id → 404 ResourceNotFound; Cached (here: the pinned
/// core context) is never served on demand → 422 OperationNotSupported,
/// while its metadata stays retrievable.
#[tokio::test(flavor = "multi_thread")]
async fn clause_5_13_4_serve_errors() {
    let st = state();

    let (status, _, body) = get(&st, "/ngsi-ld/v1/jsonldContexts/urn:ngsi-ld:none").await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    assert_eq!(
        body["type"], "https://uri.etsi.org/ngsi-ld/errors/ResourceNotFound",
        "{body}"
    );

    let core = "https%3A%2F%2Furi.etsi.org%2Fngsi-ld%2Fv1%2Fngsi-ld-core-context-v1.8.jsonld";
    let (status, _, body) = get(&st, &format!("/ngsi-ld/v1/jsonldContexts/{core}")).await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{body}");
    assert_eq!(
        body["type"], "https://uri.etsi.org/ngsi-ld/errors/OperationNotSupported",
        "{body}"
    );

    let (status, _, meta) = get(
        &st,
        &format!("/ngsi-ld/v1/jsonldContexts/{core}?details=true"),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(meta["kind"], "Cached", "{meta}");
    // metadata, never the content
    assert!(meta.get("@context").is_none(), "{meta}");
}

/// 5.13.5.4: delete → 204 (no body) and the entry is gone; second delete →
/// 404; reload on a non-Cached kind → 400; malformed reload flag → 400;
/// unknown id without reload → 404.
#[tokio::test(flavor = "multi_thread")]
async fn clause_5_13_5_delete_and_reload_errors() {
    let st = state();
    let loc = add_hosted(&st).await;

    // reload=true on a Hosted @context → 400 (kind is not "Cached")
    let (status, _, body) = delete(&st, &format!("{loc}?reload=true")).await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
    assert_eq!(
        body["type"], "https://uri.etsi.org/ngsi-ld/errors/BadRequestData",
        "{body}"
    );
    // ... and the failed reload must NOT have deleted it
    let (status, _, _) = get(&st, &loc).await;
    assert_eq!(status, StatusCode::OK);

    // malformed reload value → 400
    let (status, _, _) = delete(&st, &format!("{loc}?reload=xxx")).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);

    // plain delete → 204 with an empty body
    let (status, _, body) = delete(&st, &loc).await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    assert_eq!(body, Value::Null);

    // gone from serve and from the list
    let (status, _, _) = get(&st, &loc).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    let (_, _, list) = get(&st, "/ngsi-ld/v1/jsonldContexts?kind=Hosted").await;
    assert_eq!(list, json!([]));

    // second delete → 404 ResourceNotFound
    let (status, _, body) = delete(&st, &loc).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    assert_eq!(
        body["type"], "https://uri.etsi.org/ngsi-ld/errors/ResourceNotFound",
        "{body}"
    );

    // unknown id, no reload → 404 (5.13.5.4)
    let (status, _, _) = delete(&st, "/ngsi-ld/v1/jsonldContexts/urn:ngsi-ld:none").await;
    assert_eq!(status, StatusCode::NOT_FOUND);

    // unknown id with reload=true → 400: the reload bullets only distinguish
    // Cached vs not-Cached (5.13.5.4), and the 404 rule is restated inside
    // the reload=false bullet — the official suite pins this reading
    // (051_04_01).
    let (status, _, _) = delete(
        &st,
        "/ngsi-ld/v1/jsonldContexts/urn:ngsi-ld:none?reload=true",
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
}

/// Request carrying an explicit NGSILD-Tenant (6.3.14).
fn tenant_req(method: &str, uri: &str, tenant: &str, body: Option<Value>) -> Request<Body> {
    let b = Request::builder()
        .method(method)
        .uri(uri)
        .header("NGSILD-Tenant", tenant);
    match body {
        None => b.body(Body::empty()).expect("request"),
        Some(v) => {
            let s = v.to_string();
            b.header("Content-Type", "application/json")
                .header("Content-Length", s.len())
                .body(Body::from(s))
                .expect("request")
        }
    }
}

/// Add a Hosted @context on behalf of `tenant`, returning its Location path.
async fn add_hosted_as(st: &AppState, tenant: &str, term: &str) -> String {
    let (status, headers, _) = send(
        st,
        tenant_req(
            "POST",
            "/ngsi-ld/v1/jsonldContexts",
            tenant,
            Some(json!({"@context": {term: format!("urn:ngsi-ld:attributes:{term}")}})),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);
    headers
        .get("Location")
        .and_then(|l| l.to_str().ok())
        .expect("Location header")
        .to_owned()
}

/// 5.13.2.4 stores the @context supplied by a requesting client and 5.13.4.4
/// serves it back; the client is identified by its tenant (6.3.14), so a
/// Hosted @context belongs to the tenant that added it. For any other tenant
/// it must be exactly as absent as an @context that never existed: not
/// served, not listed, not deletable. Cached @contexts are copies of public
/// documents the broker fetched and stay visible to every tenant (5.13.1).
#[tokio::test(flavor = "multi_thread")]
async fn clause_5_13_hosted_context_is_private_to_its_tenant() {
    let st = state();
    let loc_a = add_hosted_as(&st, "tenant-a", "A1").await;
    let id_a = loc_a.rsplit('/').next().expect("id").to_owned();
    let loc_b = add_hosted_as(&st, "tenant-b", "B1").await;
    let id_b = loc_b.rsplit('/').next().expect("id").to_owned();

    // 5.13.4.4: tenant B must NOT be served tenant A's term mappings
    let (status, _, body) = send(&st, tenant_req("GET", &loc_a, "tenant-b", None)).await;
    assert_eq!(status, StatusCode::NOT_FOUND, "{body}");
    assert_eq!(
        body["type"], "https://uri.etsi.org/ngsi-ld/errors/ResourceNotFound",
        "{body}"
    );
    assert!(!body.to_string().contains("A1"), "leaked mappings: {body}");
    // ... not even the metadata (createdAt/URL/localId of another tenant)
    let (status, _, body) = send(
        &st,
        tenant_req("GET", &format!("{loc_a}?details=true"), "tenant-b", None),
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND, "{body}");

    // 5.13.3.4: the details listing must NOT expose the other tenant's URLs
    let (status, _, list) = send(
        &st,
        tenant_req(
            "GET",
            "/ngsi-ld/v1/jsonldContexts?details=true",
            "tenant-b",
            None,
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let dump = list.to_string();
    assert!(!dump.contains(&id_a), "tenant A entry listed to B: {list}");
    assert!(dump.contains(&id_b), "tenant B's own entry missing: {list}");
    // the simple (URL-only) listing leaks nothing either
    let (_, _, list) = send(
        &st,
        tenant_req("GET", "/ngsi-ld/v1/jsonldContexts", "tenant-b", None),
    )
    .await;
    assert!(!list.to_string().contains(&id_a), "{list}");

    // 5.13.5.4: tenant B must NOT be able to delete tenant A's @context
    let (status, _, body) = send(&st, tenant_req("DELETE", &loc_a, "tenant-b", None)).await;
    assert_eq!(status, StatusCode::NOT_FOUND, "{body}");
    let (status, _, body) = send(&st, tenant_req("GET", &loc_a, "tenant-a", None)).await;
    assert_eq!(status, StatusCode::OK, "owner lost its @context: {body}");
    assert_eq!(
        body["@context"]["A1"], "urn:ngsi-ld:attributes:A1",
        "{body}"
    );

    // the default tenant is just another tenant here
    let (status, _, _) = get(&st, &loc_a).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    let (_, _, list) = get(&st, "/ngsi-ld/v1/jsonldContexts?kind=Hosted").await;
    assert_eq!(list, json!([]), "{list}");

    // 5.13.1: Cached @contexts are broker-fetched copies of public documents
    // — the pinned core context stays visible to every tenant
    let core = "https%3A%2F%2Furi.etsi.org%2Fngsi-ld%2Fv1%2Fngsi-ld-core-context-v1.8.jsonld";
    let (status, _, meta) = send(
        &st,
        tenant_req(
            "GET",
            &format!("/ngsi-ld/v1/jsonldContexts/{core}?details=true"),
            "tenant-b",
            None,
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{meta}");
    assert_eq!(meta["kind"], "Cached", "{meta}");

    // and the owner can still delete its own entry (5.13.5.4)
    let (status, _, _) = send(&st, tenant_req("DELETE", &loc_b, "tenant-b", None)).await;
    assert_eq!(status, StatusCode::NO_CONTENT);
}

/// 5.5.10: "If a Tenant is specified for an NGSI-LD operation, the operation
/// shall only be applied to information related to the specified Tenant." A
/// Hosted @context is stored by one Tenant (5.13.1), so another Tenant naming
/// its URL in a Link header must not have its payload expanded by those
/// mappings — the serve endpoint being Tenant-gated is not enough on its own,
/// since resolution is what decides how the request body is read.
#[tokio::test(flavor = "multi_thread")]
async fn clause_5_5_10_hosted_context_does_not_resolve_for_another_tenant() {
    let st = state();
    let loc = add_hosted_as(&st, "alpha", "A1").await;
    let (_, _, meta) = send(
        &st,
        tenant_req("GET", &format!("{loc}?details=true"), "alpha", None),
    )
    .await;
    let url = meta["URL"]
        .as_str()
        .expect("stored @context URL")
        .to_owned();
    let link = format!(
        "<{url}>; rel=\"http://www.w3.org/ns/json-ld#context\"; type=\"application/ld+json\""
    );

    let entity = json!({
        "id": "urn:ngsi-ld:Device:ctx-tenant",
        "type": "Device",
        "A1": {"type": "Property", "value": 1},
    });
    let post = |tenant: &str| {
        let body = entity.to_string();
        Request::builder()
            .method("POST")
            .uri("/ngsi-ld/v1/entities")
            .header("NGSILD-Tenant", tenant)
            .header("Content-Type", "application/json")
            .header("Content-Length", body.len())
            .header("Link", &link)
            .body(Body::from(body))
            .expect("request")
    };

    // the owner resolves its own @context: the term expands to the mapping
    let (status, _, body) = send(&st, post("alpha")).await;
    assert_eq!(status, StatusCode::CREATED, "{body}");
    let (status, _, stored) = send(
        &st,
        tenant_req(
            "GET",
            "/ngsi-ld/v1/entities/urn:ngsi-ld:Device:ctx-tenant",
            "alpha",
            None,
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{stored}");
    assert!(
        stored.get("urn:ngsi-ld:attributes:A1").is_some(),
        "the owner's term must have expanded through its own @context: {stored}"
    );

    // another Tenant naming the same URL: 5.5.6 — the @context is not
    // available to it, and nothing is stored under its Tenant
    let (status, _, body) = send(&st, post("beta")).await;
    assert_eq!(
        status,
        StatusCode::GATEWAY_TIMEOUT,
        "another Tenant must not resolve a Hosted @context it does not own: {body}"
    );
    assert_eq!(
        body["type"], "https://uri.etsi.org/ngsi-ld/errors/LdContextNotAvailable",
        "{body}"
    );
    let (status, _, body) = send(
        &st,
        tenant_req(
            "GET",
            "/ngsi-ld/v1/entities/urn:ngsi-ld:Device:ctx-tenant",
            "beta",
            None,
        ),
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND, "nothing was stored: {body}");
}
