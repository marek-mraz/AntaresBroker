// SPDX-License-Identifier: EUPL-1.2
//! 4.15 Language Filter on the temporal surface (Table 6.18.3.2-1 and
//! 6.19.3.1 `lang`): a LanguageProperty "shall be converted into a
//! Property" whose value is the languageMap entry for the chosen language,
//! "a non-reified subproperty lang shall be included"; no match → "a single
//! language shall be chosen, up to the implementation".

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

const ID: &str = "urn:ngsi-ld:V:lang-1";
const WINDOW: &str = "timerel=after&timeAt=2026-03-01T00:00:00Z";

async fn seed(st: &AppState) {
    let body = json!({"id": ID, "type": "Vehicle",
        "label": [
            {"type": "LanguageProperty", "languageMap": {"en": "hello", "fr": "bonjour"},
             "observedAt": "2026-03-01T12:00:00Z"},
            {"type": "LanguageProperty", "languageMap": {"en": "bye", "fr": "salut"},
             "observedAt": "2026-03-01T13:00:00Z"}
        ],
        "speed": [{"type": "Property", "value": 30, "observedAt": "2026-03-01T12:00:00Z"}]})
    .to_string();
    let (status, b) = send(
        st,
        Request::builder()
            .method("POST")
            .uri("/ngsi-ld/v1/temporal/entities")
            .header("Content-Type", "application/json")
            .header("Content-Length", body.len())
            .body(Body::from(body))
            .expect("request"),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{b}");
}

fn label_instances(entity: &Value) -> Vec<Value> {
    match &entity["label"] {
        Value::Array(a) => a.clone(),
        other => vec![other.clone()],
    }
}

fn assert_reduced(inst: &Value, lang: &str, value: &str) {
    assert_eq!(inst["type"], "Property", "{inst}");
    assert_eq!(inst["value"], value, "{inst}");
    assert_eq!(inst["lang"], lang, "{inst}");
    assert!(inst.get("languageMap").is_none(), "{inst}");
    assert!(inst.get("languageMaps").is_none(), "{inst}");
}

/// Query form, normalized: every instance of the LanguageProperty is a
/// Property in the requested language carrying `lang`.
#[tokio::test(flavor = "multi_thread")]
async fn clause_5_7_4_lang_reduces_every_temporal_instance() {
    let st = AppState::new("me".into());
    seed(&st).await;
    let (status, body) = get(
        &st,
        &format!("/ngsi-ld/v1/temporal/entities?type=Vehicle&lang=fr&{WINDOW}"),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let insts = label_instances(&body[0]);
    assert_eq!(insts.len(), 2, "{body}");
    let mut values: Vec<&str> = insts.iter().filter_map(|i| i["value"].as_str()).collect();
    values.sort_unstable();
    assert_eq!(values, ["bonjour", "salut"], "{body}");
    for i in &insts {
        assert_reduced(i, "fr", i["value"].as_str().unwrap_or(""));
    }
    // the plain Property is untouched
    assert!(body[0]["speed"].to_string().contains("30"), "{body}");
    assert!(!body[0]["speed"].to_string().contains("lang"), "{body}");
}

/// Retrieve form (6.19.3.1 `lang`): the same reduction on one evolution.
#[tokio::test(flavor = "multi_thread")]
async fn clause_5_7_3_lang_reduces_on_retrieve() {
    let st = AppState::new("me".into());
    seed(&st).await;
    let (status, body) = get(
        &st,
        &format!("/ngsi-ld/v1/temporal/entities/{ID}?lang=en&{WINDOW}"),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let insts = label_instances(&body);
    assert_eq!(insts.len(), 2, "{body}");
    let mut values: Vec<&str> = insts.iter().filter_map(|i| i["value"].as_str()).collect();
    values.sort_unstable();
    assert_eq!(values, ["bye", "hello"], "{body}");
    for i in &insts {
        assert_reduced(i, "en", i["value"].as_str().unwrap_or(""));
    }
}

/// temporalValues + lang: the reduced Property renders as 4.5.9 `values`
/// pairs of the chosen string, never as `languageMaps`.
#[tokio::test(flavor = "multi_thread")]
async fn clause_5_7_4_lang_with_temporal_values_yields_values_pairs() {
    let st = AppState::new("me".into());
    seed(&st).await;
    let (status, body) = get(
        &st,
        &format!(
            "/ngsi-ld/v1/temporal/entities?type=Vehicle&lang=fr&format=temporalValues&{WINDOW}"
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let label = &body[0]["label"];
    assert_eq!(label["type"], "Property", "{body}");
    assert!(label.get("languageMaps").is_none(), "{body}");
    let pairs = label["values"].as_array().expect("values pairs");
    let mut got: Vec<&str> = pairs.iter().filter_map(|p| p[0].as_str()).collect();
    got.sort_unstable();
    assert_eq!(got, ["bonjour", "salut"], "{body}");
    assert!(pairs.iter().all(|p| p[1].as_str().is_some()), "{body}");
}

/// Without `lang` the LanguageProperty keeps its full languageMap.
#[tokio::test(flavor = "multi_thread")]
async fn clause_5_7_4_no_lang_keeps_the_language_map() {
    let st = AppState::new("me".into());
    seed(&st).await;
    let (status, body) = get(
        &st,
        &format!("/ngsi-ld/v1/temporal/entities?type=Vehicle&{WINDOW}"),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    for i in label_instances(&body[0]) {
        assert_eq!(i["type"], "LanguageProperty", "{body}");
        assert!(i["languageMap"]["en"].is_string(), "{body}");
        assert!(i["languageMap"]["fr"].is_string(), "{body}");
        assert!(i.get("lang").is_none(), "{body}");
        assert!(i.get("value").is_none(), "{body}");
    }
}

/// No match for the requested language: a single available language is
/// chosen and named in `lang` (5.7.2.5 wording, applied to 6.18.3.2-1).
#[tokio::test(flavor = "multi_thread")]
async fn clause_5_7_4_lang_without_match_falls_back_to_one_language() {
    let st = AppState::new("me".into());
    seed(&st).await;
    let (status, body) = get(
        &st,
        &format!("/ngsi-ld/v1/temporal/entities?type=Vehicle&lang=de&{WINDOW}"),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    for i in label_instances(&body[0]) {
        let lang = i["lang"].as_str().unwrap_or("");
        assert!(lang == "en" || lang == "fr", "{body}");
        let value = i["value"].as_str().unwrap_or("");
        assert!(
            ["hello", "bonjour", "bye", "salut"].contains(&value),
            "{body}"
        );
        assert_reduced(&i, lang, value);
    }
}
