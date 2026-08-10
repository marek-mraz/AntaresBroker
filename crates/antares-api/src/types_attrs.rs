//! Discovery: /types and /attributes (5.7.5–5.7.10; resources 6.25–6.28).

use crate::negotiate::*;
use crate::state::AppState;
use antares_model::NgsiError;
use antares_sql::store::Kind;
use axum::extract::{Path, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use serde_json::{json, Value};
use std::collections::{BTreeMap, BTreeSet};

use crate::negotiate::CleanParams;

fn is_meta(k: &str) -> bool {
    matches!(
        k,
        "id" | "type" | "scope" | "createdAt" | "modifiedAt" | "deletedAt" | "expiresAt"
    )
}

/// type IRI → (entity count, attr IRI → attribute types seen)
type TypeStats = BTreeMap<String, (usize, BTreeMap<String, BTreeSet<String>>)>;

fn type_stats(st: &AppState, tenant: &antares_model::TenantId) -> TypeStats {
    let mut map: TypeStats = BTreeMap::new();
    for doc in st.store.list(tenant, Kind::Entity).unwrap_or_default() {
        let mut attrs: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
        if let Some(o) = doc.as_object() {
            for (k, v) in o {
                if is_meta(k) {
                    continue;
                }
                let types: BTreeSet<String> = v
                    .as_array()
                    .map(|arr| {
                        arr.iter()
                            .filter_map(|i| i.get("type").and_then(Value::as_str))
                            .map(str::to_owned)
                            .collect()
                    })
                    .unwrap_or_default();
                attrs.entry(k.clone()).or_default().extend(types);
            }
        }
        for t in doc["type"].as_array().cloned().unwrap_or_default() {
            if let Some(t) = t.as_str() {
                let e = map.entry(t.to_owned()).or_default();
                e.0 += 1;
                for (a, tys) in &attrs {
                    e.1.entry(a.clone())
                        .or_default()
                        .extend(tys.iter().cloned());
                }
            }
        }
    }
    map
}

/// attr IRI → (count, attribute types, entity type IRIs)
fn attr_stats(
    st: &AppState,
    tenant: &antares_model::TenantId,
) -> BTreeMap<String, (usize, BTreeSet<String>, BTreeSet<String>)> {
    let mut map: BTreeMap<String, (usize, BTreeSet<String>, BTreeSet<String>)> = BTreeMap::new();
    for doc in st.store.list(tenant, Kind::Entity).unwrap_or_default() {
        let etypes: Vec<String> = doc["type"]
            .as_array()
            .cloned()
            .unwrap_or_default()
            .iter()
            .filter_map(Value::as_str)
            .map(str::to_owned)
            .collect();
        if let Some(o) = doc.as_object() {
            for (k, v) in o {
                if is_meta(k) {
                    continue;
                }
                let e = map.entry(k.clone()).or_default();
                e.0 += 1;
                if let Some(arr) = v.as_array() {
                    for inst in arr {
                        if let Some(t) = inst.get("type").and_then(Value::as_str) {
                            e.1.insert(t.to_owned());
                        }
                    }
                }
                e.2.extend(etypes.iter().cloned());
            }
        }
    }
    map
}

// ---------- GET /types (5.7.5/5.7.6) ----------

/// 4.5.10 Entity Type List Representation: id (URI), fixed type
/// "EntityTypeList", typeList of entity type names — 4.5.11 EntityType
/// detail objects when details=true (5.7.5/5.7.6).
pub async fn entity_types(
    State(st): State<AppState>,
    CleanParams(params): CleanParams,
    headers: HeaderMap,
) -> Response {
    let go = async {
        let tenant = tenant_from(&headers)?;
        check_params(&params, &["details", "local", "count"])?;
        let accept = parse_accept(&headers)?;
        let ctx = request_context(&st.loader, &headers).await?;
        let stats = type_stats(&st, &tenant);
        let details = params.get("details").map(String::as_str) == Some("true");
        let payload = if details {
            Value::Array(
                stats
                    .iter()
                    .map(|(t, (_, attrs))| {
                        json!({
                            "id": t,
                            "type": "EntityType",
                            "typeName": ctx.compact_iri(t),
                            "attributeNames": attrs.keys().map(|a| ctx.compact_iri(a)).collect::<Vec<_>>(),
                        })
                    })
                    .collect(),
            )
        } else {
            json!({
                "id": format!("urn:ngsi-ld:EntityTypeList:{}", uuid::Uuid::new_v4()),
                "type": "EntityTypeList",
                "typeList": stats.keys().map(|t| ctx.compact_iri(t)).collect::<Vec<_>>(),
            })
        };
        Ok::<_, ApiError>(respond(StatusCode::OK, payload, &ctx, accept, &tenant))
    };
    go.await.unwrap_or_else(|e| e.into_response())
}

// ---------- GET /types/{type} (5.7.7) ----------

/// 4.5.12 Entity Type Information Representation: id = the entity type URI,
/// fixed type "EntityTypeInfo", typeName (short name under the @context);
/// entityCount and attributeDetails are the 5.2.26 detail members (5.7.7).
pub async fn entity_type_info(
    State(st): State<AppState>,
    Path(type_name): Path<String>,
    CleanParams(params): CleanParams,
    headers: HeaderMap,
) -> Response {
    let go = async {
        let tenant = tenant_from(&headers)?;
        check_params(&params, &["local"])?;
        let accept = parse_accept(&headers)?;
        let ctx = request_context(&st.loader, &headers).await?;
        let iri = ctx.expand_key(&type_name);
        let stats = type_stats(&st, &tenant);
        let Some((count, attrs)) = stats.get(&iri) else {
            return Err(
                NgsiError::ResourceNotFound(format!("no entities of type {type_name}")).into(),
            );
        };
        let attr_details: Vec<Value> = attrs
            .iter()
            .map(|(a, atypes)| {
                json!({
                    "id": a,
                    "type": "Attribute",
                    "attributeName": ctx.compact_iri(a),
                    "attributeTypes": atypes.iter().cloned().collect::<Vec<_>>(),
                })
            })
            .collect();
        let payload = json!({
            "id": iri,
            "type": "EntityTypeInfo",
            "typeName": ctx.compact_iri(&iri),
            "entityCount": count,
            "attributeDetails": attr_details,
        });
        Ok::<_, ApiError>(respond(StatusCode::OK, payload, &ctx, accept, &tenant))
    };
    go.await.unwrap_or_else(|e| e.into_response())
}

// ---------- GET /attributes (5.7.8/5.7.9) ----------

pub async fn attributes(
    State(st): State<AppState>,
    CleanParams(params): CleanParams,
    headers: HeaderMap,
) -> Response {
    let go = async {
        let tenant = tenant_from(&headers)?;
        check_params(&params, &["details", "local", "count"])?;
        let accept = parse_accept(&headers)?;
        let ctx = request_context(&st.loader, &headers).await?;
        let stats = attr_stats(&st, &tenant);
        let details = params.get("details").map(String::as_str) == Some("true");
        let payload = if details {
            Value::Array(
                stats
                    .iter()
                    .map(|(a, (_, _, etypes))| {
                        json!({
                            "id": a,
                            "type": "Attribute",
                            "attributeName": ctx.compact_iri(a),
                            "typeNames": etypes.iter().map(|t| ctx.compact_iri(t)).collect::<Vec<_>>(),
                        })
                    })
                    .collect(),
            )
        } else {
            json!({
                "id": format!("urn:ngsi-ld:AttributeList:{}", uuid::Uuid::new_v4()),
                "type": "AttributeList",
                "attributeList": stats.keys().map(|a| ctx.compact_iri(a)).collect::<Vec<_>>(),
            })
        };
        Ok::<_, ApiError>(respond(StatusCode::OK, payload, &ctx, accept, &tenant))
    };
    go.await.unwrap_or_else(|e| e.into_response())
}

// ---------- GET /attributes/{attrId} (5.7.10) ----------

pub async fn attribute_info(
    State(st): State<AppState>,
    Path(attr): Path<String>,
    CleanParams(params): CleanParams,
    headers: HeaderMap,
) -> Response {
    let go = async {
        let tenant = tenant_from(&headers)?;
        check_params(&params, &["local"])?;
        let accept = parse_accept(&headers)?;
        let ctx = request_context(&st.loader, &headers).await?;
        let iri = ctx.expand_key(&attr);
        let stats = attr_stats(&st, &tenant);
        let Some((count, attr_types, etypes)) = stats.get(&iri) else {
            return Err(NgsiError::ResourceNotFound(format!("attribute {attr} not found")).into());
        };
        let payload = json!({
            "id": iri,
            "type": "Attribute",
            "attributeName": ctx.compact_iri(&iri),
            "attributeCount": count,
            "attributeTypes": attr_types.iter().cloned().collect::<Vec<_>>(),
            "typeNames": etypes.iter().map(|t| ctx.compact_iri(t)).collect::<Vec<_>>(),
        });
        Ok::<_, ApiError>(respond(StatusCode::OK, payload, &ctx, accept, &tenant))
    };
    go.await.unwrap_or_else(|e| e.into_response())
}
