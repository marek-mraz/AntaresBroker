//! /temporal/entities (5.6.11–5.6.16, 5.7.3/5.7.4; resources 6.18–6.22).

use crate::negotiate::*;
use crate::state::{now_iso, AppState};
use antares_jsonld::compact::compact_instance;
use antares_jsonld::{expand_entity, parse_datetime, Context, ExpandOpts};
use antares_model::{NgsiError, TenantId};
use antares_ql::parse_q;
use antares_sql::store::Kind;
use axum::body::Bytes;
use axum::extract::{Path, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use serde_json::{Map, Value};
use std::collections::HashMap;

use crate::negotiate::CleanParams;

fn is_meta(k: &str) -> bool {
    matches!(
        k,
        "id" | "type" | "scope" | "createdAt" | "modifiedAt" | "deletedAt" | "expiresAt"
    )
}

/// 5.6.11 input: the pushed Temporal Evolution may carry the 4.5.7
/// deleted-instance representation (value = NGSI-LD Null), which 5.5.4
/// explicitly excepts for "the temporal evolution" — hence allow_null.
const TEMPORAL_OPTS: ExpandOpts = ExpandOpts {
    fragment: false,
    allow_null: true,
    merge: false,
    temporal: true,
    sys: false,
};

fn stamp_instances(doc: &mut Value, ts: &str) {
    if let Some(obj) = doc.as_object_mut() {
        for (k, v) in obj.iter_mut() {
            if is_meta(k) {
                continue;
            }
            if let Some(arr) = v.as_array_mut() {
                for inst in arr {
                    if let Some(o) = inst.as_object_mut() {
                        o.entry("instanceId".to_owned()).or_insert_with(|| {
                            Value::String(format!("urn:ngsi-ld:Instance:{}", uuid::Uuid::new_v4()))
                        });
                        o.insert("createdAt".into(), Value::String(ts.to_owned()));
                        o.insert("modifiedAt".into(), Value::String(ts.to_owned()));
                    }
                }
            }
        }
    }
}

// ---------- POST /temporal/entities/ — Upsert temporal (5.6.11) ----------

pub async fn upsert_temporal(
    State(st): State<AppState>,
    CleanParams(params): CleanParams,
    headers: HeaderMap,
    body: Bytes,
) -> Response {
    let go = async {
        let tenant = tenant_from(&headers)?;
        check_params(&params, &["options", "local"])?;
        let parsed = parse_body(&st.loader, &headers, &body, BodyKind::Standard).await?;
        let obj = parsed.value.as_object().ok_or_else(|| {
            NgsiError::BadRequestData("temporal entity must be a JSON object".into())
        })?;
        let expanded = expand_entity(obj, &parsed.ctx, TEMPORAL_OPTS)?;
        let id = expanded["id"].as_str().expect("validated").to_owned();
        // 5.6.11.4: exclusive/redirect registrations matching the input are
        // forwarded when "Create or Update Temporal" is supported; proxy
        // modes without it are an error of type Conflict; inclusive ones
        // forward when supported. Matching attributes are removed from the
        // local fragment.
        let spec = crate::csource::CsrSpec {
            ids: Some(vec![id.clone()]),
            ..Default::default()
        };
        let mut regs =
            crate::federation::write_regs(&st, &tenant, &spec, &parsed.ctx, &params, &headers);
        if let Some(r) = crate::federation::handle_via_loop(
            &headers,
            &crate::federation::alias_for(&st.host_alias, &tenant),
            &tenant,
            &mut regs,
        ) {
            return Ok(r);
        }
        if !regs.is_empty() {
            let mut parts = Vec::new();
            let mut fwd = Vec::new();
            for reg in &regs {
                if !reg.supports("upsertTemporal") {
                    if reg.is_proxy() {
                        parts.push(crate::federation::conflict_part("upsertTemporal"));
                    }
                    continue;
                }
                if let Some(frag) = crate::federation::reduce_to_scope(obj, reg, &parsed.ctx) {
                    fwd.push((reg.clone(), frag));
                }
            }
            let proxies: Vec<&crate::federation::FedReg> =
                regs.iter().filter(|r| r.is_proxy()).collect();
            let (rest, has_attrs) = crate::federation::strip_proxied(obj, &proxies, &parsed.ctx);
            if has_attrs || proxies.is_empty() {
                let local = expand_entity(&rest, &parsed.ctx, TEMPORAL_OPTS)?;
                let status = upsert_temporal_local(&st, &tenant, &id, local)?;
                parts.push(crate::federation::Part {
                    status: status.as_u16(),
                    detail: "local temporal upsert".into(),
                });
            }
            let ctx_url = crate::federation::ctx_link_url(&headers, &parsed.ctx.source);
            for (reg, frag) in fwd {
                parts.push(
                    crate::federation::forward_part(
                        &st,
                        reqwest::Method::POST,
                        format!("{}/ngsi-ld/v1/temporal/entities", reg.endpoint),
                        &[],
                        &headers,
                        &tenant,
                        &reg,
                        &ctx_url,
                        Some(frag),
                    )
                    .await,
                );
            }
            return Ok(crate::federation::combine(
                parts,
                created(format!("/ngsi-ld/v1/temporal/entities/{id}"), &tenant),
                &tenant,
            ));
        }
        let status = upsert_temporal_local(&st, &tenant, &id, expanded)?;
        Ok::<_, ApiError>(if status == StatusCode::CREATED {
            created(format!("/ngsi-ld/v1/temporal/entities/{id}"), &tenant)
        } else {
            no_content(&tenant)
        })
    };
    go.await.unwrap_or_else(|e| e.into_response())
}

/// 5.6.11.4 local half: create the Temporal Evolution, or add the provided
/// instances to the existing one per 5.6.12 (merge key = datasetId +
/// observedAt) with Entity Type names unioned. Returns 201 vs 204.
fn upsert_temporal_local(
    st: &AppState,
    tenant: &antares_model::TenantId,
    id: &str,
    mut expanded: Value,
) -> ApiResult<StatusCode> {
    let ts = now_iso();
    stamp_instances(&mut expanded, &ts);
    // get->create/mutate is a TOCTOU pair: two concurrent first-upserts
    // both see "absent", and the loser's create must NOT be silently
    // dropped (201 with a discarded payload). Loop: a lost create retries
    // as a merge, a mutate on a just-deleted doc retries as a create.
    let mut attempts = 0;
    loop {
        attempts += 1;
        if attempts > 16 {
            return Err(NgsiError::InternalError("upsert retry storm".into()).into());
        }
        let existed = st.store.get(tenant, Kind::Temporal, id)?.is_some();
        if existed {
            let res = st.store.mutate(tenant, Kind::Temporal, id, |doc| {
                let target = doc.as_object_mut().expect("temporal object");
                // 5.6.11.4: new Entity Type names are added to the target
                if let Some(new_types) = expanded.get("type").and_then(Value::as_array) {
                    let mut cur: Vec<Value> = target
                        .get("type")
                        .and_then(Value::as_array)
                        .cloned()
                        .unwrap_or_default();
                    for t in new_types {
                        if !cur.contains(t) {
                            cur.push(t.clone());
                        }
                    }
                    target.insert("type".into(), Value::Array(cur));
                }
                for (k, v) in expanded.as_object().expect("expanded") {
                    if is_meta(k) {
                        continue;
                    }
                    let incoming = v.as_array().cloned().unwrap_or_default();
                    match target.get_mut(k).and_then(Value::as_array_mut) {
                        Some(cur) => {
                            // 5.6.11: instances merge by (datasetId, observedAt)
                            for ni in incoming {
                                let key = (
                                    ni.get("datasetId")
                                        .and_then(Value::as_str)
                                        .map(String::from),
                                    ni.get("observedAt")
                                        .and_then(Value::as_str)
                                        .map(String::from),
                                );
                                let pos = cur.iter().position(|ci| {
                                    (
                                        ci.get("datasetId")
                                            .and_then(Value::as_str)
                                            .map(String::from),
                                        ci.get("observedAt")
                                            .and_then(Value::as_str)
                                            .map(String::from),
                                    ) == key
                                        && key.1.is_some()
                                });
                                match pos {
                                    Some(p) => cur[p] = ni,
                                    None => cur.push(ni),
                                }
                            }
                        }
                        None => {
                            target.insert(k.clone(), Value::Array(incoming));
                        }
                    }
                }
                target.insert("modifiedAt".into(), Value::String(ts.clone()));
                Ok::<(), NgsiError>(())
            })?;
            match res {
                Some(Err(e)) => return Err(ApiError::from(e)),
                Some(Ok(())) => return Ok(StatusCode::NO_CONTENT),
                None => continue, // deleted between get and mutate - retry as create
            }
        } else {
            let mut doc = expanded.clone();
            if let Some(o) = doc.as_object_mut() {
                o.insert("createdAt".into(), Value::String(ts.clone()));
                o.insert("modifiedAt".into(), Value::String(ts.clone()));
            }
            if st.store.create(tenant, Kind::Temporal, id, doc)? {
                return Ok(StatusCode::CREATED);
            }
            // lost the create race - the doc exists now; retry as a merge
        }
    }
}

// ---------- temporal query params (4.11) ----------

pub struct TemporalQ {
    pub timerel: String,
    pub time_at: String,
    pub end_time_at: Option<String>,
    pub timeproperty: String,
}

impl TemporalQ {
    pub fn from_params(
        params: &HashMap<String, String>,
        required: bool,
    ) -> Result<Option<Self>, NgsiError> {
        let bad = |m: String| NgsiError::BadRequestData(m);
        let Some(timerel) = params.get("timerel") else {
            if required {
                return Err(bad("temporal query requires timerel (5.7.4)".into()));
            }
            if params.contains_key("timeAt") || params.contains_key("endTimeAt") {
                return Err(bad("timeAt given without timerel".into()));
            }
            // bare timeproperty: representation keyed on it; instances that
            // lack it are excluded (retrieval-by-deletedAt, 020_17/18)
            if let Some(tp) = params.get("timeproperty") {
                if !["observedAt", "createdAt", "modifiedAt", "deletedAt"].contains(&tp.as_str()) {
                    return Err(bad(format!("invalid timeproperty {tp:?}")));
                }
                return Ok(Some(Self {
                    timerel: "any".into(),
                    time_at: String::new(),
                    end_time_at: None,
                    timeproperty: tp.clone(),
                }));
            }
            return Ok(None);
        };
        if !["before", "after", "between"].contains(&timerel.as_str()) {
            return Err(bad(format!("invalid timerel {timerel:?}")));
        }
        let time_at = params
            .get("timeAt")
            .filter(|s| parse_datetime(s))
            .ok_or_else(|| bad("timeAt must be a valid ISO 8601 DateTime (4.11)".into()))?
            .clone();
        let end_time_at = match params.get("endTimeAt") {
            Some(s) if parse_datetime(s) => Some(s.clone()),
            Some(_) => return Err(bad("endTimeAt must be a valid ISO 8601 DateTime".into())),
            None => None,
        };
        if timerel == "between" && end_time_at.is_none() {
            return Err(bad("timerel=between requires endTimeAt (4.11)".into()));
        }
        let timeproperty = params
            .get("timeproperty")
            .cloned()
            .unwrap_or_else(|| "observedAt".into());
        if !["observedAt", "createdAt", "modifiedAt", "deletedAt"].contains(&timeproperty.as_str())
        {
            return Err(bad(format!("invalid timeproperty {timeproperty:?}")));
        }
        Ok(Some(Self {
            timerel: timerel.clone(),
            time_at,
            end_time_at,
            timeproperty,
        }))
    }

    fn instance_matches(&self, inst: &Value) -> bool {
        let Some(t) = inst.get(&self.timeproperty).and_then(Value::as_str) else {
            return false;
        };
        // 4.11: before = exclusive bound, after = inclusive bound, between =
        // inclusive lower / exclusive upper. Compared on the canonical key so
        // equal instants written with different 4.6.3 fraction forms
        // ("…00Z" / "…00.000Z" / "…00,5Z") hit the bounds exactly.
        let t = dt_key(t);
        match self.timerel.as_str() {
            "any" => true, // bare timeproperty: presence is the filter
            "before" => t < dt_key(&self.time_at),
            "after" => t >= dt_key(&self.time_at),
            "between" => {
                t >= dt_key(&self.time_at)
                    && self.end_time_at.as_deref().is_some_and(|e| t < dt_key(e))
            }
            _ => false,
        }
    }
}

/// 5.2.21 TemporalQuery (JSON form): flatten the object's members into
/// query-param form, enforcing the Table 5.2.21-1 value spaces — the string
/// members must be JSON strings, aggrMethods a comma separated list of
/// string (string or string-array spelling), lastN a positive integer.
/// Vocabulary/range rules are then enforced by the shared param validators
/// (TemporalQ::from_params, parse_trepr).
pub(crate) fn temporal_q_params(
    tq: &Map<String, Value>,
    out: &mut HashMap<String, String>,
) -> Result<(), NgsiError> {
    let bad = NgsiError::BadRequestData;
    for k in [
        "timerel",
        "timeAt",
        "endTimeAt",
        "timeproperty",
        "aggrPeriodDuration",
    ] {
        match tq.get(k) {
            None => {}
            Some(Value::String(s)) => {
                out.insert(k.into(), s.clone());
            }
            Some(_) => return Err(bad(format!("temporalQ {k} must be a string (5.2.21)"))),
        }
    }
    if let Some(n) = tq.get("lastN") {
        let v = n
            .as_u64()
            .filter(|v| *v >= 1)
            .ok_or_else(|| bad("temporalQ lastN must be a positive integer (5.2.21)".into()))?;
        out.insert("lastN".into(), v.to_string());
    }
    match tq.get("aggrMethods") {
        None => {}
        Some(Value::String(s)) => {
            out.insert("aggrMethods".into(), s.clone());
        }
        Some(Value::Array(a)) => {
            let mut parts = Vec::with_capacity(a.len());
            for m in a {
                parts.push(m.as_str().ok_or_else(|| {
                    bad("temporalQ aggrMethods entries must be strings (5.2.21)".into())
                })?);
            }
            out.insert("aggrMethods".into(), parts.join(","));
        }
        Some(_) => {
            return Err(bad(
                "temporalQ aggrMethods must be a comma separated list of string (5.2.21)".into(),
            ))
        }
    }
    Ok(())
}

/// Canonical lexicographic comparison key for a 4.6.3 DateTime: the trailing
/// `Z` dropped and the optional seconds fraction (`.` or the request-side `,`
/// separator) zero-padded to six digits, so string order equals temporal
/// order across spellings of the same instant. Non-DateTime input is
/// returned as-is (callers validated at write/parse time).
pub(crate) fn dt_key(s: &str) -> String {
    let Some(body) = s.strip_suffix('Z') else {
        return s.to_owned();
    };
    if body.len() < 19 {
        return s.to_owned();
    }
    let (base, frac) = body.split_at(19);
    let digits = frac
        .strip_prefix('.')
        .or_else(|| frac.strip_prefix(','))
        .unwrap_or("");
    format!("{base}.{digits:0<6}")
}

/// Windowed per-entity temporal data: filtered+ordered instances per attr.
struct Windowed {
    attrs: std::collections::BTreeMap<String, Vec<Value>>,
    max_per_attr: usize,
    ts_min: Option<String>,
    ts_max: Option<String>,
}

/// NGSI-LD 6.3.10 only paginates ("206") when an attribute has "too many"
/// instances. The ETSI suite triggers 206 at 20 instances and expects 200 at
/// <=5 — any limit in (5,20) is spec-valid; 9 keeps margin (Scorpio parity).
const TEMPORAL_INSTANCE_LIMIT: usize = 9;

fn window(
    doc: &Value,
    tq: Option<&TemporalQ>,
    last_n: Option<usize>,
    attrs_filter: Option<&Vec<String>>,
    omit: Option<&Vec<crate::repr::ProjNode>>,
    dataset: Option<&Vec<String>>,
    timeprop: &str,
) -> Windowed {
    let mut w = Windowed {
        attrs: std::collections::BTreeMap::new(),
        max_per_attr: 0,
        ts_min: None,
        ts_max: None,
    };
    let Some(obj) = doc.as_object() else { return w };
    for (k, v) in obj {
        // 4.5.6: the Scope of a Temporal Evolution is represented as the
        // temporal representation of a Property — instance-shaped scope
        // arrays window like attributes (plain-string scope stays meta).
        let scope_instances = k == "scope"
            && v.as_array()
                .is_some_and(|a| a.first().is_some_and(Value::is_object));
        if is_meta(k) && !scope_instances {
            continue;
        }
        if let Some(want) = attrs_filter {
            if !want.contains(k) {
                continue;
            }
        }
        if let Some(omit) = omit {
            if omit.iter().any(|n| n.iri == *k && n.children.is_none()) {
                continue;
            }
        }
        let mut instances: Vec<Value> = v
            .as_array()
            .cloned()
            .unwrap_or_default()
            .into_iter()
            .filter(|inst| tq.is_none_or(|tq| tq.instance_matches(inst)))
            .filter(|inst| match (dataset, inst.get("datasetId")) {
                (None, _) => true,
                (Some(want), Some(Value::String(have))) => want.iter().any(|w| w == have),
                (Some(want), None) => want.iter().any(|w| w == "@none"),
                _ => false,
            })
            .collect();
        instances.sort_by(|a, b| {
            let ta = a.get(timeprop).and_then(Value::as_str).unwrap_or("");
            let tb = b.get(timeprop).and_then(Value::as_str).unwrap_or("");
            ta.cmp(tb)
        });
        if let Some(n) = last_n {
            if instances.len() > n {
                instances = instances.split_off(instances.len() - n);
            }
            // lastN delivers newest-first (DESC), Scorpio parity
            instances.reverse();
        }
        if instances.is_empty() {
            continue;
        }
        w.max_per_attr = w.max_per_attr.max(instances.len());
        for inst in &instances {
            if let Some(t) = inst.get(timeprop).and_then(Value::as_str) {
                if w.ts_min.as_deref().is_none_or(|m| t < m) {
                    w.ts_min = Some(t.to_owned());
                }
                if w.ts_max.as_deref().is_none_or(|m| t > m) {
                    w.ts_max = Some(t.to_owned());
                }
            }
        }
        w.attrs.insert(k.clone(), instances);
    }
    w
}

/// 6.3.10 attribute-gap cut (retrieve only): in the truncation regime, when
/// attributes occupy disjoint time ranges, keep the attribute whose range is
/// first in the query direction and empty the ones entirely beyond it.
fn gap_cut(w: &mut Windowed, timeprop: &str, descending: bool) {
    if w.max_per_attr <= TEMPORAL_INSTANCE_LIMIT || w.attrs.len() < 2 {
        return;
    }
    let mut ranges: Vec<(String, String, String)> = Vec::new(); // (attr, min, max)
    for (k, instances) in &w.attrs {
        let mut min: Option<&str> = None;
        let mut max: Option<&str> = None;
        for inst in instances {
            if let Some(t) = inst.get(timeprop).and_then(Value::as_str) {
                if min.is_none_or(|m| t < m) {
                    min = Some(t);
                }
                if max.is_none_or(|m| t > m) {
                    max = Some(t);
                }
            }
        }
        if let (Some(min), Some(max)) = (min, max) {
            ranges.push((k.clone(), min.to_owned(), max.to_owned()));
        }
    }
    if ranges.len() < 2 {
        return;
    }
    let keep = ranges
        .iter()
        .min_by(|a, b| {
            if descending {
                b.2.cmp(&a.2)
            } else {
                a.1.cmp(&b.1)
            }
        })
        .cloned()
        .expect("nonempty");
    let mut new_min: Option<String> = None;
    let mut new_max: Option<String> = None;
    for (attr, min, max) in &ranges {
        let cut = if descending {
            max < &keep.1
        } else {
            min > &keep.2
        };
        if *attr != keep.0 && cut {
            if let Some(list) = w.attrs.get_mut(attr) {
                list.clear();
            }
        } else {
            if new_min.as_deref().is_none_or(|m| min.as_str() < m) {
                new_min = Some(min.clone());
            }
            if new_max.as_deref().is_none_or(|m| max.as_str() > m) {
                new_max = Some(max.clone());
            }
        }
    }
    w.ts_min = new_min;
    w.ts_max = new_max;
}

/// Content-Range: date-time <start>-<end>/<size> (Scorpio-parity semantics).
fn content_range(
    max_per_attr: usize,
    ts_min: Option<&str>,
    ts_max: Option<&str>,
    tq: Option<&TemporalQ>,
    last_n: Option<usize>,
) -> Option<String> {
    if max_per_attr <= TEMPORAL_INSTANCE_LIMIT {
        return None;
    }
    let (data_min, data_max) = (ts_min?, ts_max?);
    let timerel = tq.map(|t| t.timerel.as_str()).filter(|t| *t != "any");
    let (start, end) = if last_n.is_none() {
        let start = match timerel {
            Some("after") | Some("between") => tq.expect("tq").time_at.clone(),
            _ => data_min.to_owned(),
        };
        (start, data_max.to_owned())
    } else {
        let start = match timerel {
            Some("before") => tq.expect("tq").time_at.clone(),
            Some("between") => tq
                .expect("tq")
                .end_time_at
                .clone()
                .unwrap_or_else(|| data_max.to_owned()),
            _ => data_max.to_owned(),
        };
        (start, data_min.to_owned())
    };
    let size = last_n.map_or_else(|| "*".to_owned(), |n| n.to_string());
    Some(format!("date-time {start}-{end}/{size}"))
}

/// Render one temporal entity from its windowed data.
fn present_temporal(
    doc: &Value,
    w: &Windowed,
    ctx: &Context,
    r: &TRepr,
    tq: Option<&TemporalQ>,
    timeprop: &str,
) -> Result<Value, NgsiError> {
    let Some(obj) = doc.as_object() else {
        return Ok(doc.clone());
    };
    let mut out = Map::new();
    for (k, v) in obj {
        let scope_instances = k == "scope"
            && v.as_array()
                .is_some_and(|a| a.first().is_some_and(Value::is_object));
        if is_meta(k) && !scope_instances {
            match k.as_str() {
                "createdAt" | "modifiedAt" if !r.sys => continue,
                _ => {}
            }
            // pick/omit constrain core members too (4.21)
            if let Some(pick) = &r.pick {
                if !pick.iter().any(|n| n.raw == *k) {
                    continue;
                }
            }
            if let Some(omit) = &r.omit {
                if omit.iter().any(|n| n.raw == *k && n.children.is_none()) {
                    continue;
                }
            }
            if k == "type" {
                out.insert("type".into(), antares_jsonld::compact_types(v, ctx));
            } else {
                out.insert(k.clone(), v.clone());
            }
        }
    }
    if r.aggregated {
        for (k, v) in render_aggregated(w, tq, r, ctx, timeprop)? {
            out.insert(k, v);
        }
        return Ok(Value::Object(out));
    }
    for (k, instances) in &w.attrs {
        if instances.is_empty() {
            // gap-cut leftovers render as empty arrays
            out.insert(ctx.compact_iri(k), Value::Array(vec![]));
            continue;
        }
        if r.temporal_values {
            // group instances by datasetId (4.5.9)
            let mut groups: Vec<(Option<String>, Vec<&Value>)> = Vec::new();
            for inst in instances {
                let ds = inst
                    .get("datasetId")
                    .and_then(Value::as_str)
                    .map(String::from);
                match groups.iter_mut().find(|(g, _)| *g == ds) {
                    Some((_, list)) => list.push(inst),
                    None => groups.push((ds, vec![inst])),
                }
            }
            let mut rendered: Vec<Value> = groups
                .iter()
                .map(|(ds, list)| {
                    let atype = list
                        .first()
                        .and_then(|i| i.get("type"))
                        .cloned()
                        .unwrap_or_else(|| Value::String("Property".into()));
                    let values: Vec<Value> = list
                        .iter()
                        .map(|inst| {
                            // 4.5.9: Property/Relationship pairs carry the bare
                            // value/object; other attribute kinds wrap it under
                            // their member name.
                            let v = if let Some(v) = inst.get("value") {
                                ts_float(v)
                            } else if let Some(o) = inst.get("object") {
                                o.clone()
                            } else if let Some(lm) = inst.get("languageMap") {
                                serde_json::json!({"languageMap": lm})
                            } else if let Some(j) = inst.get("json") {
                                serde_json::json!({"json": j})
                            } else if let Some(vv) = inst.get("vocab") {
                                let compacted = match vv {
                                    Value::String(iri) => Value::String(ctx.compact_iri(iri)),
                                    Value::Array(a) => Value::Array(
                                        a.iter()
                                            .map(|s| match s {
                                                Value::String(iri) => {
                                                    Value::String(ctx.compact_iri(iri))
                                                }
                                                o => o.clone(),
                                            })
                                            .collect(),
                                    ),
                                    o => o.clone(),
                                };
                                serde_json::json!({"vocab": compacted})
                            } else if let Some(l) = inst.get("valueList") {
                                // 4.5.9 p.63 EXAMPLE 3: the pair's first element
                                // is the BARE ordered array, not a {"valueList"}
                                // wrapper (audit V-24) — unlike languageMap/
                                // json/vocab, which the clause does wrap
                                l.clone()
                            } else if let Some(l) = inst.get("objectList") {
                                // 4.5.9 p.65: same bare form for ListRelationship
                                l.clone()
                            } else {
                                Value::Null
                            };
                            let t = inst.get(timeprop).cloned().unwrap_or(Value::Null);
                            Value::Array(vec![v, t])
                        })
                        .collect();
                    let mut o = Map::new();
                    // 4.5.9: the simplified member name follows the attribute type
                    let member = match atype.as_str() {
                        Some("Relationship") => "objects",
                        Some("LanguageProperty") => "languageMaps",
                        Some("VocabProperty") => "vocabs",
                        Some("JsonProperty") => "jsons",
                        Some("ListProperty") => "valueLists",
                        Some("ListRelationship") => "objectLists",
                        _ => "values",
                    };
                    o.insert("type".into(), atype);
                    if let Some(ds) = ds {
                        o.insert("datasetId".into(), Value::String(ds.clone()));
                    }
                    o.insert(member.into(), Value::Array(values));
                    Value::Object(o)
                })
                .collect();
            let rendered = if rendered.len() == 1 {
                rendered.remove(0)
            } else {
                Value::Array(rendered)
            };
            out.insert(ctx.compact_iri(k), rendered);
        } else {
            let presented: Vec<Value> = instances
                .iter()
                .map(|inst| {
                    let mut ci = inst.clone();
                    if !r.sys {
                        if let Some(o) = ci.as_object_mut() {
                            o.remove("createdAt");
                            o.remove("modifiedAt");
                            // 6.3.11: expiresAt is sysAttrs-gated too
                            o.remove("expiresAt");
                        }
                    }
                    compact_instance(&ci, ctx)
                })
                .collect();
            out.insert(ctx.compact_iri(k), Value::Array(presented));
        }
    }
    Ok(Value::Object(out))
}

/// Parsed temporal representation params (options/format/lastN/pick/omit/
/// datasetId/aggregation), fully validated up front.
#[derive(Default, Clone)]
struct TRepr {
    temporal_values: bool,
    aggregated: bool,
    sys: bool,
    last_n: Option<usize>,
    pick: Option<Vec<crate::repr::ProjNode>>,
    omit: Option<Vec<crate::repr::ProjNode>>,
    dataset_id: Option<Vec<String>>,
    attrs: Option<Vec<String>>,
    aggr_methods: Vec<String>,
    aggr_period: AggrPeriod,
}

#[derive(Clone, Copy, Default, PartialEq)]
enum AggrPeriod {
    /// PT0S / absent: one bucket over the whole range
    #[default]
    Whole,
    Seconds(i64),
    Months(u32),
}

fn parse_iso_duration(s: &str) -> Option<AggrPeriod> {
    let rest = s.strip_prefix('P')?;
    let (date, time) = match rest.split_once('T') {
        Some((d, t)) => (d, t),
        None => (rest, ""),
    };
    let mut months = 0u32;
    let mut secs = 0i64;
    let mut num = String::new();
    for c in date.chars() {
        if c.is_ascii_digit() || c == '.' {
            num.push(c);
        } else {
            let n: f64 = num.parse().ok()?;
            num.clear();
            // saturating: an absurd magnitude must not panic (debug) or wrap
            // (release) — f64→int `as` casts already saturate, guard the ops.
            match c {
                'Y' => months = months.saturating_add((n as u32).saturating_mul(12)),
                'M' => months = months.saturating_add(n as u32),
                'W' => secs = secs.saturating_add((n * 604800.0) as i64),
                'D' => secs = secs.saturating_add((n * 86400.0) as i64),
                _ => return None,
            }
        }
    }
    for c in time.chars() {
        if c.is_ascii_digit() || c == '.' {
            num.push(c);
        } else {
            let n: f64 = num.parse().ok()?;
            num.clear();
            match c {
                'H' => secs = secs.saturating_add((n * 3600.0) as i64),
                'M' => secs = secs.saturating_add((n * 60.0) as i64),
                'S' => secs = secs.saturating_add(n as i64),
                _ => return None,
            }
        }
    }
    if !num.is_empty() {
        return None;
    }
    Some(match (months, secs) {
        (0, 0) => AggrPeriod::Whole,
        (m, 0) => AggrPeriod::Months(m),
        (0, sc) => AggrPeriod::Seconds(sc),
        _ => return None,
    })
}

const AGGR_METHODS: &[&str] = &[
    "totalCount",
    "distinctCount",
    "sum",
    "avg",
    "min",
    "max",
    "stddev",
    "sumsq",
];

fn parse_trepr(params: &HashMap<String, String>, ctx: &Context) -> Result<TRepr, NgsiError> {
    let mut r = TRepr::default();
    if let Some(opts) = params.get("options") {
        for o in opts.split(',') {
            match o.trim() {
                "sysAttrs" => r.sys = true,
                "temporalValues" => r.temporal_values = true,
                "aggregatedValues" => r.aggregated = true,
                "normalized" => {}
                other => {
                    return Err(NgsiError::InvalidRequest(format!(
                        "unsupported options value {other:?}"
                    )))
                }
            }
        }
    }
    // format wins over options on conflict (6.3.7)
    if let Some(f) = params.get("format") {
        match f.as_str() {
            "temporalValues" => {
                r.temporal_values = true;
                r.aggregated = false;
            }
            "aggregatedValues" => {
                r.aggregated = true;
                r.temporal_values = false;
            }
            "normalized" => {
                r.temporal_values = false;
                r.aggregated = false;
            }
            other => {
                return Err(NgsiError::InvalidRequest(format!(
                    "unsupported format value {other:?}"
                )))
            }
        }
    }
    // 4.21 mutual exclusivity
    let excl = ["pick", "omit", "attrs"]
        .iter()
        .filter(|k| params.contains_key(**k))
        .count();
    if excl > 1 {
        return Err(NgsiError::BadRequestData(
            "pick, omit and attrs are mutually exclusive (4.21)".into(),
        ));
    }
    if let Some(pck) = params.get("pick") {
        r.pick = Some(crate::repr::parse_projection(pck, ctx)?);
    }
    if let Some(o) = params.get("omit") {
        r.omit = Some(crate::repr::parse_projection(o, ctx)?);
    }
    if let Some(a) = params.get("attrs") {
        r.attrs = Some(a.split(',').map(|t| ctx.expand_key(t.trim())).collect());
    }
    r.dataset_id = params
        .get("datasetId")
        .map(|s| s.split(',').map(|d| d.trim().to_owned()).collect());
    r.last_n = match params.get("lastN") {
        Some(n) => {
            // 5.2.21: lastN is a POSITIVE integer — 0 is outside the value
            // space.
            let v = n
                .parse::<usize>()
                .ok()
                .filter(|v| *v >= 1)
                .ok_or_else(|| NgsiError::BadRequestData(format!("invalid lastN {n:?}")))?;
            // Above i64::MAX it wraps negative when bound as the RANK cap
            // (`rk <= $n::bigint`), silently returning an empty set.
            if v > i64::MAX as usize {
                return Err(NgsiError::BadRequestData(format!(
                    "lastN {v} is out of range"
                )));
            }
            Some(v)
        }
        None => None,
    };
    if let Some(m) = params.get("aggrMethods") {
        for method in m.split(',') {
            let method = method.trim();
            if !AGGR_METHODS.contains(&method) {
                return Err(NgsiError::BadRequestData(format!(
                    "invalid aggrMethods value {method:?} (4.5.19)"
                )));
            }
            r.aggr_methods.push(method.to_owned());
        }
        // aggrMethods implies aggregation UNLESS an explicit format says otherwise
        if !params.contains_key("format") {
            r.aggregated = true;
        }
    }
    if r.aggregated && r.aggr_methods.is_empty() {
        return Err(NgsiError::BadRequestData(
            "aggregatedValues requires aggrMethods (4.5.19)".into(),
        ));
    }
    if let Some(d) = params.get("aggrPeriodDuration") {
        r.aggr_period = parse_iso_duration(d).ok_or_else(|| {
            NgsiError::BadRequestData(format!("invalid aggrPeriodDuration {d:?}"))
        })?;
    }
    Ok(r)
}

/// The attribute-selection set for windowing: attrs= or pick=.
fn selection(r: &TRepr) -> Option<Vec<String>> {
    if let Some(a) = &r.attrs {
        return Some(a.clone());
    }
    // core-member picks (id/type/…) are presentation-only, not attr selection
    r.pick.as_ref().map(|p| {
        p.iter()
            .filter(|n| !is_meta(&n.raw))
            .map(|n| n.iri.clone())
            .collect()
    })
}

fn ts_float(v: &Value) -> Value {
    match v.as_f64() {
        Some(f) => serde_json::json!(f),
        None => v.clone(),
    }
}

/// Aggregated representation (4.5.19): attr → {type, <method>: [[v,start,end]]}.
/// Aggregation datatype class per 4.5.19.1 (Tables -1, -2, -3). Booleans
/// count as numbers (1/0, table NOTE); DateTime/Date and plain strings share
/// the lexicographic min/max column; Time additionally supports avg.
#[derive(Clone, Copy, PartialEq, Debug)]
enum AggrClass {
    Number,
    Text,
    TimeOfDay,
    List,
    Opaque,
    Relationship,
}

fn classify_instance(inst: &Value) -> AggrClass {
    if inst.get("object").is_some() {
        return AggrClass::Relationship;
    }
    if inst.get("valueList").is_some() || inst.get("objectList").is_some() {
        return AggrClass::List;
    }
    if inst.get("vocab").is_some()
        || inst.get("languageMap").is_some()
        || inst.get("json").is_some()
    {
        // URI / JSON-object valued kinds: only counting methods apply
        return AggrClass::Opaque;
    }
    match inst.get("value") {
        Some(Value::Number(_)) | Some(Value::Bool(_)) => AggrClass::Number,
        Some(Value::String(s)) if seconds_of_day(s).is_some() => AggrClass::TimeOfDay,
        Some(Value::String(_)) => AggrClass::Text,
        Some(Value::Array(_)) => AggrClass::List,
        _ => AggrClass::Opaque,
    }
}

/// Table 4.5.19.1 eligibility: which methods apply to which datatype class.
fn aggr_eligible(class: AggrClass, method: &str) -> bool {
    match method {
        "totalCount" | "distinctCount" => true,
        "min" | "max" => !matches!(class, AggrClass::Opaque | AggrClass::Relationship),
        "avg" => matches!(
            class,
            AggrClass::Number | AggrClass::List | AggrClass::TimeOfDay
        ),
        "sum" => matches!(class, AggrClass::Number | AggrClass::List),
        "stddev" | "sumsq" => matches!(class, AggrClass::Number),
        _ => false,
    }
}

/// `HH:MM:SS[.f]` → seconds of day (4.6.3 Time is UTC with optional `Z`).
fn seconds_of_day(s: &str) -> Option<f64> {
    let t = s.strip_suffix('Z').unwrap_or(s);
    let b = t.as_bytes();
    if b.len() < 8 || b[2] != b':' || b[5] != b':' {
        return None;
    }
    let h: f64 = t.get(0..2)?.parse().ok()?;
    let m: f64 = t.get(3..5)?.parse().ok()?;
    let sec: f64 = t.get(6..)?.parse().ok()?;
    (h < 24.0 && m < 60.0 && sec < 62.0).then_some(h * 3600.0 + m * 60.0 + sec)
}

/// The raw member an instance carries its data under.
fn raw_of(inst: &Value) -> Option<&Value> {
    for k in [
        "value",
        "object",
        "valueList",
        "objectList",
        "vocab",
        "languageMap",
        "json",
    ] {
        if let Some(v) = inst.get(k) {
            return Some(v);
        }
    }
    None
}

/// Numeric view of one raw value for the class (None ⇒ excluded from
/// numeric methods; List aggregates SIZES per Table 4.5.19.1-1).
fn numeric_of(class: AggrClass, v: &Value) -> Option<f64> {
    match class {
        AggrClass::Number => match v {
            Value::Bool(b) => Some(if *b { 1.0 } else { 0.0 }),
            _ => v.as_f64(),
        },
        AggrClass::List => v.as_array().map(|a| a.len() as f64),
        AggrClass::TimeOfDay => v.as_str().and_then(seconds_of_day),
        _ => None,
    }
}

fn render_aggregated(
    w: &Windowed,
    tq: Option<&TemporalQ>,
    r: &TRepr,
    ctx: &Context,
    timeprop: &str,
) -> Result<Map<String, Value>, NgsiError> {
    use chrono::{DateTime, FixedOffset};
    let fmt = |d: DateTime<FixedOffset>| d.format("%Y-%m-%dT%H:%M:%SZ").to_string();
    let mut out = Map::new();
    for (k, instances) in &w.attrs {
        let mut times: Vec<(DateTime<FixedOffset>, &Value)> = Vec::new();
        let mut class: Option<AggrClass> = None;
        for inst in instances {
            let Some(t) = inst
                .get(timeprop)
                .and_then(Value::as_str)
                .and_then(|t| DateTime::parse_from_rfc3339(t).ok())
            else {
                continue;
            };
            if class.is_none() {
                class = Some(classify_instance(inst));
            }
            let Some(raw) = raw_of(inst) else { continue };
            times.push((t, raw));
        }
        if times.is_empty() {
            continue;
        }
        let class = class.expect("set with first instance");
        // 5.7.4.4 p.211: "If an aggregated temporal representation is
        // requested and any of the requested Attributes is not eligible for
        // at least one of the aggregation methods specified in the request
        // parameters, then an error of type InvalidRequest shall be raised."
        for method in &r.aggr_methods {
            if !aggr_eligible(class, method) {
                return Err(NgsiError::InvalidRequest(format!(
                    "attribute {} ({class:?}-valued) is not eligible for \
                     aggregation method {method} (4.5.19.1, 5.7.4.4)",
                    ctx.compact_iri(k)
                )));
            }
        }
        times.sort_by_key(|(t, _)| *t);
        let anchor = tq
            .and_then(|tq| DateTime::parse_from_rfc3339(&tq.time_at).ok())
            .unwrap_or(times[0].0);
        // bucket boundaries
        let bucket_of =
            |t: DateTime<FixedOffset>| -> (DateTime<FixedOffset>, DateTime<FixedOffset>) {
                match r.aggr_period {
                    AggrPeriod::Whole => {
                        let last = times.last().expect("nonempty").0;
                        (anchor, last + chrono::Duration::seconds(1))
                    }
                    AggrPeriod::Seconds(sc) => {
                        let idx = (t - anchor).num_seconds().div_euclid(sc);
                        let start = anchor + chrono::Duration::seconds(idx * sc);
                        (start, start + chrono::Duration::seconds(sc))
                    }
                    AggrPeriod::Months(m) => {
                        let mut start = anchor;
                        loop {
                            // saturate instead of panic: a huge month period or
                            // far-future timeAt overflows chrono's range — treat
                            // the remainder as one open-ended bucket
                            let Some(next) = start.checked_add_months(chrono::Months::new(m))
                            else {
                                break (start, chrono::DateTime::<chrono::Utc>::MAX_UTC.into());
                            };
                            if next > t {
                                break (start, next);
                            }
                            start = next;
                        }
                    }
                }
            };
        type Bucket = (DateTime<FixedOffset>, DateTime<FixedOffset>);
        let mut buckets: Vec<(Bucket, Vec<&Value>)> = Vec::new();
        for (t, v) in &times {
            let b = bucket_of(*t);
            match buckets.iter_mut().find(|(bb, _)| bb.0 == b.0) {
                Some((_, vals)) => vals.push(v),
                None => buckets.push((b, vec![v])),
            }
        }
        buckets.sort_by_key(|((s, _), _)| *s);
        let mut attr_out = Map::new();
        // 4.5.19.0: the member is labelled "Property" for Properties and
        // "Relationship" for Relationships.
        let label = if class == AggrClass::Relationship {
            "Relationship"
        } else {
            "Property"
        };
        attr_out.insert("type".into(), Value::String(label.into()));
        for method in &r.aggr_methods {
            let rows: Vec<Value> = buckets
                .iter()
                .map(|((bs, be), vals)| {
                    let val = aggregate_bucket(method, class, vals);
                    Value::Array(vec![val, Value::String(fmt(*bs)), Value::String(fmt(*be))])
                })
                .collect();
            attr_out.insert(method.clone(), Value::Array(rows));
        }
        out.insert(ctx.compact_iri(k), Value::Object(attr_out));
    }
    Ok(out)
}

/// One bucket, one method — per-class semantics from Tables 4.5.19.1-1/2/3.
/// Never emits an out-of-range float (audit V-27: the old fold seeded with
/// f64::INFINITY, which serde_json serializes as null).
fn aggregate_bucket(method: &str, class: AggrClass, vals: &[&Value]) -> Value {
    let nums: Vec<f64> = vals.iter().filter_map(|v| numeric_of(class, v)).collect();
    let finite = |x: f64| {
        if x.is_finite() {
            serde_json::json!(x)
        } else {
            Value::Null
        }
    };
    match method {
        "totalCount" => serde_json::json!(vals.len()),
        "distinctCount" => {
            // Relationship: "count of distinct relationship TARGETS" — an
            // object may be a URI or an array of URIs, so flatten first.
            let mut seen: Vec<String> = Vec::new();
            for v in vals {
                let items: Vec<&Value> = match (class, v) {
                    (AggrClass::Relationship, Value::Array(a)) => a.iter().collect(),
                    _ => vec![*v],
                };
                for it in items {
                    let key = it.to_string();
                    if !seen.contains(&key) {
                        seen.push(key);
                    }
                }
            }
            serde_json::json!(seen.len())
        }
        "min" | "max" => match class {
            // lexicographic first/last for strings, dates and times
            AggrClass::Text | AggrClass::TimeOfDay => {
                let mut strs: Vec<&str> = vals.iter().filter_map(|v| v.as_str()).collect();
                strs.sort_unstable();
                let pick = if method == "min" {
                    strs.first()
                } else {
                    strs.last()
                };
                pick.map_or(Value::Null, |s| Value::String((*s).to_owned()))
            }
            _ => {
                let it = nums.iter().copied();
                let picked = if method == "min" {
                    it.fold(None, |a: Option<f64>, v| Some(a.map_or(v, |x| x.min(v))))
                } else {
                    it.fold(None, |a: Option<f64>, v| Some(a.map_or(v, |x| x.max(v))))
                };
                picked.map_or(Value::Null, &finite)
            }
        },
        "sum" => finite(nums.iter().sum::<f64>()),
        "avg" => {
            if nums.is_empty() {
                Value::Null
            } else if class == AggrClass::TimeOfDay {
                let mean = nums.iter().sum::<f64>() / nums.len() as f64;
                let (h, m, sec) = (
                    (mean / 3600.0) as u32,
                    ((mean % 3600.0) / 60.0) as u32,
                    (mean % 60.0) as u32,
                );
                Value::String(format!("{h:02}:{m:02}:{sec:02}"))
            } else {
                finite(nums.iter().sum::<f64>() / nums.len() as f64)
            }
        }
        "stddev" => {
            if nums.is_empty() {
                Value::Null
            } else {
                let n = nums.len() as f64;
                let mean = nums.iter().sum::<f64>() / n;
                finite((nums.iter().map(|v| (v - mean).powi(2)).sum::<f64>() / n).sqrt())
            }
        }
        "sumsq" => finite(nums.iter().map(|v| v * v).sum::<f64>()),
        _ => Value::Null,
    }
}

// ---------- GET /temporal/entities/ (5.7.4) ----------

pub async fn query_temporal(
    State(st): State<AppState>,
    CleanParams(params): CleanParams,
    headers: HeaderMap,
) -> Response {
    match query_temporal_outer(&st, params, &headers).await {
        Ok(r) => r,
        Err(e) => e.into_response(),
    }
}

/// 5.7.4.4 EntityMap usage on the temporal query: a live map referenced by
/// the NGSILD-EntityMap header fixes the result set to the map's Entities
/// (5.5.14) and its location is echoed; an unknown or expired reference
/// means "a new EntityMap shall be created" (the entityMap=true branch,
/// answering 201 + the fresh location).
async fn query_temporal_outer(
    st: &AppState,
    mut params: HashMap<String, String>,
    headers: &HeaderMap,
) -> ApiResult<Response> {
    let Some(map_ref) = headers
        .get("NGSILD-EntityMap")
        .and_then(|v| v.to_str().ok())
        .map(str::to_owned)
    else {
        return query_temporal_inner(st, &params, headers).await;
    };
    let tenant = tenant_from(headers)?;
    let map_id = map_ref.rsplit('/').next().unwrap_or(&map_ref).to_owned();
    let Some(mut map) = crate::entity_maps::map_get(st, &tenant, &map_id) else {
        params.insert("entityMap".into(), "true".into());
        return query_temporal_inner(st, &params, headers).await;
    };
    params.remove("entityMap");
    // 5.5.14: the creator removes Entities that no longer match the query
    // filters at processing time — judgeable locally only for "@none"
    // entries. ponytail: this recheck is a second temporal query per
    // map-using request, same shape as the entity query's filter re-run.
    let matching: std::collections::HashSet<String> = {
        let mut eff = params.clone();
        for k in ["limit", "offset", "count"] {
            eff.remove(k);
        }
        eff.insert("limit".into(), st.max_limit.to_string());
        let resp = query_temporal_inner(st, &eff, headers).await?;
        let bytes = axum::body::to_bytes(resp.into_body(), usize::MAX)
            .await
            .map_err(|e| NgsiError::InternalError(format!("entityMap recheck read: {e}")))?;
        serde_json::from_slice::<Value>(&bytes)
            .ok()
            .and_then(|v| v.as_array().cloned())
            .unwrap_or_default()
            .iter()
            .filter_map(|d| d.get("id").and_then(Value::as_str).map(str::to_owned))
            .collect()
    };
    if let Some(emap) = map.get_mut("entityMap").and_then(Value::as_object_mut) {
        let stale: Vec<String> = emap
            .iter()
            .filter(|(eid, srcs)| {
                srcs.as_array()
                    .is_some_and(|a| a.len() == 1 && a[0] == "@none")
                    && !matching.contains(*eid)
            })
            .map(|(k, _)| k.clone())
            .collect();
        for k in stale {
            emap.remove(&k);
        }
    }
    crate::entity_maps::map_put(st, &tenant, map.clone());
    // fix the query to the Entities listed in the map (5.5.14)
    let ids: Vec<&str> = map["entityMap"]
        .as_object()
        .map(|o| o.keys().map(String::as_str).collect())
        .unwrap_or_default();
    params.insert(
        "id".into(),
        if ids.is_empty() {
            "urn:ngsi-ld:entitymap:empty".to_owned()
        } else {
            ids.join(",")
        },
    );
    let mut resp = query_temporal_inner(st, &params, headers).await?;
    if let Ok(v) = format!("/ngsi-ld/v1/entityMaps/{map_id}").parse() {
        resp.headers_mut().insert("NGSILD-EntityMap", v);
    }
    Ok(resp)
}

pub(crate) async fn query_temporal_inner(
    st: &AppState,
    params: &HashMap<String, String>,
    headers: &HeaderMap,
) -> ApiResult<Response> {
    let tenant = tenant_from(headers)?;
    check_params(
        params,
        &[
            "id",
            "idPattern",
            "type",
            "attrs",
            "q",
            "georel",
            "geometry",
            "coordinates",
            "geoproperty",
            "scopeQ",
            "csf",
            "timerel",
            "timeAt",
            "endTimeAt",
            "timeproperty",
            "aggrMethods",
            "aggrPeriodDuration",
            "lastN",
            "limit",
            "offset",
            "count",
            "options",
            "format",
            "lang",
            "local",
            "entityMap",
            "pick",
            "omit",
            "datasetId",
            "orderBy",
            "orderFrom",
            "orderGeometry",
            "collation",
            "entityMapLifetime",
            "splitEntities",
            "expandValues",
            "jsonKeys",
        ],
    )?;
    let accept = parse_accept(headers)?;
    let ctx = request_context(&st.loader, headers).await?;
    // 5.7.4.4 a-e: id/idPattern alone are NOT sufficient, and the attrs
    // list / q must include at least one non-system Attribute to qualify.
    // 5.7.4.3 expandValues: the same 4.9 EXAMPLE 12 coercion as the entity
    // query — term values expanded against the @context before executing;
    // jsonKeys needs no action (raw JSON targets are navigated without term
    // expansion by default).
    let q_ast = params.get("q").map(|q| parse_q(q)).transpose()?.map(|ast| {
        crate::qeval::apply_expand_values(ast, params.get("expandValues").map(String::as_str), &ctx)
    });
    let attrs_qualify = params.get("attrs").is_some_and(|a| {
        a.split(',')
            .any(|n| antares_ql::is_non_system_attr(n.trim()))
    });
    let q_qualifies = q_ast.as_ref().is_some_and(|ast| {
        ast.attribute_paths()
            .iter()
            .any(|h| antares_ql::is_non_system_attr(h))
    });
    let has_filter = params.contains_key("type")
        || attrs_qualify
        || q_qualifies
        || params.contains_key("georel")
        || params.get("local").map(String::as_str) == Some("true");
    if !has_filter {
        return Err(NgsiError::BadRequestData(
            "temporal query needs at least one of type, attrs, q, georel (5.7.4)".into(),
        )
        .into());
    }
    // 5.7.4.4: Linked Entity retrieval is not defined for temporal queries —
    // linked filter conditions are an unconditional BadRequestData
    if q_ast
        .as_ref()
        .map(antares_ql::QNode::max_link_depth)
        .unwrap_or(0)
        > 0
    {
        return Err(NgsiError::BadRequestData(
            "temporal q must not reference Linked Entity attributes (5.7.4.4)".into(),
        )
        .into());
    }
    // 5.7.4.4: a syntactically invalid context source filter is 400. Named
    // gap: csf is validated but not applied to Context Source matching.
    if let Some(csf) = params.get("csf") {
        parse_q(csf)?;
    }
    // 5.7.4.4: temporal ordering may only refer to the "id" entity member
    if let Some(spec) = params.get("orderBy") {
        let non_id = spec.split(',').any(|part| {
            let m = part.trim().split(';').next().unwrap_or("").trim();
            m.split('[').next().unwrap_or(m) != "id"
        });
        if non_id {
            return Err(NgsiError::BadRequestData(
                "temporal orderBy may only name \"id\" (5.7.4.4)".into(),
            )
            .into());
        }
    }
    let tq = TemporalQ::from_params(params, true)?;
    let trepr = parse_trepr(params, &ctx)?;
    // 5.7.4.4: {…} projection is Linked Entity retrieval — unconditional 400
    if trepr
        .pick
        .as_deref()
        .map(crate::repr::proj_depth)
        .unwrap_or(0)
        > 0
        || trepr
            .omit
            .as_deref()
            .map(crate::repr::proj_depth)
            .unwrap_or(0)
            > 0
    {
        return Err(NgsiError::BadRequestData(
            "temporal projection must not use Linked Entity selection (5.7.4.4)".into(),
        )
        .into());
    }
    let last_n = trepr.last_n;

    let ids: Option<Vec<&str>> = params.get("id").map(|s| s.split(',').collect());
    // 5.7.4.4: an invalid URI in the id list is BadRequestData
    if let Some(ids) = &ids {
        for id in ids {
            antares_model::EntityId::new(id)?;
        }
    }
    let id_pattern = match params.get("idPattern") {
        Some(p) => Some(
            regex::Regex::new(p)
                .map_err(|_| NgsiError::BadRequestData(format!("invalid idPattern {p:?}")))?,
        ),
        None => None,
    };
    let types: Option<Vec<String>> = params.get("type").map(|s| {
        s.split([',', '|'])
            .map(|t| ctx.expand_key(t.trim()))
            .collect()
    });
    let attrs_filter = selection(&trepr);
    // only the attrs= param excludes entities; pick is projection-only
    let entity_attr_filter = trepr.attrs.clone();
    let geo = crate::geo::GeoQuery::from_params(params)?;

    // C11: push entity narrowing (ids/types/attrs) and instance-window
    // pruning (range + RANK()-capped lastN) into the store. The loop below
    // and window() stay the arbiters — pruning is byte-exact against
    // instance_matches (compile::temporal), so it cannot change an answer.
    // With q= or geo present the instance pruning is withheld: both evaluate
    // over the FULL instance set, and a pruned doc would flip their verdicts
    // (memory mode would answer differently — the one unforgivable bug).
    let push_instances = q_ast.is_none() && geo.is_none();
    // Entity-page pushdown (audit 2026-08-08): a temporal query used to
    // materialize the tenant's ENTIRE history. Pushed only when every filter
    // the store cannot see is absent — same gate family as C11 entities.
    let (p_offset, p_limit, _) = crate::entities::page_params(st, params)?;
    let push_page =
        push_instances && id_pattern.is_none() && params.get("orderBy").is_none() && p_limit > 0;
    let tf = antares_sql::store::filter::TemporalFilter {
        ids: ids.as_deref(),
        types: types.as_deref(),
        attrs: entity_attr_filter.as_deref(),
        range: tq.as_ref().filter(|_| push_instances).map(|t| {
            antares_sql::compile::temporal::InstanceRange {
                timerel: &t.timerel,
                time_at: &t.time_at,
                end_time_at: t.end_time_at.as_deref(),
                timeproperty: &t.timeproperty,
            }
        }),
        last_n: match (last_n, push_instances) {
            (Some(n), true) => Some(n as i64),
            _ => None,
        },
        timeproperty: tq
            .as_ref()
            .map_or("observedAt", |t| t.timeproperty.as_str()),
        page: push_page.then_some(antares_sql::store::filter::Page {
            offset: p_offset as i64,
            limit: p_limit as i64,
        }),
    };
    let outcome = st.store.query_temporal(&tenant, &tf)?;
    let (all, pre_paged, pre_total) = (outcome.rows, outcome.paged, outcome.total);
    // 5.7.4.4: fan the query out to matching queryTemporal registrations
    // and merge the remote Temporal Evolutions with the local set (4.5.5;
    // auxiliary data never introduces new entities)
    let mut warnings: Vec<String> = Vec::new();
    let looped = crate::federation::via_loop(
        headers,
        &crate::federation::alias_for(&st.host_alias, &tenant),
    );
    let timeprop = tq
        .as_ref()
        .map_or("observedAt", |t| t.timeproperty.as_str())
        .to_owned();
    let all = if crate::federation::active(params) && !looped {
        let fed = crate::federation::fed_query_temporal(
            st,
            &tenant,
            headers,
            &ctx,
            params,
            &mut warnings,
        )
        .await;
        let mut order: Vec<String> = Vec::new();
        let mut by_id: std::collections::HashMap<String, Value> = Default::default();
        for doc in all {
            if let Some(id) = doc.get("id").and_then(Value::as_str) {
                order.push(id.to_owned());
                by_id.insert(id.to_owned(), doc);
            }
        }
        for aux_pass in [false, true] {
            for (aux, d) in &fed {
                if *aux != aux_pass {
                    continue;
                }
                let Some(id) = d.get("id").and_then(Value::as_str) else {
                    continue;
                };
                match by_id.get_mut(id) {
                    Some(base) => merge_temporal_docs(base, d, *aux, &timeprop),
                    None if !aux => {
                        order.push(id.to_owned());
                        by_id.insert(id.to_owned(), d.clone());
                    }
                    None => {}
                }
            }
        }
        order
            .into_iter()
            .filter_map(|id| by_id.remove(&id))
            .collect()
    } else {
        all
    };
    let mut matches = Vec::new();
    for doc in all {
        let id = doc["id"].as_str().unwrap_or("");
        if let Some(ids) = &ids {
            if !ids.contains(&id) {
                continue;
            }
        }
        // 5.2.33: "id takes precedence over idPattern"
        if ids.is_none() {
            if let Some(re) = &id_pattern {
                if !re.is_match(id) {
                    continue;
                }
            }
        }
        if let Some(types) = &types {
            let etypes = doc["type"].as_array().cloned().unwrap_or_default();
            if !etypes
                .iter()
                .any(|t| types.iter().any(|w| Some(w.as_str()) == t.as_str()))
            {
                continue;
            }
        }
        if let Some(want) = &entity_attr_filter {
            if !want.iter().any(|a| doc.get(a).is_some()) {
                continue;
            }
        }
        // 5.7.4.4 S2/S3: the values filter and geoquery are checked against
        // the Attribute instances WITHIN the temporal-query interval — an
        // out-of-window instance must not satisfy them
        if q_ast.is_some() || geo.is_some() {
            let mut eval_doc = doc.clone();
            if let (Some(tqv), Some(o)) = (tq.as_ref(), eval_doc.as_object_mut()) {
                for (k, v) in o.iter_mut() {
                    if is_meta(k) {
                        continue;
                    }
                    if let Some(arr) = v.as_array_mut() {
                        arr.retain(|inst| tqv.instance_matches(inst));
                    }
                }
            }
            if let Some(ast) = &q_ast {
                if !crate::qeval::eval_q(ast, &eval_doc, &ctx, &|_| None) {
                    continue;
                }
            }
            if let Some(g) = &geo {
                if !g.matches(&eval_doc, &ctx) {
                    continue;
                }
            }
        }
        // entity qualifies only if some instance falls in the window
        let any_instance = doc.as_object().is_some_and(|o| {
            o.iter().any(|(k, v)| {
                !is_meta(k)
                    && v.as_array().is_some_and(|arr| {
                        arr.iter()
                            .any(|inst| tq.as_ref().is_none_or(|tq| tq.instance_matches(inst)))
                    })
            })
        });
        if !any_instance {
            continue;
        }
        matches.push(doc);
    }
    if let Some(spec) = params.get("orderBy") {
        crate::entities::order_entities(&mut matches, spec, params, &ctx)?;
    }
    let (page, count_hdr, links) = if pre_paged {
        let total = pre_total.map(|t| t as usize).unwrap_or(matches.len());
        crate::entities::paginate_pre(st, params, matches, "/ngsi-ld/v1/temporal/entities", total)?
    } else {
        crate::entities::paginate(st, params, matches, "/ngsi-ld/v1/temporal/entities")?
    };
    let core_only_pick = attrs_filter.as_ref().is_some_and(Vec::is_empty);
    let mut payload: Vec<Value> = Vec::new();
    let (mut g_max, mut g_min, mut g_maxts) = (0usize, None::<String>, None::<String>);
    for d in &page {
        let w = window(
            d,
            tq.as_ref(),
            last_n,
            attrs_filter.as_ref(),
            trepr.omit.as_ref(),
            trepr.dataset_id.as_ref(),
            &timeprop,
        );
        g_max = g_max.max(w.max_per_attr);
        if let Some(m) = &w.ts_min {
            if g_min.as_deref().is_none_or(|c| m.as_str() < c) {
                g_min = Some(m.clone());
            }
        }
        if let Some(m) = &w.ts_max {
            if g_maxts.as_deref().is_none_or(|c| m.as_str() > c) {
                g_maxts = Some(m.clone());
            }
        }
        // no instance survived the window/dataset filters ⇒ the entity is
        // not part of the temporal result (unless the projection is
        // deliberately core-only, e.g. pick=id)
        if w.attrs.is_empty() && !core_only_pick {
            continue;
        }
        let presented = present_temporal(d, &w, &ctx, &trepr, tq.as_ref(), &timeprop)?;
        if trepr.pick.is_some() && presented.as_object().is_some_and(|o| o.is_empty()) {
            continue;
        }
        payload.push(presented);
    }
    // aggregated responses are complete by construction — never 206 (6.3.10)
    let cr = if trepr.aggregated {
        None
    } else {
        content_range(
            g_max,
            g_min.as_deref(),
            g_maxts.as_deref(),
            tq.as_ref(),
            last_n,
        )
    };
    let status = if cr.is_some() {
        StatusCode::PARTIAL_CONTENT
    } else {
        StatusCode::OK
    };
    let mut resp = crate::negotiate::respond_list(status, payload, &ctx, accept, &tenant);
    crate::entities::attach_warnings(&mut resp, &warnings);
    if let Some(cr) = cr {
        if let Ok(v) = cr.parse() {
            resp.headers_mut().insert("Content-Range", v);
        }
    }
    if let Some(total) = count_hdr {
        if let Ok(v) = total.to_string().parse() {
            resp.headers_mut().insert("NGSILD-Results-Count", v);
        }
    }
    for l in links {
        if let Ok(v) = l.parse() {
            resp.headers_mut().append(axum::http::header::LINK, v);
        }
    }
    // 6.18.3.2: entityMap=true — the temporal EntityMap for this query is
    // (re)created; the response carries NGSILD-EntityMap and 201 Created.
    if params.get("entityMap").map(String::as_str) == Some("true") {
        let map =
            crate::entity_maps::build_temporal_map(st, &tenant, headers, &ctx, params).await?;
        *resp.status_mut() = StatusCode::CREATED;
        if let Some(id) = map.get("id").and_then(Value::as_str) {
            if let Ok(v) = format!("/ngsi-ld/v1/entityMaps/{id}").parse() {
                resp.headers_mut().insert("NGSILD-EntityMap", v);
            }
        }
    }
    Ok(resp)
}

// ---------- GET /temporal/entities/{id} (5.7.3) ----------

pub async fn retrieve_temporal(
    State(st): State<AppState>,
    Path(id): Path<String>,
    CleanParams(params): CleanParams,
    headers: HeaderMap,
) -> Response {
    match retrieve_temporal_outer(&st, &id, params, &headers).await {
        Ok(r) => r,
        Err(e) => e.into_response(),
    }
}

/// 5.7.3.4 EntityMap usage on the temporal retrieve: a supplied
/// NGSILD-EntityMap location is retrieved and, if live, is the only source
/// used to determine which registrations match; an unknown/expired
/// reference — or the entityMap=true flag — creates a new map, whose
/// location is returned in the NGSILD-EntityMap response header.
async fn retrieve_temporal_outer(
    st: &AppState,
    id: &str,
    params: HashMap<String, String>,
    headers: &HeaderMap,
) -> ApiResult<Response> {
    let tenant = tenant_from(headers)?;
    let map_ref = headers
        .get("NGSILD-EntityMap")
        .and_then(|v| v.to_str().ok())
        .map(|r| r.rsplit('/').next().unwrap_or(r).to_owned());
    if let Some(map) = map_ref
        .as_deref()
        .and_then(|mid| crate::entity_maps::map_get(st, &tenant, mid))
    {
        let mut resp = retrieve_temporal_inner(st, id, &params, headers, Some(&map)).await?;
        let mid = map_ref.unwrap_or_default();
        if let Ok(v) = format!("/ngsi-ld/v1/entityMaps/{mid}").parse() {
            resp.headers_mut().insert("NGSILD-EntityMap", v);
        }
        return Ok(resp);
    }
    let want_map = map_ref.is_some() || params.get("entityMap").map(String::as_str) == Some("true");
    let mut resp = retrieve_temporal_inner(st, id, &params, headers, None).await?;
    if want_map && resp.status().is_success() {
        let ctx = request_context(&st.loader, headers).await?;
        let local_held = st
            .store
            .get_temporal(
                &tenant,
                id,
                &antares_sql::store::filter::TemporalFilter::default(),
            )?
            .is_some();
        let map = crate::entity_maps::build_retrieve_map(
            st, &tenant, &ctx, headers, id, &params, true, local_held,
        )?;
        if let Some(mid) = map.get("id").and_then(Value::as_str) {
            if let Ok(v) = format!("/ngsi-ld/v1/entityMaps/{mid}").parse() {
                resp.headers_mut().insert("NGSILD-EntityMap", v);
            }
        }
    }
    Ok(resp)
}

async fn retrieve_temporal_inner(
    st: &AppState,
    id: &str,
    params: &HashMap<String, String>,
    headers: &HeaderMap,
    map: Option<&Value>,
) -> ApiResult<Response> {
    {
        let tenant = tenant_from(headers)?;
        check_params(
            params,
            &[
                "attrs",
                "timerel",
                "timeAt",
                "endTimeAt",
                "timeproperty",
                "lastN",
                "aggrMethods",
                "aggrPeriodDuration",
                "options",
                "format",
                "lang",
                "local",
                "pick",
                "omit",
                "datasetId",
                "entityMap",
                "entityMapLifetime",
            ],
        )?;
        let accept = parse_accept(headers)?;
        let ctx = request_context(&st.loader, headers).await?;
        let tq = TemporalQ::from_params(params, false)?;
        let trepr = parse_trepr(params, &ctx)?;
        // 5.7.3.4: "If projection attributes are present and indicate the
        // use of Linked Entity retrieval, an error of type BadRequestData
        // shall be raised" — unconditional, temporal defines no join.
        if trepr
            .pick
            .as_deref()
            .map(crate::repr::proj_depth)
            .unwrap_or(0)
            > 0
            || trepr
                .omit
                .as_deref()
                .map(crate::repr::proj_depth)
                .unwrap_or(0)
                > 0
        {
            return Err(NgsiError::BadRequestData(
                "temporal projection must not use Linked Entity selection (5.7.3.4)".into(),
            )
            .into());
        }
        let last_n = trepr.last_n;
        antares_model::EntityId::new(id)?;
        // C11: instance pruning pushed into the store (no q=/geo on retrieve,
        // so it is always safe here); window() below stays the arbiter.
        let tf = antares_sql::store::filter::TemporalFilter {
            range: tq
                .as_ref()
                .map(|t| antares_sql::compile::temporal::InstanceRange {
                    timerel: &t.timerel,
                    time_at: &t.time_at,
                    end_time_at: t.end_time_at.as_deref(),
                    timeproperty: &t.timeproperty,
                }),
            last_n: last_n.map(|n| n as i64),
            timeproperty: tq
                .as_ref()
                .map_or("observedAt", |t| t.timeproperty.as_str()),
            ..Default::default()
        };
        let timeprop = tq
            .as_ref()
            .map_or("observedAt", |t| t.timeproperty.as_str())
            .to_owned();
        let local = st.store.get_temporal(&tenant, id, &tf)?;
        // 5.7.3.4: forward to matching retrieveTemporal registrations and
        // merge the remote instance data (4.5.5; auxiliary instances only
        // fill timestamps absent elsewhere)
        let mut warnings: Vec<String> = Vec::new();
        let looped = crate::federation::via_loop(
            headers,
            &crate::federation::alias_for(&st.host_alias, &tenant),
        );
        let doc = if crate::federation::active(params) && !looped {
            let fed = crate::federation::fed_retrieve_temporal(
                st,
                &tenant,
                headers,
                &ctx,
                id,
                params,
                map,
                &mut warnings,
            )
            .await;
            let (mut base, skip) = match local {
                Some(b) => (b, None),
                None => {
                    let idx = fed.iter().position(|(aux, _)| !aux).or(if fed.is_empty() {
                        None
                    } else {
                        Some(0)
                    });
                    match idx {
                        Some(i) => (fed[i].1.clone(), Some(i)),
                        None => {
                            return Err(NgsiError::ResourceNotFound(format!(
                                "temporal entity {id} not found"
                            ))
                            .into())
                        }
                    }
                }
            };
            for aux_pass in [false, true] {
                for (i, (aux, d)) in fed.iter().enumerate() {
                    if Some(i) == skip {
                        continue;
                    }
                    if *aux == aux_pass {
                        merge_temporal_docs(&mut base, d, *aux, &timeprop);
                    }
                }
            }
            base
        } else {
            local.ok_or_else(|| {
                NgsiError::ResourceNotFound(format!("temporal entity {id} not found"))
            })?
        };
        let attrs_filter = selection(&trepr);
        // 5.7.3: attrs matching nothing ⇒ 404
        if let Some(want) = &attrs_filter {
            if !want.iter().any(|a| doc.get(a).is_some()) {
                return Err(NgsiError::ResourceNotFound(format!(
                    "temporal entity {id} has none of the requested attributes"
                ))
                .into());
            }
        }
        let mut w = window(
            &doc,
            tq.as_ref(),
            last_n,
            attrs_filter.as_ref(),
            trepr.omit.as_ref(),
            trepr.dataset_id.as_ref(),
            &timeprop,
        );
        gap_cut(&mut w, &timeprop, last_n.is_some());
        let cr = if trepr.aggregated {
            None
        } else {
            content_range(
                w.max_per_attr,
                w.ts_min.as_deref(),
                w.ts_max.as_deref(),
                tq.as_ref(),
                last_n,
            )
        };
        let payload = present_temporal(&doc, &w, &ctx, &trepr, tq.as_ref(), &timeprop)?;
        if (trepr.pick.is_some() || trepr.omit.is_some())
            && payload.as_object().is_some_and(|o| o.is_empty())
        {
            return Err(NgsiError::ResourceNotFound(format!(
                "projection matches nothing on temporal entity {id}"
            ))
            .into());
        }
        let status = if cr.is_some() {
            StatusCode::PARTIAL_CONTENT
        } else {
            StatusCode::OK
        };
        let mut resp = respond(status, payload, &ctx, accept, &tenant);
        if let Some(cr) = cr {
            if let Ok(v) = cr.parse() {
                resp.headers_mut().insert("Content-Range", v);
            }
        }
        crate::entities::attach_warnings(&mut resp, &warnings);
        Ok(resp)
    }
}

/// 5.7.3.4 / 4.5.5: merge one remote Temporal Evolution into `base` by
/// appending instances per Attribute; auxiliary data contributes an
/// instance only when no instance with the same timeproperty value was
/// received from elsewhere.
fn merge_temporal_docs(base: &mut Value, add: &Value, aux: bool, timeprop: &str) {
    let (Some(target), Some(source)) = (base.as_object_mut(), add.as_object()) else {
        return;
    };
    for (k, v) in source {
        if [
            "id",
            "type",
            "scope",
            "createdAt",
            "modifiedAt",
            "deletedAt",
            "expiresAt",
        ]
        .contains(&k.as_str())
        {
            target.entry(k.clone()).or_insert_with(|| v.clone());
            continue;
        }
        let incoming: Vec<Value> = match v {
            Value::Array(x) => x.clone(),
            other => vec![other.clone()],
        };
        match target.get_mut(k).and_then(Value::as_array_mut) {
            None => {
                target.insert(k.clone(), Value::Array(incoming));
            }
            Some(cur) => {
                for ni in incoming {
                    if aux {
                        let ts = ni.get(timeprop).and_then(Value::as_str);
                        if ts.is_some()
                            && cur
                                .iter()
                                .any(|ci| ci.get(timeprop).and_then(Value::as_str) == ts)
                        {
                            continue;
                        }
                    }
                    cur.push(ni);
                }
            }
        }
    }
}

// ---------- DELETE /temporal/entities/{id} (5.6.16) ----------

pub async fn delete_temporal(
    State(st): State<AppState>,
    Path(id): Path<String>,
    CleanParams(params): CleanParams,
    headers: HeaderMap,
) -> Response {
    let go = async {
        let tenant = tenant_from(&headers)?;
        antares_model::EntityId::new(&id)?;
        check_params(&params, &["local"])?;
        let deleted = st.store.delete(&tenant, Kind::Temporal, &id)?;
        // 5.6.16.4: forward to registrations supporting the operation;
        // unsupported proxy modes are Conflict.
        let ctx = st.loader.core();
        let local_part = crate::federation::Part {
            status: if deleted { 204 } else { 404 },
            detail: if deleted {
                "deleted locally".into()
            } else {
                format!("temporal entity {id} not found locally")
            },
        };
        if let Some(r) = temporal_attr_fed(
            &st,
            &tenant,
            &headers,
            &ctx,
            &params,
            &id,
            "deleteTemporal",
            reqwest::Method::DELETE,
            "",
            None,
            local_part,
        )
        .await?
        {
            return Ok(r);
        }
        if deleted {
            Ok::<_, ApiError>(no_content(&tenant))
        } else {
            Err(NgsiError::ResourceNotFound(format!("temporal entity {id} not found")).into())
        }
    };
    go.await.unwrap_or_else(|e| e.into_response())
}

// ---------- POST /temporal/entities/{id}/attrs/ (5.6.12) ----------

pub async fn add_temporal_attrs(
    State(st): State<AppState>,
    Path(id): Path<String>,
    CleanParams(params): CleanParams,
    headers: HeaderMap,
    body: Bytes,
) -> Response {
    let go = async {
        let tenant = tenant_from(&headers)?;
        antares_model::EntityId::new(&id)?;
        check_params(&params, &["local"])?;
        let parsed = parse_body(&st.loader, &headers, &body, BodyKind::Standard).await?;
        let obj = parsed
            .value
            .as_object()
            .ok_or_else(|| NgsiError::BadRequestData("fragment must be a JSON object".into()))?;
        // 5.6.12 input is pushed history — the 4.5.7 deleted-instance
        // representation is legal (5.5.4 temporal exception), hence
        // allow_null (mirrors 5.6.11).
        let mut expanded = expand_entity(
            obj,
            &parsed.ctx,
            ExpandOpts {
                fragment: true,
                allow_null: true,
                temporal: true,
                ..Default::default()
            },
        )?;
        // 5.6.12.4: forwarding — proxy modes without appendAttrsTemporal
        // are Conflict; supporting registrations receive the fragment and
        // the matching attributes are stripped from the local half.
        let spec = crate::csource::CsrSpec {
            ids: Some(vec![id.clone()]),
            ..Default::default()
        };
        let mut regs =
            crate::federation::write_regs(&st, &tenant, &spec, &parsed.ctx, &params, &headers);
        if let Some(r) = crate::federation::handle_via_loop(
            &headers,
            &crate::federation::alias_for(&st.host_alias, &tenant),
            &tenant,
            &mut regs,
        ) {
            return Ok(r);
        }
        if !regs.is_empty() {
            let mut parts = Vec::new();
            let mut fwd = Vec::new();
            for reg in &regs {
                if !reg.supports("appendAttrsTemporal") {
                    if reg.is_proxy() {
                        parts.push(crate::federation::conflict_part("appendAttrsTemporal"));
                    }
                    continue;
                }
                if let Some(frag) = crate::federation::reduce_to_scope(obj, reg, &parsed.ctx) {
                    fwd.push((reg.clone(), frag));
                }
            }
            let proxies: Vec<&crate::federation::FedReg> =
                regs.iter().filter(|r| r.is_proxy()).collect();
            let (rest, has_attrs) = crate::federation::strip_proxied(obj, &proxies, &parsed.ctx);
            if has_attrs || proxies.is_empty() {
                let mut local = expand_entity(
                    &rest,
                    &parsed.ctx,
                    ExpandOpts {
                        fragment: true,
                        allow_null: true,
                        temporal: true,
                        ..Default::default()
                    },
                )?;
                let ts = now_iso();
                stamp_instances(&mut local, &ts);
                let res = st.store.mutate(&tenant, Kind::Temporal, &id, |doc| {
                    add_temporal_instances(doc, &local, &ts);
                    Ok::<(), NgsiError>(())
                })?;
                parts.push(match res {
                    Some(Ok(())) => crate::federation::Part {
                        status: 204,
                        detail: "added locally".into(),
                    },
                    _ => crate::federation::Part {
                        status: 404,
                        detail: format!("temporal entity {id} not found locally"),
                    },
                });
            }
            let ctx_url = crate::federation::ctx_link_url(&headers, &parsed.ctx.source);
            for (reg, frag) in fwd {
                parts.push(
                    crate::federation::forward_part(
                        &st,
                        reqwest::Method::POST,
                        format!("{}/ngsi-ld/v1/temporal/entities/{id}/attrs", reg.endpoint),
                        &[],
                        &headers,
                        &tenant,
                        &reg,
                        &ctx_url,
                        Some(frag),
                    )
                    .await,
                );
            }
            return Ok(crate::federation::combine(
                parts,
                no_content(&tenant),
                &tenant,
            ));
        }
        let ts = now_iso();
        stamp_instances(&mut expanded, &ts);
        let res = st.store.mutate(&tenant, Kind::Temporal, &id, |doc| {
            add_temporal_instances(doc, &expanded, &ts);
            Ok::<(), NgsiError>(())
        })?;
        match res {
            None => {
                Err(NgsiError::ResourceNotFound(format!("temporal entity {id} not found")).into())
            }
            Some(Err(e)) => Err(ApiError::from(e)),
            Some(Ok(())) => Ok(no_content(&tenant)),
        }
    };
    go.await.unwrap_or_else(|e| e.into_response())
}

/// 5.6.12.4: append the fragment's Attribute instances to the Temporal
/// Evolution (instances accumulate — history is never overwritten here).
fn add_temporal_instances(doc: &mut Value, expanded: &Value, ts: &str) {
    let Some(target) = doc.as_object_mut() else {
        return;
    };
    for (k, v) in expanded.as_object().into_iter().flatten() {
        if is_meta(k) {
            continue;
        }
        let incoming = v.as_array().cloned().unwrap_or_default();
        match target.get_mut(k).and_then(Value::as_array_mut) {
            Some(cur) => cur.extend(incoming),
            None => {
                target.insert(k.clone(), Value::Array(incoming));
            }
        }
    }
    target.insert("modifiedAt".into(), Value::String(ts.to_owned()));
}

// ---------- DELETE /temporal/entities/{id}/attrs/{attrId} (5.6.13) ----------

pub async fn delete_temporal_attr(
    State(st): State<AppState>,
    Path((id, attr)): Path<(String, String)>,
    CleanParams(params): CleanParams,
    headers: HeaderMap,
) -> Response {
    let go = async {
        let tenant = tenant_from(&headers)?;
        antares_model::EntityId::new(&id)?;
        if attr.is_empty()
            || !attr
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || "_:.#/%-+@".contains(c))
        {
            return Err(
                NgsiError::BadRequestData(format!("invalid attribute name {attr:?}")).into(),
            );
        }
        check_params(&params, &["datasetId", "deleteAll", "local"])?;
        let ctx = request_context(&st.loader, &headers).await?;
        let attr_iri = ctx.expand_key(&attr);
        let delete_all = params.get("deleteAll").map(String::as_str) == Some("true");
        let want_ds = params.get("datasetId").cloned();
        let mut found = false;
        let ts = now_iso();
        let res = st.store.mutate(&tenant, Kind::Temporal, &id, |doc| {
            let target = doc.as_object_mut().expect("temporal object");
            if delete_all
                || (want_ds.is_none()
                    && !target
                        .get(&attr_iri)
                        .and_then(Value::as_array)
                        .is_some_and(|a| a.iter().any(|i| i.get("datasetId").is_some())))
            {
                // deleteAll, or single-instance-set attribute: drop it whole
                if target.remove(&attr_iri).is_some() {
                    found = true;
                }
            } else if let Some(arr) = target.get_mut(&attr_iri).and_then(Value::as_array_mut) {
                // 5.6.13: only the matching datasetId instance set is deleted
                let before = arr.len();
                arr.retain(|i| i.get("datasetId").and_then(Value::as_str) != want_ds.as_deref());
                found = arr.len() != before;
                if arr.is_empty() {
                    target.remove(&attr_iri);
                }
            }
            if found {
                target.insert("modifiedAt".into(), Value::String(ts.clone()));
            }
            Ok::<(), NgsiError>(())
        })?;
        let local_part = match &res {
            None => crate::federation::Part {
                status: 404,
                detail: format!("temporal entity {id} not found locally"),
            },
            Some(_) if found => crate::federation::Part {
                status: 204,
                detail: "deleted locally".into(),
            },
            Some(_) => crate::federation::Part {
                status: 404,
                detail: format!("attribute {attr} not found locally"),
            },
        };
        if let Some(r) = temporal_attr_fed(
            &st,
            &tenant,
            &headers,
            &ctx,
            &params,
            &id,
            "deleteAttrsTemporal",
            reqwest::Method::DELETE,
            &format!("/attrs/{attr}"),
            None,
            local_part,
        )
        .await?
        {
            return Ok(r);
        }
        match res {
            None => {
                Err(NgsiError::ResourceNotFound(format!("temporal entity {id} not found")).into())
            }
            Some(Err(e)) => Err(ApiError::from(e)),
            Some(Ok(())) if found => Ok(no_content(&tenant)),
            Some(Ok(())) => {
                Err(NgsiError::ResourceNotFound(format!("attribute {attr} not found")).into())
            }
        }
    };
    go.await.unwrap_or_else(|e| e.into_response())
}

/// 5.6.13.4-5.6.15.4 shared forwarding: proxy registrations without the
/// operation's support are an error of type Conflict and are never
/// contacted; supporting registrations receive the forwarded request. None
/// = no matching registrations (the operation stays purely local).
#[allow(clippy::too_many_arguments)]
async fn temporal_attr_fed(
    st: &AppState,
    tenant: &antares_model::TenantId,
    headers: &HeaderMap,
    ctx: &antares_jsonld::Context,
    params: &HashMap<String, String>,
    id: &str,
    op: &str,
    method: reqwest::Method,
    path_suffix: &str,
    body: Option<Value>,
    local_part: crate::federation::Part,
) -> ApiResult<Option<Response>> {
    let spec = crate::csource::CsrSpec {
        ids: Some(vec![id.to_owned()]),
        ..Default::default()
    };
    let mut regs = crate::federation::write_regs(st, tenant, &spec, ctx, params, headers);
    if let Some(r) = crate::federation::handle_via_loop(
        headers,
        &crate::federation::alias_for(&st.host_alias, tenant),
        tenant,
        &mut regs,
    ) {
        return Ok(Some(r));
    }
    if regs.is_empty() {
        return Ok(None);
    }
    let mut parts = vec![local_part];
    let ctx_url = crate::federation::ctx_link_url(headers, &ctx.source);
    for reg in &regs {
        if !reg.supports(op) {
            if reg.is_proxy() {
                parts.push(crate::federation::conflict_part(op));
            }
            continue;
        }
        parts.push(
            crate::federation::forward_part(
                st,
                method.clone(),
                format!(
                    "{}/ngsi-ld/v1/temporal/entities/{id}{path_suffix}",
                    reg.endpoint
                ),
                &[],
                headers,
                tenant,
                reg,
                &ctx_url,
                body.clone(),
            )
            .await,
        );
    }
    Ok(Some(crate::federation::combine(
        parts,
        no_content(tenant),
        tenant,
    )))
}

// ---------- PATCH/DELETE .../attrs/{attrId}/{instanceId} (5.6.14/5.6.15) ----------

pub async fn modify_temporal_instance(
    State(st): State<AppState>,
    Path((id, attr, instance_id)): Path<(String, String, String)>,
    CleanParams(params): CleanParams,
    headers: HeaderMap,
    body: Bytes,
) -> Response {
    let go = async {
        let tenant = tenant_from(&headers)?;
        // Empty attr segment ⇒ the URI names no resource with a PATCH method
        // (suite 016_02_06 asserts 405 here, vs 400 on the DELETE sibling).
        if attr.is_empty() {
            return Err(ApiError::Bare(StatusCode::METHOD_NOT_ALLOWED));
        }
        antares_model::EntityId::new(&id)?;
        crate::attrs::check_attr_name(&attr)?;
        antares_model::EntityId::new(&instance_id)
            .map_err(|_| NgsiError::BadRequestData("invalid instance id".into()))?;
        check_params(&params, &["local"])?;
        let parsed = parse_body(&st.loader, &headers, &body, BodyKind::MergePatch).await?;
        let obj = parsed
            .value
            .as_object()
            .ok_or_else(|| NgsiError::BadRequestData("fragment must be a JSON object".into()))?;
        let mut wrapper = Map::new();
        let mut frag = obj.clone();
        frag.remove("@context");
        wrapper.insert(attr.clone(), Value::Object(frag));
        let expanded = expand_entity(
            &wrapper,
            &parsed.ctx,
            ExpandOpts {
                fragment: true,
                allow_null: false,
                temporal: true,
                ..Default::default()
            },
        )?;
        let attr_iri = parsed.ctx.expand_key(&attr);
        let frag_inst = expanded
            .get(&attr_iri)
            .and_then(Value::as_array)
            .and_then(|a| a.first())
            .cloned()
            .ok_or_else(|| NgsiError::BadRequestData("invalid instance fragment".into()))?;
        let ts = now_iso();
        let mut found = false;
        let res = st.store.mutate(&tenant, Kind::Temporal, &id, |doc| {
            let target = doc.as_object_mut().expect("temporal object");
            if let Some(arr) = target.get_mut(&attr_iri).and_then(Value::as_array_mut) {
                if let Some(inst) = arr.iter_mut().find(|i| {
                    i.get("instanceId").and_then(Value::as_str) == Some(instance_id.as_str())
                }) {
                    found = true;
                    let t = inst.as_object_mut().expect("instance");
                    for (k, v) in frag_inst.as_object().expect("fragment") {
                        if matches!(k.as_str(), "createdAt" | "instanceId") {
                            continue;
                        }
                        t.insert(k.clone(), v.clone());
                    }
                    t.insert("modifiedAt".into(), Value::String(ts.clone()));
                }
            }
            Ok::<(), NgsiError>(())
        })?;
        let local_part = match &res {
            None => crate::federation::Part {
                status: 404,
                detail: format!("temporal entity {id} not found locally"),
            },
            Some(_) if found => crate::federation::Part {
                status: 204,
                detail: "applied locally".into(),
            },
            Some(_) => crate::federation::Part {
                status: 404,
                detail: format!("instance {instance_id} not found locally"),
            },
        };
        if let Some(r) = temporal_attr_fed(
            &st,
            &tenant,
            &headers,
            &parsed.ctx,
            &params,
            &id,
            "updateAttrInstanceTemporal",
            reqwest::Method::PATCH,
            &format!("/attrs/{attr}/{instance_id}"),
            Some(parsed.value.clone()),
            local_part,
        )
        .await?
        {
            return Ok(r);
        }
        match res {
            None => {
                Err(NgsiError::ResourceNotFound(format!("temporal entity {id} not found")).into())
            }
            Some(Err(e)) => Err(ApiError::from(e)),
            Some(Ok(())) if found => Ok(no_content(&tenant)),
            Some(Ok(())) => {
                Err(NgsiError::ResourceNotFound(format!("instance {instance_id} not found")).into())
            }
        }
    };
    go.await.unwrap_or_else(|e| e.into_response())
}

pub async fn delete_temporal_instance(
    State(st): State<AppState>,
    Path((id, attr, instance_id)): Path<(String, String, String)>,
    CleanParams(params): CleanParams,
    headers: HeaderMap,
) -> Response {
    let go = async {
        let tenant = tenant_from(&headers)?;
        antares_model::EntityId::new(&id)?;
        crate::attrs::check_attr_name(&attr)?;
        antares_model::EntityId::new(&instance_id)
            .map_err(|_| NgsiError::BadRequestData("invalid instance id".into()))?;
        check_params(&params, &["local"])?;
        let ctx = request_context(&st.loader, &headers).await?;
        let attr_iri = ctx.expand_key(&attr);
        let mut found = false;
        let ts = now_iso();
        let res = st.store.mutate(&tenant, Kind::Temporal, &id, |doc| {
            let target = doc.as_object_mut().expect("temporal object");
            if let Some(arr) = target.get_mut(&attr_iri).and_then(Value::as_array_mut) {
                let before = arr.len();
                arr.retain(|i| {
                    i.get("instanceId").and_then(Value::as_str) != Some(instance_id.as_str())
                });
                found = arr.len() != before;
                if arr.is_empty() {
                    target.remove(&attr_iri);
                }
            }
            if found {
                target.insert("modifiedAt".into(), Value::String(ts.clone()));
            }
            Ok::<(), NgsiError>(())
        })?;
        let local_part = match &res {
            None => crate::federation::Part {
                status: 404,
                detail: format!("temporal entity {id} not found locally"),
            },
            Some(_) if found => crate::federation::Part {
                status: 204,
                detail: "applied locally".into(),
            },
            Some(_) => crate::federation::Part {
                status: 404,
                detail: format!("instance {instance_id} not found locally"),
            },
        };
        if let Some(r) = temporal_attr_fed(
            &st,
            &tenant,
            &headers,
            &ctx,
            &params,
            &id,
            "deleteAttrInstanceTemporal",
            reqwest::Method::DELETE,
            &format!("/attrs/{attr}/{instance_id}"),
            None,
            local_part,
        )
        .await?
        {
            return Ok(r);
        }
        match res {
            None => {
                Err(NgsiError::ResourceNotFound(format!("temporal entity {id} not found")).into())
            }
            Some(Err(e)) => Err(ApiError::from(e)),
            Some(Ok(())) if found => Ok(no_content(&tenant)),
            Some(Ok(())) => {
                Err(NgsiError::ResourceNotFound(format!("instance {instance_id} not found")).into())
            }
        }
    };
    go.await.unwrap_or_else(|e| e.into_response())
}

// ---------- POST /temporal/entityOperations/query (6.24) ----------

pub async fn batch_temporal_query(
    State(st): State<AppState>,
    CleanParams(params): CleanParams,
    headers: HeaderMap,
    body: Bytes,
) -> Response {
    let go = async {
        let tenant = tenant_from(&headers)?;
        check_params(
            &params,
            &["limit", "offset", "count", "options", "format", "local"],
        )?;
        let accept = parse_accept(&headers)?;
        let parsed = parse_body(&st.loader, &headers, &body, BodyKind::Standard).await?;
        let q = parsed
            .value
            .as_object()
            .ok_or_else(|| NgsiError::BadRequestData("query body must be an object".into()))?;
        if q.get("type").and_then(Value::as_str) != Some("Query") {
            return Err(NgsiError::BadRequestData("body type must be Query".into()).into());
        }
        // 5.2.23 Query (temporal reading): members flattened with their
        // Table 5.2.23-1 value spaces enforced, incl. temporalQ (5.2.21)
        // and aggrParams (5.2.44).
        let mut vp: HashMap<String, String> = params.clone();
        crate::batch::query_doc_params(q, true, &mut vp)?;
        let _ = accept;
        query_temporal_inner_with(&st, &vp, &headers, &tenant).await
    };
    go.await.unwrap_or_else(|e| e.into_response())
}

async fn query_temporal_inner_with(
    st: &AppState,
    params: &HashMap<String, String>,
    headers: &HeaderMap,
    _tenant: &TenantId,
) -> ApiResult<Response> {
    query_temporal_inner(st, params, headers).await
}

#[cfg(test)]
mod clause_4_11 {
    use super::*;
    use serde_json::json;

    fn tq(timerel: &str, time_at: &str, end: Option<&str>) -> TemporalQ {
        let mut p = HashMap::new();
        p.insert("timerel".to_owned(), timerel.to_owned());
        p.insert("timeAt".to_owned(), time_at.to_owned());
        if let Some(e) = end {
            p.insert("endTimeAt".to_owned(), e.to_owned());
        }
        TemporalQ::from_params(&p, true).unwrap().unwrap()
    }

    fn inst(observed_at: &str) -> Value {
        json!({"observedAt": observed_at, "value": 1})
    }

    /// 4.11 after: "The specified value is used as an INCLUSIVE bound" — an
    /// instance at exactly timeAt matches, regardless of the equal instant
    /// being written with or without a seconds fraction (4.6.3 allows both).
    #[test]
    fn after_is_inclusive_across_fraction_forms() {
        let q = tq("after", "2017-12-13T14:20:00Z", None);
        assert!(q.instance_matches(&inst("2017-12-13T14:20:00Z")));
        assert!(
            q.instance_matches(&inst("2017-12-13T14:20:00.000Z")),
            "same instant with a fraction must be included"
        );
        assert!(!q.instance_matches(&inst("2017-12-13T14:19:59.999999Z")));
    }

    /// 4.11 before: "The specified value is used as an EXCLUSIVE bound" — an
    /// instance at exactly timeAt does not match, in any equal spelling.
    #[test]
    fn before_is_exclusive_across_fraction_forms() {
        let q = tq("before", "2017-12-13T14:20:00Z", None);
        assert!(!q.instance_matches(&inst("2017-12-13T14:20:00Z")));
        assert!(
            !q.instance_matches(&inst("2017-12-13T14:20:00.000Z")),
            "same instant with a fraction must stay excluded"
        );
        assert!(q.instance_matches(&inst("2017-12-13T14:19:59.999999Z")));
    }

    /// 4.11 between: "the lower bound of the range is inclusive and ... the
    /// upper bound of the range is exclusive."
    #[test]
    fn between_bounds_inclusive_lower_exclusive_upper() {
        let q = tq(
            "between",
            "2017-12-13T14:20:00Z",
            Some("2017-12-13T14:40:00Z"),
        );
        assert!(
            q.instance_matches(&inst("2017-12-13T14:20:00.000Z")),
            "lower incl"
        );
        assert!(q.instance_matches(&inst("2017-12-13T14:30:00Z")));
        assert!(
            !q.instance_matches(&inst("2017-12-13T14:40:00.000Z")),
            "upper excl in any spelling"
        );
        assert!(!q.instance_matches(&inst("2017-12-13T14:19:59Z")));
    }

    /// 4.6.3: "a comma instead of a decimal point may be used" in requests —
    /// the comma form must compare as the same instant.
    #[test]
    fn comma_fraction_compares_as_the_same_instant() {
        let q = tq("after", "2017-12-13T14:20:00,500000Z", None);
        assert!(q.instance_matches(&inst("2017-12-13T14:20:00.5Z")));
        assert!(!q.instance_matches(&inst("2017-12-13T14:20:00.499999Z")));
    }

    /// 4.11: "Entities which do not convey the target Temporal Property of
    /// the query shall be considered as non-matching" + timeproperty
    /// defaults to observedAt.
    #[test]
    fn missing_timeproperty_is_a_nonmatch_and_default_is_observed_at() {
        let q = tq("after", "1970-01-01T00:00:00Z", None);
        assert_eq!(q.timeproperty, "observedAt");
        assert!(!q.instance_matches(&json!({"modifiedAt": "2020-01-01T00:00:00Z"})));
    }

    /// 4.11 grammar: only before/after/between; timeAt mandatory and a
    /// DateTime; between requires endTimeAt.
    #[test]
    fn grammar_rejections() {
        let mk = |pairs: &[(&str, &str)]| {
            let mut p = HashMap::new();
            for (k, v) in pairs {
                p.insert((*k).to_owned(), (*v).to_owned());
            }
            TemporalQ::from_params(&p, false)
        };
        assert!(mk(&[("timerel", "during"), ("timeAt", "2020-01-01T00:00:00Z")]).is_err());
        assert!(mk(&[("timerel", "before")]).is_err(), "timeAt mandatory");
        assert!(
            mk(&[("timerel", "before"), ("timeAt", "2020-01-01")]).is_err(),
            "Date is not a DateTime"
        );
        assert!(
            mk(&[("timerel", "between"), ("timeAt", "2020-01-01T00:00:00Z")]).is_err(),
            "between requires endTimeAt"
        );
        assert!(
            mk(&[("timeAt", "2020-01-01T00:00:00Z")]).is_err(),
            "timeAt without timerel"
        );
    }
}
