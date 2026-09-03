// SPDX-License-Identifier: EUPL-1.2
//! EntityMaps (5.14; resources 6.32, 6.34, 6.35): per-query candidate maps
//! recording which Entities — and which Context Sources — are relevant to an
//! ongoing consumption request (4.5.25, data type 5.2.39).

use crate::entity_map::{created_response, dt, map_delete, map_get, map_put, open_map};
use crate::negotiate::*;
use crate::state::AppState;
use antares_model::NgsiError;
use axum::body::Bytes;
use axum::extract::{Path, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use serde_json::{json, Value};
use std::collections::HashMap;

// ---------- storage (5.14.1.1: "internal storage, or memory") ----------

// ---------- 5.14.1 / 5.14.2 / 5.14.3: /entityMaps/{id} (6.32) ----------

/// 5.14.1.4 Retrieve EntityMap: invalid-URI id → 400 BadRequestData, unknown
/// id → 404 ResourceNotFound, else the 5.2.39 JSON-LD object.
pub async fn retrieve_entity_map(
    State(st): State<AppState>,
    Path(id): Path<String>,
    CleanParams(params): CleanParams,
    headers: HeaderMap,
) -> Response {
    let go = async {
        let tenant = open_map(&params, &headers, &id)?;
        gate!(st, &tenant, &headers, "5.14.1", ids: &[&id]).await?;
        let accept = parse_accept(&headers)?;
        let ctx = request_context(&st.loader, &headers).await?;
        let doc = map_get(&st, &tenant, &id)?
            .ok_or_else(|| NgsiError::ResourceNotFound(format!("EntityMap {id} not found")))?;
        Ok::<_, ApiError>(respond(StatusCode::OK, doc, &ctx, accept, &tenant))
    };
    go.await.unwrap_or_else(|e| e.into_response())
}

/// 5.14.2.4 Update EntityMap: partial update of the target EntityMap;
/// output-only members (entityMap, linkedMaps — 5.2.39) are ignored, and per
/// 5.5.14 other components may only update the expiry timestamp.
pub async fn update_entity_map(
    State(st): State<AppState>,
    Path(id): Path<String>,
    CleanParams(params): CleanParams,
    headers: HeaderMap,
    body: Bytes,
) -> Response {
    let go = async {
        let tenant = open_map(&params, &headers, &id)?;
        gate!(st, &tenant, &headers, "5.14.2", ids: &[&id]).await?;
        let frag: Value = serde_json::from_slice(&body)
            .map_err(|e| NgsiError::InvalidRequest(format!("body is not valid JSON: {e}")))?;
        let obj = frag.as_object().ok_or_else(|| {
            NgsiError::BadRequestData("EntityMap fragment must be a JSON object".into())
        })?;
        let mut doc = map_get(&st, &tenant, &id)?
            .ok_or_else(|| NgsiError::ResourceNotFound(format!("EntityMap {id} not found")))?;
        if let Some(e) = obj.get("expiresAt") {
            let s = e.as_str().filter(|s| dt(s).is_some()).ok_or_else(|| {
                NgsiError::BadRequestData("expiresAt must be a DateTime (4.6.3)".into())
            })?;
            doc["expiresAt"] = json!(s);
        }
        map_put(&st, &tenant, doc)?;
        Ok::<_, ApiError>(no_content(&tenant))
    };
    go.await.unwrap_or_else(|e| e.into_response())
}

/// 5.14.3.4 Delete EntityMap: invalid-URI id → 400, unknown id → 404, else
/// the EntityMap is removed from storage/memory (204).
pub async fn delete_entity_map(
    State(st): State<AppState>,
    Path(id): Path<String>,
    CleanParams(params): CleanParams,
    headers: HeaderMap,
) -> Response {
    let go = async {
        let tenant = open_map(&params, &headers, &id)?;
        gate!(st, &tenant, &headers, "5.14.3", ids: &[&id]).await?;
        if !map_delete(&st, &tenant, &id)? {
            return Err(NgsiError::ResourceNotFound(format!("EntityMap {id} not found")).into());
        }
        Ok::<_, ApiError>(no_content(&tenant))
    };
    go.await.unwrap_or_else(|e| e.into_response())
}

// ---------- 5.14.4: Create EntityMap for Query Entities (6.34) ----------

fn allowed_create_params() -> Vec<&'static str> {
    let mut v = crate::negotiate::QUERY_PARAMS.to_vec();
    v.extend(["entityMapLifetime", "splitEntities"]);
    v
}

/// The 6.35.3.1/6.35.3.2 parameters: the query set above plus the temporal
/// query's own (5.7.4). Checked in the handler because a split-reduced
/// temporal query rebuilds its parameters from a fixed list and would drop an
/// unknown one before 5.7.4 ever sees it (6.3.20).
fn allowed_temporal_create_params() -> Vec<&'static str> {
    let mut v = allowed_create_params();
    v.extend([
        "timerel",
        "timeAt",
        "endTimeAt",
        "timeproperty",
        "aggrMethods",
        "aggrPeriodDuration",
        "lastN",
    ]);
    v
}

/// 5.14.4 / 5.14.5 Create EntityMap. The four resource methods are one
/// operation: 6.34.3.2 and 6.35.3.2 carry the 5.2.23 Query object where
/// 6.34.3.1 and 6.35.3.1 carry query parameters, and the temporal pair adds
/// the 5.7.4 temporal query to the same pipeline.
async fn create_map(
    st: AppState,
    params: HashMap<String, String>,
    headers: HeaderMap,
    body: Option<Bytes>,
    temporal: bool,
) -> Response {
    let go = async {
        let tenant = tenant_from(&headers)?;
        gate!(
            st,
            &tenant,
            &headers,
            if temporal { "5.14.5" } else { "5.14.4" }
        )
        .await?;
        let allowed = if temporal {
            allowed_temporal_create_params()
        } else {
            allowed_create_params()
        };
        // The Query object is folded into the parameters BEFORE 6.3.20 runs:
        // a member it carries is a parameter of this request.
        let vp = match &body {
            Some(b) => query_body_params(b, &params, temporal)?,
            None => params,
        };
        check_params(&vp, &allowed)?;
        let accept = parse_accept(&headers)?;
        let ctx = request_context(&st.loader, &headers).await?;
        let doc = if temporal {
            crate::temporal::build_temporal_map(&st, &tenant, &headers, &ctx, &vp).await?
        } else {
            crate::entities::build_query_map(&st, &tenant, &headers, &ctx, &vp).await?
        };
        Ok::<_, ApiError>(created_response(doc, &ctx, accept, &tenant))
    };
    go.await.unwrap_or_else(|e| e.into_response())
}

/// 5.2.23: a Query object stands in for the query parameters. Its members are
/// folded into them, so one rule serves both forms of every Create EntityMap.
fn query_body_params(
    body: &Bytes,
    params: &HashMap<String, String>,
    temporal: bool,
) -> ApiResult<HashMap<String, String>> {
    let q: Value = serde_json::from_slice(body)
        .map_err(|e| NgsiError::InvalidRequest(format!("body is not valid JSON: {e}")))?;
    if q.get("type").and_then(Value::as_str) != Some("Query") {
        return Err(NgsiError::BadRequestData("body type must be Query".into()).into());
    }
    let qo = q
        .as_object()
        .ok_or_else(|| NgsiError::BadRequestData("query body must be an object".into()))?;
    let mut vp: HashMap<String, String> = params.clone();
    crate::paging::query_doc_params(qo, temporal, &mut vp)?;
    Ok(vp)
}

/// GET /entityMaps — Create EntityMap for Query Entities (6.34.3.1).
pub async fn create_entity_map(
    State(st): State<AppState>,
    CleanParams(params): CleanParams,
    headers: HeaderMap,
) -> Response {
    create_map(st, params, headers, None, false).await
}

/// POST /entityMaps — the 5.2.23 Query-object form (6.34.3.2).
pub async fn create_entity_map_post(
    State(st): State<AppState>,
    CleanParams(params): CleanParams,
    headers: HeaderMap,
    body: Bytes,
) -> Response {
    create_map(st, params, headers, Some(body), false).await
}

// ------ 5.14.5: Create EntityMap for Query Temporal Evolution (6.35) ------

/// GET /temporal/entityMaps — Create EntityMap for Query Temporal Evolution
/// of Entities (6.35.3.1).
pub async fn create_temporal_entity_map(
    State(st): State<AppState>,
    CleanParams(params): CleanParams,
    headers: HeaderMap,
) -> Response {
    create_map(st, params, headers, None, true).await
}

/// POST /temporal/entityMaps — the Query-object form (6.35.3.2).
pub async fn create_temporal_entity_map_post(
    State(st): State<AppState>,
    CleanParams(params): CleanParams,
    headers: HeaderMap,
    body: Bytes,
) -> Response {
    create_map(st, params, headers, Some(body), true).await
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::entities::build_query_map;
    use crate::entity_map::map_id_check;
    use antares_model::TenantId;
    use antares_store::Kind;
    use serde_json::json;
    use std::collections::HashMap;

    fn params(pairs: &[(&str, &str)]) -> HashMap<String, String> {
        pairs
            .iter()
            .map(|(k, v)| ((*k).to_owned(), (*v).to_owned()))
            .collect()
    }

    /// 5.14.4.4 + 5.5.9.3: the EntityMap holds the CANDIDATE identifiers a
    /// later paginated request re-checks, so building one must not depend on
    /// materializing the tenant's whole match set. The candidate set is
    /// bounded by the broker ceiling, exactly as the temporal twin is.
    #[tokio::test]
    async fn clause_5_14_4_query_map_candidate_set_is_bounded() {
        let mut st = AppState::new("antares-em-bound".into());
        st.max_limit = 8;
        let t = TenantId::default();
        for i in 0..40 {
            let id = format!("urn:ngsi-ld:Vehicle:{i:03}");
            st.store
                .create(
                    &t,
                    Kind::Entity,
                    &id,
                    json!({
                        "id": id,
                        "type": ["https://uri.etsi.org/ngsi-ld/default-context/Vehicle"],
                    }),
                )
                .expect("seed");
        }
        let doc = build_query_map(
            &st,
            &t,
            &HeaderMap::new(),
            &antares_jsonld::Context::default(),
            &params(&[("type", "Vehicle"), ("local", "true")]),
        )
        .await
        .expect("EntityMap");
        let emap = doc["entityMap"].as_object().expect("entityMap object");
        assert!(!emap.is_empty(), "the matching entities are candidates");
        assert!(
            emap.len() <= st.max_limit,
            "the candidate set is unbounded: {} entries for a ceiling of {}",
            emap.len(),
            st.max_limit
        );
        // a query that matches nothing must not pick up unrelated entities
        let none = build_query_map(
            &st,
            &t,
            &HeaderMap::new(),
            &antares_jsonld::Context::default(),
            &params(&[("type", "Ship"), ("local", "true")]),
        )
        .await
        .expect("EntityMap");
        assert_eq!(none["entityMap"], json!({}));
    }

    /// 6.3.20: an unknown query parameter is InvalidRequest. The temporal
    /// EntityMap resources take the 6.35.3.1/6.35.3.2 parameters and nothing
    /// else — including when splitEntities=true reduces the query, where the
    /// unknown parameter would otherwise be dropped before anything sees it.
    #[tokio::test]
    async fn clause_6_3_20_temporal_map_rejects_unknown_parameters() {
        let st = AppState::new("antares-em-params".into());
        let p = params(&[
            ("type", "Vehicle"),
            ("timerel", "before"),
            ("timeAt", "2020-01-01T00:00:00Z"),
            ("splitEntities", "true"),
            ("bogus", "1"),
        ]);
        let resp =
            create_temporal_entity_map(State(st.clone()), CleanParams(p), HeaderMap::new()).await;
        assert_eq!(resp.status(), StatusCode::BAD_REQUEST, "GET");
        let body = Bytes::from_static(
            br#"{"type":"Query","timerel":"before","timeAt":"2020-01-01T00:00:00Z"}"#,
        );
        let resp = create_temporal_entity_map_post(
            State(st.clone()),
            CleanParams(params(&[("bogus", "1")])),
            HeaderMap::new(),
            body,
        )
        .await;
        assert_eq!(resp.status(), StatusCode::BAD_REQUEST, "POST");
    }

    /// 5.14.1.4 / 5.14.3.4: "If the EntityMap id is not present or it is not
    /// a valid URI, then an error of type BadRequestData shall be raised."
    #[test]
    fn clause_5_14_1_map_id_must_be_a_uri() {
        assert!(map_id_check("urn:ngsi-ld:entitymap:1").is_ok());
        for bad in [
            "",
            "entitymap",
            "urn:ngsi-ld:entity map:1",
            "urn:ngsi-ld:entitymap:1\r\nX: y",
            ":nostem",
            "urn:",
        ] {
            match map_id_check(bad) {
                Err(NgsiError::BadRequestData(_)) => {}
                other => panic!("{bad:?} must be BadRequestData, got {other:?}"),
            }
        }
    }
}
