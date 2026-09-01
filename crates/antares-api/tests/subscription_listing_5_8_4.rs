// SPDX-License-Identifier: EUPL-1.2
//! 5.8.4 Query Subscriptions under 5.5.9 pagination.
//!
//! 5.5.9.1 (p.153): "the query resolution mechanisms of the NGSI-LD System
//! shall ensure that only up to a maximum of L NGSI-LD Elements are
//! retrieved and returned to the NGSI-LD client". RETRIEVED, not just
//! returned — reading a whole tenant to serve one page of it is the thing
//! the clause names, and it is what made a tenant at the document ceiling
//! unable to list its subscriptions at all.
#![allow(clippy::unwrap_used)] // an unwrap here is the assertion

mod common;

use antares_api::AppState;
use antares_model::TenantId;
use antares_store::Kind;
use axum::body::Body;
use axum::http::{Request, StatusCode};
use common::Double;
use http_body_util::BodyExt;
use serde_json::{json, Value};
use std::sync::Arc;
use tower::ServiceExt;

async fn get(st: &AppState, uri: &str) -> (StatusCode, Value, Option<String>) {
    let resp = antares_api::router(st.clone())
        .oneshot(
            Request::builder()
                .method("GET")
                .uri(uri)
                .body(Body::empty())
                .expect("request"),
        )
        .await
        .expect("response");
    let status = resp.status();
    let count = resp
        .headers()
        .get("NGSILD-Results-Count")
        .and_then(|v| v.to_str().ok())
        .map(str::to_owned);
    let bytes = resp.into_body().collect().await.expect("body").to_bytes();
    let body = serde_json::from_slice(&bytes).unwrap_or(Value::Null);
    (status, body, count)
}

fn seed(st: &AppState, n: usize) {
    let tenant = TenantId::new("default").expect("tenant");
    for i in 0..n {
        let id = format!("urn:ngsi-ld:Subscription:list-{i:03}");
        st.store
            .create(
                &tenant,
                Kind::Subscription,
                &id,
                json!({
                    "id": id, "type": "Subscription",
                    "entities": [{"type": "Vehicle"}],
                    "notification": {"endpoint": {"uri": "http://sink.invalid/n"}},
                }),
            )
            .expect("seed subscription");
    }
}

/// A tenant past the document ceiling could not list its subscriptions at
/// all — permanently, for every request, because the listing read the whole
/// tenant and paged the result in memory. The ceiling is the one `list`
/// carries for client queries; 5.5.6 licenses it for "a query operation
/// ... producing so many results that can potentially exhaust client or
/// server resources", and a page of 5 is not that.
///
/// 5.8.4 takes no filter parameters, so the tenant IS the match set and the
/// window can be decided by the store. The store is asked for the page.
#[tokio::test(flavor = "multi_thread")]
async fn a_tenant_over_the_document_ceiling_can_still_list_a_page_of_subscriptions() {
    let mut st = AppState::new("me".into());
    seed(&st, 6);
    // Refuses every whole-tenant `list`, the way the Postgres arm refuses
    // one past its ceiling. The windowed read is not refused.
    st.store = Arc::new(Double::flaky_list(st.store.clone(), usize::MAX));

    let (status, body, count) =
        get(&st, "/ngsi-ld/v1/subscriptions?limit=2&offset=2&count=true").await;
    assert_eq!(
        status,
        StatusCode::OK,
        "a stored volume the client never asked for decided this request: {body}"
    );
    let arr = body.as_array().expect("a list response is an array");
    assert_eq!(arr.len(), 2, "limit=2 must return 2 elements: {body}");
    assert_eq!(
        arr[0]["id"], "urn:ngsi-ld:Subscription:list-002",
        "offset=2 must start at the third element in id order: {body}"
    );
    assert_eq!(arr[1]["id"], "urn:ngsi-ld:Subscription:list-003", "{body}");
    assert_eq!(
        count.as_deref(),
        Some("6"),
        "6.3.10 count=true must report the whole match set, not the page"
    );
}

/// The window the store applies must be the window the client asked for,
/// and the same one the in-memory path produced: same order, same
/// boundaries, an offset past the end empty rather than an error.
#[tokio::test(flavor = "multi_thread")]
async fn the_pushed_down_window_is_the_window_the_client_asked_for() {
    let st = AppState::new("me".into());
    seed(&st, 5);

    let (status, body, _) = get(&st, "/ngsi-ld/v1/subscriptions?limit=5&offset=0").await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let ids: Vec<&str> = body
        .as_array()
        .expect("array")
        .iter()
        .map(|d| d["id"].as_str().expect("id"))
        .collect();
    let mut sorted = ids.clone();
    sorted.sort_unstable();
    assert_eq!(ids, sorted, "5.5.9 pages a stable order: {ids:?}");

    // Every single-element window lands on the element the full list has
    // there: a pushdown that is off by one pages the wrong rows silently.
    for (i, want) in ids.iter().enumerate() {
        let (status, body, _) = get(
            &st,
            &format!("/ngsi-ld/v1/subscriptions?limit=1&offset={i}"),
        )
        .await;
        assert_eq!(status, StatusCode::OK, "{body}");
        assert_eq!(
            body[0]["id"].as_str(),
            Some(*want),
            "offset={i} served the wrong element: {body}"
        );
    }

    // Past the end: an empty page, not an error and not the last page again.
    let (status, body, count) = get(
        &st,
        "/ngsi-ld/v1/subscriptions?limit=2&offset=99&count=true",
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert!(
        body.as_array().expect("array").is_empty(),
        "an offset past the end must be an empty page: {body}"
    );
    assert_eq!(
        count.as_deref(),
        Some("5"),
        "the count is of the match set, which an offset does not change"
    );
}

/// 4.14: operations "only apply to the information of the specified Tenant
/// in isolation". A windowed read pushed into the store must carry the
/// tenant with it, or one tenant's page is filled from another's rows.
#[tokio::test(flavor = "multi_thread")]
async fn a_pushed_down_window_never_reaches_another_tenants_subscriptions() {
    let st = AppState::new("me".into());
    seed(&st, 4);

    let resp = antares_api::router(st.clone())
        .oneshot(
            Request::builder()
                .method("GET")
                .uri("/ngsi-ld/v1/subscriptions?limit=10&offset=0&count=true")
                .header("NGSILD-Tenant", "other")
                .body(Body::empty())
                .expect("request"),
        )
        .await
        .expect("response");
    let count = resp
        .headers()
        .get("NGSILD-Results-Count")
        .and_then(|v| v.to_str().ok())
        .map(str::to_owned);
    let bytes = resp.into_body().collect().await.expect("body").to_bytes();
    let body: Value = serde_json::from_slice(&bytes).unwrap_or(Value::Null);
    assert!(
        body.as_array().is_none_or(|a| a.is_empty()),
        "another tenant's subscriptions were served: {body}"
    );
    assert!(
        count.as_deref() != Some("4"),
        "the count crossed the tenant boundary even though the page did not"
    );
}
