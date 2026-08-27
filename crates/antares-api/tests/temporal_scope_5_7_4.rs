// SPDX-License-Identifier: EUPL-1.2
//! 5.7.4.4 S4 (and S7 post-aggregation): the Scope query on the temporal
//! query surface, matched per 4.19 with 4.18 validity semantics — "a given
//! Scope is considered valid from the time it has been set until the time
//! it has been explicitly removed by an update or delete operation"
//! (worked example: annex C.5.16).

use antares_api::AppState;
use axum::body::Body;
use axum::http::{Request, StatusCode};
use serde_json::{json, Value};
use tower::ServiceExt;

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

async fn post(st: &AppState, uri: &str, body: Value) -> (StatusCode, Value) {
    let body = body.to_string();
    send(
        st,
        Request::builder()
            .method("POST")
            .uri(uri)
            .header("Content-Type", "application/json")
            .header("Content-Length", body.len())
            .body(Body::from(body))
            .expect("request"),
    )
    .await
}

fn ids(body: &Value) -> Vec<&str> {
    body.as_array()
        .map(|a| a.iter().filter_map(|d| d["id"].as_str()).collect())
        .unwrap_or_default()
}

const WINDOW: &str = "timerel=between&timeAt=2026-03-01T12:00:00Z&endTimeAt=2026-03-01T13:00:00Z";

/// Seeds three temporal evolutions: scope /A/B set before the window,
/// scope /X set before the window, and one without any scope.
async fn seed_scoped(st: &AppState) {
    for (id, scope) in [
        ("urn:ngsi-ld:V:sc-ab", Some("/A/B")),
        ("urn:ngsi-ld:V:sc-x", Some("/X")),
        ("urn:ngsi-ld:V:sc-none", None),
    ] {
        let mut e = json!({"id": id, "type": "Vehicle",
            "speed": [{"type": "Property", "value": 30, "observedAt": "2026-03-01T12:05:00Z"}]});
        if let Some(s) = scope {
            e["scope"] = json!([{"type": "Property", "value": s,
                "observedAt": "2026-03-01T10:00:00Z"}]);
        }
        let (status, b) = post(st, "/ngsi-ld/v1/temporal/entities", e).await;
        assert_eq!(status, StatusCode::CREATED, "{b}");
    }
}

/// 5.7.4.4 S4: exact scope, subtree `/#`-suffix, any-scope `/#`, and
/// `|`-alternatives select entities; non-matching scopeQ excludes them.
#[tokio::test(flavor = "multi_thread")]
async fn clause_5_7_4_4_s4_scopeq_selects_entities() {
    let st = AppState::new("me".into());
    seed_scoped(&st).await;

    for (sq, expect) in [
        ("/A/B", vec!["urn:ngsi-ld:V:sc-ab"]),
        ("/A/%23", vec!["urn:ngsi-ld:V:sc-ab"]),
        ("/%23", vec!["urn:ngsi-ld:V:sc-ab", "urn:ngsi-ld:V:sc-x"]),
        (
            "/X%7C/A/B",
            vec!["urn:ngsi-ld:V:sc-ab", "urn:ngsi-ld:V:sc-x"],
        ),
        ("/Y", vec![]),
    ] {
        let (status, body) = get(
            &st,
            &format!("/ngsi-ld/v1/temporal/entities?type=Vehicle&scopeQ={sq}&{WINDOW}"),
        )
        .await;
        assert_eq!(status, StatusCode::OK, "{body}");
        let mut got = ids(&body);
        got.sort_unstable();
        assert_eq!(got, expect, "scopeQ={sq}: {body}");
        // the unscoped entity must NEVER appear under any scopeQ
        assert!(
            !got.contains(&"urn:ngsi-ld:V:sc-none"),
            "scopeQ={sq}: {body}"
        );
    }
}

/// C.5.16 (B9211 half): a scope set BEFORE the window is valid during it —
/// the entity matches, and the presented scope carries the value even
/// though its set-time predates the window.
#[tokio::test(flavor = "multi_thread")]
async fn clause_5_7_4_4_c516_scope_set_before_window_carries_in() {
    let st = AppState::new("me".into());
    seed_scoped(&st).await;

    let (status, body) = get(
        &st,
        &format!("/ngsi-ld/v1/temporal/entities?type=Vehicle&scopeQ=/A/B&{WINDOW}"),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(ids(&body), vec!["urn:ngsi-ld:V:sc-ab"], "{body}");
    let text = body.to_string();
    assert!(
        text.contains("/A/B"),
        "valid scope must be presented: {body}"
    );
    assert!(!text.contains("/X"), "{body}");
}

/// C.5.16 (A8311 half): a scope set WITHIN the window qualifies the entity,
/// but only Attribute instances observed after the scope became valid are
/// included.
#[tokio::test(flavor = "multi_thread")]
async fn clause_5_7_4_4_c516_scope_set_mid_window_bounds_instances() {
    let st = AppState::new("me".into());
    let (status, b) = post(
        &st,
        "/ngsi-ld/v1/temporal/entities",
        json!({"id": "urn:ngsi-ld:V:sc-mid", "type": "Vehicle",
            "scope": [{"type": "Property", "value": "/M", "observedAt": "2026-03-01T12:10:00Z"}],
            "speed": [
                {"type": "Property", "value": "preset", "observedAt": "2026-03-01T12:05:00Z"},
                {"type": "Property", "value": "postset", "observedAt": "2026-03-01T12:15:00Z"}]}),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{b}");

    let (status, body) = get(
        &st,
        &format!("/ngsi-ld/v1/temporal/entities?type=Vehicle&scopeQ=/M&{WINDOW}"),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(ids(&body), vec!["urn:ngsi-ld:V:sc-mid"], "{body}");
    let text = body.to_string();
    assert!(
        text.contains("postset"),
        "in-validity instance kept: {body}"
    );
    assert!(
        !text.contains("preset"),
        "instance observed before the scope was set must be excluded: {body}"
    );
}

/// 4.18: a scope replaced by an update stops being valid — instances after
/// the replacement are excluded for the OLD scope's query, and the NEW
/// scope's query only sees instances from its set-time on.
#[tokio::test(flavor = "multi_thread")]
async fn clause_5_7_4_4_c516_scope_replaced_mid_window() {
    let st = AppState::new("me".into());
    let (status, b) = post(
        &st,
        "/ngsi-ld/v1/temporal/entities",
        json!({"id": "urn:ngsi-ld:V:sc-rep", "type": "Vehicle",
            "scope": [
                {"type": "Property", "value": "/R", "observedAt": "2026-03-01T10:00:00Z"},
                {"type": "Property", "value": "/S", "observedAt": "2026-03-01T12:30:00Z"}],
            "speed": [
                {"type": "Property", "value": "oldscope", "observedAt": "2026-03-01T12:15:00Z"},
                {"type": "Property", "value": "newscope", "observedAt": "2026-03-01T12:45:00Z"}]}),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{b}");

    // the old scope: valid [10:00, 12:30) — only the 12:15 instance
    let (status, body) = get(
        &st,
        &format!("/ngsi-ld/v1/temporal/entities?type=Vehicle&scopeQ=/R&{WINDOW}"),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(ids(&body), vec!["urn:ngsi-ld:V:sc-rep"], "{body}");
    let text = body.to_string();
    assert!(text.contains("oldscope"), "{body}");
    assert!(
        !text.contains("newscope"),
        "instance after scope removal excluded: {body}"
    );

    // the new scope: valid [12:30, ∞) — only the 12:45 instance
    let (status, body) = get(
        &st,
        &format!("/ngsi-ld/v1/temporal/entities?type=Vehicle&scopeQ=/S&{WINDOW}"),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(ids(&body), vec!["urn:ngsi-ld:V:sc-rep"], "{body}");
    let text = body.to_string();
    assert!(text.contains("newscope"), "{body}");
    assert!(
        !text.contains("oldscope"),
        "instance before the new scope excluded: {body}"
    );
}

/// 5.7.4.4 S4: a scope whose validity never intersects the window — set
/// only after the window's end — must not qualify the entity.
#[tokio::test(flavor = "multi_thread")]
async fn clause_5_7_4_4_scope_valid_only_after_window_no_match() {
    let st = AppState::new("me".into());
    let (status, b) = post(
        &st,
        "/ngsi-ld/v1/temporal/entities",
        json!({"id": "urn:ngsi-ld:V:sc-late", "type": "Vehicle",
            "scope": [{"type": "Property", "value": "/Z", "observedAt": "2026-03-01T14:00:00Z"}],
            "speed": [{"type": "Property", "value": 30, "observedAt": "2026-03-01T12:05:00Z"}]}),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{b}");

    let (status, body) = get(
        &st,
        &format!("/ngsi-ld/v1/temporal/entities?type=Vehicle&scopeQ=/Z&{WINDOW}"),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body.as_array().map(Vec::len), Some(0), "{body}");
}

/// 4.5.6 + 5.7.4.4: a Core-API-created entity's scope is auto-recorded as a
/// temporal scope instance; scopeQ filters the auto-recorded evolution too.
#[tokio::test(flavor = "multi_thread")]
async fn clause_5_7_4_4_scopeq_on_auto_recorded_evolution() {
    let mut st = AppState::new("me".into());
    antares_api::notify::wire(&mut st); // temporal auto-recording

    let (status, b) = post(
        &st,
        "/ngsi-ld/v1/entities",
        json!({"id": "urn:ngsi-ld:V:sc-core", "type": "Vehicle", "scope": "/A/B",
            "speed": {"type": "Property", "value": 30}}),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{b}");

    let win = "timerel=after&timeAt=2000-01-01T00:00:00Z&timeproperty=createdAt";
    let (status, body) = get(
        &st,
        &format!("/ngsi-ld/v1/temporal/entities?type=Vehicle&scopeQ=/A/%23&{win}"),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(ids(&body), vec!["urn:ngsi-ld:V:sc-core"], "{body}");

    let (status, body) = get(
        &st,
        &format!("/ngsi-ld/v1/temporal/entities?type=Vehicle&scopeQ=/X&{win}"),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(body.as_array().map(Vec::len), Some(0), "{body}");
}

/// 5.14.5.4 + p.257: the temporal EntityMap is "created based on S4" — a
/// scopeQ on the map-creating query must keep non-matching entities OUT of
/// the map.
#[tokio::test(flavor = "multi_thread")]
async fn clause_5_14_5_temporal_map_created_based_on_s4() {
    let st = AppState::new("me".into());
    seed_scoped(&st).await;

    let (status, body) = get(
        &st,
        &format!("/ngsi-ld/v1/temporal/entityMaps?type=Vehicle&scopeQ=/A/B&{WINDOW}"),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{body}");
    let map = body["entityMap"].as_object().expect("entityMap object");
    assert!(map.contains_key("urn:ngsi-ld:V:sc-ab"), "{body}");
    assert!(
        !map.contains_key("urn:ngsi-ld:V:sc-x") && !map.contains_key("urn:ngsi-ld:V:sc-none"),
        "S4-excluded entities must not enter the map: {body}"
    );
}

/// The SQL lastN cap and entity paging must be withheld when scopeQ is
/// present — they run before the scope-validity filter and would
/// under-return: lastN=1 must yield the last instance WHILE THE SCOPE WAS
/// VALID, not the last in-window instance.
#[tokio::test(flavor = "multi_thread")]
async fn clause_5_7_4_4_scopeq_disables_lastn_and_paging_pushdown() {
    let st = AppState::new("me".into());
    let (status, b) = post(
        &st,
        "/ngsi-ld/v1/temporal/entities",
        json!({"id": "urn:ngsi-ld:V:sc-ln", "type": "Vehicle",
            "scope": [
                {"type": "Property", "value": "/R", "observedAt": "2026-03-01T10:00:00Z"},
                {"type": "Property", "value": "/S", "observedAt": "2026-03-01T12:30:00Z"}],
            "speed": [
                {"type": "Property", "value": "lnold", "observedAt": "2026-03-01T12:15:00Z"},
                {"type": "Property", "value": "lnnew", "observedAt": "2026-03-01T12:45:00Z"}]}),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{b}");

    let (status, body) = get(
        &st,
        &format!("/ngsi-ld/v1/temporal/entities?type=Vehicle&scopeQ=/R&lastN=1&limit=5&{WINDOW}"),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(ids(&body), vec!["urn:ngsi-ld:V:sc-ln"], "{body}");
    let text = body.to_string();
    assert!(
        text.contains("lnold"),
        "lastN must apply AFTER the scope-validity filter: {body}"
    );
    assert!(!text.contains("lnnew"), "{body}");
}

/// Same invariant at the SQL layer: on a Pg-backed AppState the lastN RANK()
/// cap and entity paging must NOT be pushed down when scopeQ is present —
/// the store would cap to the last in-window instance BEFORE the 4.18
/// validity filter runs. Skips loudly without ANTARES_TEST_DATABASE_URL.
#[tokio::test(flavor = "multi_thread")]
async fn clause_5_7_4_4_scopeq_gates_the_sql_pushdown() {
    let Ok(url) = std::env::var("ANTARES_TEST_DATABASE_URL") else {
        eprintln!("SKIP: ANTARES_TEST_DATABASE_URL not set");
        return;
    };
    let pool = antares_sql::store::pg::connect(&url, 5)
        .await
        .expect("connect");
    let store = antares_sql::store::any::AnyStore::Pg(antares_sql::store::any::PgBackend::new(
        pool.clone(),
    ));
    let st = AppState::with_store(
        "me".into(),
        std::sync::Arc::new(store),
        antares_sql::StoreMode::Postgres,
    );
    // isolate this run's rows from earlier local runs
    let tenant = format!("scln{}", &uuid::Uuid::new_v4().simple().to_string()[..12]);

    let e = json!({"id": "urn:ngsi-ld:V:sc-pg", "type": "Vehicle",
        "scope": [
            {"type": "Property", "value": "/R", "observedAt": "2026-03-01T10:00:00Z"},
            {"type": "Property", "value": "/S", "observedAt": "2026-03-01T12:30:00Z"}],
        "speed": [
            {"type": "Property", "value": "pgold", "observedAt": "2026-03-01T12:15:00Z"},
            {"type": "Property", "value": "pgnew", "observedAt": "2026-03-01T12:45:00Z"}]})
    .to_string();
    let (status, b) = send(
        &st,
        Request::builder()
            .method("POST")
            .uri("/ngsi-ld/v1/temporal/entities")
            .header("Content-Type", "application/json")
            .header("NGSILD-Tenant", &tenant)
            .header("Content-Length", e.len())
            .body(Body::from(e))
            .expect("request"),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{b}");

    let (status, body) = send(
        &st,
        Request::builder()
            .method("GET")
            .uri(format!(
                "/ngsi-ld/v1/temporal/entities?type=Vehicle&scopeQ=/R&lastN=1&limit=5&{WINDOW}"
            ))
            .header("NGSILD-Tenant", &tenant)
            .body(Body::empty())
            .expect("request"),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(ids(&body), vec!["urn:ngsi-ld:V:sc-pg"], "{body}");
    let text = body.to_string();
    assert!(
        text.contains("pgold"),
        "lastN must apply AFTER the scope-validity filter on Pg too: {body}"
    );
    assert!(!text.contains("pgnew"), "{body}");
}
