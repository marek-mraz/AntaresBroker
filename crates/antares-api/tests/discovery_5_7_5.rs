//! Discovery contract beyond the representation shapes: tenant scoping,
//! what must NOT be discoverable, and the parameter surface of
//! GET /types, /types/{type}, /attributes and /attributes/{attrId}
//! (5.7.5-5.7.10).

use antares_api::AppState;
use axum::body::Body;
use axum::http::{HeaderMap, Request, StatusCode};
use http_body_util::BodyExt;
use tower::ServiceExt;

async fn send_h(st: &AppState, req: Request<Body>) -> (StatusCode, HeaderMap, String) {
    let resp = antares_api::router(st.clone())
        .oneshot(req)
        .await
        .expect("response");
    let status = resp.status();
    let headers = resp.headers().clone();
    let bytes = resp.into_body().collect().await.expect("body").to_bytes();
    (
        status,
        headers,
        String::from_utf8_lossy(&bytes).into_owned(),
    )
}

async fn send(st: &AppState, req: Request<Body>) -> (StatusCode, String) {
    let (status, _, body) = send_h(st, req).await;
    (status, body)
}

/// GET with an optional NGSILD-Tenant.
async fn get(st: &AppState, uri: &str, tenant: Option<&str>) -> (StatusCode, String) {
    let mut req = Request::builder().uri(uri);
    if let Some(t) = tenant {
        req = req.header("NGSILD-Tenant", t);
    }
    send(st, req.body(Body::empty()).expect("request")).await
}

async fn create(st: &AppState, tenant: Option<&str>, body: &'static str) {
    let mut req = Request::builder()
        .method("POST")
        .uri("/ngsi-ld/v1/entities")
        .header("Content-Type", "application/json")
        .header("Content-Length", body.len());
    if let Some(t) = tenant {
        req = req.header("NGSILD-Tenant", t);
    }
    let (status, b) = send(st, req.body(Body::from(body)).expect("request")).await;
    assert_eq!(status, StatusCode::CREATED, "{b}");
}

fn json(body: &str) -> serde_json::Value {
    serde_json::from_str(body).unwrap_or_else(|e| panic!("json ({e}): {body}"))
}

/// 4.15: tenants share one datastore but never each other's data. Discovery
/// is a fold over entities, so every one of the four resources must report
/// only the requesting tenant's types and attributes.
#[tokio::test(flavor = "multi_thread")]
async fn discovery_is_tenant_scoped() {
    let st = AppState::new("test".into());
    create(
        &st,
        Some("ta"),
        r#"{"id":"urn:ngsi-ld:Iso:1","type":"Warehouse",
            "onlyInTa":{"type":"Property","value":1}}"#,
    )
    .await;
    create(
        &st,
        Some("tb"),
        r#"{"id":"urn:ngsi-ld:Iso:2","type":"Shop",
            "onlyInTb":{"type":"Property","value":1}}"#,
    )
    .await;

    let (status, body) = get(&st, "/ngsi-ld/v1/types", Some("tb")).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let list = json(&body)["typeList"]
        .as_array()
        .cloned()
        .expect("typeList");
    assert!(list.iter().any(|t| t == "Shop"), "{body}");
    assert!(
        !list.iter().any(|t| t == "Warehouse"),
        "another tenant's entity type leaked: {body}"
    );

    let (status, body) = get(&st, "/ngsi-ld/v1/types?details=true", Some("tb")).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert!(
        !body.contains("Warehouse") && !body.contains("onlyInTa"),
        "detailed type list leaked another tenant: {body}"
    );

    let (status, body) = get(&st, "/ngsi-ld/v1/types/Warehouse", Some("tb")).await;
    assert_eq!(
        status,
        StatusCode::NOT_FOUND,
        "another tenant's type must be unknown here: {body}"
    );

    let (status, body) = get(&st, "/ngsi-ld/v1/attributes", Some("tb")).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let list = json(&body)["attributeList"]
        .as_array()
        .cloned()
        .expect("attributeList");
    assert!(list.iter().any(|a| a == "onlyInTb"), "{body}");
    assert!(
        !list.iter().any(|a| a == "onlyInTa"),
        "another tenant's attribute leaked: {body}"
    );

    let (status, body) = get(&st, "/ngsi-ld/v1/attributes/onlyInTa", Some("tb")).await;
    assert_eq!(status, StatusCode::NOT_FOUND, "{body}");

    // the default tenant sees neither
    for uri in ["/ngsi-ld/v1/types", "/ngsi-ld/v1/attributes"] {
        let (status, body) = get(&st, uri, None).await;
        assert_eq!(status, StatusCode::OK, "{body}");
        assert!(
            !body.contains("Warehouse") && !body.contains("Shop"),
            "default tenant sees a named tenant's data at {uri}: {body}"
        );
    }
    let (status, body) = get(&st, "/ngsi-ld/v1/types/Shop", None).await;
    assert_eq!(status, StatusCode::NOT_FOUND, "{body}");
}

/// The Entity members of 4.5.1 and the system temporal attributes of 6.3.11
/// are not Attributes: they appear in no attribute list, in no
/// attributeNames, and GET /attributes/{attrId} does not know them.
#[tokio::test(flavor = "multi_thread")]
async fn system_members_are_not_discoverable_attributes() {
    let st = AppState::new("test".into());
    create(
        &st,
        None,
        r#"{"id":"urn:ngsi-ld:Sys:1","type":"Building","scope":"/a/b",
            "v":{"type":"Property","value":1}}"#,
    )
    .await;

    let (status, body) = get(&st, "/ngsi-ld/v1/attributes", None).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let list = json(&body)["attributeList"]
        .as_array()
        .cloned()
        .expect("attributeList");
    assert!(list.iter().any(|a| a == "v"), "{body}");
    for meta in [
        "id",
        "type",
        "scope",
        "createdAt",
        "modifiedAt",
        "deletedAt",
        "expiresAt",
    ] {
        assert!(
            !list.iter().any(|a| a == meta),
            "{meta} must not be listed as an attribute: {body}"
        );
        let (status, b) = get(&st, &format!("/ngsi-ld/v1/attributes/{meta}"), None).await;
        assert_eq!(status, StatusCode::NOT_FOUND, "{meta}: {b}");
    }

    let (status, body) = get(&st, "/ngsi-ld/v1/types?details=true", None).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let names = json(&body)
        .as_array()
        .and_then(|a| a.iter().find(|e| e["typeName"] == "Building").cloned())
        .map(|e| e["attributeNames"].clone())
        .expect("Building EntityType");
    let names = names.as_array().cloned().expect("attributeNames");
    assert!(names.iter().any(|n| n == "v"), "{body}");
    for meta in ["id", "type", "scope", "createdAt", "modifiedAt"] {
        assert!(
            !names.iter().any(|n| n == meta),
            "{meta} must not be an attributeName: {body}"
        );
    }
}

/// 6.3.20: a query parameter the resource does not define is rejected with
/// 400 InvalidRequest — /types and /attributes take details/local/count,
/// their by-id forms only local.
#[tokio::test(flavor = "multi_thread")]
async fn discovery_rejects_undefined_query_parameters() {
    let st = AppState::new("test".into());
    create(
        &st,
        None,
        r#"{"id":"urn:ngsi-ld:Par:1","type":"Building",
            "v":{"type":"Property","value":1}}"#,
    )
    .await;
    for uri in [
        "/ngsi-ld/v1/types?limit=5",
        "/ngsi-ld/v1/attributes?limit=5",
        "/ngsi-ld/v1/types/Building?details=true",
        "/ngsi-ld/v1/attributes/v?details=true",
    ] {
        let (status, body) = get(&st, uri, None).await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "{uri}: {body}");
        assert_eq!(
            json(&body)["type"],
            "https://uri.etsi.org/ngsi-ld/errors/InvalidRequest",
            "{uri}: {body}"
        );
    }
    // the defined ones are accepted (5.7.11 makes local a no-op here: the
    // fold already only sees the local datastore)
    for uri in [
        "/ngsi-ld/v1/types?local=true&count=true",
        "/ngsi-ld/v1/attributes?local=true&count=true",
        "/ngsi-ld/v1/types/Building?local=true",
        "/ngsi-ld/v1/attributes/v?local=true",
    ] {
        let (status, body) = get(&st, uri, None).await;
        assert_eq!(status, StatusCode::OK, "{uri}: {body}");
    }
}

/// Table 5.2.26-1 entityCount is per entity type, Table 5.2.28-1
/// attributeCount per attribute instance — and a type/attribute nobody uses
/// any more is not reported at all.
#[tokio::test(flavor = "multi_thread")]
async fn counts_track_the_entities_that_exist() {
    let st = AppState::new("test".into());
    create(
        &st,
        None,
        r#"{"id":"urn:ngsi-ld:Cnt:1","type":"Building",
            "v":{"type":"Property","value":1},
            "gone":{"type":"Property","value":1}}"#,
    )
    .await;
    create(
        &st,
        None,
        r#"{"id":"urn:ngsi-ld:Cnt:2","type":["Building","Sensor"],
            "v":[{"type":"Property","value":1,"datasetId":"urn:ngsi-ld:d:1"},
                 {"type":"Property","value":2,"datasetId":"urn:ngsi-ld:d:2"}]}"#,
    )
    .await;

    let (status, body) = get(&st, "/ngsi-ld/v1/types/Building", None).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(json(&body)["entityCount"], 2, "{body}");
    let (status, body) = get(&st, "/ngsi-ld/v1/types/Sensor", None).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(json(&body)["entityCount"], 1, "{body}");

    let (status, body) = get(&st, "/ngsi-ld/v1/attributes/v", None).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(
        json(&body)["attributeCount"],
        3,
        "one instance on Cnt:1 and two datasetId instances on Cnt:2: {body}"
    );

    // delete the only entity carrying "gone" — the attribute stops existing
    let (status, body) = send(
        &st,
        Request::builder()
            .method("DELETE")
            .uri("/ngsi-ld/v1/entities/urn:ngsi-ld:Cnt:1")
            .body(Body::empty())
            .expect("request"),
    )
    .await;
    assert_eq!(status, StatusCode::NO_CONTENT, "{body}");
    let (status, body) = get(&st, "/ngsi-ld/v1/attributes/gone", None).await;
    assert_eq!(status, StatusCode::NOT_FOUND, "{body}");
    let (status, body) = get(&st, "/ngsi-ld/v1/attributes", None).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert!(
        !body.contains("gone"),
        "attribute of a deleted entity still listed: {body}"
    );
}

/// 5.7.7/5.7.10 take the type/attribute name in the path and expand it
/// against the @context — the fully qualified name addresses the same
/// resource, an unknown one is a 404 and never a 500.
#[tokio::test(flavor = "multi_thread")]
async fn by_id_forms_expand_the_path_name() {
    let st = AppState::new("test".into());
    create(
        &st,
        None,
        r#"{"id":"urn:ngsi-ld:Exp:1","type":"Building",
            "v":{"type":"Property","value":1}}"#,
    )
    .await;
    for uri in [
        "/ngsi-ld/v1/types/https%3A%2F%2Furi.etsi.org%2Fngsi-ld%2Fdefault-context%2FBuilding",
        "/ngsi-ld/v1/types/Building",
    ] {
        let (status, body) = get(&st, uri, None).await;
        assert_eq!(status, StatusCode::OK, "{uri}: {body}");
        assert_eq!(
            json(&body)["id"],
            "https://uri.etsi.org/ngsi-ld/default-context/Building",
            "{uri}: {body}"
        );
    }
    // names that are not types/attributes of any entity: 404, no leak of
    // internal detail in the body
    for uri in [
        "/ngsi-ld/v1/types/Building%20",
        "/ngsi-ld/v1/types/../../etc",
        "/ngsi-ld/v1/attributes/%00",
        "/ngsi-ld/v1/attributes/v.value",
    ] {
        let (status, body) = get(&st, uri, None).await;
        assert!(
            status == StatusCode::NOT_FOUND || status == StatusCode::BAD_REQUEST,
            "{uri}: {status} {body}"
        );
        assert!(
            !body.contains("panicked") && !body.contains("src/"),
            "{uri}: internal detail in the response: {body}"
        );
    }
}

/// Table 5.2.24-1 EntityTypeList and Table 5.2.25-1 EntityType: the exact
/// member sets of GET /types with and without details — an EntityTypeList
/// carries no counts, an EntityType none of the 5.2.26 EntityTypeInfo
/// members.
#[tokio::test(flavor = "multi_thread")]
async fn entity_type_list_carries_exactly_the_5_2_24_members() {
    let st = AppState::new("test".into());
    create(
        &st,
        None,
        r#"{"id":"urn:ngsi-ld:Shp:1","type":"Building",
            "v":{"type":"Property","value":1},
            "r":{"type":"Relationship","object":"urn:ngsi-ld:Shp:2"}}"#,
    )
    .await;

    let (status, body) = get(&st, "/ngsi-ld/v1/types", None).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let doc = json(&body);
    let obj = doc.as_object().expect("EntityTypeList object");
    assert_eq!(obj["type"], "EntityTypeList", "{body}");
    assert!(
        obj["id"]
            .as_str()
            .is_some_and(|id| id.starts_with("urn:ngsi-ld:EntityTypeList:")),
        "id must be a valid URI: {body}"
    );
    assert_eq!(obj["typeList"], serde_json::json!(["Building"]), "{body}");
    let mut members: Vec<&str> = obj.keys().map(String::as_str).collect();
    members.sort_unstable();
    assert_eq!(members, ["id", "type", "typeList"], "{body}");

    let (status, body) = get(&st, "/ngsi-ld/v1/types?details=true", None).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let arr = json(&body).as_array().cloned().expect("EntityType array");
    assert_eq!(arr.len(), 1, "{body}");
    let mut members: Vec<&str> = arr[0]
        .as_object()
        .expect("EntityType object")
        .keys()
        .map(String::as_str)
        .collect();
    members.sort_unstable();
    assert_eq!(
        members,
        ["attributeNames", "id", "type", "typeName"],
        "an EntityType is not an EntityTypeInfo: {body}"
    );
    assert_eq!(arr[0]["type"], "EntityType", "{body}");
    assert_eq!(
        arr[0]["id"], "https://uri.etsi.org/ngsi-ld/default-context/Building",
        "{body}"
    );
    assert_eq!(arr[0]["typeName"], "Building", "{body}");
    assert_eq!(
        arr[0]["attributeNames"],
        serde_json::json!(["r", "v"]),
        "{body}"
    );
}

/// Table 5.2.26-1 EntityTypeInfo: id/type/typeName/entityCount plus
/// attributeDetails, whose elements are 5.2.28 Attributes restricted to
/// id/type/attributeName/attributeTypes — attributeCount and typeNames
/// belong to GET /attributes/{attrId}, not here (5.7.7).
#[tokio::test(flavor = "multi_thread")]
async fn entity_type_info_carries_exactly_the_5_2_26_members() {
    let st = AppState::new("test".into());
    create(
        &st,
        None,
        r#"{"id":"urn:ngsi-ld:Inf:1","type":"Building",
            "v":{"type":"Property","value":1},
            "g":{"type":"GeoProperty",
                 "value":{"type":"Point","coordinates":[0,0]}}}"#,
    )
    .await;

    let (status, body) = get(&st, "/ngsi-ld/v1/types/Building", None).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let doc = json(&body);
    let mut members: Vec<&str> = doc
        .as_object()
        .expect("EntityTypeInfo object")
        .keys()
        .map(String::as_str)
        .collect();
    members.sort_unstable();
    assert_eq!(
        members,
        ["attributeDetails", "entityCount", "id", "type", "typeName"],
        "{body}"
    );
    assert_eq!(doc["type"], "EntityTypeInfo", "{body}");
    assert_eq!(
        doc["id"], "https://uri.etsi.org/ngsi-ld/default-context/Building",
        "{body}"
    );
    assert_eq!(doc["typeName"], "Building", "{body}");
    assert_eq!(doc["entityCount"], 1, "{body}");

    let details = doc["attributeDetails"]
        .as_array()
        .cloned()
        .expect("attributeDetails");
    assert_eq!(details.len(), 2, "{body}");
    for d in &details {
        let mut members: Vec<&str> = d
            .as_object()
            .expect("Attribute object")
            .keys()
            .map(String::as_str)
            .collect();
        members.sort_unstable();
        assert_eq!(
            members,
            ["attributeName", "attributeTypes", "id", "type"],
            "5.7.7 restricts attributeDetails to these members: {body}"
        );
        assert_eq!(d["type"], "Attribute", "{body}");
    }
    let g = details
        .iter()
        .find(|d| d["attributeName"] == "g")
        .expect("g");
    assert_eq!(g["attributeTypes"], serde_json::json!(["GeoProperty"]));
}

/// Table 5.2.27-1 AttributeList and Table 5.2.28-1 Attribute: the member
/// sets of GET /attributes with and without details. The detailed form
/// reports typeNames, never the by-id form's attributeCount/attributeTypes.
#[tokio::test(flavor = "multi_thread")]
async fn attribute_list_carries_exactly_the_5_2_27_members() {
    let st = AppState::new("test".into());
    create(
        &st,
        None,
        r#"{"id":"urn:ngsi-ld:Att:1","type":"Building",
            "v":{"type":"Property","value":1}}"#,
    )
    .await;
    create(
        &st,
        None,
        r#"{"id":"urn:ngsi-ld:Att:2","type":"Sensor",
            "v":{"type":"Property","value":2}}"#,
    )
    .await;

    let (status, body) = get(&st, "/ngsi-ld/v1/attributes", None).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let doc = json(&body);
    let obj = doc.as_object().expect("AttributeList object");
    assert_eq!(obj["type"], "AttributeList", "{body}");
    assert!(
        obj["id"]
            .as_str()
            .is_some_and(|id| id.starts_with("urn:ngsi-ld:AttributeList:")),
        "id must be a valid URI: {body}"
    );
    assert_eq!(obj["attributeList"], serde_json::json!(["v"]), "{body}");
    let mut members: Vec<&str> = obj.keys().map(String::as_str).collect();
    members.sort_unstable();
    assert_eq!(members, ["attributeList", "id", "type"], "{body}");

    let (status, body) = get(&st, "/ngsi-ld/v1/attributes?details=true", None).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let arr = json(&body).as_array().cloned().expect("Attribute array");
    assert_eq!(arr.len(), 1, "{body}");
    let mut members: Vec<&str> = arr[0]
        .as_object()
        .expect("Attribute object")
        .keys()
        .map(String::as_str)
        .collect();
    members.sort_unstable();
    assert_eq!(
        members,
        ["attributeName", "id", "type", "typeNames"],
        "{body}"
    );
    assert_eq!(arr[0]["type"], "Attribute", "{body}");
    assert_eq!(
        arr[0]["id"], "https://uri.etsi.org/ngsi-ld/default-context/v",
        "{body}"
    );
    assert_eq!(
        arr[0]["typeNames"],
        serde_json::json!(["Building", "Sensor"]),
        "both entity types carrying the attribute: {body}"
    );
}

/// Table 5.2.28-1 Attribute at GET /attributes/{attrId} (5.7.10): the full
/// member set — attributeCount counts instances, attributeTypes the NGSI-LD
/// attribute types, typeNames the entity types carrying it.
#[tokio::test(flavor = "multi_thread")]
async fn attribute_info_carries_exactly_the_5_2_28_members() {
    let st = AppState::new("test".into());
    create(
        &st,
        None,
        r#"{"id":"urn:ngsi-ld:Ai:1","type":"Building",
            "v":[{"type":"Property","value":1,"datasetId":"urn:ngsi-ld:d:1"},
                 {"type":"GeoProperty",
                  "value":{"type":"Point","coordinates":[0,0]},
                  "datasetId":"urn:ngsi-ld:d:2"}]}"#,
    )
    .await;

    let (status, body) = get(&st, "/ngsi-ld/v1/attributes/v", None).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let doc = json(&body);
    let mut members: Vec<&str> = doc
        .as_object()
        .expect("Attribute object")
        .keys()
        .map(String::as_str)
        .collect();
    members.sort_unstable();
    assert_eq!(
        members,
        [
            "attributeCount",
            "attributeName",
            "attributeTypes",
            "id",
            "type",
            "typeNames"
        ],
        "{body}"
    );
    assert_eq!(doc["type"], "Attribute", "{body}");
    assert_eq!(
        doc["id"], "https://uri.etsi.org/ngsi-ld/default-context/v",
        "{body}"
    );
    assert_eq!(doc["attributeName"], "v", "{body}");
    assert_eq!(doc["attributeCount"], 2, "{body}");
    assert_eq!(
        doc["attributeTypes"],
        serde_json::json!(["GeoProperty", "Property"]),
        "{body}"
    );
    assert_eq!(doc["typeNames"], serde_json::json!(["Building"]), "{body}");
}

/// A tenant holding no entities has no types and no attributes: the list
/// resources answer 200 with an empty list (5.7.5/5.7.8 fold nothing), the
/// by-id resources 404 ResourceNotFound (Table 6.3.2-1).
#[tokio::test(flavor = "multi_thread")]
async fn empty_tenant_lists_are_empty_and_by_id_is_not_found() {
    let st = AppState::new("test".into());

    let (status, body) = get(&st, "/ngsi-ld/v1/types", None).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(json(&body)["typeList"], serde_json::json!([]), "{body}");
    let (status, body) = get(&st, "/ngsi-ld/v1/types?details=true", None).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(json(&body), serde_json::json!([]), "{body}");

    let (status, body) = get(&st, "/ngsi-ld/v1/attributes", None).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(
        json(&body)["attributeList"],
        serde_json::json!([]),
        "{body}"
    );
    let (status, body) = get(&st, "/ngsi-ld/v1/attributes?details=true", None).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(json(&body), serde_json::json!([]), "{body}");

    for uri in ["/ngsi-ld/v1/types/Building", "/ngsi-ld/v1/attributes/v"] {
        let (status, body) = get(&st, uri, None).await;
        assert_eq!(status, StatusCode::NOT_FOUND, "{uri}: {body}");
        assert_eq!(
            json(&body)["type"],
            "https://uri.etsi.org/ngsi-ld/errors/ResourceNotFound",
            "{uri}: {body}"
        );
    }
}

/// The scan ceiling is what bounds a discovery fold, and 6.3.17's
/// NGSILD-Warning is how an incomplete answer says so. Under the ceiling
/// every one of the four resources answers complete, so none of them may
/// carry the header.
#[tokio::test(flavor = "multi_thread")]
async fn a_complete_discovery_answer_carries_no_warning() {
    let st = AppState::new("test".into());
    create(
        &st,
        None,
        r#"{"id":"urn:ngsi-ld:Wrn:1","type":"Building",
            "v":{"type":"Property","value":1}}"#,
    )
    .await;
    for uri in [
        "/ngsi-ld/v1/types",
        "/ngsi-ld/v1/types?details=true",
        "/ngsi-ld/v1/types/Building",
        "/ngsi-ld/v1/attributes",
        "/ngsi-ld/v1/attributes?details=true",
        "/ngsi-ld/v1/attributes/v",
        "/ngsi-ld/v1/types/Absent",
        "/ngsi-ld/v1/attributes/absent",
    ] {
        let (_, headers, body) = send_h(
            &st,
            Request::builder()
                .uri(uri)
                .body(Body::empty())
                .expect("request"),
        )
        .await;
        assert!(
            headers.get("NGSILD-Warning").is_none(),
            "{uri} answered complete but warned: {body}"
        );
    }
}

/// 6.3.4 content negotiation applies to discovery like every other GET: an
/// Accept the resource cannot serve is 406, and application/ld+json puts the
/// @context in the body.
#[tokio::test(flavor = "multi_thread")]
async fn discovery_negotiates_content() {
    let st = AppState::new("test".into());
    create(
        &st,
        None,
        r#"{"id":"urn:ngsi-ld:Neg:1","type":"Building",
            "v":{"type":"Property","value":1}}"#,
    )
    .await;
    for uri in [
        "/ngsi-ld/v1/types",
        "/ngsi-ld/v1/types/Building",
        "/ngsi-ld/v1/attributes",
        "/ngsi-ld/v1/attributes/v",
    ] {
        let (status, body) = send(
            &st,
            Request::builder()
                .uri(uri)
                .header("Accept", "application/xml")
                .body(Body::empty())
                .expect("request"),
        )
        .await;
        assert_eq!(status, StatusCode::NOT_ACCEPTABLE, "{uri}: {body}");

        let (status, headers, body) = send_h(
            &st,
            Request::builder()
                .uri(uri)
                .header("Accept", "application/ld+json")
                .body(Body::empty())
                .expect("request"),
        )
        .await;
        assert_eq!(status, StatusCode::OK, "{uri}: {body}");
        assert_eq!(
            headers
                .get("Content-Type")
                .and_then(|v| v.to_str().ok())
                .unwrap_or_default(),
            "application/ld+json",
            "{uri}: {body}"
        );
        assert!(
            json(&body)["@context"] != serde_json::Value::Null,
            "{uri}: {body}"
        );
    }
}
