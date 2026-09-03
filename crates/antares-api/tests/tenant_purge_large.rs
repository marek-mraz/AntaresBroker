// SPDX-License-Identifier: EUPL-1.2
//! A Tenant stays purgeable however much it holds.
//!
//! 5.5.10 makes a Tenant the isolation unit, and the admin purge is the only
//! way to take one back. Collecting what to remove through the store's
//! all-at-once `list` puts the purge behind that read's ceiling: the Postgres
//! arm answers `TooManyResults` past `MAX_UNDECIDED_ROWS` (10 000), which the
//! API renders as a 403 — so the Tenants most worth reclaiming would be the
//! ones that can never be reclaimed, and the rows would stay readable to
//! anyone sending that Tenant's header. The paged walk has no ceiling by
//! design, which is why every other internal walker uses it.
#![allow(clippy::unwrap_used)] // an unwrap here is the assertion

mod common;

use antares_api::AppState;
use antares_model::TenantId;
use antares_store::Kind;
use axum::body::Body;
use axum::http::{Request, StatusCode};
use common::Double;
use serde_json::json;
use std::sync::Arc;
use tower::ServiceExt;

async fn call(st: &AppState, method: &str, uri: &str) -> StatusCode {
    antares_api::router(st.clone())
        .oneshot(
            Request::builder()
                .method(method)
                .uri(uri)
                .body(Body::empty())
                .expect("request"),
        )
        .await
        .expect("response")
        .status()
}

/// The store answers `TooManyResults` to every `list` — what the Postgres arm
/// does for a Tenant past the ceiling — while the paged read keeps working.
#[tokio::test]
async fn a_tenant_past_the_list_ceiling_is_still_purgeable() {
    let mut st = AppState::new("me".into());
    let tenant = TenantId::new("big").expect("tenant");
    for i in 0..3 {
        let id = format!("urn:ngsi-ld:Subscription:big-{i}");
        st.store
            .create(
                &tenant,
                Kind::Subscription,
                &id,
                json!({"id": id, "type": "Subscription",
                       "entities": [{"type": "Vehicle"}],
                       "notification": {"endpoint": {"uri": "http://127.0.0.1:9/n"}}}),
            )
            .await
            .expect("seed subscription");
    }
    st.store
        .create(
            &tenant,
            Kind::Entity,
            "urn:ngsi-ld:Vehicle:big",
            json!({"id": "urn:ngsi-ld:Vehicle:big", "type": "Vehicle"}),
        )
        .await
        .expect("seed entity");
    let inner = st.store.clone();
    st.store = Arc::new(Double::flaky_list(inner.clone(), usize::MAX));

    assert_eq!(
        call(&st, "DELETE", "/q/tenants/big").await,
        StatusCode::NO_CONTENT,
        "a Tenant whose listing is over the ceiling must still be reclaimable"
    );
    assert!(
        inner
            .list_page(&tenant, Kind::Subscription, None, 10)
            .await
            .expect("paged read")
            .is_empty(),
        "the purge removed what it collected"
    );
    assert!(
        inner
            .get(&tenant, Kind::Entity, "urn:ngsi-ld:Vehicle:big")
            .await
            .expect("store")
            .is_none(),
        "the Tenant's entities are gone"
    );
}
