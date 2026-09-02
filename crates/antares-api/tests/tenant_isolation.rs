// SPDX-License-Identifier: EUPL-1.2
//! Tenant isolation, the router-level half: no existence oracle across
//! tenants and tenant-keyed store state. The RLS denial half runs against
//! live Postgres in `antares-sql/tests`; the NATS subject re-verification
//! half lands with the NATS messaging backend.

use antares_api::AppState;
use axum::body::Body;
use axum::http::{Request, StatusCode};
use http_body_util::BodyExt;
use tower::ServiceExt;

async fn send(
    st: &AppState,
    method: &str,
    path: &str,
    tenant: Option<&str>,
    body: Option<&str>,
) -> (StatusCode, String) {
    let mut req = Request::builder().method(method).uri(path);
    if let Some(t) = tenant {
        req = req.header("NGSILD-Tenant", t);
    }
    if let Some(b) = body {
        req = req
            .header("Content-Type", "application/json")
            // 6.3.4: body-bearing methods without Content-Length are a bare 411
            .header("Content-Length", b.len());
    }
    let req = req
        .body(body.map_or(Body::empty(), |b| Body::from(b.to_owned())))
        .expect("request");
    let resp = antares_api::router(st.clone())
        .oneshot(req)
        .await
        .expect("response");
    let status = resp.status();
    let bytes = resp.into_body().collect().await.expect("body").to_bytes();
    (status, String::from_utf8_lossy(&bytes).into_owned())
}

async fn create_entity(st: &AppState, tenant: &str, id: &str) {
    let doc = format!(r#"{{"id":"{id}","type":"Isolation"}}"#);
    let (status, body) = send(st, "POST", "/ngsi-ld/v1/entities", Some(tenant), Some(&doc)).await;
    assert_eq!(status, StatusCode::CREATED, "seed {id} in {tenant}: {body}");
}

/// For a tenant-B observer, an id that exists in tenant A must be
/// EXACTLY as absent as an id that exists nowhere — same status, same body
/// shape. A difference is an existence oracle across the tenant boundary.
#[tokio::test(flavor = "multi_thread")]
async fn cross_tenant_probe_is_indistinguishable_from_nonexistence() {
    let st = AppState::new("test".into());
    create_entity(&st, "tenant-a", "urn:ngsi-ld:Isolation:in-a").await;
    // tenant B must exist (a read against an unknown tenant is
    // NonexistentTenant by 6.3.14 — a different, correct 404)
    create_entity(&st, "tenant-b", "urn:ngsi-ld:Isolation:b-seed").await;

    let (s_cross, b_cross) = send(
        &st,
        "GET",
        "/ngsi-ld/v1/entities/urn:ngsi-ld:Isolation:in-a",
        Some("tenant-b"),
        None,
    )
    .await;
    let (s_ghost, b_ghost) = send(
        &st,
        "GET",
        "/ngsi-ld/v1/entities/urn:ngsi-ld:Isolation:in-a", // same id, never in B
        Some("tenant-c-never-written"),
        None,
    )
    .await;
    // the cross-tenant probe: 404, never 200
    assert_eq!(s_cross, StatusCode::NOT_FOUND);
    // same id probed in a tenant where it CAN'T exist gives some 404 too;
    // the bodies may differ only in the *tenant* dimension (NonexistentTenant
    // vs ResourceNotFound is spec-mandated 6.3.14) — so compare within ONE
    // existing tenant instead:
    let (s_none, b_none) = send(
        &st,
        "GET",
        "/ngsi-ld/v1/entities/urn:ngsi-ld:Isolation:nowhere",
        Some("tenant-b"),
        None,
    )
    .await;
    assert_eq!(s_none, StatusCode::NOT_FOUND);
    assert_eq!(
        b_cross.replace("urn:ngsi-ld:Isolation:in-a", "{id}"),
        b_none.replace("urn:ngsi-ld:Isolation:nowhere", "{id}"),
        "existing-elsewhere and existing-nowhere must be the same 404"
    );
    // the tenant dimension is the one place the two 404s legitimately
    // differ (6.3.14); it must not blur into the document dimension
    assert_eq!(s_ghost, StatusCode::NOT_FOUND);
    assert!(
        b_ghost.contains("NonexistentTenant"),
        "an unknown tenant answers NonexistentTenant, not ResourceNotFound: {b_ghost}"
    );

    // same property for subscriptions and registrations
    for (path, seed_body) in [
        (
            "/ngsi-ld/v1/subscriptions",
            r#"{"id":"urn:ngsi-ld:Subscription:iso","type":"Subscription",
                "entities":[{"type":"Isolation"}],
                "notification":{"endpoint":{"uri":"http://localhost:9/never"}}}"#,
        ),
        (
            "/ngsi-ld/v1/csourceRegistrations",
            r#"{"id":"urn:ngsi-ld:ContextSourceRegistration:iso",
                "type":"ContextSourceRegistration",
                "information":[{"entities":[{"type":"Isolation"}]}],
                "endpoint":"http://localhost:9/never"}"#,
        ),
    ] {
        let (status, body) = send(&st, "POST", path, Some("tenant-a"), Some(seed_body)).await;
        assert_eq!(status, StatusCode::CREATED, "seed {path}: {body}");
        let seeded_id = if path.contains("subscriptions") {
            "urn:ngsi-ld:Subscription:iso"
        } else {
            "urn:ngsi-ld:ContextSourceRegistration:iso"
        };
        let (s_cross, b_cross) = send(
            &st,
            "GET",
            &format!("{path}/{seeded_id}"),
            Some("tenant-b"),
            None,
        )
        .await;
        let (s_none, b_none) = send(
            &st,
            "GET",
            &format!("{path}/{}", seeded_id.replace(":iso", ":nowhere")),
            Some("tenant-b"),
            None,
        )
        .await;
        assert_eq!(s_cross, StatusCode::NOT_FOUND, "{path}: {b_cross}");
        assert_eq!(s_none, StatusCode::NOT_FOUND, "{path}: {b_none}");
        assert_eq!(
            b_cross.replace(seeded_id, "{id}"),
            b_none.replace(&seeded_id.replace(":iso", ":nowhere"), "{id}"),
            "{path}: cross-tenant id must not read differently from a ghost"
        );
    }
}

/// The in-memory mirrors are tenant-keyed — one tenant's
/// subscriptions/registrations never appear in another tenant's yield.
#[test]
fn doc_mirror_is_tenant_keyed() {
    let m = antares_api::mirror::DocMirror::default();
    m.apply(
        "tenant-a",
        "urn:s:1",
        Some(serde_json::json!({"id": "urn:s:1"})),
    );
    m.apply(
        "tenant-b",
        "urn:s:2",
        Some(serde_json::json!({"id": "urn:s:2"})),
    );
    assert_eq!(m.docs("tenant-a").len(), 1);
    assert_eq!(m.docs("tenant-b").len(), 1);
    assert!(
        m.docs("tenant-c").is_empty(),
        "no bleed into unknown tenants"
    );
    // deleting under the WRONG tenant must not touch the other tenant's doc
    m.apply("tenant-b", "urn:s:1", None);
    assert_eq!(
        m.docs("tenant-a").len(),
        1,
        "cross-tenant delete is a no-op"
    );
}

/// Store state is tenant-keyed — nothing leaks into another tenant's
/// list/query/delete view, and a cross-tenant delete cannot destroy data.
#[tokio::test(flavor = "multi_thread")]
async fn store_state_is_tenant_keyed() {
    let st = AppState::new("test".into());
    create_entity(&st, "tenant-a", "urn:ngsi-ld:Isolation:mine").await;
    create_entity(&st, "tenant-b", "urn:ngsi-ld:Isolation:theirs").await;

    // query in B never sees A's entity
    let (status, body) = send(
        &st,
        "GET",
        "/ngsi-ld/v1/entities?type=Isolation",
        Some("tenant-b"),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert!(
        !body.contains("urn:ngsi-ld:Isolation:mine"),
        "tenant-b sees tenant-a data: {body}"
    );
    assert!(body.contains("urn:ngsi-ld:Isolation:theirs"), "{body}");

    // delete across the boundary 404s and destroys nothing
    let (status, _) = send(
        &st,
        "DELETE",
        "/ngsi-ld/v1/entities/urn:ngsi-ld:Isolation:mine",
        Some("tenant-b"),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    let (status, _) = send(
        &st,
        "GET",
        "/ngsi-ld/v1/entities/urn:ngsi-ld:Isolation:mine",
        Some("tenant-a"),
        None,
    )
    .await;
    assert_eq!(
        status,
        StatusCode::OK,
        "the cross-tenant delete must be a no-op"
    );

    // discovery endpoints are tenant-scoped too
    let (status, body) = send(&st, "GET", "/ngsi-ld/v1/types", Some("tenant-never"), None).await;
    // read path on an unknown tenant: 404 NonexistentTenant (6.3.14) — and
    // definitely not tenant-a's type list
    assert!(
        status == StatusCode::NOT_FOUND || !body.contains("Isolation"),
        "unknown tenant must not see another tenant's types: {status} {body}"
    );
}

/// 5.5.10: creates implicitly create the Tenant; "All other NGSI-LD
/// operations … that target a non-existing Tenant should raise an error of
/// type NonexistentTenant" (404).
#[tokio::test(flavor = "multi_thread")]
async fn clause_5_5_10_nonexistent_tenant_on_non_create_ops() {
    let st = AppState::new("test".into());
    // query / retrieve / discovery / subscription list on an unknown tenant
    for (method, path) in [
        ("GET", "/ngsi-ld/v1/entities?type=T"),
        ("GET", "/ngsi-ld/v1/entities/urn:ngsi-ld:X:1"),
        ("GET", "/ngsi-ld/v1/types"),
        ("GET", "/ngsi-ld/v1/subscriptions"),
        ("DELETE", "/ngsi-ld/v1/entities/urn:ngsi-ld:X:1"),
    ] {
        let (status, body) = send(&st, method, path, Some("tp5510-ghost"), None).await;
        assert_eq!(
            status,
            StatusCode::NOT_FOUND,
            "{method} {path} on unknown tenant: {body}"
        );
        assert!(
            body.contains("NonexistentTenant"),
            "{method} {path}: expected NonexistentTenant, got {body}"
        );
    }
    // a create implicitly creates the tenant …
    create_entity(&st, "tp5510-ghost", "urn:ngsi-ld:X:seed").await;
    // … and the same read now succeeds (no NonexistentTenant anymore)
    let (status, body) = send(
        &st,
        "GET",
        "/ngsi-ld/v1/entities?type=Isolation",
        Some("tp5510-ghost"),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert!(
        !body.contains("NonexistentTenant"),
        "tenant exists after implicit creation: {body}"
    );
    // the default tenant always exists — a tenant-less query is never 404
    let (status, _) = send(&st, "GET", "/ngsi-ld/v1/entities?type=T", None, None).await;
    assert_eq!(status, StatusCode::OK);
}

/// 6.3.14: the broker mints tenants of its own — `snap-index` holds the
/// synthetic-tenant reverse index, `snap-<uuid>` holds one snapshot's frozen
/// copy, `distsub-index` holds the distributed-subscription inbound index. A
/// client-supplied NGSILD-Tenant naming one would write request-shaped
/// documents into the keyspace the broker keeps that state in, and read and
/// delete another tenant's snapshot bookkeeping. Every one is refused, and
/// the refusal happens before the write: a create implicitly creates its
/// tenant (5.5.10), so a guard that let one through would leave the internal
/// tenant listed.
#[tokio::test(flavor = "multi_thread")]
async fn internal_tenants_are_not_addressable_by_a_client() {
    let st = AppState::new("test".into());
    let doc = r#"{"id":"urn:ngsi-ld:Isolation:probe","type":"Isolation"}"#;
    for t in [
        "snap-index",
        "snap-0123456789abcdef0123456789abcdef",
        "distsub-index",
    ] {
        let (status, body) = send(&st, "POST", "/ngsi-ld/v1/entities", Some(t), Some(doc)).await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "{t} was accepted: {body}");
        assert!(
            body.contains("BadRequestData"),
            "{t}: expected BadRequestData, got {body}"
        );
        // and a read is refused the same way, not answered as an empty tenant
        let (status, _) = send(
            &st,
            "GET",
            "/ngsi-ld/v1/entities?type=Isolation",
            Some(t),
            None,
        )
        .await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "{t} readable");
    }

    // The guard is the `snap-` PREFIX and the one exact name, matched
    // literally: a name that merely starts with the same letters, or differs
    // only in case, is an ordinary tenant and keeps its own keyspace. A guard
    // that over-rejected would take legal names away from clients.
    for t in ["snap", "snapshot-data", "SNAP-index", "distsub-index-2"] {
        let (status, body) = send(&st, "POST", "/ngsi-ld/v1/entities", Some(t), Some(doc)).await;
        assert_eq!(status, StatusCode::CREATED, "{t} was refused: {body}");
    }

    let (status, tenants) = send(&st, "GET", "/q/tenants", None, None).await;
    assert_eq!(status, StatusCode::OK);
    for t in ["\"snap-index\"", "\"distsub-index\""] {
        assert!(
            !tenants.contains(t),
            "a refused create still made the tenant: {tenants}"
        );
    }
    assert!(
        tenants.contains("\"SNAP-index\""),
        "the case-different tenant is a normal one: {tenants}"
    );
}

/// 4.14: "any information related to one `Tenant` (e.g. Entities,
/// Subscriptions, `Context Source Registrations`) are only visible to users
/// of the same `Tenant`, but not to users of a different `Tenant`" — and the
/// operations of one Tenant "only apply to the information of the specified
/// `Tenant` in isolation and never have any effect on the information of
/// other `Tenants`".
///
/// The probe above covers the reading half. This is the writing half, which
/// is the one that loses data rather than leaking it: a cross-tenant PATCH,
/// PUT or DELETE that quietly succeeds leaves no trace for the owning tenant
/// beyond the damage. Every mutating shape the router exposes for a document
/// addressed by id is tried from the wrong tenant, and the owner's copy is
/// compared byte for byte afterwards — a 404 alone would not catch an
/// operation that answers 404 and mutates anyway.
#[tokio::test(flavor = "multi_thread")]
async fn no_operation_of_one_tenant_touches_another_tenants_document() {
    let mut st = AppState::new("test-write-isolation".into());
    // the temporal attacks below need a Temporal Evolution to attack: the
    // auto-record hook is what writes one, and AppState::new installs none
    antares_api::notify::wire(&mut st);
    const A: &str = "tenant-owner";
    const B: &str = "tenant-intruder";
    const ENT: &str = "urn:ngsi-ld:Isolation:owned";
    const SUB: &str = "urn:ngsi-ld:Subscription:owned";
    const REG: &str = "urn:ngsi-ld:ContextSourceRegistration:owned";

    // tenant B must exist, or every answer is NonexistentTenant (6.3.14) and
    // the test proves nothing about the document dimension
    create_entity(&st, B, "urn:ngsi-ld:Isolation:intruder-seed").await;

    let seeds: [(&str, &str); 3] = [
        (
            "/ngsi-ld/v1/entities",
            r#"{"id":"urn:ngsi-ld:Isolation:owned","type":"Isolation",
                "speed":{"type":"Property","value":10,
                         "observedAt":"2026-08-01T00:00:00Z"}}"#,
        ),
        (
            "/ngsi-ld/v1/subscriptions",
            r#"{"id":"urn:ngsi-ld:Subscription:owned","type":"Subscription",
                "entities":[{"type":"Isolation"}],
                "notification":{"endpoint":{"uri":"http://localhost:9/never"}}}"#,
        ),
        (
            "/ngsi-ld/v1/csourceRegistrations",
            r#"{"id":"urn:ngsi-ld:ContextSourceRegistration:owned",
                "type":"ContextSourceRegistration",
                "information":[{"entities":[{"type":"Isolation"}]}],
                "endpoint":"http://localhost:9/never"}"#,
        ),
    ];
    for (path, doc) in seeds {
        let (status, body) = send(&st, "POST", path, Some(A), Some(doc)).await;
        assert_eq!(status, StatusCode::CREATED, "seed {path}: {body}");
    }

    let owner_view = |path: String| {
        let st = st.clone();
        async move { send(&st, "GET", &path, Some(A), None).await }
    };
    let ent_path = format!("/ngsi-ld/v1/entities/{ENT}");
    let sub_path = format!("/ngsi-ld/v1/subscriptions/{SUB}");
    let reg_path = format!("/ngsi-ld/v1/csourceRegistrations/{REG}");
    let before = (
        owner_view(ent_path.clone()).await,
        owner_view(sub_path.clone()).await,
        owner_view(reg_path.clone()).await,
    );
    assert_eq!(before.0 .0, StatusCode::OK, "{}", before.0 .1);
    // the two temporal attacks only mean something against a Temporal
    // Evolution that exists — pin that it does, in the owning tenant
    let temporal_path =
        format!("/ngsi-ld/v1/temporal/entities/{ENT}?timerel=after&timeAt=2020-01-01T00:00:00Z");
    let (status, temporal_before) = send(&st, "GET", &temporal_path, Some(A), None).await;
    assert_eq!(status, StatusCode::OK, "{temporal_before}");
    assert!(
        temporal_before.contains("speed"),
        "the owner has a Temporal Evolution to attack: {temporal_before}"
    );

    // every mutating shape the API offers for a document named by id
    let attacks: Vec<(&str, String, Option<&str>)> = vec![
        // 5.6.16 delete the Temporal Evolution, 5.6.13 delete its attribute.
        // Ordered ahead of the Entity deletes: run against its own tenant
        // to check the list is load-bearing, a delete earlier in the list
        // takes the target of every later one away.
        (
            "DELETE",
            format!("/ngsi-ld/v1/temporal/entities/{ENT}"),
            None,
        ),
        (
            "DELETE",
            format!("/ngsi-ld/v1/temporal/entities/{ENT}/attrs/speed"),
            None,
        ),
        // 5.6.2 append / 5.6.4 partial update / 5.6.5 delete attribute
        (
            "POST",
            format!("{ent_path}/attrs"),
            Some(r#"{"planted":{"type":"Property","value":"x"}}"#),
        ),
        (
            "PATCH",
            format!("{ent_path}/attrs"),
            Some(r#"{"speed":{"type":"Property","value":999}}"#),
        ),
        (
            "PATCH",
            format!("{ent_path}/attrs/speed"),
            Some(r#"{"type":"Property","value":999}"#),
        ),
        ("DELETE", format!("{ent_path}/attrs/speed"), None),
        // 5.6.18 replace, 5.6.6 delete
        (
            "PUT",
            ent_path.clone(),
            Some(r#"{"id":"urn:ngsi-ld:Isolation:owned","type":"Isolation"}"#),
        ),
        ("DELETE", ent_path.clone(), None),
        // 5.8.3 update subscription, 5.8.4 delete
        (
            "PATCH",
            sub_path.clone(),
            Some(r#"{"notification":{"endpoint":{"uri":"http://localhost:9/stolen"}}}"#),
        ),
        ("DELETE", sub_path.clone(), None),
        // 5.9.3 update registration, 5.9.4 delete
        (
            "PATCH",
            reg_path.clone(),
            Some(r#"{"endpoint":"http://localhost:9/stolen"}"#),
        ),
        ("DELETE", reg_path.clone(), None),
    ];
    // collected, not asserted per iteration: one run names every shape that
    // crosses the boundary instead of stopping at the first
    let mut crossed: Vec<String> = Vec::new();
    for (method, path, body) in &attacks {
        let (status, resp) = send(&st, method, path, Some(B), *body).await;
        if !status.is_client_error() {
            crossed.push(format!("{method} {path} -> {status} {resp}"));
        }
    }
    assert!(
        crossed.is_empty(),
        "these operations crossed the tenant boundary:\n{}",
        crossed.join("\n")
    );

    let after = (
        owner_view(ent_path).await,
        owner_view(sub_path).await,
        owner_view(reg_path).await,
    );
    assert_eq!(before.0, after.0, "the owner's Entity changed");
    assert_eq!(before.1, after.1, "the owner's Subscription changed");
    assert_eq!(before.2, after.2, "the owner's Registration changed");
    let (status, temporal_after) = send(&st, "GET", &temporal_path, Some(A), None).await;
    assert_eq!(status, StatusCode::OK, "{temporal_after}");
    assert_eq!(
        temporal_before, temporal_after,
        "the owner's Temporal Evolution changed"
    );

    // and nothing was conjured into the intruder's own tenant on the way
    let (status, listing) = send(
        &st,
        "GET",
        "/ngsi-ld/v1/entities?type=Isolation",
        Some(B),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{listing}");
    assert!(
        !listing.contains(ENT),
        "a refused cross-tenant write must not create a local copy: {listing}"
    );
}

/// 6.3.14: "If the HTTP header `NGSILD-Tenant` is present in the HTTP
/// request, it shall also be present in HTTP response." The sentence has no
/// exception for failures, and the failures are where it matters most: a
/// client (or a proxy in front of one) that routes on the echoed header
/// cannot tell which tenant a 400 or a 404 belongs to when the header is
/// missing, and a multi-tenant client reading a batch of responses has
/// nothing to attribute them by.
#[tokio::test(flavor = "multi_thread")]
async fn clause_6_3_14_every_response_echoes_the_request_tenant() {
    const T: &str = "echoer";
    let st = AppState::new("antares-echo".into());
    create_entity(&st, T, "urn:ngsi-ld:Room:7").await;

    let cases: [(&str, &str, Option<&str>); 8] = [
        // the shapes that answer from a handler's own error path
        ("GET", "/ngsi-ld/v1/entities/urn:ngsi-ld:Room:missing", None),
        (
            "DELETE",
            "/ngsi-ld/v1/entities/urn:ngsi-ld:Room:missing",
            None,
        ),
        ("GET", "/ngsi-ld/v1/subscriptions/not%20a%20uri", None),
        // 6.3.20 unknown parameter, and 5.7.2.4 too-wide query
        ("GET", "/ngsi-ld/v1/entities?type=Room&bogus=1", None),
        ("GET", "/ngsi-ld/v1/entities", None),
        // a body that never reaches expansion
        ("POST", "/ngsi-ld/v1/entities", Some("{\"nope\": 1}")),
        // and the two that already answered correctly, so a fix cannot
        // regress them
        ("GET", "/ngsi-ld/v1/nosuchresource", None),
        ("GET", "/ngsi-ld/v1/entities?type=Room", None),
    ];
    for (method, path, body) in cases {
        let mut req = Request::builder()
            .method(method)
            .uri(path)
            .header("NGSILD-Tenant", T);
        if let Some(b) = body {
            req = req
                .header("Content-Type", "application/json")
                .header("Content-Length", b.len());
        }
        let req = req
            .body(body.map_or_else(Body::empty, |b| Body::from(b.to_owned())))
            .expect("request");
        let resp = antares_api::router(st.clone())
            .oneshot(req)
            .await
            .expect("response");
        let echoed = resp
            .headers()
            .get("NGSILD-Tenant")
            .and_then(|v| v.to_str().ok())
            .map(str::to_owned);
        assert_eq!(
            echoed.as_deref(),
            Some(T),
            "{method} {path} answered {} without echoing the tenant",
            resp.status()
        );
    }
}

/// 4.14 on the surfaces the matrix above cannot reach. Every probe there
/// names a document by id, so it exercises only the paths that take one.
/// Two families answer without an id: discovery folds every Entity of the
/// Tenant into a type or attribute list (5.7.5-5.7.8), and the batch
/// operations take a LIST of ids or documents on a resource of their own
/// (5.6.7-5.6.10). Which types a Tenant stores and which attributes they
/// carry is information about that Tenant's Entities, which 4.14 makes
/// visible only to users of the same Tenant; the batch half is the one that
/// deletes rather than leaks, and it names its targets in a body the
/// id-addressed guards never see.
#[tokio::test(flavor = "multi_thread")]
async fn no_discovery_or_batch_surface_reaches_another_tenants_entities() {
    let st = AppState::new("test-discovery-isolation".into());
    const A: &str = "tenant-owner";
    const B: &str = "tenant-intruder";
    const ENT: &str = "urn:ngsi-ld:Isolation:discoverable";

    // B has to exist, or every answer is NonexistentTenant (6.3.14) and the
    // probes prove nothing about the document dimension
    create_entity(&st, B, "urn:ngsi-ld:Isolation:intruder-seed").await;
    let owned = format!(
        r#"{{"id":"{ENT}","type":"IsolationSecret",
             "secretAttr":{{"type":"Property","value":42}}}}"#
    );
    let (status, body) = send(&st, "POST", "/ngsi-ld/v1/entities", Some(A), Some(&owned)).await;
    assert_eq!(status, StatusCode::CREATED, "{body}");
    let ent_path = format!("/ngsi-ld/v1/entities/{ENT}");
    let (status, before) = send(&st, "GET", &ent_path, Some(A), None).await;
    assert_eq!(status, StatusCode::OK, "{before}");

    // The owner DOES see both through discovery. Without this the absence
    // assertions below would hold just as well against a broker whose
    // discovery answers nothing at all.
    for (path, name) in [
        ("/ngsi-ld/v1/types", "IsolationSecret"),
        ("/ngsi-ld/v1/attributes", "secretAttr"),
    ] {
        let (status, mine) = send(&st, "GET", path, Some(A), None).await;
        assert_eq!(status, StatusCode::OK, "{mine}");
        assert!(mine.contains(name), "the owner sees its own {name}: {mine}");
    }

    // Discovery from the intruder. Each probe looks for the name the OTHER
    // resource carries, so a body that merely echoes its own path cannot be
    // mistaken for a leak.
    for (path, needle) in [
        ("/ngsi-ld/v1/types", "IsolationSecret"),
        ("/ngsi-ld/v1/types/IsolationSecret", "secretAttr"),
        ("/ngsi-ld/v1/attributes", "secretAttr"),
        ("/ngsi-ld/v1/attributes/secretAttr", "IsolationSecret"),
    ] {
        let (status, body) = send(&st, "GET", path, Some(B), None).await;
        assert!(
            !body.contains(needle),
            "{path} named {needle} to another tenant: {status} {body}"
        );
    }

    // 5.6.14: the query resource takes its filter in the body, so the tenant
    // never appears beside it in the path.
    let (status, resp) = send(
        &st,
        "POST",
        "/ngsi-ld/v1/entityOperations/query",
        Some(B),
        Some(r#"{"type":"Query","entities":[{"type":"IsolationSecret"}]}"#),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{resp}");
    assert!(!resp.contains(ENT), "the batch query crossed over: {resp}");

    // Every batch resource, naming the owner's Entity from the intruder. An
    // upsert legitimately CREATES that id inside B, so the verdict is not the
    // status but the owner's copy afterwards.
    for (path, body) in [
        (
            "/ngsi-ld/v1/entityOperations/delete",
            format!(r#"["{ENT}"]"#),
        ),
        (
            "/ngsi-ld/v1/entityOperations/update",
            format!(
                r#"[{{"id":"{ENT}","type":"IsolationSecret",
                      "secretAttr":{{"type":"Property","value":999}}}}]"#
            ),
        ),
        (
            "/ngsi-ld/v1/entityOperations/merge",
            format!(
                r#"[{{"id":"{ENT}","type":"IsolationSecret",
                      "planted":{{"type":"Property","value":"x"}}}}]"#
            ),
        ),
        (
            "/ngsi-ld/v1/entityOperations/upsert",
            format!(
                r#"[{{"id":"{ENT}","type":"IsolationSecret",
                      "secretAttr":{{"type":"Property","value":999}}}}]"#
            ),
        ),
    ] {
        let (status, resp) = send(&st, "POST", path, Some(B), Some(&body)).await;
        assert!(
            !status.is_server_error(),
            "{path} failed inside the broker: {status} {resp}"
        );
    }

    let (status, after) = send(&st, "GET", &ent_path, Some(A), None).await;
    assert_eq!(
        status,
        StatusCode::OK,
        "the owner's Entity is gone: {after}"
    );
    assert_eq!(before, after, "the owner's Entity changed");

    // The upsert above is the one probe that legitimately writes: it creates
    // that id inside B. Both copies now exist under the same id and must be
    // each Tenant's own — the same read, answered differently per Tenant, is
    // what "in isolation" means when the identifiers collide.
    let (status, theirs) = send(&st, "GET", &ent_path, Some(B), None).await;
    assert_eq!(status, StatusCode::OK, "{theirs}");
    assert!(theirs.contains("999"), "B reads its own upsert: {theirs}");
    assert!(after.contains("42"), "A still reads its own value: {after}");
}

/// 4.14 through the notification pipeline. "An NGSI-LD system shall behave
/// as if the tenants were separate systems", and a delivery is decided by
/// the matcher, not by a store read: the read-side isolation pinned above
/// says nothing about a payload that has already left the process. Two
/// tenants each hold a Subscription watching the same Entity Type, and a
/// change in one must reach only that one.
///
/// The two Subscriptions carry DIFFERENT ids on purpose. `process_changes`
/// groups matches by `(tenant, subscription id)` and the tenant it stamps is
/// the CHANGE's, so under the same id a candidate that leaked in from
/// another tenant folds into the owner's group and is never delivered
/// separately — the delivery would be suppressed by the grouping rather than
/// by the tenant boundary, and the assertion would pass without holding
/// anything. Distinct ids leave the boundary as the only thing between the
/// change and B's endpoint. Proved by making `subs_for` return both tenants'
/// candidates: B's sink is then served, and this test fails.
#[tokio::test(flavor = "multi_thread")]
async fn a_change_in_one_tenant_never_reaches_another_tenants_endpoint() {
    use serde_json::Value;

    async fn sink() -> (String, tokio::sync::mpsc::Receiver<Value>) {
        let (tx, rx) = tokio::sync::mpsc::channel::<Value>(8);
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind");
        let addr = listener.local_addr().expect("addr");
        let app = axum::Router::new().route(
            "/notify",
            axum::routing::post(move |body: axum::body::Bytes| {
                let tx = tx.clone();
                async move {
                    let _ = tx
                        .send(serde_json::from_slice(&body).unwrap_or(Value::Null))
                        .await;
                    StatusCode::OK
                }
            }),
        );
        tokio::spawn(async move {
            axum::serve(listener, app).await.expect("serve");
        });
        (format!("http://{addr}/notify"), rx)
    }

    antares_jsonld::allow_private_egress(true);
    let mut st = AppState::new("test-notify-isolation".into());
    antares_api::notify::wire(&mut st);
    const A: &str = "tenant-listener";
    const B: &str = "tenant-eavesdropper";
    const SUB_A: &str = "urn:ngsi-ld:Subscription:listener";
    const SUB_B: &str = "urn:ngsi-ld:Subscription:eavesdropper";

    let (uri_a, mut rx_a) = sink().await;
    let (uri_b, mut rx_b) = sink().await;
    for (tenant, id, uri) in [(A, SUB_A, &uri_a), (B, SUB_B, &uri_b)] {
        let doc = format!(
            r#"{{"id":"{id}","type":"Subscription",
                 "entities":[{{"type":"Isolation"}}],
                 "notification":{{"endpoint":{{"uri":"{uri}"}}}}}}"#
        );
        let (status, body) = send(
            &st,
            "POST",
            "/ngsi-ld/v1/subscriptions",
            Some(tenant),
            Some(&doc),
        )
        .await;
        assert_eq!(status, StatusCode::CREATED, "subscribe in {tenant}: {body}");
    }

    let doc = r#"{"id":"urn:ngsi-ld:Isolation:only-in-a","type":"Isolation",
                  "speed":{"type":"Property","value":10}}"#;
    let (status, body) = send(&st, "POST", "/ngsi-ld/v1/entities", Some(A), Some(doc)).await;
    assert_eq!(status, StatusCode::CREATED, "seed in {A}: {body}");

    let wait = std::time::Duration::from_secs(5 * antares_api::state::slow_factor());
    let n = tokio::time::timeout(wait, rx_a.recv())
        .await
        .expect("A's own subscription must be served")
        .expect("one notification");
    assert_eq!(n["subscriptionId"], SUB_A, "{n}");
    assert_eq!(n["data"][0]["id"], "urn:ngsi-ld:Isolation:only-in-a", "{n}");

    // A was served, so the fan-out for this change has run — but a delivery
    // to B would be a separate request on its own task, so `try_recv` here
    // would race it and read silence that has not happened yet. The negative
    // half is only worth its name if it waits.
    let quiet = tokio::time::timeout(wait, rx_b.recv()).await;
    assert!(
        quiet.is_err(),
        "a change in {A} was delivered to {B}'s endpoint: {:?}",
        quiet.ok().flatten()
    );
}
