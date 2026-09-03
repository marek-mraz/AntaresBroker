// SPDX-License-Identifier: EUPL-1.2
//! The routing seam: one `ApiSurface` mounted beside the NGSI-LD API root,
//! and the façade seam it is also the worked example of — one route that
//! answers in a shape NGSI-LD does not define, served by driving this
//! broker's own router in process (`AppState::call`).

use antares_api::negotiate::{tenant_from, ApiError};
use antares_api::{ApiSurface, AppState};
use axum::body::Body;
use axum::http::{HeaderMap, Request, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::get;
use axum::Router;
use serde_json::{json, Value};
use std::collections::HashMap;

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
        Router::new()
            .route("/entities/count", get(count))
            .route("/things", get(things))
    }

    fn version_info(&self) -> Value {
        json!({"routes": ["/x/example/entities/count", "/x/example/things"]})
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

/// A façade in one route: `GET /x/example/things?kind=<type>` answers the
/// `{"value": [...]}` envelope other standards use, over the Entities this
/// broker holds.
///
/// It is deliberately the SHAPE of a façade rather than any real standard's
/// mapping — what it demonstrates is the rule every façade follows. The
/// translation happens twice and the data path once: the request becomes an
/// NGSI-LD request, `AppState::call` serves it through the same router a
/// socket client reaches, and the NGSI-LD answer becomes the façade's
/// answer. Nothing here touches the store, re-implements a query, or
/// repeats a check the broker already makes — negotiation, the bounds wall,
/// tenancy (6.3.14) and the policy seam all run inside the call, on the
/// caller's own headers, because the handle carried them.
async fn things(
    axum::extract::State(st): axum::extract::State<AppState>,
    axum::extract::Query(q): axum::extract::Query<HashMap<String, String>>,
    headers: HeaderMap,
) -> Response {
    let kind = q.get("kind").map(String::as_str).unwrap_or("Thing");
    // `keyValues` (4.5.4) is the representation a façade wants: values
    // without the NGSI-LD envelope, which is most of the mapping already
    // done by the broker rather than by hand here.
    let inner = format!("/ngsi-ld/v1/entities?type={kind}&options=keyValues");
    let Ok(req) = Request::get(&inner).body(Body::empty()) else {
        // Only an unbuildable URI reaches here — a `kind` with a character
        // no request line may carry. It is the caller's input, so it is the
        // caller's error, not a 500.
        return error(StatusCode::BAD_REQUEST, "kind is not a usable Entity type");
    };
    let resp = st.call(&headers, req).await;
    let status = resp.status();
    let Ok(body) = axum::body::to_bytes(resp.into_body(), usize::MAX).await else {
        return error(
            StatusCode::INTERNAL_SERVER_ERROR,
            "the answer was not readable",
        );
    };
    if !status.is_success() {
        // An NGSI-LD error becomes the façade's error. The status is kept —
        // it is the broker's verdict on the operation and the façade has no
        // better one — while the body is re-rendered, because a caller of
        // this API is not expecting Table 6.3.2-1 ProblemDetails. 6.3.4's
        // bare statuses (411, 414, 415) carry no body at all, so the reason
        // phrase stands in.
        let detail = serde_json::from_slice::<Value>(&body)
            .ok()
            .and_then(|v| v.get("detail").and_then(Value::as_str).map(str::to_owned))
            .unwrap_or_else(|| status.canonical_reason().unwrap_or("error").to_owned());
        return error(status, &detail);
    }
    let value: Value = serde_json::from_slice(&body).unwrap_or_else(|_| json!([]));
    axum::Json(json!({"value": value})).into_response()
}

/// The façade's own error shape — not ProblemDetails, which is the point:
/// a façade answers in its standard's vocabulary, and the NGSI-LD error is
/// what it translates from.
fn error(status: StatusCode, message: &str) -> Response {
    (
        status,
        axum::Json(json!({"error": {"code": status.as_u16(), "message": message}})),
    )
        .into_response()
}
