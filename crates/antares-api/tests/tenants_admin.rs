// SPDX-License-Identifier: EUPL-1.2
//! Tenant inventory and purge on the admin surface. Tenants come to exist
//! implicitly (CIM 009 5.5.10) and the NGSI-LD API has no operation to
//! remove one; `/q/tenants` lists the tenant names and
//! `DELETE /q/tenants/{tenant}` removes every document of that tenant from
//! the current-state and the temporal backend, leaving other tenants
//! untouched. `GET /q/tenants/{tenant}` answers for one tenant: at the
//! 10 000-tenant target (ADR-0001) the inventory is not a lookup, and
//! counting every tenant to read one is the wrong shape.

use antares_api::AppState;
use antares_model::TenantId;
use antares_store::Kind;
use axum::body::Body;
use axum::http::{Request, StatusCode};
use serde_json::{json, Value};
use tower::ServiceExt;

async fn send(
    st: &AppState,
    method: &str,
    path: &str,
    body: Option<Value>,
    tenant: Option<&str>,
) -> (StatusCode, Value) {
    let mut b = Request::builder().method(method).uri(path);
    if let Some(t) = tenant {
        b = b.header("NGSILD-Tenant", t);
    }
    let req = match body {
        Some(v) => {
            let body = v.to_string();
            b.header("Content-Type", "application/json")
                .header("Content-Length", body.len())
                .body(Body::from(body))
        }
        None => b.body(Body::empty()),
    }
    .expect("request");
    let res = antares_api::router(st.clone())
        .oneshot(req)
        .await
        .expect("response");
    let status = res.status();
    let bytes = axum::body::to_bytes(res.into_body(), usize::MAX)
        .await
        .expect("body");
    let body = if bytes.is_empty() {
        Value::Null
    } else {
        serde_json::from_slice(&bytes).unwrap_or(Value::Null)
    };
    (status, body)
}

fn entity(id: &str) -> Value {
    json!({
        "id": id, "type": "Room",
        "temperature": {"type": "Property", "value": 21.5,
                        "observedAt": "2026-01-01T00:00:00Z"}
    })
}

fn state() -> AppState {
    let mut st = AppState::new("antares-test".into());
    antares_api::notify::wire(&mut st);
    st
}

async fn seed(st: &AppState, tenant: &str, id: &str) {
    let (s, _) = send(
        st,
        "POST",
        "/ngsi-ld/v1/entities",
        Some(entity(id)),
        Some(tenant),
    )
    .await;
    assert_eq!(s, StatusCode::CREATED);
    let sub = json!({
        "id": format!("urn:ngsi-ld:Subscription:{tenant}"), "type": "Subscription",
        "entities": [{"type": "Room"}],
        "notification": {"endpoint": {"uri": "http://127.0.0.1:9/notify"}}
    });
    let (s, _) = send(
        st,
        "POST",
        "/ngsi-ld/v1/subscriptions",
        Some(sub),
        Some(tenant),
    )
    .await;
    assert_eq!(s, StatusCode::CREATED);
}

fn listed(list: &Value, tenant: &str) -> bool {
    list.as_array().expect("array").iter().any(|t| t == tenant)
}

#[tokio::test(flavor = "multi_thread")]
async fn inventory_lists_the_tenant_names_and_nothing_else() {
    let st = state();
    seed(&st, "inva", "urn:ngsi-ld:Room:1").await;
    seed(&st, "invb", "urn:ngsi-ld:Room:2").await;
    let (s, list) = send(&st, "GET", "/q/tenants", None, None).await;
    assert_eq!(s, StatusCode::OK, "{list}");
    assert!(listed(&list, "inva"), "{list}");
    assert!(listed(&list, "invb"), "{list}");
    // the default tenant implicitly exists and is listed even when empty
    assert!(listed(&list, "default"), "{list}");
    let names = list.as_array().expect("array");
    assert!(
        names.iter().all(Value::is_string),
        "names only — the counts are paid per lookup at /q/tenants/{{tenant}}: {list}"
    );
    let sorted: Vec<&Value> = {
        let mut v: Vec<&Value> = names.iter().collect();
        v.sort_by_key(|t| t.as_str().unwrap_or_default());
        v
    };
    assert_eq!(
        names.iter().collect::<Vec<&Value>>(),
        sorted,
        "sorted, so a client can page or binary-search it: {list}"
    );
}

/// One tenant is addressable directly, and that is where its counts live.
#[tokio::test(flavor = "multi_thread")]
async fn one_tenant_is_addressable_and_matches_its_inventory_row() {
    let st = state();
    seed(&st, "geta", "urn:ngsi-ld:Room:1").await;
    seed(&st, "getb", "urn:ngsi-ld:Room:2").await;
    let (s, one) = send(&st, "GET", "/q/tenants/geta", None, None).await;
    assert_eq!(s, StatusCode::OK, "{one}");
    assert_eq!(one["tenant"], "geta", "{one}");
    assert_eq!(one["counts"]["entities"], 1, "{one}");
    assert_eq!(one["counts"]["subscriptions"], 1, "{one}");
    assert_eq!(one["counts"]["attrInstances"], 1, "{one}");
    assert!(one.get("tenant_id").is_none(), "no internal names: {one}");
    let (_, list) = send(&st, "GET", "/q/tenants", None, None).await;
    assert!(listed(&list, "geta"), "named in the inventory too: {list}");
    // one tenant, not a filtered list
    assert!(one.is_object(), "{one}");
    // 5.5.10: the default tenant always exists, so it is always readable
    let (s, def) = send(&st, "GET", "/q/tenants/default", None, None).await;
    assert_eq!(s, StatusCode::OK, "{def}");
    assert_eq!(def["tenant"], "default", "{def}");
}

/// The same guards the purge carries: an unknown tenant is 404, a name no
/// request could carry is 400, and the broker's internal snapshot tenants
/// are not addressable at all.
#[tokio::test(flavor = "multi_thread")]
async fn reading_one_tenant_rejects_unknown_and_malformed_names() {
    let st = state();
    seed(&st, "getguard", "urn:ngsi-ld:Room:1").await;
    let (s, _) = send(&st, "GET", "/q/tenants/never-seen", None, None).await;
    assert_eq!(s, StatusCode::NOT_FOUND);
    let (s, _) = send(&st, "GET", "/q/tenants/bad%20name", None, None).await;
    assert_eq!(s, StatusCode::BAD_REQUEST);
    let (s, _) = send(&st, "GET", "/q/tenants/snap-index", None, None).await;
    assert_eq!(s, StatusCode::BAD_REQUEST);
}

/// The path names the tenant; the NGSILD-Tenant header never redirects the
/// read to another one.
#[tokio::test(flavor = "multi_thread")]
async fn reading_one_tenant_is_addressed_by_the_path_never_by_the_header() {
    let st = state();
    seed(&st, "getpath", "urn:ngsi-ld:Room:1").await;
    seed(&st, "getother", "urn:ngsi-ld:Room:2").await;
    let (s, one) = send(&st, "GET", "/q/tenants/getpath", None, Some("getother")).await;
    assert_eq!(s, StatusCode::OK, "{one}");
    assert_eq!(one["tenant"], "getpath", "{one}");
}

/// Not under the API root: the tenant admin is the operator's, not a client's.
#[tokio::test(flavor = "multi_thread")]
async fn reading_one_tenant_is_not_reachable_under_the_api_root() {
    let st = state();
    seed(&st, "getroot", "urn:ngsi-ld:Room:1").await;
    let (s, _) = send(&st, "GET", "/ngsi-ld/v1/q/tenants/getroot", None, None).await;
    assert_eq!(s, StatusCode::NOT_FOUND);
}

#[tokio::test(flavor = "multi_thread")]
async fn purge_removes_one_tenant_and_leaves_the_other() {
    let st = state();
    seed(&st, "purgea", "urn:ngsi-ld:Room:1").await;
    seed(&st, "purgeb", "urn:ngsi-ld:Room:2").await;

    let (s, body) = send(&st, "DELETE", "/q/tenants/purgea", None, None).await;
    assert_eq!(s, StatusCode::NO_CONTENT, "{body}");

    // 5.5.10: a tenant that no longer exists answers NonexistentTenant
    let (s, err) = send(
        &st,
        "GET",
        "/ngsi-ld/v1/entities/urn:ngsi-ld:Room:1",
        None,
        Some("purgea"),
    )
    .await;
    assert_eq!(s, StatusCode::NOT_FOUND, "{err}");
    assert!(
        err["type"]
            .as_str()
            .is_some_and(|t| t.ends_with("NonexistentTenant")),
        "{err}"
    );
    let (s, _) = send(
        &st,
        "GET",
        "/ngsi-ld/v1/temporal/entities/urn:ngsi-ld:Room:1",
        None,
        Some("purgea"),
    )
    .await;
    assert_eq!(s, StatusCode::NOT_FOUND, "history must go with the tenant");

    let (_, list) = send(&st, "GET", "/q/tenants", None, None).await;
    assert!(!listed(&list, "purgea"), "{list}");
    assert!(listed(&list, "purgeb"), "other tenant kept: {list}");
    let (_, b) = send(&st, "GET", "/q/tenants/purgeb", None, None).await;
    assert_eq!(b["counts"]["entities"], 1);
    assert_eq!(b["counts"]["subscriptions"], 1);
    assert_eq!(b["counts"]["attrInstances"], 1);
    let (s, doc) = send(
        &st,
        "GET",
        "/ngsi-ld/v1/entities/urn:ngsi-ld:Room:2",
        None,
        Some("purgeb"),
    )
    .await;
    assert_eq!(s, StatusCode::OK, "{doc}");

    // a second purge finds nothing
    let (s, _) = send(&st, "DELETE", "/q/tenants/purgea", None, None).await;
    assert_eq!(s, StatusCode::NOT_FOUND);
}

#[tokio::test(flavor = "multi_thread")]
async fn purge_rejects_unknown_and_malformed_tenants() {
    let st = state();
    let (s, _) = send(&st, "DELETE", "/q/tenants/never-seen", None, None).await;
    assert_eq!(s, StatusCode::NOT_FOUND);
    let (s, _) = send(&st, "DELETE", "/q/tenants/bad%20name", None, None).await;
    assert_eq!(s, StatusCode::BAD_REQUEST);
    // the broker's internal snapshot tenants are not addressable
    let (s, _) = send(&st, "DELETE", "/q/tenants/snap-index", None, None).await;
    assert_eq!(s, StatusCode::BAD_REQUEST);
}

#[tokio::test(flavor = "multi_thread")]
async fn purge_is_refused_while_distributed_subscriptions_are_active() {
    let st = state();
    seed(&st, "purgedist", "urn:ngsi-ld:Room:1").await;
    let t = TenantId::new("purgedist").expect("tenant");
    assert!(st
        .store
        .create(
            &t,
            Kind::DistSub,
            "urn:ngsi-ld:DistSub:1",
            json!({"id": "urn:ngsi-ld:DistSub:1", "status": "active"}),
        )
        .expect("dist sub"));
    let (s, body) = send(&st, "DELETE", "/q/tenants/purgedist", None, None).await;
    assert_eq!(s, StatusCode::CONFLICT, "{body}");
    let (s, _) = send(
        &st,
        "GET",
        "/ngsi-ld/v1/entities/urn:ngsi-ld:Room:1",
        None,
        Some("purgedist"),
    )
    .await;
    assert_eq!(
        s,
        StatusCode::OK,
        "a refused purge must not delete anything"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn tenant_routes_are_not_reachable_under_the_api_root() {
    let st = state();
    let (s, _) = send(&st, "GET", "/ngsi-ld/v1/q/tenants", None, None).await;
    assert_eq!(s, StatusCode::NOT_FOUND);
    let (s, _) = send(&st, "DELETE", "/ngsi-ld/v1/tenants/x", None, None).await;
    assert_eq!(s, StatusCode::NOT_FOUND);
}

#[tokio::test(flavor = "multi_thread")]
async fn purge_is_addressed_by_the_path_never_by_the_tenant_header() {
    let st = state();
    seed(&st, "hdra", "urn:ngsi-ld:Room:1").await;
    seed(&st, "hdrb", "urn:ngsi-ld:Room:2").await;
    // a stray NGSILD-Tenant header on the admin call must not redirect the purge
    let (s, _) = send(&st, "DELETE", "/q/tenants/hdra", None, Some("hdrb")).await;
    assert_eq!(s, StatusCode::NO_CONTENT);
    let (_, list) = send(&st, "GET", "/q/tenants", None, Some("hdra")).await;
    assert!(!listed(&list, "hdra"), "{list}");
    assert!(listed(&list, "hdrb"), "{list}");
    let (_, kept) = send(&st, "GET", "/q/tenants/hdrb", None, None).await;
    assert_eq!(kept["counts"]["entities"], 1);
}

#[tokio::test(flavor = "multi_thread")]
async fn purging_the_default_tenant_empties_it_but_it_keeps_existing() {
    let st = state();
    let (s, _) = send(
        &st,
        "POST",
        "/ngsi-ld/v1/entities",
        Some(entity("urn:ngsi-ld:Room:d")),
        None,
    )
    .await;
    assert_eq!(s, StatusCode::CREATED);
    let (s, _) = send(&st, "DELETE", "/q/tenants/default", None, None).await;
    assert_eq!(s, StatusCode::NO_CONTENT);
    // 5.5.10: the default Tenant always exists, so the entity is a plain
    // ResourceNotFound, never NonexistentTenant
    let (s, err) = send(
        &st,
        "GET",
        "/ngsi-ld/v1/entities/urn:ngsi-ld:Room:d",
        None,
        None,
    )
    .await;
    assert_eq!(s, StatusCode::NOT_FOUND);
    assert!(
        err["type"]
            .as_str()
            .is_some_and(|t| t.ends_with("ResourceNotFound")),
        "{err}"
    );
    let (_, list) = send(&st, "GET", "/q/tenants", None, None).await;
    assert!(listed(&list, "default"), "{list}");
    let (_, def) = send(&st, "GET", "/q/tenants/default", None, None).await;
    assert_eq!(def["counts"]["entities"], 0, "{def}");
    let (s, _) = send(
        &st,
        "POST",
        "/ngsi-ld/v1/entities",
        Some(entity("urn:ngsi-ld:Room:d")),
        None,
    )
    .await;
    assert_eq!(
        s,
        StatusCode::CREATED,
        "the default tenant is writable after a purge"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn tenant_names_outside_the_grammar_are_refused_before_any_lookup() {
    let st = state();
    for raw in [
        "a".repeat(65),
        "..%2F..".into(),
        "x%00y".into(),
        "%C3%A9".into(),
    ] {
        let (s, _) = send(&st, "DELETE", &format!("/q/tenants/{raw}"), None, None).await;
        assert_eq!(s, StatusCode::BAD_REQUEST, "{raw}");
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn purge_of_one_tenant_does_not_touch_a_same_prefixed_tenant() {
    let st = state();
    seed(&st, "pre", "urn:ngsi-ld:Room:1").await;
    seed(&st, "prefix", "urn:ngsi-ld:Room:1").await;
    let (s, _) = send(&st, "DELETE", "/q/tenants/pre", None, None).await;
    assert_eq!(s, StatusCode::NO_CONTENT);
    let (s, _) = send(
        &st,
        "GET",
        "/ngsi-ld/v1/entities/urn:ngsi-ld:Room:1",
        None,
        Some("prefix"),
    )
    .await;
    assert_eq!(s, StatusCode::OK);
    let (s, _) = send(
        &st,
        "GET",
        "/ngsi-ld/v1/temporal/entities/urn:ngsi-ld:Room:1",
        None,
        Some("prefix"),
    )
    .await;
    assert_eq!(s, StatusCode::OK, "history of the neighbour tenant kept");
}
