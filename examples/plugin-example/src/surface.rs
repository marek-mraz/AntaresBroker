// SPDX-License-Identifier: EUPL-1.2
//! The routing seam: one `ApiSurface` mounted beside the NGSI-LD API root.

use antares_api::negotiate::{tenant_from, ApiError};
use antares_api::{ApiSurface, AppState};
use axum::http::HeaderMap;
use axum::routing::get;
use axum::Router;
use serde_json::{json, Value};

/// A surface a deployment adds without touching a core crate. It reports how
/// many entities the plugin's store holds for ONE tenant — enough to prove
/// the surface reaches the same `AppState` every handler does, and short
/// enough to read as the template it is.
pub struct ExampleSurface;

impl ApiSurface for ExampleSurface {
    fn name(&self) -> &str {
        crate::NAME
    }

    fn prefix(&self) -> &str {
        "/x/example"
    }

    fn router(&self, _st: AppState) -> Router<AppState> {
        Router::new().route("/entities/count", get(count))
    }

    fn version_info(&self) -> Value {
        json!({"routes": ["/x/example/entities/count"]})
    }
}

/// Entities the plugin's store holds for the requesting tenant.
///
/// Two rules a surface does not get to opt out of. The tenant comes from
/// the validated `NGSILD-Tenant` header (6.3.14) and is passed to the store
/// on the call — never a default standing in for whoever asked, which is
/// how one deployment's numbers end up in another's answer. And a store
/// error is returned, not defaulted: a swallowed failure here reports zero
/// entities, which reads as an empty tenant rather than a broken store.
async fn count(
    axum::extract::State(st): axum::extract::State<AppState>,
    headers: HeaderMap,
) -> Result<axum::Json<Value>, ApiError> {
    let tenant = tenant_from(&headers)?;
    let rows = st.store.list(&tenant, antares_store::Kind::Entity)?;
    Ok(axum::Json(
        json!({"tenant": tenant.as_str(), "entities": rows.len()}),
    ))
}
