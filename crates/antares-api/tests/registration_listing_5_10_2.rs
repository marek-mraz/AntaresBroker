// SPDX-License-Identifier: EUPL-1.2
//! 5.10.2 Query Context Source Registrations under 5.5.9 pagination.
//!
//! 5.10.2.4 (p.233-234) fixes the order: implementations "shall run a query
//! that shall return context source registrations that meet all the
//! applicable conditions" — entity specification, attribute names, geoquery,
//! temporal interval, context source query filter, Scope query — and only
//! then "Pagination logic shall be in place as mandated by clause 5.5.9".
//! Filter first, page second. A window applied before the filter pages rows
//! the client never matched, which is why this listing cannot take the
//! store-side window 5.8.4's can.
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

/// `n` registrations of `ty`, plus `n` of a type the query never asks for,
/// interleaved by id so a window taken before the filter cannot accidentally
/// land on the right rows.
fn seed(st: &AppState, n: usize, ty: &str) {
    let tenant = TenantId::new("default").expect("tenant");
    for i in 0..n {
        for (suffix, t) in [("want", ty), ("other", "Nothing")] {
            let id = format!("urn:ngsi-ld:ContextSourceRegistration:{i:03}-{suffix}");
            st.store
                .create(
                    &tenant,
                    Kind::Registration,
                    &id,
                    json!({
                        "id": id,
                        "type": "ContextSourceRegistration",
                        "endpoint": "http://cs.invalid/ngsi-ld/v1",
                        "information": [{"entities": [{"type": t}]}],
                    }),
                )
                .expect("seed registration");
        }
    }
}

/// A tenant at the document ceiling could not list its registrations at
/// all: the listing read the whole tenant through the read that carries the
/// ceiling for client queries. 5.5.6 licenses TooManyResults for "a query
/// operation ... producing so many results that can potentially exhaust
/// client or server resources" — that is a statement about the RESULT, not
/// about how much the tenant happens to store.
#[tokio::test(flavor = "multi_thread")]
async fn a_tenant_over_the_document_ceiling_can_still_list_its_registrations() {
    let mut st = AppState::new("me".into());
    seed(&st, 4, "Vehicle");
    st.store = Arc::new(Double::flaky_list(st.store.clone(), usize::MAX));

    let (status, body, count) = get(
        &st,
        "/ngsi-ld/v1/csourceRegistrations?type=Vehicle&count=true",
    )
    .await;
    assert_eq!(
        status,
        StatusCode::OK,
        "the stored volume decided a request about 4 matches: {body}"
    );
    assert_eq!(
        count.as_deref(),
        Some("4"),
        "the count is of the match set: {body}"
    );
}

/// The page must be cut from the MATCHES, not from the stored rows. Half the
/// tenant is a type the query never asks for, so a window applied before the
/// filter returns the wrong registrations — or none.
#[tokio::test(flavor = "multi_thread")]
async fn the_page_is_cut_from_the_matches_and_not_from_the_stored_rows() {
    let st = AppState::new("me".into());
    seed(&st, 5, "Vehicle");

    let (status, body, count) = get(
        &st,
        "/ngsi-ld/v1/csourceRegistrations?type=Vehicle&limit=2&offset=2&count=true",
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(
        count.as_deref(),
        Some("5"),
        "5 of the 10 stored registrations match: {body}"
    );
    let arr = body.as_array().expect("array");
    assert_eq!(arr.len(), 2, "limit=2: {body}");
    for d in arr {
        let id = d["id"].as_str().expect("id");
        assert!(
            id.ends_with("-want"),
            "a registration the query never matched was paged in: {id}"
        );
    }
    assert_eq!(
        arr[0]["id"].as_str(),
        Some("urn:ngsi-ld:ContextSourceRegistration:002-want"),
        "offset=2 must skip 2 MATCHES, not 2 stored rows: {body}"
    );

    // Every single-match window lands where the unpaged list has it.
    let (_, all, _) = get(&st, "/ngsi-ld/v1/csourceRegistrations?type=Vehicle&limit=5").await;
    let want: Vec<&str> = all
        .as_array()
        .expect("array")
        .iter()
        .map(|d| d["id"].as_str().expect("id"))
        .collect();
    assert_eq!(want.len(), 5, "{all}");
    for (i, id) in want.iter().enumerate() {
        let (status, body, _) = get(
            &st,
            &format!("/ngsi-ld/v1/csourceRegistrations?type=Vehicle&limit=1&offset={i}"),
        )
        .await;
        assert_eq!(status, StatusCode::OK, "{body}");
        assert_eq!(
            body[0]["id"].as_str(),
            Some(*id),
            "offset={i} served the wrong match: {body}"
        );
    }

    // Past the end of the match set: empty, and the count is unchanged.
    let (status, body, count) = get(
        &st,
        "/ngsi-ld/v1/csourceRegistrations?type=Vehicle&limit=2&offset=50&count=true",
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert!(
        body.as_array().expect("array").is_empty(),
        "past the end is an empty page: {body}"
    );
    assert_eq!(count.as_deref(), Some("5"), "an offset does not filter");
}

/// 4.14: the walk must stay inside the tenant it was asked about.
#[tokio::test(flavor = "multi_thread")]
async fn the_walk_never_reaches_another_tenants_registrations() {
    let st = AppState::new("me".into());
    seed(&st, 3, "Vehicle");

    let resp = antares_api::router(st.clone())
        .oneshot(
            Request::builder()
                .method("GET")
                .uri("/ngsi-ld/v1/csourceRegistrations?type=Vehicle&count=true")
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
        "another tenant's registrations were served: {body}"
    );
    assert!(
        count.as_deref() != Some("3"),
        "the count crossed the tenant boundary"
    );
}

/// The 5.9.2.4 conflict check reads the registration set too, and it read it
/// the same refusable way — so a tenant past the document ceiling could not
/// CREATE an exclusive registration either. The check must still see every
/// registration (a subset would admit the second exclusive registration for
/// one scope that the clause forbids), so the fix is the same paged walk and
/// not a narrower read.
#[tokio::test(flavor = "multi_thread")]
async fn an_exclusive_registration_is_creatable_past_the_document_ceiling() {
    let mut st = AppState::new("me".into());
    seed(&st, 3, "Vehicle");
    st.store = Arc::new(Double::flaky_list(st.store.clone(), usize::MAX));

    let body = json!({
        "id": "urn:ngsi-ld:ContextSourceRegistration:excl-1",
        "type": "ContextSourceRegistration",
        "mode": "exclusive",
        "endpoint": "http://cs.invalid/ngsi-ld/v1",
        "information": [{"entities": [{"type": "Boat", "id": "urn:ngsi-ld:Boat:1"}],
                        "propertyNames": ["speed"]}],
    })
    .to_string();
    let resp = antares_api::router(st.clone())
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/ngsi-ld/v1/csourceRegistrations")
                .header("Content-Type", "application/json")
                .header("Content-Length", body.len())
                .body(Body::from(body))
                .expect("request"),
        )
        .await
        .expect("response");
    let status = resp.status();
    let bytes = resp.into_body().collect().await.expect("body").to_bytes();
    let detail = String::from_utf8_lossy(&bytes).to_string();
    assert_eq!(
        status,
        StatusCode::CREATED,
        "the stored volume refused a registration that conflicts with nothing: {detail}"
    );

    // And the check still refuses a real conflict: the same scope twice.
    let dup = json!({
        "id": "urn:ngsi-ld:ContextSourceRegistration:excl-2",
        "type": "ContextSourceRegistration",
        "mode": "exclusive",
        "endpoint": "http://cs.invalid/ngsi-ld/v1",
        "information": [{"entities": [{"type": "Boat", "id": "urn:ngsi-ld:Boat:1"}],
                        "propertyNames": ["speed"]}],
    })
    .to_string();
    let resp = antares_api::router(st.clone())
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/ngsi-ld/v1/csourceRegistrations")
                .header("Content-Type", "application/json")
                .header("Content-Length", dup.len())
                .body(Body::from(dup))
                .expect("request"),
        )
        .await
        .expect("response");
    assert_eq!(
        resp.status(),
        StatusCode::CONFLICT,
        "a second exclusive registration for one scope must still be refused \
         (5.9.2.4) — a walk that stopped early would admit it"
    );
}

/// 6.3.1: the query parameters of a request arrive percent-encoded and are
/// decoded ONCE, at the extractor. A second decode of `q` reads the escapes
/// that were part of the value: a query whose decoded text legitimately
/// contains `%22` becomes one carrying a bare quote, and 4.9's parser
/// refuses it. `csf`, the other 4.9 query on this operation, is decoded once
/// and answers the same request.
#[tokio::test(flavor = "multi_thread")]
async fn a_percent_escape_inside_q_is_not_decoded_a_second_time() {
    let st = AppState::new("antares-test".into());
    seed(&st, 1, "Vehicle");
    // the client asks for the literal text `%22`: on the wire the percent
    // itself is escaped, so one decode yields q = speed=="%22"
    let (status, body, _) = get(
        &st,
        "/ngsi-ld/v1/csourceRegistrations?type=Vehicle&q=speed%3D%3D%22%2522%22",
    )
    .await;
    assert_eq!(
        status,
        StatusCode::OK,
        "a legal q must not be re-decoded into an unparseable one: {body}"
    );
    // the same text through csf, which was never double-decoded
    let (status, body, _) = get(
        &st,
        "/ngsi-ld/v1/csourceRegistrations?type=Vehicle&csf=endpoint%3D%3D%22%2522%22",
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
}
