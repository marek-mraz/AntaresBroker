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

async fn state() -> AppState {
    let mut st = AppState::new("antares-test".into());
    antares_api::wire(&mut st).await;
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
    let st = state().await;
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

/// The inventory is the list of customer accounts. The broker mints tenants
/// for its own bookkeeping — `snap-<uuid>` per snapshot, `snap-index` for the
/// synthetic-tenant reverse index, `distsub-index` for the distributed
/// subscription inbound index — and none of them is an account: on Postgres
/// they are never written to the `tenants` table at all, and the memory arm,
/// which derives the inventory from the data, filters them the same way, so
/// one deployment's snapshot churn cannot bury the accounts an operator came
/// to read. Showing them bought nothing either: the per-tenant routes REFUSE
/// an internal name, so an operator who saw one could not act on it.
#[tokio::test(flavor = "multi_thread")]
async fn broker_tenants_are_not_in_the_inventory_and_not_addressable() {
    let st = state().await;
    seed(&st, "invreal", "urn:ngsi-ld:Room:9").await;
    let internal = ["snap-index", "snap-0000", "distsub-index"];
    for name in internal {
        let t = TenantId::new_internal(name).expect("a legal tenant name");
        st.store
            .create(
                &t,
                Kind::Entity,
                "urn:ngsi-ld:Room:x",
                entity("urn:ngsi-ld:Room:x"),
            )
            .await
            .expect("seed the broker's own tenant");
    }
    let (s, list) = send(&st, "GET", "/q/tenants", None, None).await;
    assert_eq!(s, StatusCode::OK, "{list}");
    assert!(listed(&list, "invreal"), "{list}");
    for name in internal {
        assert!(
            !listed(&list, name),
            "the broker's own {name} is in the account inventory: {list}"
        );
        for method in ["GET", "DELETE"] {
            let (s, _) = send(&st, method, &format!("/q/tenants/{name}"), None, None).await;
            assert_eq!(
                s,
                StatusCode::BAD_REQUEST,
                "{method} /q/tenants/{name} must not address broker state"
            );
        }
    }
    // and the tenant that owns real data is addressable as before
    let (s, one) = send(&st, "GET", "/q/tenants/invreal", None, None).await;
    assert_eq!(s, StatusCode::OK, "{one}");
}

/// One tenant is addressable directly, and that is where its counts live.
#[tokio::test(flavor = "multi_thread")]
async fn one_tenant_is_addressable_and_matches_its_inventory_row() {
    let st = state().await;
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
    let st = state().await;
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
    let st = state().await;
    seed(&st, "getpath", "urn:ngsi-ld:Room:1").await;
    seed(&st, "getother", "urn:ngsi-ld:Room:2").await;
    let (s, one) = send(&st, "GET", "/q/tenants/getpath", None, Some("getother")).await;
    assert_eq!(s, StatusCode::OK, "{one}");
    assert_eq!(one["tenant"], "getpath", "{one}");
}

/// Not under the API root: the tenant admin is the operator's, not a client's.
#[tokio::test(flavor = "multi_thread")]
async fn reading_one_tenant_is_not_reachable_under_the_api_root() {
    let st = state().await;
    seed(&st, "getroot", "urn:ngsi-ld:Room:1").await;
    let (s, _) = send(&st, "GET", "/ngsi-ld/v1/q/tenants/getroot", None, None).await;
    assert_eq!(s, StatusCode::NOT_FOUND);
}

#[tokio::test(flavor = "multi_thread")]
async fn purge_removes_one_tenant_and_leaves_the_other() {
    let st = state().await;
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

/// The synthetic tenants a Snapshot's isolated copy lives under, and what
/// each still holds. They are not in `/q/tenants` — that is the inventory of
/// customer accounts — so they are read from the reverse index that names
/// them, which is also what a teardown removes last.
async fn synth_tenants(st: &AppState) -> Vec<String> {
    let idx = TenantId::new_internal("snap-index").expect("index tenant");
    let mut out = Vec::new();
    for doc in st.store.list(&idx, Kind::Snapshot).await.expect("index") {
        let Some(name) = doc.get("id").and_then(Value::as_str) else {
            continue;
        };
        let t = TenantId::new_internal(name).expect("synthetic tenant");
        if !st
            .store
            .list(&t, Kind::Entity)
            .await
            .expect("list")
            .is_empty()
        {
            out.push(name.to_owned());
        }
    }
    out
}

/// 5.2.41 + 4.14: `DELETE /q/tenants/{tenant}` answers 204, which asserts the
/// tenant's information is gone. A Snapshot's isolated copy is NOT stored
/// under the Tenant — it goes under a synthetic `snap-<uuid>` tenant named in
/// the Snapshot's internal `__tenant` member, and only the snapshot-delete
/// path frees it. The purge removed the Snapshot document, which is the only
/// pointer to that synthetic tenant, so the copied Entities and their history
/// stayed behind with nothing left that could ever reach them: a 204
/// asserting an erasure that did not happen. The Hosted `@context` is handled
/// for exactly this reason already — "`jsonld_contexts` has no tenant column,
/// so the tenant-keyed purge above cannot see it".
#[tokio::test(flavor = "multi_thread")]
async fn purge_frees_the_snapshot_copies_the_tenant_left_behind() {
    let st = state().await;
    assert!(
        synth_tenants(&st).await.is_empty(),
        "a fresh state holds no snapshot copies"
    );
    seed(&st, "purgesnap", "urn:ngsi-ld:Room:9").await;

    let snap = json!({"type": "Snapshot",
        "snapshotQueries": [{"type": "Query", "entities": [{"type": "Room"}]}]});
    let (s, body) = send(
        &st,
        "POST",
        "/ngsi-ld/v1/snapshots",
        Some(snap),
        Some("purgesnap"),
    )
    .await;
    assert_eq!(s, StatusCode::CREATED, "{body}");

    // 5.16.1.4 fills the copy in the background; the synthetic tenant shows up
    // in the inventory once it lands. Waiting for it IS the premise — without
    // a copy there is nothing for the purge to leave behind.
    let mut before = Vec::new();
    for _ in 0..100 {
        before = synth_tenants(&st).await;
        if !before.is_empty() {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    }
    assert_eq!(before.len(), 1, "one snapshot, one synthetic tenant");

    let (s, body) = send(&st, "DELETE", "/q/tenants/purgesnap", None, None).await;
    assert_eq!(s, StatusCode::NO_CONTENT, "{body}");

    // The free runs in the background, like the snapshot-delete path it shares.
    let mut after = before.clone();
    for _ in 0..100 {
        after = synth_tenants(&st).await;
        if after.is_empty() {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    }
    assert!(
        after.is_empty(),
        "the purge left the tenant's snapshot copies behind, unreachable and \
         unfreeable: {after:?}"
    );
}

/// The same leak on the ordinary path. Removing a Snapshot frees the copy,
/// and has to free the synthetic tenant with it: `/q/tenants` reports what
/// the store holds a tenant entry for, so a free that empties the documents
/// but leaves the tenant adds one permanent `snap-<uuid>` name per snapshot
/// ever deleted — an inventory that only grows, against the 10 000 tenant
/// target of ADR-0001. Every removal path (6.37 delete, 6.36 purge, the
/// expiry reaper and the over-cap eviction) goes through the one helper this
/// exercises.
#[tokio::test(flavor = "multi_thread")]
async fn removing_a_snapshot_leaves_no_tenant_behind() {
    let st = state().await;
    seed(&st, "snapghost", "urn:ngsi-ld:Room:8").await;

    let snap = json!({"type": "Snapshot",
        "snapshotQueries": [{"type": "Query", "entities": [{"type": "Room"}]}]});
    let (s, body) = send(
        &st,
        "POST",
        "/ngsi-ld/v1/snapshots",
        Some(snap),
        Some("snapghost"),
    )
    .await;
    assert_eq!(s, StatusCode::CREATED, "{body}");

    let mut before = Vec::new();
    for _ in 0..100 {
        before = synth_tenants(&st).await;
        if !before.is_empty() {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    }
    assert_eq!(before.len(), 1, "one snapshot, one synthetic tenant");

    let (s, body) = send(
        &st,
        "DELETE",
        // 5.16.7.4: the purge is scoped by a q over Snapshot members;
        // the default priority is the one every snapshot here carries.
        "/ngsi-ld/v1/snapshots?q=snapshotPriority==5",
        None,
        Some("snapghost"),
    )
    .await;
    assert_eq!(s, StatusCode::NO_CONTENT, "{body}");

    let mut after = before.clone();
    for _ in 0..100 {
        after = synth_tenants(&st).await;
        if after.is_empty() {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    }
    assert!(
        after.is_empty(),
        "a deleted snapshot left its synthetic tenant in the inventory: {after:?}"
    );

    // and the tenant that owned it is untouched
    let (_, b) = send(&st, "GET", "/q/tenants/snapghost", None, None).await;
    assert_eq!(b["counts"]["entities"], 1, "{b}");
}

#[tokio::test(flavor = "multi_thread")]
async fn purge_rejects_unknown_and_malformed_tenants() {
    let st = state().await;
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
    let st = state().await;
    seed(&st, "purgedist", "urn:ngsi-ld:Room:1").await;
    let t = TenantId::new("purgedist").expect("tenant");
    // the 5.8.1.4 mapping of one Subscription that reached a Context
    // Source: `remotes` names the copy living at that source, which only
    // deleting the Subscription removes there (5.8.5.4)
    assert!(st
        .store
        .create(
            &t,
            Kind::DistSub,
            "urn:ngsi-ld:Subscription:own",
            json!({"csr_sub": "urn:ngsi-ld:CSourceSubscription:distsub:1",
                   "remotes": {"urn:ngsi-ld:ContextSourceRegistration:r1":
                       ["http://source.example.org", "urn:ngsi-ld:Subscription:remote1"]}}),
        )
        .await
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
    let st = state().await;
    let (s, _) = send(&st, "GET", "/ngsi-ld/v1/q/tenants", None, None).await;
    assert_eq!(s, StatusCode::NOT_FOUND);
    let (s, _) = send(&st, "DELETE", "/ngsi-ld/v1/tenants/x", None, None).await;
    assert_eq!(s, StatusCode::NOT_FOUND);
}

#[tokio::test(flavor = "multi_thread")]
async fn purge_is_addressed_by_the_path_never_by_the_tenant_header() {
    let st = state().await;
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
    let st = state().await;
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
    let st = state().await;
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
    let st = state().await;
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

/// A Hosted @context (5.13.1) is a document of the Tenant that stored it:
/// it holds term mappings authored through that Tenant's requests, and
/// 5.5.7 makes those mappings decide what every payload of that Tenant
/// means. `jsonld_contexts` carries no tenant column — the owner lives
/// inside the row — so the store's tenant-keyed purge cannot see it and the
/// row outlives the Tenant it belongs to.
///
/// The consequence is inheritance rather than leakage, and it is worse for
/// it: tenant names are client-chosen and come to exist implicitly (5.5.10),
/// so the next holder of a purged name expands its terms through the
/// previous holder's mappings, silently.
#[tokio::test(flavor = "multi_thread")]
async fn purging_a_tenant_takes_its_hosted_contexts_with_it() {
    let st = state().await;
    seed(&st, "ctxowner", "urn:ngsi-ld:Room:ctx1").await;
    seed(&st, "ctxother", "urn:ngsi-ld:Room:ctx2").await;

    let store_context = |tenant: &'static str, iri: &'static str| {
        let st = st.clone();
        async move {
            let (s, _) = send(
                &st,
                "POST",
                "/ngsi-ld/v1/jsonldContexts",
                Some(json!({"@context": {"Vehicle": iri}})),
                Some(tenant),
            )
            .await;
            assert_eq!(s, StatusCode::CREATED);
        }
    };
    store_context("ctxowner", "https://first.example/Vehicle").await;
    store_context("ctxother", "https://other.example/Vehicle").await;

    let hosted = |tenant: &'static str| {
        let st = st.clone();
        async move {
            let (s, list) = send(
                &st,
                "GET",
                "/ngsi-ld/v1/jsonldContexts?kind=Hosted",
                None,
                Some(tenant),
            )
            .await;
            assert_eq!(s, StatusCode::OK, "{list}");
            list.as_array().map(Vec::len).unwrap_or(0)
        }
    };
    assert_eq!(hosted("ctxowner").await, 1, "the owner stored one");
    assert_eq!(hosted("ctxother").await, 1, "so did the other tenant");

    let (s, body) = send(&st, "DELETE", "/q/tenants/ctxowner", None, None).await;
    assert_eq!(s, StatusCode::NO_CONTENT, "{body}");

    // the name is free again, and comes back the way any tenant does (5.5.10)
    seed(&st, "ctxowner", "urn:ngsi-ld:Room:ctx1").await;
    assert_eq!(
        hosted("ctxowner").await,
        0,
        "the purged tenant's Hosted @context outlived the purge, and its \
         term mappings now apply to whoever holds the name next"
    );
    assert_eq!(
        hosted("ctxother").await,
        1,
        "and no other tenant's @context was taken with it"
    );
}

/// A purge that ran while the tenant still holds subscription copies at
/// context sources would orphan them: 5.8.5.4 removes a copy at its source
/// when the Subscription is deleted, and a purge deletes documents without
/// forwarding anything. The refusal is therefore about the copies that
/// exist remotely, held in the `remotes` member of the 5.8.1.4 mapping —
/// not about the mapping documents themselves, which every ordinary
/// Subscription owns from the moment it is created and which would make
/// every tenant that ever held a Subscription unpurgeable.
#[tokio::test]
async fn purge_goes_ahead_when_no_copy_lives_at_a_context_source() {
    let st = state().await;
    let t = TenantId::new("distsubpurge").expect("tenant");
    let (status, _) = send(
        &st,
        "POST",
        "/ngsi-ld/v1/subscriptions",
        Some(json!({
            "id": "urn:ngsi-ld:Subscription:purge-me",
            "type": "Subscription",
            "entities": [{"type": "Vehicle"}],
            "notification": {"endpoint": {"uri": "http://127.0.0.1:9/cb"}},
        })),
        Some("distsubpurge"),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);
    // the Subscription owns a mapping document and an internal Registration
    // Subscription; no Context Source has matched it, so nothing lives
    // remotely and the purge goes ahead
    assert!(
        st.store
            .get(&t, Kind::DistSub, "urn:ngsi-ld:Subscription:purge-me")
            .await
            .expect("mapping read")
            .is_some(),
        "the distributed half stores a mapping for an ordinary Subscription"
    );
    let (status, body) = send(&st, "DELETE", "/q/tenants/distsubpurge", None, None).await;
    assert_eq!(status, StatusCode::NO_CONTENT, "{body}");
    assert!(
        st.store
            .get(&t, Kind::Subscription, "urn:ngsi-ld:Subscription:purge-me")
            .await
            .expect("read")
            .is_none(),
        "the purge took the Subscription with it"
    );
}
