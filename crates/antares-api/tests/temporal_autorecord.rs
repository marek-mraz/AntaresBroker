// SPDX-License-Identifier: EUPL-1.2
//! 5.6.4 + auto-recording: every Partial Attribute Update appends a new
//! attribute instance to the temporal evolution — the regression behind the
//! playground's flat history charts (create recorded, PATCHes silently not).

use antares_api::AppState;
use axum::body::Body;
use axum::http::{Request, StatusCode};
use http_body_util::BodyExt;
use tower::ServiceExt;

async fn send(st: &AppState, req: Request<Body>) -> (StatusCode, String) {
    let resp = antares_api::router(st.clone())
        .oneshot(req)
        .await
        .expect("response");
    let status = resp.status();
    let bytes = resp.into_body().collect().await.expect("body").to_bytes();
    (status, String::from_utf8_lossy(&bytes).into_owned())
}

#[tokio::test(flavor = "multi_thread")]
async fn partial_update_appends_temporal_instances() {
    let mut st = AppState::new("test".into());
    antares_api::wire(&mut st);

    let (status, body) = send(
        &st,
        Request::builder()
            .method("POST")
            .uri("/ngsi-ld/v1/entities")
            .header("Content-Type", "application/json")
            .header(
                "Content-Length",
                r#"{"id":"urn:ngsi-ld:Rec:1","type":"Rec",
                    "v":{"type":"Property","value":1,"observedAt":"2026-08-08T14:00:00Z"}}"#
                    .len(),
            )
            .body(Body::from(
                r#"{"id":"urn:ngsi-ld:Rec:1","type":"Rec",
                    "v":{"type":"Property","value":1,"observedAt":"2026-08-08T14:00:00Z"}}"#,
            ))
            .expect("request"),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{body}");

    for (i, ts) in [(2, "2026-08-08T14:00:10Z"), (3, "2026-08-08T14:00:20Z")] {
        let (status, body) = send(
            &st,
            Request::builder()
                .method("PATCH")
                .uri("/ngsi-ld/v1/entities/urn:ngsi-ld:Rec:1/attrs/v")
                .header("Content-Type", "application/json")
                .header(
                    "Content-Length",
                    format!(r#"{{"type":"Property","value":{i},"observedAt":"{ts}"}}"#).len(),
                )
                .body(Body::from(format!(
                    r#"{{"type":"Property","value":{i},"observedAt":"{ts}"}}"#
                )))
                .expect("request"),
        )
        .await;
        assert_eq!(status, StatusCode::NO_CONTENT, "{body}");
    }

    let (status, body) = send(
        &st,
        Request::builder()
            .uri("/ngsi-ld/v1/temporal/entities/urn:ngsi-ld:Rec:1")
            .body(Body::empty())
            .expect("request"),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let doc: serde_json::Value = serde_json::from_str(&body).expect("json");
    let instances = doc["v"].as_array().expect("v instance array");
    let values: Vec<i64> = instances
        .iter()
        .filter_map(|i| i["value"].as_i64())
        .collect();
    assert_eq!(
        values.len(),
        3,
        "each PATCH must append an instance, got {body}"
    );
    assert!(
        [1, 2, 3].iter().all(|v| values.contains(v)),
        "expected values 1..3, got {values:?}"
    );
}

/// 4.5.6: a scope change from the Core API is recorded in the temporal
/// evolution as a temporal Property instance whose observedAt "should be set
/// as a copy of the modifiedAt sub-Property".
#[tokio::test(flavor = "multi_thread")]
async fn scope_update_appends_temporal_property_instance() {
    let mut st = AppState::new("test".into());
    antares_api::wire(&mut st);

    let create = r#"{"id":"urn:ngsi-ld:Rec:2","type":"Rec","scope":"/A",
        "v":{"type":"Property","value":1}}"#;
    let (status, body) = send(
        &st,
        Request::builder()
            .method("POST")
            .uri("/ngsi-ld/v1/entities")
            .header("Content-Type", "application/json")
            .header("Content-Length", create.len())
            .body(Body::from(create))
            .expect("request"),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{body}");

    let patch = r#"{"type":"Rec","scope":"/B"}"#;
    let (status, body) = send(
        &st,
        Request::builder()
            .method("PATCH")
            .uri("/ngsi-ld/v1/entities/urn:ngsi-ld:Rec:2")
            .header("Content-Type", "application/json")
            .header("Content-Length", patch.len())
            .body(Body::from(patch))
            .expect("request"),
    )
    .await;
    assert_eq!(status, StatusCode::NO_CONTENT, "{body}");

    let (status, body) = send(
        &st,
        Request::builder()
            .uri("/ngsi-ld/v1/temporal/entities/urn:ngsi-ld:Rec:2?options=sysAttrs")
            .body(Body::empty())
            .expect("request"),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let doc: serde_json::Value = serde_json::from_str(&body).expect("json");
    let scope = doc["scope"].as_array().expect("scope instance array");
    let inst = scope
        .iter()
        .find(|i| i["value"] == serde_json::json!(["/B"]))
        .unwrap_or_else(|| panic!("no /B scope instance in {body}"));
    assert_eq!(inst["type"], "Property");
    assert_eq!(
        inst["observedAt"], inst["modifiedAt"],
        "observedAt must copy modifiedAt"
    );
}

/// 4.5.7: the temporal representation of a Property is an array of 4.5.2
/// instances; deletion records an instance with value "urn:ngsi-ld:null" and
/// deletedAt set; every instance carries an instanceId.
#[tokio::test(flavor = "multi_thread")]
async fn property_deletion_records_null_instance() {
    let mut st = AppState::new("test".into());
    antares_api::wire(&mut st);

    let create = r#"{"id":"urn:ngsi-ld:Rec:3","type":"Rec",
        "v":{"type":"Property","value":7,"observedAt":"2026-08-10T00:00:00Z"}}"#;
    let (status, body) = send(
        &st,
        Request::builder()
            .method("POST")
            .uri("/ngsi-ld/v1/entities")
            .header("Content-Type", "application/json")
            .header("Content-Length", create.len())
            .body(Body::from(create))
            .expect("request"),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{body}");

    let (status, body) = send(
        &st,
        Request::builder()
            .method("DELETE")
            .uri("/ngsi-ld/v1/entities/urn:ngsi-ld:Rec:3/attrs/v")
            .body(Body::empty())
            .expect("request"),
    )
    .await;
    assert_eq!(status, StatusCode::NO_CONTENT, "{body}");

    let (status, body) = send(
        &st,
        Request::builder()
            .uri("/ngsi-ld/v1/temporal/entities/urn:ngsi-ld:Rec:3?timeproperty=deletedAt")
            .body(Body::empty())
            .expect("request"),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let doc: serde_json::Value = serde_json::from_str(&body).expect("json");
    let instances = doc["v"].as_array().expect("v instance array");
    let deleted = instances
        .iter()
        .find(|i| i["value"] == "urn:ngsi-ld:null")
        .unwrap_or_else(|| panic!("no null deletion instance in {body}"));
    assert!(deleted["deletedAt"].is_string(), "deletedAt must be set");
    assert!(
        deleted["instanceId"]
            .as_str()
            .is_some_and(|s| s.starts_with("urn:")),
        "instanceId must be maintained"
    );
}

/// 4.5.8: the temporal representation of a Relationship is an array of 4.5.3
/// instances; deletion records an instance with object "urn:ngsi-ld:null" and
/// deletedAt set; every instance carries an instanceId.
#[tokio::test(flavor = "multi_thread")]
async fn relationship_deletion_records_null_instance() {
    let mut st = AppState::new("test".into());
    antares_api::wire(&mut st);

    let create = r#"{"id":"urn:ngsi-ld:Rec:4","type":"Rec",
        "r":{"type":"Relationship","object":"urn:ngsi-ld:X:1",
             "observedAt":"2026-08-10T00:00:00Z"}}"#;
    let (status, body) = send(
        &st,
        Request::builder()
            .method("POST")
            .uri("/ngsi-ld/v1/entities")
            .header("Content-Type", "application/json")
            .header("Content-Length", create.len())
            .body(Body::from(create))
            .expect("request"),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{body}");

    let (status, body) = send(
        &st,
        Request::builder()
            .method("DELETE")
            .uri("/ngsi-ld/v1/entities/urn:ngsi-ld:Rec:4/attrs/r")
            .body(Body::empty())
            .expect("request"),
    )
    .await;
    assert_eq!(status, StatusCode::NO_CONTENT, "{body}");

    let (status, body) = send(
        &st,
        Request::builder()
            .uri("/ngsi-ld/v1/temporal/entities/urn:ngsi-ld:Rec:4?timeproperty=deletedAt")
            .body(Body::empty())
            .expect("request"),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let doc: serde_json::Value = serde_json::from_str(&body).expect("json");
    let instances = doc["r"].as_array().expect("r instance array");
    let deleted = instances
        .iter()
        .find(|i| i["object"] == "urn:ngsi-ld:null")
        .unwrap_or_else(|| panic!("no null deletion instance in {body}"));
    assert_eq!(deleted["type"], "Relationship");
    assert!(
        deleted.get("value").is_none(),
        "a Relationship deletion instance must not carry a Property value"
    );
    assert!(deleted["deletedAt"].is_string(), "deletedAt must be set");
    assert!(
        deleted["instanceId"]
            .as_str()
            .is_some_and(|s| s.starts_with("urn:")),
        "instanceId must be maintained"
    );
}

/// 4.5.9: simplified temporal representation — per-type member names
/// (values/objects/languageMaps/valueLists/objectLists), [value, time] pairs,
/// bare ordered arrays for the List types (Examples 3 and 7), wrapped
/// {"languageMap": …} for LanguageProperty (Example 2).
#[tokio::test(flavor = "multi_thread")]
async fn simplified_temporal_pairs_per_attribute_type() {
    let mut st = AppState::new("test".into());
    antares_api::wire(&mut st);

    let create = r#"{"id":"urn:ngsi-ld:Rec:5","type":"Rec",
        "speed":[{"type":"Property","value":1,"observedAt":"2026-08-10T00:00:01Z"},
                 {"type":"Property","value":2,"observedAt":"2026-08-10T00:00:02Z"}],
        "location":{"type":"GeoProperty","observedAt":"2026-08-10T00:00:01Z",
                    "value":{"type":"Point","coordinates":[1.0,2.0]}},
        "says":{"type":"LanguageProperty","observedAt":"2026-08-10T00:00:01Z",
                "languageMap":{"en":"yes"}},
        "steps":{"type":"ListProperty","observedAt":"2026-08-10T00:00:01Z",
                 "valueList":["a","b"]},
        "spouse":{"type":"Relationship","observedAt":"2026-08-10T00:00:01Z",
                  "object":"urn:ngsi-ld:P:1"},
        "route":{"type":"ListRelationship","observedAt":"2026-08-10T00:00:01Z",
                 "objectList":["urn:ngsi-ld:R:1","urn:ngsi-ld:R:2"]}}"#;
    let (status, body) = send(
        &st,
        Request::builder()
            .method("POST")
            .uri("/ngsi-ld/v1/temporal/entities")
            .header("Content-Type", "application/json")
            .header("Content-Length", create.len())
            .body(Body::from(create))
            .expect("request"),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{body}");

    let (status, body) = send(
        &st,
        Request::builder()
            .uri("/ngsi-ld/v1/temporal/entities/urn:ngsi-ld:Rec:5?options=temporalValues")
            .body(Body::empty())
            .expect("request"),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let doc: serde_json::Value = serde_json::from_str(&body).expect("json");

    let speed = &doc["speed"];
    assert_eq!(speed["type"], "Property");
    let pairs = speed["values"].as_array().expect("values pairs");
    assert_eq!(pairs.len(), 2);
    assert_eq!(pairs[0].as_array().expect("pair").len(), 2);
    assert_eq!(pairs[0][0].as_f64(), Some(1.0));
    assert_eq!(pairs[0][1], "2026-08-10T00:00:01Z");

    assert_eq!(doc["location"]["type"], "GeoProperty");
    assert_eq!(doc["location"]["values"][0][0]["type"], "Point");

    assert_eq!(
        doc["says"]["languageMaps"][0][0],
        serde_json::json!({"languageMap": {"en": "yes"}})
    );

    // Lists carry the BARE ordered array as the pair's first element
    assert_eq!(doc["steps"]["type"], "ListProperty");
    assert_eq!(
        doc["steps"]["valueLists"][0][0],
        serde_json::json!(["a", "b"])
    );
    assert_eq!(doc["spouse"]["objects"][0][0], "urn:ngsi-ld:P:1");
    assert_eq!(doc["route"]["type"], "ListRelationship");
    assert_eq!(
        doc["route"]["objectLists"][0][0],
        serde_json::json!(["urn:ngsi-ld:R:1", "urn:ngsi-ld:R:2"])
    );

    // the simplified object shall ONLY contain type + the per-type member
    // (datasetId when grouped): no reified value/object/instanceId leakage
    for (attr, member) in [
        ("speed", "values"),
        ("says", "languageMaps"),
        ("steps", "valueLists"),
        ("spouse", "objects"),
        ("route", "objectLists"),
    ] {
        let extra: Vec<&String> = doc[attr]
            .as_object()
            .expect("object")
            .keys()
            .filter(|k| !["type", "datasetId", member].contains(&k.as_str()))
            .collect();
        assert!(extra.is_empty(), "{attr} leaks members: {extra:?}");
    }
}

/// 4.5.19: aggregated temporal representation — per-method members of
/// [value, periodStart, periodEnd] triples; Properties labelled "Property",
/// Relationships "Relationship"; PT0S = one whole-range period; only the
/// requested methods (plus type) appear.
#[tokio::test(flavor = "multi_thread")]
async fn aggregated_temporal_representation() {
    let mut st = AppState::new("test".into());
    antares_api::wire(&mut st);

    let create = r#"{"id":"urn:ngsi-ld:Rec:6","type":"Rec",
        "speed":[{"type":"Property","value":1,"observedAt":"2026-08-10T00:00:01Z"},
                 {"type":"Property","value":2,"observedAt":"2026-08-10T00:00:02Z"},
                 {"type":"Property","value":3,"observedAt":"2026-08-10T00:00:03Z"}],
        "spouse":[{"type":"Relationship","object":"urn:ngsi-ld:P:1",
                   "observedAt":"2026-08-10T00:00:01Z"},
                  {"type":"Relationship","object":"urn:ngsi-ld:P:1",
                   "observedAt":"2026-08-10T00:00:02Z"}]}"#;
    let (status, body) = send(
        &st,
        Request::builder()
            .method("POST")
            .uri("/ngsi-ld/v1/temporal/entities")
            .header("Content-Type", "application/json")
            .header("Content-Length", create.len())
            .body(Body::from(create))
            .expect("request"),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{body}");

    let (status, body) = send(
        &st,
        Request::builder()
            .uri("/ngsi-ld/v1/temporal/entities/urn:ngsi-ld:Rec:6?options=aggregatedValues&aggrMethods=sum,avg,totalCount&aggrPeriodDuration=PT0S&attrs=speed&timerel=after&timeAt=2026-08-10T00:00:00Z")
            .body(Body::empty())
            .expect("request"),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let doc: serde_json::Value = serde_json::from_str(&body).expect("json");

    let speed = &doc["speed"];
    assert_eq!(speed["type"], "Property");
    let sum = speed["sum"].as_array().expect("sum rows");
    assert_eq!(sum.len(), 1, "PT0S = one whole-range period: {body}");
    let row = sum[0].as_array().expect("triple");
    assert_eq!(row.len(), 3, "[value, start, end]");
    assert_eq!(row[0].as_f64(), Some(6.0));
    assert!(row[1].as_str().is_some_and(|s| s.contains('T')));
    assert_eq!(speed["avg"][0][0].as_f64(), Some(2.0));
    assert_eq!(speed["totalCount"][0][0], 3);
    let extra: Vec<&String> = speed
        .as_object()
        .expect("object")
        .keys()
        .filter(|k| !["type", "sum", "avg", "totalCount"].contains(&k.as_str()))
        .collect();
    assert!(extra.is_empty(), "only requested methods: {extra:?}");

    // Relationship label + totalCount eligibility
    let (status, body) = send(
        &st,
        Request::builder()
            .uri("/ngsi-ld/v1/temporal/entities/urn:ngsi-ld:Rec:6?options=aggregatedValues&aggrMethods=totalCount&attrs=spouse&timerel=after&timeAt=2026-08-10T00:00:00Z")
            .body(Body::empty())
            .expect("request"),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let doc: serde_json::Value = serde_json::from_str(&body).expect("json");
    assert_eq!(doc["spouse"]["type"], "Relationship");
    assert_eq!(doc["spouse"]["totalCount"][0][0], 2);

    // ineligible method on a Relationship → 400 InvalidRequest
    let (status, body) = send(
        &st,
        Request::builder()
            .uri("/ngsi-ld/v1/temporal/entities/urn:ngsi-ld:Rec:6?options=aggregatedValues&aggrMethods=sum&attrs=spouse&timerel=after&timeAt=2026-08-10T00:00:00Z")
            .body(Body::empty())
            .expect("request"),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
    assert!(body.contains("InvalidRequest"), "{body}");
}

/// 4.11: aggrPeriodDuration is an ISO 8601 duration; a magnitude far beyond
/// any representable time range violates the parameter's value space and
/// answers 400 BadRequestData — it must never reach chrono duration
/// arithmetic, which panics (a remote 500) on such values.
#[tokio::test(flavor = "multi_thread")]
async fn absurd_aggr_period_duration_is_400_not_a_panic() {
    let mut st = AppState::new("test".into());
    antares_api::wire(&mut st);

    let create = r#"{"id":"urn:ngsi-ld:Rec:7","type":"Rec",
        "speed":[{"type":"Property","value":1,"observedAt":"2026-08-10T00:00:01Z"},
                 {"type":"Property","value":2,"observedAt":"2026-08-10T00:00:02Z"}]}"#;
    let (status, body) = send(
        &st,
        Request::builder()
            .method("POST")
            .uri("/ngsi-ld/v1/temporal/entities")
            .header("Content-Type", "application/json")
            .header("Content-Length", create.len())
            .body(Body::from(create))
            .expect("request"),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{body}");

    for period in ["PT99999999999999999S", "P99999999999W"] {
        let (status, body) = send(
            &st,
            Request::builder()
                .uri(format!(
                    "/ngsi-ld/v1/temporal/entities/urn:ngsi-ld:Rec:7\
                     ?options=aggregatedValues&aggrMethods=sum&attrs=speed\
                     &timerel=after&timeAt=2026-08-10T00:00:00Z\
                     &aggrPeriodDuration={period}"
                ))
                .body(Body::empty())
                .expect("request"),
        )
        .await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "{period}: {body}");
        assert!(body.contains("BadRequestData"), "{period}: {body}");
        assert!(
            !body.contains("InternalError"),
            "must not be a 500-class answer: {body}"
        );
    }

    // a sane sub-range period still aggregates
    let (status, body) = send(
        &st,
        Request::builder()
            .uri(
                "/ngsi-ld/v1/temporal/entities/urn:ngsi-ld:Rec:7\
                 ?options=aggregatedValues&aggrMethods=sum&attrs=speed\
                 &timerel=after&timeAt=2026-08-10T00:00:00Z&aggrPeriodDuration=PT1H",
            )
            .body(Body::empty())
            .expect("request"),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
}
