// SPDX-License-Identifier: EUPL-1.2
//! One EntityMap document (5.2.39) and the rules for using one: store it
//! under its tenant with a lifetime, read it back only while it is alive,
//! take the candidate ids of a page out of it, merge the registrations a
//! distributed query reached into it, and serve a retrieve through the map
//! a client presented. The /entityMaps resource itself is `entity_maps`,
//! which composes this over the queries in `entities` and `temporal`.

use crate::negotiate::*;
use crate::state::AppState;
use antares_model::{NgsiError, TenantId};
use antares_store::CurrentStateDriverExt;
use antares_store::Kind;
use axum::http::{HeaderMap, StatusCode};
use axum::response::Response;
use serde_json::{json, Map, Value};
use std::collections::HashMap;

/// Per-tenant EntityMap cap (every buffer bounded); earliest-expiring evicted.
pub(crate) const MAX_MAPS_PER_TENANT: usize = 512;

/// Default lifetime when the client suggests none — 5.5.14: "the caching
/// strategy and expiry time … depend on implementation specific
/// configurations".
pub(crate) const DEFAULT_LIFETIME_SECS: i64 = 3600;

/// Ceiling on client-suggested lifetimes — 6.4.3.2-1: "the actual expiresAt
/// time of the EntityMap shall be set by the Context Broker or Context
/// Source, possibly overriding the requested duration".
pub(crate) const MAX_LIFETIME_SECS: i64 = 86_400;

pub(crate) fn dt(s: &str) -> Option<chrono::DateTime<chrono::Utc>> {
    chrono::DateTime::parse_from_rfc3339(s)
        .ok()
        .map(|d| d.with_timezone(&chrono::Utc))
}

/// Fetch a live EntityMap; an expired one "cannot be accessed" (5.5.14) and
/// is pruned on touch. Maps live in the store (Kind::EntityMap) so
/// persistent modes survive restarts.
pub(crate) fn map_get(
    st: &AppState,
    tenant: &TenantId,
    id: &str,
) -> Result<Option<Value>, NgsiError> {
    let Some(doc) = st.store.get(tenant, Kind::EntityMap, id)? else {
        return Ok(None);
    };
    // 5.5.14 is a positive condition: a map is served only while a READABLE
    // expiry is still in the future. Judging "expired" instead lets a map
    // whose expiresAt is missing or unparseable outlive every ceiling.
    let live = doc
        .get("expiresAt")
        .and_then(Value::as_str)
        .and_then(dt)
        .is_some_and(|e| e > chrono::Utc::now());
    if !live {
        // Pruning is opportunistic: the map is unusable either way, and the
        // expiry sweep collects what a refused delete leaves behind.
        let _ = st.store.delete(tenant, Kind::EntityMap, id);
        return Ok(None);
    }
    Ok(Some(doc))
}

/// The map a consumption request named, or nothing.
///
/// 5.5.14: "If an EntityMap has expired, or cannot be accessed, no inference
/// can be made as to which entities are held within the Context Sources and a
/// new one shall be created." A store that refuses the read is one way a map
/// cannot be accessed, so these paths recover the way they recover from an
/// expiry — with a new map — instead of failing the request.
pub(crate) fn map_if_accessible(st: &AppState, tenant: &TenantId, id: &str) -> Option<Value> {
    map_get(st, tenant, id).ok().flatten()
}

/// The Entities of a map a given request may be answered from.
///
/// 5.5.9.3: "the set of Entities considered for the result is fixed with the
/// initial query creating the Entity map." The map is the CANDIDATE set; the
/// request's own filters still apply on top of it, so a request that names
/// `id=` is asking for the intersection and never for the map's whole set.
/// Returned in the map's own order, which is the pagination order.
pub(crate) fn candidate_ids(map: &Value, params: &HashMap<String, String>) -> Vec<String> {
    let named: Option<std::collections::HashSet<&str>> =
        params.get("id").map(|s| s.split(',').collect());
    map["entityMap"]
        .as_object()
        .map(|o| {
            o.keys()
                .filter(|k| named.as_ref().is_none_or(|n| n.contains(k.as_str())))
                .cloned()
                .collect()
        })
        .unwrap_or_default()
}

///
/// Every store failure here is the caller's: a map the broker could not count
/// against its ceiling, or could not write, is not a map the client can be
/// handed the id of (Table 6.3.2-1 InternalError).
pub(crate) fn map_put(st: &AppState, tenant: &TenantId, mut doc: Value) -> Result<(), NgsiError> {
    let Some(id) = doc.get("id").and_then(Value::as_str).map(str::to_owned) else {
        return Ok(());
    };
    // 6.4.3.2-1: "the actual expiresAt time of the EntityMap shall be set by
    // the Context Broker or Context Source, possibly overriding the requested
    // duration" — the 5.14.2.4 update path carries a client-chosen instant, so
    // the ceiling binds here, at the one point every writer goes through. An
    // absent or unreadable expiry is left alone: 5.5.14 keeps it unusable.
    let ceiling = chrono::Utc::now() + chrono::Duration::seconds(MAX_LIFETIME_SECS);
    if doc
        .get("expiresAt")
        .and_then(Value::as_str)
        .and_then(dt)
        .is_some_and(|e| e > ceiling)
    {
        doc["expiresAt"] = json!(ceiling.to_rfc3339_opts(chrono::SecondsFormat::Millis, true));
    }
    // The ceiling decides whether a NEW map fits; rewriting a map that is
    // already stored replaces a row that is already counted, so the paging
    // path neither lists the tenant's maps nor evicts one per page. A count
    // the store refuses leaves the ceiling unenforceable, and an unbounded
    // buffer is not the safer half of that choice: the write is refused.
    if st.store.get(tenant, Kind::EntityMap, &id)?.is_none() {
        let existing = st.store.list(tenant, Kind::EntityMap)?;
        if existing.len() >= MAX_MAPS_PER_TENANT {
            // eviction order is a heuristic — earliest expiresAt string wins
            if let Some(victim) = existing
                .iter()
                .min_by(|a, b| {
                    a["expiresAt"]
                        .as_str()
                        .unwrap_or("")
                        .cmp(b["expiresAt"].as_str().unwrap_or(""))
                })
                .and_then(|d| d.get("id").and_then(Value::as_str))
            {
                st.store.delete(tenant, Kind::EntityMap, victim)?;
            }
        }
    }
    let updated = st
        .store
        .mutate(tenant, Kind::EntityMap, &id, |d| {
            *d = doc.clone();
            Ok::<_, std::convert::Infallible>(())
        })?
        .is_some();
    if !updated {
        st.store.create(tenant, Kind::EntityMap, &id, doc)?;
    }
    Ok(())
}

/// 5.14.3.4: "If the NGSI-LD endpoint does not know about a matching EntityMap
/// for the EntityMap ID, then an error of type ResourceNotFound shall be
/// raised." What the endpoint knows about is what [`map_get`] serves, so the
/// delete reads through it: an expired map is beyond access (5.5.14) for the
/// retrieve and for the delete alike, and `map_get` prunes the row on the way
/// past, which leaves nothing for a later sweep to collect.
pub(crate) fn map_delete(st: &AppState, tenant: &TenantId, id: &str) -> Result<bool, NgsiError> {
    if map_get(st, tenant, id)?.is_none() {
        return Ok(false);
    }
    st.store.delete(tenant, Kind::EntityMap, id)
}

/// Parse an ISO 8601 duration (entityMapLifetime, Table 6.4.3.2-1) to whole
/// seconds; years/months are approximated (365/30 days), fractions rejected.
pub(crate) fn iso8601_secs(s: &str) -> Option<i64> {
    let rest = s.strip_prefix('P')?;
    if rest.is_empty() {
        return None;
    }
    let (date, time) = match rest.split_once('T') {
        Some((d, t)) => (d, Some(t)),
        None => (rest, None),
    };
    let mut secs: i64 = 0;
    let mut num = String::new();
    for c in date.chars() {
        if c.is_ascii_digit() {
            num.push(c);
        } else {
            let n: i64 = num.parse().ok()?;
            num.clear();
            secs = secs.checked_add(n.checked_mul(match c {
                'Y' => 31_536_000,
                'M' => 2_592_000,
                'W' => 604_800,
                'D' => 86_400,
                _ => return None,
            })?)?;
        }
    }
    if !num.is_empty() {
        return None; // trailing digits without a designator
    }
    if let Some(t) = time {
        if t.is_empty() {
            return None;
        }
        for c in t.chars() {
            if c.is_ascii_digit() {
                num.push(c);
            } else {
                let n: i64 = num.parse().ok()?;
                num.clear();
                secs = secs.checked_add(n.checked_mul(match c {
                    'H' => 3600,
                    'M' => 60,
                    'S' => 1,
                    _ => return None,
                })?)?;
            }
        }
        if !num.is_empty() {
            return None;
        }
    }
    Some(secs)
}

/// The expiresAt the broker assigns (5.2.39): now + suggested lifetime,
/// bounded by the broker's ceiling; the default applies when none is given.
pub(crate) fn expires_at(params: &HashMap<String, String>) -> Result<String, NgsiError> {
    let secs = match params.get("entityMapLifetime") {
        Some(d) => iso8601_secs(d)
            .ok_or_else(|| {
                NgsiError::BadRequestData(format!(
                    "entityMapLifetime is not an ISO 8601 duration: {d:?}"
                ))
            })?
            // A zero or negative suggestion would answer 201 with a map that
            // 5.5.14 already forbids anyone from accessing, so the broker
            // floor applies as well as the ceiling.
            .clamp(1, MAX_LIFETIME_SECS),
        None => DEFAULT_LIFETIME_SECS,
    };
    Ok((chrono::Utc::now() + chrono::Duration::seconds(secs))
        .to_rfc3339_opts(chrono::SecondsFormat::Millis, true))
}

/// 5.14.1.4 / 5.14.2.4 / 5.14.3.4: every /entityMaps/{id} method opens the
/// same way — the tenant it runs in, `local` (6.3.18) as the only parameter
/// this resource takes, and an id that must be a valid URI before the store
/// is touched at all.
pub(crate) fn open_map(
    params: &HashMap<String, String>,
    headers: &HeaderMap,
    id: &str,
) -> ApiResult<TenantId> {
    let tenant = tenant_from(headers)?;
    check_params(params, &["local"])?;
    map_id_check(id)?;
    Ok(tenant)
}

/// 5.14.1.4 / 5.14.3.4: "If the EntityMap id is not present or it is not a
/// valid URI, then an error of type BadRequestData shall be raised."
pub(crate) fn map_id_check(id: &str) -> Result<(), NgsiError> {
    antares_model::EntityId::new(id)
        .map(|_| ())
        .map_err(|_| NgsiError::BadRequestData(format!("EntityMap id is not a valid URI: {id:?}")))
}

/// 5.14.4.4 and 5.14.5.4 end the same way: "The mapping between the Context
/// Source Registration and the EntityMap Id is added to the linkedMaps
/// element of the local EntityMap and for the Entity ids included in the
/// returned Entity Maps a mapping to the Context Source Registration is added
/// to the entityMap element of the local EntityMap. The local EntityMap is
/// stored and made accessible based on its identifier." The two operations
/// differ only in which registration operation they ask for and which peer
/// resource carries it.
pub(crate) async fn merge_and_store_map(
    st: &AppState,
    tenant: &TenantId,
    headers: &HeaderMap,
    ctx: &antares_jsonld::Context,
    params: &HashMap<String, String>,
    temporal: bool,
    mut emap: Map<String, Value>,
) -> ApiResult<Value> {
    let (op, path) = if temporal {
        ("createEntityMapQueryTemporal", "temporal/entityMaps")
    } else {
        ("createEntityMapQueryEntity", "entityMaps")
    };
    let mut linked = Map::new();
    // 5.5.13 local=true: no Context Source Registration is considered, so
    // nothing merges in and linkedMaps stays empty.
    if params.get("local").map(String::as_str) != Some("true") {
        let split = params.get("splitEntities").map(String::as_str) == Some("true");
        for (reg_id, remote) in
            crate::federation::fed_entity_maps(st, tenant, headers, ctx, params, split, op, path)
                .await?
        {
            if let Some(obj) = remote.get("entityMap").and_then(Value::as_object) {
                for eid in obj.keys() {
                    if let Some(a) = emap
                        .entry(eid.clone())
                        .or_insert_with(|| json!([]))
                        .as_array_mut()
                    {
                        a.push(json!(reg_id.clone()));
                    }
                }
            }
            if let Some(mid) = remote.get("id").and_then(Value::as_str) {
                linked.insert(reg_id, json!(mid));
            }
        }
    }
    let doc = json!({
        "id": format!("urn:ngsi-ld:entitymap:{}", uuid::Uuid::new_v4()),
        "type": "EntityMap",
        "expiresAt": expires_at(params)?,
        "entityMap": Value::Object(emap),
        "linkedMaps": Value::Object(linked),
    });
    map_put(st, tenant, doc.clone())?;
    Ok(doc)
}

/// 5.7.1.4 / 5.7.3.4: the EntityMap created for a single-Entity retrieve —
/// its one entry lists "@none" when Attribute data is held locally plus
/// every matching Context Source Registration supporting the retrieve
/// operation ("only the retrieved Entity Map shall be used to determine
/// which Context Source Registrations match the Entity ID").
#[allow(clippy::too_many_arguments)] // one param per 5.7.1.4 input
pub(crate) fn build_retrieve_map(
    st: &AppState,
    tenant: &TenantId,
    ctx: &antares_jsonld::Context,
    headers: &HeaderMap,
    id: &str,
    params: &HashMap<String, String>,
    temporal: bool,
    local_held: bool,
) -> Result<Value, NgsiError> {
    let mut srcs: Vec<Value> = Vec::new();
    if local_held {
        srcs.push(json!("@none"));
    }
    if crate::federation::active(params) {
        let spec = crate::registry::CsrSpec {
            ids: Some(vec![id.to_owned()]),
            ..Default::default()
        };
        for reg in crate::federation::matching_regs(st, tenant, &spec, ctx, headers)? {
            let ok = if temporal {
                reg.supports("retrieveTemporal")
            } else {
                reg.read_op().is_some()
            };
            if ok {
                srcs.push(json!(reg.reg_id));
            }
        }
    }
    let mut emap = Map::new();
    if !srcs.is_empty() {
        emap.insert(id.to_owned(), Value::Array(srcs));
    }
    let doc = json!({
        "id": format!("urn:ngsi-ld:entitymap:{}", uuid::Uuid::new_v4()),
        "type": "EntityMap",
        "expiresAt": expires_at(params)?,
        "entityMap": Value::Object(emap),
        "linkedMaps": {},
    });
    map_put(st, tenant, doc.clone())?;
    Ok(doc)
}

/// The NGSILD-EntityMap response header: the resource URI of the map that
/// determined the sources of this response (6.3.17).
pub(crate) fn set_map_header(resp: &mut Response, mid: &str) {
    if let Ok(v) = format!("/ngsi-ld/v1/entityMaps/{mid}").parse() {
        resp.headers_mut().insert("NGSILD-EntityMap", v);
    }
}

/// 5.7.1.4 / 5.7.3.4 EntityMap usage on a single-Entity retrieve: a supplied
/// NGSILD-EntityMap location is retrieved and, if live, is the only source
/// used to determine which registrations match; an unknown or expired
/// reference — or the `entityMap=true` flag — creates a new map, whose
/// location is returned in the NGSILD-EntityMap response header. The two
/// clauses word that rule identically and part company only over what
/// "held locally" reads and which operation a registration must support,
/// and both of those follow from `temporal` — so the rule is read once
/// here and the retrieve itself is the caller's `inner`.
pub(crate) async fn retrieve_with_map<F, Fut>(
    st: &AppState,
    id: &str,
    params: &HashMap<String, String>,
    headers: &HeaderMap,
    temporal: bool,
    inner: F,
) -> ApiResult<Response>
where
    F: Fn(Option<Value>) -> Fut,
    Fut: std::future::Future<Output = ApiResult<Response>>,
{
    let tenant = tenant_from(headers)?;
    let map_ref = single_header(headers, "NGSILD-EntityMap")?
        .map(|r| r.rsplit('/').next().unwrap_or(&r).to_owned());
    if let Some(map) = map_ref
        .as_deref()
        .and_then(|mid| map_if_accessible(st, &tenant, mid))
    {
        let mut resp = inner(Some(map)).await?;
        set_map_header(&mut resp, &map_ref.unwrap_or_default());
        return Ok(resp);
    }
    let want_map = map_ref.is_some() || params.get("entityMap").map(String::as_str) == Some("true");
    let mut resp = inner(None).await?;
    if want_map && resp.status().is_success() {
        let ctx = request_context(&st.loader, headers).await?;
        let local_held = if temporal {
            st.temporal
                .get_temporal(
                    &tenant,
                    id,
                    &antares_store::filter::TemporalFilter::default(),
                )?
                .is_some()
        } else {
            st.store.get(&tenant, Kind::Entity, id)?.is_some()
        };
        let map = build_retrieve_map(st, &tenant, &ctx, headers, id, params, temporal, local_held)?;
        if let Some(mid) = map.get("id").and_then(Value::as_str) {
            set_map_header(&mut resp, mid);
        }
    }
    Ok(resp)
}

/// 201 + the EntityMap body + the NGSILD-EntityMap header carrying the
/// resource URI of the created map (6.34.3.1 / 6.35.3.1).
pub(crate) fn created_response(
    doc: Value,
    ctx: &antares_jsonld::Context,
    accept: Accept,
    tenant: &TenantId,
) -> Response {
    let uri = format!(
        "/ngsi-ld/v1/entityMaps/{}",
        doc.get("id").and_then(Value::as_str).unwrap_or_default()
    );
    let mut resp = respond(StatusCode::CREATED, doc, ctx, accept, tenant);
    if let Ok(v) = uri.parse() {
        resp.headers_mut().insert("NGSILD-EntityMap", v);
    }
    resp
}

#[cfg(test)]
mod tests {
    use super::*;
    use antares_store::Kind;
    use serde_json::json;

    /// `map_get` with its store failure unwrapped: these tests drive a
    /// working store, where a refusal would be the test's own bug.
    fn map_read(st: &AppState, tenant: &TenantId, id: &str) -> Option<Value> {
        map_get(st, tenant, id).expect("the store answers")
    }

    /// Table 6.4.3.2-1: entityMapLifetime is an ISO 8601 duration.
    #[test]
    fn clause_5_14_4_lifetime_parse() {
        assert_eq!(iso8601_secs("PT1H"), Some(3600));
        assert_eq!(iso8601_secs("PT90S"), Some(90));
        assert_eq!(iso8601_secs("P1DT2H3M4S"), Some(93784));
        assert_eq!(iso8601_secs("P2W"), Some(1_209_600));
        // invalid shapes are rejected (→ 400 at the handler)
        for bad in ["", "P", "PT", "1H", "PT1X", "PT1.5S", "PT1"] {
            assert_eq!(iso8601_secs(bad), None, "{bad:?}");
        }
    }

    /// Table 6.4.3.2-1: entityMapLifetime arrives on the query string, so
    /// the parser is attacker-facing — every hostile shape must return None
    /// (a 400) rather than panic, wrap or saturate.
    #[test]
    fn clause_5_14_4_lifetime_hostile_inputs() {
        for bad in [
            "P-1D",                    // negative component
            "-P1D",                    // negative duration
            "P+1D",                    // signed component
            "p1d",                     // lower case designators
            " PT1H",                   // leading whitespace
            "PT1H ",                   // trailing whitespace
            "P1DT",                    // empty time part
            "PT99999999999999999999S", // digit run past i64
            "P9999999999999Y",         // multiplication overflow
            "P92233720368547758S",     // addition overflow after scaling
            "PT1H1",                   // trailing digits, no designator
            "P١D",                     // non-ASCII digit
            "P1D\u{0}",                // embedded NUL
            "PT,5S",                   // comma fraction
            "P1S",                     // time designator in the date part
            "PT1D",                    // date designator in the time part
        ] {
            assert_eq!(iso8601_secs(bad), None, "{bad:?} must not parse");
        }
        // the whole i64 range is walked without panicking
        assert_eq!(iso8601_secs(&format!("PT{}S", i64::MAX)), Some(i64::MAX));
        assert_eq!(iso8601_secs(&format!("PT{}S", u64::MAX)), None);
        assert_eq!(iso8601_secs("PT0S"), Some(0));
    }

    fn params(pairs: &[(&str, &str)]) -> HashMap<String, String> {
        pairs
            .iter()
            .map(|(k, v)| ((*k).to_owned(), (*v).to_owned()))
            .collect()
    }

    /// 6.4.3.2-1: "the actual expiresAt time of the EntityMap shall be set by
    /// the Context Broker or Context Source, possibly overriding the
    /// requested duration" — the client suggestion is bounded above by the
    /// broker ceiling and below by a lifetime the map can actually be used
    /// for; an unparseable duration is BadRequestData.
    #[test]
    fn clause_5_14_4_expires_at_is_broker_bounded() {
        let now = chrono::Utc::now();
        let at = |p: &[(&str, &str)]| {
            let s = expires_at(&params(p)).expect("expiry");
            dt(&s).expect("RFC 3339 expiry")
        };
        let default = at(&[]);
        assert!(
            (default - now).num_seconds() >= DEFAULT_LIFETIME_SECS - 5
                && (default - now).num_seconds() <= DEFAULT_LIFETIME_SECS + 5,
            "no suggestion → the default lifetime"
        );
        let capped = at(&[("entityMapLifetime", "P30D")]);
        assert!(
            (capped - now).num_seconds() <= MAX_LIFETIME_SECS,
            "a client cannot exceed the broker ceiling"
        );
        let zero = at(&[("entityMapLifetime", "PT0S")]);
        assert!(
            zero > now,
            "a zero lifetime would return 201 for a map that is already \
             unusable (5.5.14): {zero}"
        );
        match expires_at(&params(&[("entityMapLifetime", "yesterday")])) {
            Err(NgsiError::BadRequestData(_)) => {}
            other => panic!("an invalid duration must be BadRequestData: {other:?}"),
        }
    }

    /// 5.14: EntityMaps are per-tenant resources — an EntityMap created
    /// under one tenant is invisible and undeletable from another (4.14
    /// multi-tenancy: "an NGSI-LD system shall behave as if the tenants were
    /// separate systems").
    #[test]
    fn clause_5_14_maps_are_tenant_scoped() {
        let st = AppState::new("antares-em-unit".into());
        let a = TenantId::new("alpha").expect("tenant");
        let b = TenantId::new("beta").expect("tenant");
        let id = "urn:ngsi-ld:entitymap:t1";
        map_put(&st, &a, live_map(id)).expect("stored");
        assert!(map_read(&st, &a, id).is_some());
        assert!(
            map_read(&st, &b, id).is_none(),
            "another tenant must not read the map"
        );
        assert!(
            !map_delete(&st, &b, id).expect("delete"),
            "another tenant must not delete the map"
        );
        assert!(
            map_read(&st, &a, id).is_some(),
            "the owner still has its map"
        );
        assert!(map_delete(&st, &a, id).expect("delete"));
        assert!(map_read(&st, &a, id).is_none());
    }

    fn live_map(id: &str) -> Value {
        json!({
            "id": id,
            "type": "EntityMap",
            "expiresAt": (chrono::Utc::now() + chrono::Duration::seconds(600))
                .to_rfc3339_opts(chrono::SecondsFormat::Millis, true),
            "entityMap": {},
            "linkedMaps": {},
        })
    }

    /// 5.5.14: an expired EntityMap "cannot be accessed" — it is never
    /// served, and a map whose expiry cannot be read is treated the same way
    /// rather than living forever.
    #[test]
    fn clause_5_5_14_expired_maps_are_never_served() {
        let st = AppState::new("antares-em-exp".into());
        let t = TenantId::default();
        let mut past = live_map("urn:ngsi-ld:entitymap:past");
        past["expiresAt"] = json!("2020-01-01T00:00:00.000Z");
        map_put(&st, &t, past).expect("stored");
        assert!(map_read(&st, &t, "urn:ngsi-ld:entitymap:past").is_none());
        assert!(
            st.store
                .get(&t, Kind::EntityMap, "urn:ngsi-ld:entitymap:past")
                .expect("store")
                .is_none(),
            "an expired map is pruned on touch"
        );
        for (id, expiry) in [
            ("urn:ngsi-ld:entitymap:none", None),
            ("urn:ngsi-ld:entitymap:junk", Some(json!("whenever"))),
            ("urn:ngsi-ld:entitymap:num", Some(json!(0))),
        ] {
            let mut doc = live_map(id);
            match expiry {
                Some(v) => doc["expiresAt"] = v,
                None => {
                    doc.as_object_mut().expect("object").remove("expiresAt");
                }
            }
            map_put(&st, &t, doc).expect("stored");
            assert!(
                map_read(&st, &t, id).is_none(),
                "{id} has no readable expiry and must not be served"
            );
        }
    }

    /// 5.14.1.1 storage: every buffer is bounded — the per-tenant EntityMap
    /// registry has a ceiling, and filling it evicts rather than growing.
    #[test]
    fn clause_5_14_1_map_registry_is_bounded() {
        let st = AppState::new("antares-em-cap".into());
        let t = TenantId::default();
        for i in 0..MAX_MAPS_PER_TENANT + 8 {
            let mut doc = live_map(&format!("urn:ngsi-ld:entitymap:{i:04}"));
            // earliest expiry first, so the eviction victim is deterministic
            doc["expiresAt"] = json!(format!("2099-01-01T00:00:{:02}.000Z", i % 60));
            map_put(&st, &t, doc).expect("stored");
            assert!(
                st.store.list(&t, Kind::EntityMap).expect("list").len() <= MAX_MAPS_PER_TENANT,
                "the registry exceeded its ceiling at {i}"
            );
        }
        // re-storing a known id is an update, never an eviction
        let before = st.store.list(&t, Kind::EntityMap).expect("list").len();
        let known = st.store.list(&t, Kind::EntityMap).expect("list")[0]["id"]
            .as_str()
            .expect("id")
            .to_owned();
        map_put(&st, &t, live_map(&known)).expect("stored");
        assert_eq!(
            st.store.list(&t, Kind::EntityMap).expect("list").len(),
            before
        );
    }

    /// 5.5.14 + Table 6.4.3.2-1: "the actual expiresAt time of the EntityMap
    /// shall be set by the Context Broker or Context Source, possibly
    /// overriding the requested duration" — the 5.14.2.4 update path writes a
    /// client-chosen instant, so the broker ceiling binds when the map is
    /// stored, not only when it is created.
    #[test]
    fn clause_5_5_14_stored_expiry_never_exceeds_the_broker_ceiling() {
        let st = AppState::new("antares-em-clamp".into());
        let t = TenantId::default();
        let ceiling = chrono::Utc::now() + chrono::Duration::seconds(MAX_LIFETIME_SECS);
        let far = "urn:ngsi-ld:entitymap:far";
        let mut doc = live_map(far);
        doc["expiresAt"] = json!("2099-01-01T00:00:00.000Z");
        map_put(&st, &t, doc).expect("stored");
        let stored = map_read(&st, &t, far).expect("a clamped map is still live");
        let at = dt(stored["expiresAt"].as_str().expect("expiresAt")).expect("RFC 3339 expiry");
        assert!(
            at <= ceiling + chrono::Duration::seconds(5),
            "a client cannot pin an EntityMap past the broker ceiling: {at}"
        );
        assert!(at > chrono::Utc::now(), "the map stays usable: {at}");
        // an expiry inside the ceiling is stored verbatim, not rewritten
        let near = "urn:ngsi-ld:entitymap:near";
        let doc = live_map(near);
        let want = doc["expiresAt"].clone();
        map_put(&st, &t, doc).expect("stored");
        assert_eq!(map_read(&st, &t, near).expect("live")["expiresAt"], want);
    }
}
