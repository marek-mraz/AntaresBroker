//! NGSI-LD HTTP binding (docs/deep-analysis.md §9.3).
//!
//! v0 serves: /q/health, GET /ngsi-ld/v1/info/sourceIdentity, and an honest
//! 501 ProblemDetails for every not-yet-implemented NGSI-LD resource — so the
//! ETSI suite runs end-to-end from the first commit and per-suite pass counts
//! are the progress metric (§13).

use antares_model::{NgsiError, TenantId, API_ROOT};
use axum::{
    extract::State,
    http::{header, HeaderMap, StatusCode},
    response::{IntoResponse, Response},
    routing::get,
    Json, Router,
};
use std::sync::Arc;
use std::time::Instant;

#[derive(Clone)]
pub struct AppState {
    pub started: Instant,
    pub host_alias: String,
}

pub fn router(state: AppState) -> Router {
    let state = Arc::new(state);
    Router::new()
        .route("/q/health", get(health))
        .route(
            &format!("{API_ROOT}/info/sourceIdentity"),
            get(source_identity),
        )
        .fallback(not_implemented)
        .with_state(state)
}

async fn health() -> Json<serde_json::Value> {
    Json(serde_json::json!({ "status": "UP" }))
}

/// CIM 009 5.15.1 / 6.33 — Context Source identity.
async fn source_identity(State(state): State<Arc<AppState>>, headers: HeaderMap) -> Response {
    let tenant = match tenant_from(&headers) {
        Ok(t) => t,
        Err(e) => return problem(e),
    };
    let uptime = state.started.elapsed().as_secs();
    let body = serde_json::json!({
        "id": format!("urn:ngsi-ld:ContextSourceIdentity:{}", state.host_alias),
        "type": "ContextSourceIdentity",
        "hostAlias": state.host_alias,
        "uptime": format!("PT{uptime}S"),
    });
    let mut resp = Json(body).into_response();
    echo_tenant(&tenant, &mut resp);
    resp
}

/// Honest placeholder: unimplemented NGSI-LD resources answer 501 with a
/// ProblemDetails body instead of a bare 404 (per-endpoint handlers replace
/// this fallback as phases land).
async fn not_implemented(headers: HeaderMap, uri: axum::http::Uri) -> Response {
    let path = uri.path().to_owned();
    if !path.starts_with(API_ROOT) {
        return problem(NgsiError::ResourceNotFound(format!("unknown path {path}")));
    }
    let tenant = match tenant_from(&headers) {
        Ok(t) => t,
        Err(e) => return problem(e),
    };
    let pd = NgsiError::OperationNotSupported(format!(
        "{path} is not implemented yet (Antares v0 skeleton)"
    ))
    .to_problem_details();
    let mut resp = (
        StatusCode::NOT_IMPLEMENTED,
        [(header::CONTENT_TYPE, "application/json")],
        Json(serde_json::json!({
            "type": pd.r#type,
            "title": pd.title,
            "status": 501,
            "detail": pd.detail,
        })),
    )
        .into_response();
    echo_tenant(&tenant, &mut resp);
    resp
}

fn tenant_from(headers: &HeaderMap) -> Result<TenantId, NgsiError> {
    match headers.get("NGSILD-Tenant") {
        None => Ok(TenantId::default()),
        Some(v) => {
            let raw = v
                .to_str()
                .map_err(|_| NgsiError::BadRequestData("non-ASCII NGSILD-Tenant".into()))?;
            TenantId::new(raw)
        }
    }
}

/// 6.3.14: NGSILD-Tenant present in the request ⇒ present in the response.
fn echo_tenant(tenant: &TenantId, resp: &mut Response) {
    if tenant.as_str() != TenantId::DEFAULT {
        if let Ok(v) = tenant.as_str().parse() {
            resp.headers_mut().insert("NGSILD-Tenant", v);
        }
    }
}

fn problem(err: NgsiError) -> Response {
    let pd = err.to_problem_details();
    (
        StatusCode::from_u16(pd.status).unwrap_or(StatusCode::INTERNAL_SERVER_ERROR),
        [(header::CONTENT_TYPE, "application/json")],
        Json(serde_json::json!({
            "type": pd.r#type,
            "title": pd.title,
            "status": pd.status,
            "detail": pd.detail,
        })),
    )
        .into_response()
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::body::Body;
    use axum::http::Request;
    use http_body_util::BodyExt;
    use tower::ServiceExt;

    fn app() -> Router {
        router(AppState {
            started: Instant::now(),
            host_alias: "antares-test".into(),
        })
    }

    async fn body_json(resp: Response) -> serde_json::Value {
        let bytes = resp.into_body().collect().await.expect("body").to_bytes();
        serde_json::from_slice(&bytes).expect("json body")
    }

    #[tokio::test]
    async fn health_is_up() {
        let resp = app()
            .oneshot(Request::get("/q/health").body(Body::empty()).expect("req"))
            .await
            .expect("resp");
        assert_eq!(resp.status(), StatusCode::OK);
        assert_eq!(body_json(resp).await["status"], "UP");
    }

    #[tokio::test]
    async fn source_identity_served() {
        let resp = app()
            .oneshot(
                Request::get("/ngsi-ld/v1/info/sourceIdentity")
                    .body(Body::empty())
                    .expect("req"),
            )
            .await
            .expect("resp");
        assert_eq!(resp.status(), StatusCode::OK);
        let body = body_json(resp).await;
        assert_eq!(body["type"], "ContextSourceIdentity");
        assert_eq!(body["hostAlias"], "antares-test");
    }

    #[tokio::test]
    async fn unimplemented_ngsild_resource_is_501_problem_details() {
        let resp = app()
            .oneshot(
                Request::get("/ngsi-ld/v1/entities")
                    .body(Body::empty())
                    .expect("req"),
            )
            .await
            .expect("resp");
        assert_eq!(resp.status(), StatusCode::NOT_IMPLEMENTED);
        let body = body_json(resp).await;
        assert_eq!(
            body["type"],
            "https://uri.etsi.org/ngsi-ld/errors/OperationNotSupported"
        );
    }

    #[tokio::test]
    async fn tenant_header_is_echoed_and_validated() {
        let resp = app()
            .oneshot(
                Request::get("/ngsi-ld/v1/entities")
                    .header("NGSILD-Tenant", "city-01")
                    .body(Body::empty())
                    .expect("req"),
            )
            .await
            .expect("resp");
        assert_eq!(
            resp.headers()
                .get("NGSILD-Tenant")
                .map(|v| v.to_str().expect("ascii")),
            Some("city-01")
        );

        let resp = app()
            .oneshot(
                Request::get("/ngsi-ld/v1/entities")
                    .header("NGSILD-Tenant", "bad tenant!")
                    .body(Body::empty())
                    .expect("req"),
            )
            .await
            .expect("resp");
        assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
        let body = body_json(resp).await;
        assert_eq!(
            body["type"],
            "https://uri.etsi.org/ngsi-ld/errors/BadRequestData"
        );
    }

    #[tokio::test]
    async fn non_api_path_is_404() {
        let resp = app()
            .oneshot(Request::get("/nope").body(Body::empty()).expect("req"))
            .await
            .expect("resp");
        assert_eq!(resp.status(), StatusCode::NOT_FOUND);
    }
}
