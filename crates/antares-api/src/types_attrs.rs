//! Discovery: /types and /attributes (5.7.5–5.7.10; resources 6.25–6.28).

use crate::negotiate::*;
use crate::state::AppState;
use antares_model::NgsiError;
use antares_sql::store::filter::{EntityFilter, Page};
use axum::extract::{Path, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use serde_json::{json, Value};
use std::collections::{BTreeMap, BTreeSet};

use crate::negotiate::CleanParams;

/// Deployment knob (ANTARES_DISCOVERY_SCAN_MAX): entities one discovery fold
/// may read. 5.7.5-5.7.10 give /types and /attributes no pagination, so the
/// fold is over the whole tenant — this caps the work a single request can
/// buy. Read once at first use.
static SCAN_MAX: std::sync::LazyLock<usize> = std::sync::LazyLock::new(|| {
    std::env::var("ANTARES_DISCOVERY_SCAN_MAX")
        .ok()
        .and_then(|v| v.parse().ok())
        .filter(|n| *n > 0)
        .unwrap_or(100_000)
});

/// At most `max` of the tenant's entities, plus whether more exist. The page
/// is pushed into the datastore where the backend takes it, so the ceiling
/// bounds the rows materialized, not merely the rows folded. Asking for one
/// row past `max` is what makes the overflow visible.
fn scan(
    st: &AppState,
    tenant: &antares_model::TenantId,
    max: usize,
) -> Result<(Vec<Value>, bool), NgsiError> {
    let f = EntityFilter {
        page: Some(Page {
            offset: 0,
            limit: max.saturating_add(1).min(i64::MAX as usize) as i64,
        }),
        ..Default::default()
    };
    let mut rows = st.store.query_entities(tenant, &f)?.rows;
    let more = rows.len() > max;
    rows.truncate(max);
    Ok((rows, more))
}

/// A fold that hit the scan ceiling answered from a prefix of the tenant's
/// entities: the list is a subset and a by-name lookup can miss. IETF RFC
/// 7234 5.5.1 warn-code 199 (Miscellaneous Warning) carries that fact to the
/// client in the `NGSILD-Warning` header of 6.3.17.
fn mark_partial(resp: &mut Response, partial: bool, alias: &str) {
    if partial {
        crate::entities::attach_warnings(
            resp,
            &[crate::federation::warning(
                199,
                alias,
                "entity scan ceiling reached; the discovery result is incomplete",
            )],
        );
    }
}

fn is_meta(k: &str) -> bool {
    matches!(
        k,
        "id" | "type" | "scope" | "createdAt" | "modifiedAt" | "deletedAt" | "expiresAt"
    )
}

/// type IRI → (entity count, attr IRI → attribute types seen)
type TypeStats = BTreeMap<String, (usize, BTreeMap<String, BTreeSet<String>>)>;

/// A datastore failure is an InternalError (Table 6.3.2-1), never an empty
/// fold — reporting "no such type" because the query failed would be a lie.
/// The second return member is true when the fold stopped at `max`, i.e. the
/// answer is a subset of the tenant's types.
fn type_stats(
    st: &AppState,
    tenant: &antares_model::TenantId,
    max: usize,
) -> Result<(TypeStats, bool), NgsiError> {
    let mut map: TypeStats = BTreeMap::new();
    let (rows, partial) = scan(st, tenant, max)?;
    for doc in rows {
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
    Ok((map, partial))
}

/// attr IRI → (count, attribute types, entity type IRIs)
type AttrStats = BTreeMap<String, (usize, BTreeSet<String>, BTreeSet<String>)>;

/// Same bound and same incompleteness report as `type_stats`.
fn attr_stats(
    st: &AppState,
    tenant: &antares_model::TenantId,
    max: usize,
) -> Result<(AttrStats, bool), NgsiError> {
    let mut map: AttrStats = BTreeMap::new();
    let (rows, partial) = scan(st, tenant, max)?;
    for doc in rows {
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
                // Table 5.2.28-1 attributeCount: "Number of attribute
                // instances with this attribute name" — a multi-instance
                // attribute (4.5.5 datasetId) counts once per instance.
                e.0 += v.as_array().map_or(1, Vec::len);
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
    Ok((map, partial))
}

// ---------- GET /types (5.7.5/5.7.6) ----------

/// 4.5.10 Entity Type List Representation, members per Table 5.2.24-1
/// (5.2.24 EntityTypeList): id a valid URI, type equal to "EntityTypeList",
/// typeList the entity type names — with details=true the 4.5.11 detailed
/// list of Table 5.2.25-1 EntityType objects (id = type FQN, type
/// "EntityType", attributeNames, typeName) (5.7.5/5.7.6).
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
        let (stats, partial) = type_stats(&st, &tenant, *SCAN_MAX)?;
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
        let mut resp = respond(StatusCode::OK, payload, &ctx, accept, &tenant);
        mark_partial(&mut resp, partial, &st.host_alias);
        Ok::<_, ApiError>(resp)
    };
    go.await.unwrap_or_else(|e| e.into_response())
}

// ---------- GET /types/{type} (5.7.7) ----------

/// 4.5.12 Entity Type Information Representation, members per Table
/// 5.2.26-1 (5.2.26 EntityTypeInfo): id = the entity type FQN, fixed type
/// "EntityTypeInfo", typeName (short name under the @context), entityCount
/// an unsigned integer, attributeDetails Attribute[] restricted to the
/// elements id/type/attributeName/attributeTypes (5.7.7).
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
        let (stats, partial) = type_stats(&st, &tenant, *SCAN_MAX)?;
        let Some((count, attrs)) = stats.get(&iri) else {
            let mut resp = ApiError::from(NgsiError::ResourceNotFound(format!(
                "no entities of type {type_name}"
            )))
            .into_response();
            mark_partial(&mut resp, partial, &st.host_alias);
            return Ok(resp);
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
        let mut resp = respond(StatusCode::OK, payload, &ctx, accept, &tenant);
        mark_partial(&mut resp, partial, &st.host_alias);
        Ok::<_, ApiError>(resp)
    };
    go.await.unwrap_or_else(|e| e.into_response())
}

// ---------- GET /attributes (5.7.8/5.7.9) ----------

/// 4.5.13 Attribute List Representation, members per Table 5.2.27-1
/// (5.2.27 AttributeList): id a valid URI, type "AttributeList",
/// attributeList of attribute names — with details=true the 4.5.14 detailed
/// list of Table 5.2.28-1 Attribute objects (id = attribute URI, type
/// "Attribute", attributeName, typeNames).
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
        let (stats, partial) = attr_stats(&st, &tenant, *SCAN_MAX)?;
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
        let mut resp = respond(StatusCode::OK, payload, &ctx, accept, &tenant);
        mark_partial(&mut resp, partial, &st.host_alias);
        Ok::<_, ApiError>(resp)
    };
    go.await.unwrap_or_else(|e| e.into_response())
}

// ---------- GET /attributes/{attrId} (5.7.10) ----------

/// 4.5.15 Attribute Information Representation, members per Table 5.2.28-1
/// (5.2.28 Attribute): id = the attribute URI, fixed type "Attribute",
/// attributeName (short name under @context), plus the optional
/// attributeCount (unsigned integer) / attributeTypes / typeNames members.
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
        let (stats, partial) = attr_stats(&st, &tenant, *SCAN_MAX)?;
        let Some((count, attr_types, etypes)) = stats.get(&iri) else {
            let mut resp = ApiError::from(NgsiError::ResourceNotFound(format!(
                "attribute {attr} not found"
            )))
            .into_response();
            mark_partial(&mut resp, partial, &st.host_alias);
            return Ok(resp);
        };
        let payload = json!({
            "id": iri,
            "type": "Attribute",
            "attributeName": ctx.compact_iri(&iri),
            "attributeCount": count,
            "attributeTypes": attr_types.iter().cloned().collect::<Vec<_>>(),
            "typeNames": etypes.iter().map(|t| ctx.compact_iri(t)).collect::<Vec<_>>(),
        });
        let mut resp = respond(StatusCode::OK, payload, &ctx, accept, &tenant);
        mark_partial(&mut resp, partial, &st.host_alias);
        Ok::<_, ApiError>(resp)
    };
    go.await.unwrap_or_else(|e| e.into_response())
}

#[cfg(test)]
mod discovery_folds {
    use super::*;
    use crate::state::AppState;
    use antares_model::TenantId;
    use antares_sql::store::Kind;
    use serde_json::json;

    const V: &str = "https://uri.etsi.org/ngsi-ld/default-context/v";
    const R: &str = "https://uri.etsi.org/ngsi-ld/default-context/r";
    const BUILDING: &str = "https://uri.etsi.org/ngsi-ld/default-context/Building";
    const SENSOR: &str = "https://uri.etsi.org/ngsi-ld/default-context/Sensor";

    /// A ceiling no test fixture reaches, so the fold sees every entity.
    const ALL: usize = 1_000;

    fn tid(t: &str) -> TenantId {
        TenantId::new(t).expect("tenant")
    }

    fn seed(st: &AppState, tenant: &str, id: &str, doc: Value) {
        assert!(st
            .store
            .create(&tid(tenant), Kind::Entity, id, doc)
            .expect("create"));
    }

    /// The Entity members and system temporal attributes of 4.5.1/6.3.11 are
    /// not Attributes and never enter the 5.7.5-5.7.10 folds.
    #[test]
    fn is_meta_matches_only_entity_members() {
        for k in [
            "id",
            "type",
            "scope",
            "createdAt",
            "modifiedAt",
            "deletedAt",
            "expiresAt",
        ] {
            assert!(is_meta(k), "{k} is an Entity member, not an Attribute");
        }
        for k in [
            "",
            "v",
            "location",
            "Type",
            "ID",
            "created_at",
            "createdAtX",
        ] {
            assert!(!is_meta(k), "{k} must be treated as an Attribute");
        }
    }

    /// 5.2.26: entityCount is the number of entity instances of the type;
    /// 5.2.25 attributeNames lists the attributes those instances can have —
    /// Entity members are not among them.
    #[test]
    fn type_stats_folds_types_and_attribute_names() {
        let st = AppState::new("test".into());
        seed(
            &st,
            "ta",
            "urn:ngsi-ld:B:1",
            json!({
                "id": "urn:ngsi-ld:B:1",
                "type": [BUILDING, SENSOR],
                "createdAt": "2026-01-01T00:00:00Z",
                "modifiedAt": "2026-01-01T00:00:00Z",
                "scope": ["/a/b"],
                V: [{"type": "Property", "value": 1}],
                R: [{"type": "Relationship", "object": "urn:ngsi-ld:B:2"}],
            }),
        );
        seed(
            &st,
            "ta",
            "urn:ngsi-ld:B:2",
            json!({
                "id": "urn:ngsi-ld:B:2",
                "type": [BUILDING],
                V: [{"type": "GeoProperty", "value": {"type": "Point", "coordinates": [0, 0]}}],
            }),
        );
        let (stats, partial) = type_stats(&st, &tid("ta"), ALL).expect("stats");
        assert!(!partial, "two entities are under any sane ceiling");
        let (count, attrs) = stats.get(BUILDING).expect("Building");
        assert_eq!(*count, 2, "two entities carry the Building type");
        assert_eq!(stats.get(SENSOR).expect("Sensor").0, 1);
        assert!(attrs.contains_key(V) && attrs.contains_key(R));
        for meta in ["id", "type", "scope", "createdAt", "modifiedAt"] {
            assert!(
                !attrs.contains_key(meta),
                "{meta} must not be reported as an attribute name"
            );
        }
        // 5.2.28 attributeTypes: every attribute type an instance carried
        assert!(attrs[V].contains("Property") && attrs[V].contains("GeoProperty"));
    }

    /// One shared datastore, one tenant per request: an entity of another
    /// tenant contributes to no fold (4.15 multi-tenancy).
    #[test]
    fn stats_are_tenant_scoped() {
        let st = AppState::new("test".into());
        seed(
            &st,
            "ta",
            "urn:ngsi-ld:B:1",
            json!({
                "id": "urn:ngsi-ld:B:1",
                "type": [BUILDING],
                V: [{"type": "Property", "value": 1}],
            }),
        );
        for other in ["tb", TenantId::DEFAULT] {
            let t = tid(other);
            assert!(
                type_stats(&st, &t, ALL).expect("stats").0.is_empty(),
                "{other} must not see another tenant's types"
            );
            assert!(
                attr_stats(&st, &t, ALL).expect("stats").0.is_empty(),
                "{other} must not see another tenant's attributes"
            );
        }
        assert!(type_stats(&st, &tid("ta"), ALL)
            .expect("stats")
            .0
            .contains_key(BUILDING));
    }

    /// Table 5.2.28-1: attributeCount is the "number of attribute instances
    /// with this attribute name" — multi-instance attributes (4.5.5
    /// datasetId) count once per instance, not once per entity.
    #[test]
    fn attr_stats_counts_attribute_instances() {
        let st = AppState::new("test".into());
        seed(
            &st,
            "ta",
            "urn:ngsi-ld:B:1",
            json!({
                "id": "urn:ngsi-ld:B:1",
                "type": [BUILDING],
                V: [
                    {"type": "Property", "value": 1,
                     "datasetId": "urn:ngsi-ld:ds:1"},
                    {"type": "Property", "value": 2,
                     "datasetId": "urn:ngsi-ld:ds:2"},
                ],
            }),
        );
        seed(
            &st,
            "ta",
            "urn:ngsi-ld:B:2",
            json!({
                "id": "urn:ngsi-ld:B:2",
                "type": [SENSOR],
                V: [{"type": "Property", "value": 3}],
            }),
        );
        let (stats, _) = attr_stats(&st, &tid("ta"), ALL).expect("stats");
        let (count, atypes, etypes) = stats.get(V).expect("v");
        assert_eq!(*count, 3, "two instances on B:1 plus one on B:2");
        assert!(atypes.contains("Property"));
        assert_eq!(etypes.len(), 2, "both entity types are reported");
        assert!(
            !stats.contains_key("createdAt") && !stats.contains_key("id"),
            "Entity members must not appear in the attribute fold"
        );
    }

    /// 4.8 expiresAt: an expired entity no longer exists, and an expired
    /// attribute instance is gone with it — neither is discoverable.
    #[test]
    fn expired_entities_and_attributes_are_not_discoverable() {
        let st = AppState::new("test".into());
        seed(
            &st,
            "ta",
            "urn:ngsi-ld:B:1",
            json!({
                "id": "urn:ngsi-ld:B:1",
                "type": [BUILDING],
                "expiresAt": "2000-01-01T00:00:00Z",
                V: [{"type": "Property", "value": 1}],
            }),
        );
        seed(
            &st,
            "ta",
            "urn:ngsi-ld:B:2",
            json!({
                "id": "urn:ngsi-ld:B:2",
                "type": [SENSOR],
                V: [{"type": "Property", "value": 1,
                     "expiresAt": "2000-01-01T00:00:00Z"}],
                R: [{"type": "Relationship", "object": "urn:ngsi-ld:B:1"}],
            }),
        );
        let (stats, _) = type_stats(&st, &tid("ta"), ALL).expect("stats");
        assert!(
            !stats.contains_key(BUILDING),
            "the expired entity must not appear in the type list"
        );
        let (count, attrs) = stats.get(SENSOR).expect("Sensor");
        assert_eq!(*count, 1);
        assert!(
            !attrs.contains_key(V),
            "the expired attribute instance must not appear"
        );
        assert!(attrs.contains_key(R));
        assert!(!attr_stats(&st, &tid("ta"), ALL)
            .expect("stats")
            .0
            .contains_key(V));
    }

    /// A tenant with no entities has no types and no attributes — an empty
    /// fold, not an error, and never "incomplete".
    #[test]
    fn empty_tenant_folds_to_nothing() {
        let st = AppState::new("test".into());
        for t in ["ta", TenantId::DEFAULT] {
            let (types, partial) = type_stats(&st, &tid(t), ALL).expect("stats");
            assert!(types.is_empty() && !partial);
            let (attrs, partial) = attr_stats(&st, &tid(t), ALL).expect("stats");
            assert!(attrs.is_empty() && !partial);
            let (rows, partial) = scan(&st, &tid(t), ALL).expect("scan");
            assert!(rows.is_empty() && !partial);
        }
    }

    /// The fold reads at most `max` entities and says so when the tenant
    /// holds more, instead of silently answering from a prefix.
    #[test]
    fn scan_stops_at_the_ceiling_and_reports_it() {
        let st = AppState::new("test".into());
        for i in 0..5 {
            let id = format!("urn:ngsi-ld:B:{i}");
            let mut doc = serde_json::Map::new();
            doc.insert("id".into(), Value::String(id.clone()));
            doc.insert("type".into(), json!([format!("{BUILDING}{i}")]));
            doc.insert(format!("{V}{i}"), json!([{"type": "Property", "value": i}]));
            seed(&st, "ta", &id, Value::Object(doc));
        }
        for (max, want) in [(1, 1), (4, 4), (5, 5), (6, 5)] {
            let (rows, partial) = scan(&st, &tid("ta"), max).expect("scan");
            assert_eq!(rows.len(), want, "max {max}");
            assert_eq!(partial, max < 5, "max {max} incompleteness");
        }
        // the folds inherit the bound: one entity read, one type, one attr
        let (types, partial) = type_stats(&st, &tid("ta"), 1).expect("stats");
        assert_eq!(types.len(), 1, "the fold read past its ceiling");
        assert!(partial, "a truncated type fold must report itself");
        let (attrs, partial) = attr_stats(&st, &tid("ta"), 1).expect("stats");
        assert_eq!(attrs.len(), 1, "the fold read past its ceiling");
        assert!(partial, "a truncated attribute fold must report itself");
        // at the exact size the answer is complete, so nothing is flagged
        let (types, partial) = type_stats(&st, &tid("ta"), 5).expect("stats");
        assert_eq!(types.len(), 5);
        assert!(!partial);
    }

    /// A complete answer carries no warning; a truncated one carries exactly
    /// one 199 (RFC 7234 5.5.1) naming this broker.
    #[test]
    fn mark_partial_emits_one_199_only_when_truncated() {
        let mut resp = StatusCode::OK.into_response();
        mark_partial(&mut resp, false, "broker-a");
        assert!(resp.headers().get("NGSILD-Warning").is_none());
        mark_partial(&mut resp, true, "broker-a");
        let vals: Vec<_> = resp.headers().get_all("NGSILD-Warning").iter().collect();
        assert_eq!(vals.len(), 1);
        let v = vals[0].to_str().expect("ascii");
        assert!(v.starts_with("199 broker-a \"") && v.ends_with('"'), "{v}");
        assert!(v.contains("incomplete"), "{v}");
    }
}
