//! 4.5.10+ discovery representations (/types, /attributes).

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

/// 4.5.10: the entity type list is a JSON-LD object with id (a URI), the
/// fixed type "EntityTypeList" and typeList — and nothing else beyond an
/// optional @context.
#[tokio::test(flavor = "multi_thread")]
async fn entity_type_list_shape() {
    let st = AppState::new("test".into());
    let create = r#"{"id":"urn:ngsi-ld:Disc:1","type":"Building",
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

    let (status, body) = send(
        &st,
        Request::builder()
            .uri("/ngsi-ld/v1/types")
            .body(Body::empty())
            .expect("request"),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let doc: serde_json::Value = serde_json::from_str(&body).expect("json");
    assert!(
        doc["id"].as_str().is_some_and(|s| s.starts_with("urn:")),
        "id must be a URI: {body}"
    );
    assert_eq!(doc["type"], "EntityTypeList");
    let list = doc["typeList"].as_array().expect("typeList array");
    assert!(list.iter().any(|t| t == "Building"), "{body}");
    let extra: Vec<&String> = doc
        .as_object()
        .expect("object")
        .keys()
        .filter(|k| !["id", "type", "typeList", "@context"].contains(&k.as_str()))
        .collect();
    assert!(extra.is_empty(), "unexpected members: {extra:?}");
}

/// 4.5.11: details=true returns an array of EntityType objects — id is the
/// type URI, fixed type "EntityType", typeName the short name, plus
/// attributeNames — and nothing else beyond an optional @context.
#[tokio::test(flavor = "multi_thread")]
async fn detailed_entity_type_list_shape() {
    let st = AppState::new("test".into());
    let create = r#"{"id":"urn:ngsi-ld:Disc:2","type":"Building",
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

    let (status, body) = send(
        &st,
        Request::builder()
            .uri("/ngsi-ld/v1/types?details=true")
            .body(Body::empty())
            .expect("request"),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let doc: serde_json::Value = serde_json::from_str(&body).expect("json");
    let arr = doc.as_array().expect("array of EntityType objects");
    let et = arr
        .iter()
        .find(|e| e["typeName"] == "Building")
        .unwrap_or_else(|| panic!("no Building EntityType in {body}"));
    assert_eq!(et["type"], "EntityType");
    assert_eq!(
        et["id"], "https://uri.etsi.org/ngsi-ld/default-context/Building",
        "id must be the type URI"
    );
    let names = et["attributeNames"].as_array().expect("attributeNames");
    assert!(names.iter().any(|n| n == "v"), "{body}");
    let extra: Vec<&String> = et
        .as_object()
        .expect("object")
        .keys()
        .filter(|k| !["id", "type", "typeName", "attributeNames", "@context"].contains(&k.as_str()))
        .collect();
    assert!(extra.is_empty(), "unexpected members: {extra:?}");
}

/// 4.5.12: entity type information — id is the type URI, fixed type
/// "EntityTypeInfo", typeName the short name; entityCount/attributeDetails
/// are the 5.2.26 detail members, nothing else appears.
#[tokio::test(flavor = "multi_thread")]
async fn entity_type_info_shape() {
    let st = AppState::new("test".into());
    let create = r#"{"id":"urn:ngsi-ld:Disc:3","type":"Building",
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

    let (status, body) = send(
        &st,
        Request::builder()
            .uri("/ngsi-ld/v1/types/Building")
            .body(Body::empty())
            .expect("request"),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let doc: serde_json::Value = serde_json::from_str(&body).expect("json");
    assert_eq!(doc["type"], "EntityTypeInfo");
    assert_eq!(
        doc["id"],
        "https://uri.etsi.org/ngsi-ld/default-context/Building"
    );
    assert_eq!(doc["typeName"], "Building");
    assert_eq!(doc["entityCount"], 1);
    let extra: Vec<&String> = doc
        .as_object()
        .expect("object")
        .keys()
        .filter(|k| {
            ![
                "id",
                "type",
                "typeName",
                "entityCount",
                "attributeDetails",
                "@context",
            ]
            .contains(&k.as_str())
        })
        .collect();
    assert!(extra.is_empty(), "unexpected members: {extra:?}");

    // unknown type → 404 ResourceNotFound
    let (status, body) = send(
        &st,
        Request::builder()
            .uri("/ngsi-ld/v1/types/Nonexistent")
            .body(Body::empty())
            .expect("request"),
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND, "{body}");
}

/// 4.5.13/4.5.14/4.5.15: attribute list, detailed attribute list and
/// attribute information representations — fixed types, URI ids, short
/// names, and no members beyond the clause lists.
#[tokio::test(flavor = "multi_thread")]
async fn attribute_representations_shapes() {
    let st = AppState::new("test".into());
    let create = r#"{"id":"urn:ngsi-ld:Disc:4","type":"Building",
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

    // 4.5.13 attribute list
    let (status, body) = send(
        &st,
        Request::builder()
            .uri("/ngsi-ld/v1/attributes")
            .body(Body::empty())
            .expect("request"),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let doc: serde_json::Value = serde_json::from_str(&body).expect("json");
    assert_eq!(doc["type"], "AttributeList");
    assert!(doc["id"].as_str().is_some_and(|s| s.starts_with("urn:")));
    assert!(doc["attributeList"]
        .as_array()
        .is_some_and(|a| a.iter().any(|n| n == "v")));
    let extra: Vec<&String> = doc
        .as_object()
        .expect("object")
        .keys()
        .filter(|k| !["id", "type", "attributeList", "@context"].contains(&k.as_str()))
        .collect();
    assert!(extra.is_empty(), "4.5.13 extra members: {extra:?}");

    // 4.5.14 detailed attribute list
    let (status, body) = send(
        &st,
        Request::builder()
            .uri("/ngsi-ld/v1/attributes?details=true")
            .body(Body::empty())
            .expect("request"),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let doc: serde_json::Value = serde_json::from_str(&body).expect("json");
    let a = doc
        .as_array()
        .expect("array")
        .iter()
        .find(|a| a["attributeName"] == "v")
        .unwrap_or_else(|| panic!("no v attribute in {body}"));
    assert_eq!(a["type"], "Attribute");
    assert_eq!(a["id"], "https://uri.etsi.org/ngsi-ld/default-context/v");
    assert!(a["typeNames"]
        .as_array()
        .is_some_and(|t| t.iter().any(|n| n == "Building")));
    let extra: Vec<&String> = a
        .as_object()
        .expect("object")
        .keys()
        .filter(|k| !["id", "type", "attributeName", "typeNames", "@context"].contains(&k.as_str()))
        .collect();
    assert!(extra.is_empty(), "4.5.14 extra members: {extra:?}");

    // 4.5.15 attribute information
    let (status, body) = send(
        &st,
        Request::builder()
            .uri("/ngsi-ld/v1/attributes/v")
            .body(Body::empty())
            .expect("request"),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let doc: serde_json::Value = serde_json::from_str(&body).expect("json");
    assert_eq!(doc["type"], "Attribute");
    assert_eq!(doc["attributeName"], "v");
    assert_eq!(doc["attributeCount"], 1);
    assert!(doc["attributeTypes"]
        .as_array()
        .is_some_and(|t| t.iter().any(|n| n == "Property")));
    let extra: Vec<&String> = doc
        .as_object()
        .expect("object")
        .keys()
        .filter(|k| {
            ![
                "id",
                "type",
                "attributeName",
                "attributeCount",
                "attributeTypes",
                "typeNames",
                "@context",
            ]
            .contains(&k.as_str())
        })
        .collect();
    assert!(extra.is_empty(), "4.5.15 extra members: {extra:?}");

    // unknown attribute → 404
    let (status, body) = send(
        &st,
        Request::builder()
            .uri("/ngsi-ld/v1/attributes/nonexistent")
            .body(Body::empty())
            .expect("request"),
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND, "{body}");
}

/// 4.5.23.2/4.5.22.2: inline linked retrieval joins ListRelationship targets
/// under the output-only "entityList" (always an array); Relationship targets
/// under "entity". 4.5.23.3: the flattened form appends both kinds of Linked
/// Entities to the response array.
#[tokio::test(flavor = "multi_thread")]
async fn linked_retrieval_joins_list_relationships() {
    let st = AppState::new("test".into());
    for (id, body) in [
        (
            "urn:ngsi-ld:Road:1",
            r#"{"id":"urn:ngsi-ld:Road:1","type":"Road","n":{"type":"Property","value":1}}"#,
        ),
        (
            "urn:ngsi-ld:Road:2",
            r#"{"id":"urn:ngsi-ld:Road:2","type":"Road","n":{"type":"Property","value":2}}"#,
        ),
        (
            "urn:ngsi-ld:V:1",
            r#"{"id":"urn:ngsi-ld:V:1","type":"Vehicle",
            "route":{"type":"ListRelationship","objectList":["urn:ngsi-ld:Road:1","urn:ngsi-ld:Road:2"]}}"#,
        ),
    ] {
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
        assert_eq!(status, StatusCode::CREATED, "{id}: {b}");
    }

    let (status, body) = send(
        &st,
        Request::builder()
            .uri("/ngsi-ld/v1/entities/urn:ngsi-ld:V:1?join=inline&joinLevel=1")
            .body(Body::empty())
            .expect("request"),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let doc: serde_json::Value = serde_json::from_str(&body).expect("json");
    let el = doc["route"]["entityList"]
        .as_array()
        .expect("entityList array");
    assert_eq!(el.len(), 2, "{body}");
    assert!(el.iter().any(|e| e["id"] == "urn:ngsi-ld:Road:1"));
    assert!(
        doc["route"].get("entity").is_none(),
        "entity is the Relationship member, never on a ListRelationship"
    );

    let (status, body) = send(
        &st,
        Request::builder()
            .uri("/ngsi-ld/v1/entities/urn:ngsi-ld:V:1?join=flat&joinLevel=1")
            .body(Body::empty())
            .expect("request"),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let doc: serde_json::Value = serde_json::from_str(&body).expect("json");
    let arr = doc.as_array().expect("flattened array");
    assert_eq!(arr.len(), 3, "linking + 2 linked: {body}");
}
