// SPDX-License-Identifier: EUPL-1.2
//! 5.7.3.4 and 5.7.4.4: "If projection attributes are present and indicate
//! the use of Linked Entity retrieval, an error of type BadRequestData shall
//! be raised." A `{…}` level in a projection is one Linked Entity hop
//! (5.7.1.4); the temporal consumption operations define no join, so the
//! rejection is unconditional — there is no `join` parameter that makes a
//! hopping `pick` or `omit` acceptable on either of them.

use antares_api::AppState;
use axum::body::Body;
use axum::http::{Request, StatusCode};
use serde_json::{json, Value};
use tower::ServiceExt;

const ID: &str = "urn:ngsi-ld:Vehicle:tp1";
const WINDOW: &str = "timerel=after&timeAt=2000-01-01T00:00:00Z&timeproperty=createdAt";

async fn send(st: &AppState, req: Request<Body>) -> (StatusCode, Value) {
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

async fn get(st: &AppState, uri: &str) -> (StatusCode, Value) {
    send(
        st,
        Request::builder()
            .method("GET")
            .uri(uri)
            .body(Body::empty())
            .expect("request"),
    )
    .await
}

async fn seeded() -> AppState {
    let mut st = AppState::new("me".into());
    antares_api::wire(&mut st).await;
    let body = json!({"id": ID, "type": "Vehicle",
        "speed": {"type": "Property", "value": 10}})
    .to_string();
    let (status, b) = send(
        &st,
        Request::builder()
            .method("POST")
            .uri("/ngsi-ld/v1/entities")
            .header("Content-Type", "application/json")
            .header("Content-Length", body.len())
            .body(Body::from(body))
            .expect("request"),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "seed: {b}");
    st
}

fn is_bad_request(status: StatusCode, body: &Value) {
    assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
    assert_eq!(
        body["type"], "https://uri.etsi.org/ngsi-ld/errors/BadRequestData",
        "{body}"
    );
}

/// 5.7.3.4 on the single-Entity temporal retrieve: a `pick` or an `omit`
/// carrying a `{…}` Linked Entity hop is BadRequestData, while the flat
/// projection is the accepted case on the same operation.
#[tokio::test(flavor = "multi_thread")]
async fn clause_5_7_3_4_a_hopping_projection_is_refused_on_the_retrieve() {
    let st = seeded().await;
    let base = format!("/ngsi-ld/v1/temporal/entities/{ID}?{WINDOW}");
    refuses_hops_accepts_flat(&st, &base).await;
}

/// 5.7.4.4 on the temporal query: the same rule, the same rejection.
#[tokio::test(flavor = "multi_thread")]
async fn clause_5_7_4_4_a_hopping_projection_is_refused_on_the_query() {
    let st = seeded().await;
    let base = format!("/ngsi-ld/v1/temporal/entities?type=Vehicle&{WINDOW}");
    refuses_hops_accepts_flat(&st, &base).await;
}

async fn refuses_hops_accepts_flat(st: &AppState, base: &str) {
    for proj in ["pick=speed%7Bvalue%7D", "omit=speed%7Bvalue%7D"] {
        let (status, body) = get(st, &format!("{base}&{proj}")).await;
        is_bad_request(status, &body);
        assert!(
            body["detail"]
                .as_str()
                .is_some_and(|d| d.contains("Linked Entity")),
            "{proj}: {body}"
        );
    }
    let (status, body) = get(st, &format!("{base}&pick=speed")).await;
    assert_eq!(status, StatusCode::OK, "{body}");
}

/// 5.7.3.5: "If a restrictive list of Entity member names is present, every
/// Entity within the payload body is reduced down to only contain the defined
/// Entity members." 5.7.3.3 says that list holds "a restrictive list of Entity
/// member names (`"id"`, `"type"`, `"scope"` or an Attribute name)", so a pick
/// naming only core members selects no Attribute — which is the reduced answer,
/// not a Temporal Evolution that holds none of what was asked for.
#[tokio::test(flavor = "multi_thread")]
async fn clause_5_7_3_5_a_core_only_pick_reduces_the_entity_rather_than_hiding_it() {
    let st = seeded().await;
    for (pick, kept) in [("id", "id"), ("type", "type"), ("id,type", "id")] {
        let (status, body) = get(
            &st,
            &format!("/ngsi-ld/v1/temporal/entities/{ID}?{WINDOW}&pick={pick}"),
        )
        .await;
        assert_eq!(status, StatusCode::OK, "pick={pick}: {body}");
        assert!(
            body.get(kept).is_some(),
            "pick={pick} dropped {kept}: {body}"
        );
        assert!(
            body.get("speed").is_none(),
            "pick={pick} kept an Attribute it did not name: {body}"
        );
    }
    // An Attribute name the Entity does not carry is still the 404 of 5.7.3.4.
    let (status, body) = get(
        &st,
        &format!("/ngsi-ld/v1/temporal/entities/{ID}?{WINDOW}&pick=colour"),
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND, "{body}");
}

const EXPIRING: &str = "urn:ngsi-ld:Vehicle:tpx";

/// Table 6.3.11-1 (p.276): "When its value includes the keyword "sysAttrs", a
/// representation of NGSI-LD Elements shall be provided so that the system
/// generated temporal attributes createdAt, modifiedAt and the system temporal
/// attribute expiresAt are included in the response payload body where known.
/// In the case of temporal representations, also the system generated temporal
/// attribute deletedAt is included, if the NGSI-LD Element has been deleted."
/// The option is what admits them, so without it none of them is in the
/// payload — the current-state path gates the same set.
#[tokio::test(flavor = "multi_thread")]
async fn clause_6_3_11_sys_attrs_gates_the_root_expires_at_of_a_temporal_representation() {
    let mut st = AppState::new("me".into());
    antares_api::wire(&mut st).await;
    let body = json!({
        "id": EXPIRING, "type": "Vehicle",
        "expiresAt": "2099-01-01T00:00:00Z",
        "speed": [{"type": "Property", "value": 1, "observedAt": "2026-01-01T00:00:00Z"}],
    })
    .to_string();
    let (status, b) = send(
        &st,
        Request::builder()
            .method("POST")
            .uri("/ngsi-ld/v1/temporal/entities")
            .header("Content-Type", "application/json")
            .header("Content-Length", body.len())
            .body(Body::from(body))
            .expect("request"),
    )
    .await;
    assert!(status.is_success(), "seed: {status} {b}");

    let window = "timerel=before&timeAt=2030-01-01T00:00:00Z";
    let (status, plain) = get(
        &st,
        &format!("/ngsi-ld/v1/temporal/entities/{EXPIRING}?{window}"),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{plain}");
    for gated in ["expiresAt", "createdAt", "modifiedAt"] {
        assert!(
            plain.get(gated).is_none(),
            "{gated} is a sysAttrs member and no sysAttrs was asked for: {plain}"
        );
    }

    let (status, sys) = get(
        &st,
        &format!("/ngsi-ld/v1/temporal/entities/{EXPIRING}?{window}&options=sysAttrs"),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{sys}");
    assert_eq!(
        sys["expiresAt"], "2099-01-01T00:00:00Z",
        "sysAttrs includes expiresAt where known: {sys}"
    );
    assert!(sys.get("createdAt").is_some(), "{sys}");
}
