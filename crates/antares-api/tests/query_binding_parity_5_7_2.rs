// SPDX-License-Identifier: EUPL-1.2
//! 5.7.2.4 governs Query Entities, and 6.4.3 gives it two bindings: the
//! parameters in the URI (GET /entities) and the 5.2.23 Query object in a
//! body (POST /entityOperations/query). One behaviour clause, so one
//! refusal: an input the URI form rejects has to be rejected in the body
//! form too, and an input it accepts has to be accepted.
//!
//! The divergence this pins is silent in the dangerous direction. The body
//! form does not run the URI form's handler — it folds the Query into
//! parameters and calls the shared filter — so a member validated in that
//! handler and nowhere else is carried through unchecked. `csf` was exactly
//! that: refused with the URI, dropped by the registration matcher with the
//! body, and a dropped `csf` widens the fan-out to the Context Sources the
//! filter names to avoid.

use antares_api::AppState;
use axum::body::Body;
use axum::http::{Request, StatusCode};
use serde_json::{json, Value};
use tower::ServiceExt;

async fn status(st: &AppState, req: Request<Body>) -> StatusCode {
    antares_api::router(st.clone())
        .oneshot(req)
        .await
        .expect("response")
        .status()
}

async fn get(st: &AppState, query: &str) -> StatusCode {
    status(
        st,
        Request::builder()
            .method("GET")
            .uri(format!("/ngsi-ld/v1/entities?{query}"))
            .body(Body::empty())
            .expect("request"),
    )
    .await
}

async fn post(st: &AppState, q: Value) -> StatusCode {
    let body = q.to_string();
    status(
        st,
        Request::builder()
            .method("POST")
            .uri("/ngsi-ld/v1/entityOperations/query")
            .header("Content-Type", "application/json")
            .header("Content-Length", body.len())
            .body(Body::from(body))
            .expect("request"),
    )
    .await
}

/// The same query, written both ways, answers with the same status. Every
/// case carries a member the 5.7.2.4 value spaces refuse (an id that is not
/// a URI, an idPattern that is not a regular expression, a projection or a
/// context source filter outside its grammar, a geometry that does not
/// parse, a join outside its enumeration), plus one that both forms must
/// serve — without it a broker that refused everything would pass.
#[tokio::test(flavor = "multi_thread")]
async fn clause_5_7_2_4_both_bindings_of_query_entities_refuse_the_same_input() {
    let st = AppState::new("query-parity".into());
    let cases: Vec<(&str, &str, Value)> = vec![
        (
            "id that is not a URI",
            "type=Vehicle&id=not%20a%20uri",
            json!({"type": "Query", "entities": [{"type": "Vehicle", "id": "not a uri"}]}),
        ),
        (
            "idPattern that is not a regular expression",
            "type=Vehicle&idPattern=%5B",
            json!({"type": "Query", "entities": [{"type": "Vehicle", "idPattern": "["}]}),
        ),
        (
            "pick outside the 4.21 grammar",
            "type=Vehicle&pick=a%20b",
            json!({"type": "Query", "entities": [{"type": "Vehicle"}], "pick": ["a b"]}),
        ),
        (
            "omit outside the 4.21 grammar",
            "type=Vehicle&omit=a%20b",
            json!({"type": "Query", "entities": [{"type": "Vehicle"}], "omit": ["a b"]}),
        ),
        (
            "csf outside the 4.9 grammar",
            "type=Vehicle&csf=%28%28%28",
            json!({"type": "Query", "entities": [{"type": "Vehicle"}], "csf": "((("}),
        ),
        (
            "coordinates that are not a geometry",
            "type=Vehicle&georel=near&geometry=Point&coordinates=junk",
            json!({"type": "Query", "entities": [{"type": "Vehicle"}],
                   "geoQ": {"georel": "near", "geometry": "Point", "coordinates": "junk"}}),
        ),
        (
            "join outside its enumeration",
            "type=Vehicle&join=sideways",
            json!({"type": "Query", "entities": [{"type": "Vehicle"}], "join": "sideways"}),
        ),
        (
            "a query both forms must serve",
            "type=Vehicle",
            json!({"type": "Query", "entities": [{"type": "Vehicle"}]}),
        ),
    ];
    for (what, query, body) in cases {
        let uri_form = get(&st, query).await;
        let body_form = post(&st, body).await;
        assert_eq!(
            uri_form, body_form,
            "{what}: the URI form answered {uri_form} and the Query body {body_form}"
        );
    }
}
