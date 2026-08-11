//! I5 — tenant isolation test pack (§16.1), the router-level half: no
//! existence oracle across tenants (§16.1.6) and tenant-keyed store state
//! (§16.1.4). The RLS denial half runs against live Postgres in
//! `antares-sql/tests`; the NATS subject re-verification half lands with F5.

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

/// §16.1.6: for a tenant-B observer, an id that exists in tenant A must be
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
    let _ = (s_ghost, b_ghost);

    // same property for subscriptions and registrations (§16.1.6 lists all)
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

/// §16.1.4: the F4/F5 in-memory mirrors are tenant-keyed — one tenant's
/// subscriptions/registrations never appear in another tenant's yield.
#[test]
fn doc_mirror_is_tenant_keyed() {
    let m = antares_api::notify::DocMirror::default();
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

/// §16.1.4: store state is tenant-keyed — nothing leaks into another tenant's
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
