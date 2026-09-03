// SPDX-License-Identifier: EUPL-1.2
//! Paging, ordering and the query-body parameters shared by every list
//! operation: the limit/offset/count triple and the next/prev links
//! (4.12, 6.3.10), entity ordering and ICU collation (4.23), the
//! `NGSILD-Warning` header (6.3.17) and the POST /entityOperations/query
//! body lifted into the same parameters its GET twin carries (5.2.23).

use crate::negotiate::{Accept, ApiResult};
use crate::state::AppState;
use antares_model::NgsiError;
use axum::response::Response;
use serde_json::{Map, Value};
use std::collections::HashMap;

/// 6.3.17: one `NGSILD-Warning` header per abnormal distributed-GET outcome —
/// scoped by the clause to /entities and /entities/{id}.
pub fn attach_warnings(resp: &mut Response, warnings: &[String]) {
    for w in warnings {
        if let Ok(v) = axum::http::HeaderValue::from_str(w) {
            resp.headers_mut().append("NGSILD-Warning", v);
        }
    }
}

/// limit/offset/count handling (6.3.10). Returns (page, count, link headers).
/// 4.12/5.5.9.1 Pagination: L = client limit (Mc) or the default (Md); at
/// most L elements per page; remaining elements are flagged with a next
/// pointer carrying every parameter needed to fetch the page, prev on every
/// iteration but the first, and only prev on the last. Shared by every
/// paginated list operation (5.7.2, 5.7.4, 5.8.4, 5.10.2, 5.11.5).
pub fn paginate(
    st: &AppState,
    params: &HashMap<String, String>,
    matches: Vec<Value>,
    path: &str,
) -> ApiResult<(Vec<Value>, Option<usize>, Vec<String>)> {
    paginate_impl(st, params, matches, path, Accept::Json, None)
}

/// The store already applied ORDER BY id + LIMIT/OFFSET and counted the
/// match set — `matches` IS the page; only count/links remain.
pub fn paginate_pre(
    st: &AppState,
    params: &HashMap<String, String>,
    page: Vec<Value>,
    path: &str,
    total: usize,
) -> ApiResult<(Vec<Value>, Option<usize>, Vec<String>)> {
    paginate_impl(st, params, page, path, Accept::Json, Some(total))
}

/// `paginate_pre` for a caller that negotiated a media type: the store
/// already applied ORDER BY id + LIMIT/OFFSET and counted the match set.
pub fn paginate_pre_accept(
    st: &AppState,
    params: &HashMap<String, String>,
    page: Vec<Value>,
    path: &str,
    accept: Accept,
    total: usize,
) -> ApiResult<(Vec<Value>, Option<usize>, Vec<String>)> {
    paginate_impl(st, params, page, path, accept, Some(total))
}

/// 4.12 Pagination: clients specify a limit (page size), the server defines
/// a default page size, and a hard ceiling is rejected with TooManyResults
/// rather than silently clamped. The limit/offset/count triple of 6.3.10,
/// validated (ceilings included). Shared by `paginate_impl` and the
/// pushdown gate so the two paths can never disagree on what a page is.
pub fn page_params(
    st: &AppState,
    params: &HashMap<String, String>,
) -> ApiResult<(usize, usize, bool)> {
    let count = params.get("count").map(String::as_str) == Some("true");
    let limit: usize = match params.get("limit") {
        Some(l) => l
            .parse()
            .map_err(|_| NgsiError::BadRequestData(format!("invalid limit {l:?}")))?,
        None => st.default_limit,
    };
    // 5.5.6: "so many results that can potentially exhaust client or server
    // resources" — the implementation threshold is max_limit; 403
    // TooManyResults, not silent clamping.
    if limit > st.max_limit {
        return Err(NgsiError::TooManyResults(format!(
            "limit {limit} exceeds the server maximum {}",
            st.max_limit
        ))
        .into());
    }
    if limit == 0 && !count {
        return Err(
            NgsiError::BadRequestData("limit=0 requires count=true (6.3.10)".into()).into(),
        );
    }
    let offset: usize = match params.get("offset") {
        Some(o) => o
            .parse()
            .map_err(|_| NgsiError::BadRequestData(format!("invalid offset {o:?}")))?,
        None => 0,
    };
    // An offset above i64::MAX wraps negative when bound as SQL `$n::bigint`
    // (Postgres then rejects a negative OFFSET → 500). Reject it as a bad
    // precondition instead.
    if offset > i64::MAX as usize {
        return Err(NgsiError::BadRequestData(format!("offset {offset} is out of range")).into());
    }
    Ok((offset, limit, count))
}

/// 6.3.10: next/prev Links carry the response media type; the suite asserts
/// `;type="application/ld+json"` on ld+json list responses (031_02).
pub fn paginate_accept(
    st: &AppState,
    params: &HashMap<String, String>,
    matches: Vec<Value>,
    path: &str,
    accept: Accept,
) -> ApiResult<(Vec<Value>, Option<usize>, Vec<String>)> {
    paginate_impl(st, params, matches, path, accept, None)
}

fn paginate_impl(
    st: &AppState,
    params: &HashMap<String, String>,
    matches: Vec<Value>,
    path: &str,
    accept: Accept,
    pre: Option<usize>,
) -> ApiResult<(Vec<Value>, Option<usize>, Vec<String>)> {
    let (offset, limit, count) = page_params(st, params)?;
    let total = pre.unwrap_or(matches.len());
    let page: Vec<Value> = match pre {
        Some(_) => matches, // already exactly the page (store pushdown)
        None => matches.into_iter().skip(offset).take(limit).collect(),
    };
    let mut links = Vec::new();
    // csource resources: the suite string-compares links against
    // `?other…&limit=N&offset=M` order with an unconditional ld+json type
    // suffix (037_11, 041_03); entity lists keep sorted params + accept-based
    // suffix (031_02).
    let csource_style = path.contains("csource");
    let mut mk = |off: usize, rel: &str| {
        let mut qp: Vec<String>;
        if csource_style {
            qp = params
                .iter()
                .filter(|(k, _)| !matches!(k.as_str(), "offset" | "limit"))
                .map(|(k, v)| format!("{k}={}", query_value(v)))
                .collect();
            qp.sort();
            if let Some(l) = params.get("limit") {
                qp.push(format!("limit={l}"));
            }
            qp.push(format!("offset={off}"));
        } else {
            qp = params
                .iter()
                .filter(|(k, _)| k.as_str() != "offset")
                .map(|(k, v)| format!("{k}={}", query_value(v)))
                .collect();
            qp.push(format!("offset={off}"));
            qp.sort(); // deterministic order — the suite string-compares links
        }
        // 6.3.10: "At least, the type Link Target Attribute shall be included
        // ... and its value shall be exactly equal to the media type resulting
        // from the original request" — for EVERY media type, not just ld+json.
        let ty = match accept {
            _ if csource_style => ";type=\"application/ld+json\"",
            Accept::LdJson => ";type=\"application/ld+json\"",
            Accept::Json => ";type=\"application/json\"",
            Accept::GeoJson => ";type=\"application/geo+json\"",
        };
        links.push(format!("<{path}?{}>; rel=\"{rel}\"{ty}", qp.join("&")));
    };
    if offset + limit < total && limit > 0 {
        mk(offset + limit, "next");
    }
    if offset > 0 {
        mk(offset.saturating_sub(limit.max(1)), "prev");
    }
    Ok((page, count.then_some(total), links))
}

/// Sort by an orderBy spec: comma-separated `member[;asc|desc]`.
/// 4.23 Entity Ordering: orderBy = `AttrName[;direction] *(, …)` with asc
/// (default) / desc / dist-asc / dist-desc (4.23.3); distance keys need the
/// orderFrom reference coordinates (orderGeometry, default Point) and apply
/// to GeoProperties — non-GeoProperties fall back to value order after them
/// (4.23.2). Mixed datatypes rank Numbers < Strings < Object < Array <
/// Boolean < Time < Date < DateTime < Null < absent (4.23.2). Paths may be
/// dotted (EXAMPLE 5) or carry one trailing [member.path] bracket
/// (EXAMPLE 4). String comparison is codepoint order by default; the
/// `collation` parameter selects an ICU collation (4.23.3 EXAMPLES 6/7).
///
/// 4.23.3 EXAMPLES 6/7: the ICU collator for an RFC 6067 collation tag
/// (e.g. und-u-ks-identic, de-u-co-phonebk). The co/kf/kn keywords travel
/// via CollatorPreferences; the -u-ks strength keyword maps onto
/// CollatorOptions. Invalid/unsupported tags are BadRequestData.
/// 5.7.2.4 / 5.7.4.4: "If a preferred collation setting is present and it
/// does not conform to a valid ICU collation (see IETF RFC 6067 \[36\]) then an
/// error of type BadRequestData shall be raised." The clause names the
/// parameter's presence, not an `orderBy` that happens to consume it, so the
/// check runs on every operation that accepts `collation`.
pub fn check_collation(params: &HashMap<String, String>) -> Result<(), NgsiError> {
    match params.get("collation") {
        Some(tag) => build_collator(tag).map(|_| ()),
        None => Ok(()),
    }
}

fn build_collator(tag: &str) -> Result<icu_collator::CollatorBorrowed<'static>, NgsiError> {
    let bad = |m: String| NgsiError::BadRequestData(m);
    let locale: icu_locale_core::Locale = tag.parse().map_err(|_| {
        bad(format!(
            "collation is not an RFC 6067 tag: {tag:?} (4.23.3)"
        ))
    })?;
    let mut opts = icu_collator::options::CollatorOptions::default();
    use icu_collator::options::Strength;
    use icu_locale_core::extensions::unicode::key;
    if let Some(ks) = locale.extensions.unicode.keywords.get(&key!("ks")) {
        opts.strength = Some(match ks.to_string().as_str() {
            "level1" => Strength::Primary,
            "level2" => Strength::Secondary,
            "level3" => Strength::Tertiary,
            "level4" => Strength::Quaternary,
            "identic" => Strength::Identical,
            other => {
                return Err(bad(format!(
                    "unknown collation strength {other:?} (4.23.3)"
                )))
            }
        });
    }
    icu_collator::Collator::try_new((&locale).into(), opts)
        .map_err(|_| bad(format!("unsupported collation {tag:?} (4.23.3)")))
}

pub fn order_entities(
    docs: &mut [Value],
    spec: &str,
    params: &HashMap<String, String>,
    ctx: &antares_jsonld::Context,
) -> Result<(), NgsiError> {
    #[derive(PartialEq)]
    enum Dir {
        Asc,
        Desc,
        DistAsc,
        DistDesc,
    }
    struct Key {
        path: Vec<String>,
        bracket: Option<Vec<String>>,
        dir: Dir,
    }
    let bad = |m: String| NgsiError::BadRequestData(m);
    let mut keys = Vec::new();
    for part in spec.split(',') {
        let part = part.trim();
        let (member, dir) = match part.split_once(';') {
            Some((m, d)) => (m.trim(), d.trim()),
            None => (part, "asc"),
        };
        let dir = match dir {
            "asc" => Dir::Asc,
            "desc" => Dir::Desc,
            "dist-asc" => Dir::DistAsc,
            "dist-desc" => Dir::DistDesc,
            _ => {
                return Err(bad(format!(
                    "invalid orderBy direction in {spec:?} (4.23.3)"
                )))
            }
        };
        // one trailing [member.path] bracket (EXAMPLE 4)
        let (head, bracket) = match member.split_once('[') {
            Some((h, rest)) => {
                let inner = rest
                    .strip_suffix(']')
                    .ok_or_else(|| bad(format!("unclosed bracket in orderBy {spec:?}")))?;
                (h, Some(inner.split('.').map(str::to_owned).collect()))
            }
            None => (member, None),
        };
        if head.is_empty() {
            return Err(bad(format!("invalid orderBy {spec:?} (4.23)")));
        }
        keys.push(Key {
            path: head.split('.').map(str::to_owned).collect(),
            bracket,
            dir,
        });
    }
    // 4.23.3 EXAMPLES 6/7: collation names an ICU ordering for strings
    let collator = params
        .get("collation")
        .map(|t| build_collator(t))
        .transpose()?;
    // dist-* keys need the orderFrom reference geometry (4.23.3 EXAMPLE 8-10)
    let refg = if keys
        .iter()
        .any(|k| matches!(k.dir, Dir::DistAsc | Dir::DistDesc))
    {
        let coords_raw = params
            .get("orderFrom")
            .ok_or_else(|| bad("dist ordering requires orderFrom (4.23.3)".into()))?;
        let coords: Value = serde_json::from_str(coords_raw)
            .map_err(|_| bad(format!("invalid orderFrom {coords_raw:?}")))?;
        let gtype = params
            .get("orderGeometry")
            .cloned()
            .unwrap_or_else(|| "Point".into());
        Some(crate::geo::parse_ref_geometry(&gtype, &coords).map_err(bad)?)
    } else {
        None
    };
    fn order_value(doc: &Value, k: &Key, ctx: &antares_jsonld::Context) -> Option<Value> {
        let path = &k.path;
        let head = path.first()?;
        let base = match head.as_str() {
            "id" | "createdAt" | "modifiedAt" => doc.get(head.as_str()).cloned(),
            "type" => doc["type"].as_array().and_then(|a| a.first()).cloned(),
            _ => {
                let iri = ctx.expand_key(head);
                let inst = doc.get(&iri).and_then(Value::as_array)?.first()?;
                let mut cur = inst;
                for seg in &path[1..] {
                    match seg.as_str() {
                        "createdAt" | "modifiedAt" | "observedAt" | "datasetId" | "unitCode" => {
                            cur = cur.get(seg.as_str())?;
                        }
                        _ => {
                            let siri = ctx.expand_key(seg);
                            cur = cur
                                .get(&siri)
                                .and_then(Value::as_array)
                                .and_then(|a| a.first())?;
                        }
                    }
                }
                match cur.get("value").or_else(|| cur.get("object")) {
                    Some(v) => Some(v.clone()),
                    None => Some(cur.clone()),
                }
            }
        }?;
        match &k.bracket {
            None => Some(base),
            Some(b) => {
                let mut cur = &base;
                for seg in b {
                    cur = cur.get(seg)?;
                }
                Some(cur.clone())
            }
        }
    }
    /// 4.23.2 datatype rank: Numbers < Strings < Object < Array < Boolean <
    /// Time < Date < DateTime < Null (absent is handled as Option::None).
    fn rank(v: &Value) -> u8 {
        match v {
            Value::Number(_) => 0,
            Value::String(s) => {
                if antares_jsonld::parse_datetime(s) {
                    7
                } else if is_date(s) {
                    6
                } else if is_time(s) {
                    5
                } else {
                    1
                }
            }
            Value::Object(_) => 2,
            Value::Array(_) => 3,
            Value::Bool(_) => 4,
            Value::Null => 8,
        }
    }
    /// 4.6.3 Date: YYYY-MM-DD, all components present.
    fn is_date(s: &str) -> bool {
        let b = s.as_bytes();
        b.len() == 10
            && b[4] == b'-'
            && b[7] == b'-'
            && b.iter()
                .enumerate()
                .all(|(i, c)| matches!(i, 4 | 7) || c.is_ascii_digit())
    }
    /// 4.6.3 Time: hh:mm:ss[.f*]Z.
    fn is_time(s: &str) -> bool {
        let b = s.as_bytes();
        b.len() >= 9
            && b[b.len() - 1] == b'Z'
            && b[2] == b':'
            && b[5] == b':'
            && b[..2].iter().all(u8::is_ascii_digit)
            && b[3..5].iter().all(u8::is_ascii_digit)
            && b[6..8].iter().all(u8::is_ascii_digit)
    }
    fn cmp_vals(
        a: &Option<Value>,
        b: &Option<Value>,
        coll: Option<&icu_collator::CollatorBorrowed<'static>>,
    ) -> std::cmp::Ordering {
        use std::cmp::Ordering;
        match (a, b) {
            (None, None) => Ordering::Equal,
            (None, Some(_)) => Ordering::Greater, // absent sorts last (4.23.2)
            (Some(_), None) => Ordering::Less,
            (Some(x), Some(y)) => {
                let (rx, ry) = (rank(x), rank(y));
                if rx != ry {
                    return rx.cmp(&ry);
                }
                match (x, y) {
                    (Value::Number(_), Value::Number(_)) => x
                        .as_f64()
                        .unwrap_or(f64::NAN)
                        .total_cmp(&y.as_f64().unwrap_or(f64::NAN)),
                    (Value::Bool(bx), Value::Bool(by)) => bx.cmp(by),
                    (Value::String(sx), Value::String(sy)) => {
                        if rx == 7 {
                            // DateTime: canonical key so equal instants in
                            // different 4.6.3 fraction spellings tie (4.11)
                            antares_model::dt_key(sx).cmp(&antares_model::dt_key(sy))
                        } else if let Some(c) = coll {
                            // 4.23.3 EXAMPLES 6/7: the named ICU collation
                            c.compare(sx, sy)
                        } else {
                            // 4.23.1 default: codepoint order
                            sx.cmp(sy)
                        }
                    }
                    _ => x.to_string().cmp(&y.to_string()),
                }
            }
        }
    }
    docs.sort_by(|a, b| {
        use std::cmp::Ordering;
        for k in &keys {
            let o = match k.dir {
                Dir::Asc | Dir::Desc => {
                    let va = order_value(a, k, ctx);
                    let vb = order_value(b, k, ctx);
                    let mut o = cmp_vals(&va, &vb, collator.as_ref());
                    if k.dir == Dir::Desc {
                        o = o.reverse();
                    }
                    o
                }
                Dir::DistAsc | Dir::DistDesc => {
                    // Set whenever a Dist ordering was accepted. Without it
                    // there is no distance to compare, and every pair being
                    // equal leaves the previous order untouched.
                    let Some(refg) = refg.as_ref() else {
                        return std::cmp::Ordering::Equal;
                    };
                    let da =
                        order_value(a, k, ctx).and_then(|v| crate::geo::order_distance_m(refg, &v));
                    let db =
                        order_value(b, k, ctx).and_then(|v| crate::geo::order_distance_m(refg, &v));
                    match (da, db) {
                        (Some(x), Some(y)) => {
                            let mut o = x.total_cmp(&y);
                            if k.dir == Dir::DistDesc {
                                o = o.reverse();
                            }
                            o
                        }
                        // 4.23.2 distance order: GeoProperties (by distance)
                        // rank before non-GeoProperties (by value)
                        (Some(_), None) => Ordering::Less,
                        (None, Some(_)) => Ordering::Greater,
                        (None, None) => {
                            let va = order_value(a, k, ctx);
                            let vb = order_value(b, k, ctx);
                            cmp_vals(&va, &vb, collator.as_ref())
                        }
                    }
                }
            };
            if o != Ordering::Equal {
                return o;
            }
        }
        Ordering::Equal
    });
    Ok(())
}

/// 5.2.23 Query: flatten the JSON members into query-param form with the
/// Table 5.2.23-1 value spaces enforced — entities is a non-empty
/// EntitySelector[], string members must be strings, string-array members
/// are non-empty arrays of strings, joinLevel is a positive integer,
/// entityMap/splitEntities are booleans, geoQ/ordering are objects.
/// `temporal` selects the "Query Temporal Evolution of Entities" reading:
/// temporalQ/aggrParams are only allowed there, containedBy only outside it.
pub(crate) fn query_doc_params(
    q: &Map<String, Value>,
    temporal: bool,
    vp: &mut HashMap<String, String>,
) -> Result<(), NgsiError> {
    let bad = NgsiError::BadRequestData;
    // A member lifted out of the body becomes the same parameter the GET twin
    // carries in the URI, where it is capped at MAX_URI_BYTES (6.3.4 bare
    // 414). Without the same cap here the POST form is the cheap way to hand
    // the query and projection parsers a multi-megabyte string. That includes
    // the three parameters assembled from the `entities` selectors below:
    // there is no cap on the selector array, so a body inside MAX_BODY_BYTES
    // holds hundreds of thousands of them, each costing one expanded IRI and
    // one store bind.
    let capped = |k: &str, s: String| -> Result<String, NgsiError> {
        if s.len() > crate::bounds::MAX_URI_BYTES {
            return Err(bad(format!(
                "Query {k} exceeds the {} byte limit",
                crate::bounds::MAX_URI_BYTES
            )));
        }
        Ok(s)
    };
    match q.get("entities") {
        None => {}
        Some(Value::Array(es)) if !es.is_empty() => {
            let (mut types, mut ids, mut pats) = (Vec::new(), Vec::new(), Vec::new());
            let (mut with_id, mut with_pat) = (0usize, 0usize);
            for e in es {
                if !e.is_object() {
                    return Err(bad(
                        "entities entries must be EntitySelector objects (5.2.33)".into(),
                    ));
                }
                // Table 5.2.33-1: type is the mandatory selector member (a
                // 4.17 type selection, "*" allowed)
                match e.get("type") {
                    Some(Value::String(s)) if !s.is_empty() => types.push(s.clone()),
                    _ => return Err(bad("EntitySelector requires type (5.2.33)".into())),
                }
                // id: "String or String[]", valid URI(s)
                match e.get("id") {
                    None => {}
                    Some(Value::String(s)) => {
                        antares_model::EntityId::new(s)?;
                        ids.push(s.clone());
                        with_id += 1;
                    }
                    Some(Value::Array(a)) => {
                        for i in a {
                            let s = i.as_str().ok_or_else(|| {
                                bad("EntitySelector id entries must be URIs (5.2.33)".into())
                            })?;
                            antares_model::EntityId::new(s)?;
                            ids.push(s.to_owned());
                        }
                        with_id += 1;
                    }
                    Some(_) => {
                        return Err(bad(
                            "EntitySelector id must be a URI string or array (5.2.33)".into(),
                        ))
                    }
                }
                match e.get("idPattern") {
                    None => {}
                    Some(Value::String(s)) => {
                        pats.push(s.clone());
                        with_pat += 1;
                    }
                    Some(_) => {
                        return Err(bad(
                            "EntitySelector idPattern must be a string (5.2.33)".into()
                        ))
                    }
                }
            }
            if !types.is_empty() {
                vp.insert("type".into(), capped("entities type", types.join(","))?);
            }
            // 5.2.33: the selectors are a union, and "id takes precedence over
            // idPattern" holds PER selector. These flat params carry a single
            // id/idPattern pair applied to the whole result, so a member is
            // only emitted when every selector agrees on it — otherwise one
            // selector's id would filter away the Entities another selector
            // selects on its own. Where they disagree the type predicate alone
            // stands, which over-matches rather than losing Entities.
            if with_id == es.len() && !ids.is_empty() {
                vp.insert("id".into(), capped("entities id", ids.join(","))?);
            }
            if with_id == 0 && with_pat == es.len() && !pats.is_empty() {
                vp.insert(
                    "idPattern".into(),
                    capped("entities idPattern", pats.join("|"))?,
                );
            }
        }
        Some(_) => {
            return Err(bad(
                "entities must be a non-empty EntitySelector array (5.2.23)".into(),
            ))
        }
    }
    for k in [
        "q",
        "scopeQ",
        "csf",
        "lang",
        "join",
        "expandValues",
        "jsonKeys",
    ] {
        match q.get(k) {
            None => {}
            Some(Value::String(s)) => {
                vp.insert(k.into(), capped(k, s.clone())?);
            }
            Some(_) => return Err(bad(format!("Query {k} must be a string (5.2.23)"))),
        }
    }
    // string-array members; "Empty array (0 length) is not allowed"
    for k in ["attrs", "pick", "omit", "containedBy", "datasetId"] {
        match q.get(k) {
            None => {}
            Some(Value::Array(a)) if !a.is_empty() => {
                if k == "containedBy" && temporal {
                    return Err(bad(
                        "containedBy is only applicable to Retrieve Entity and Query Entities (5.2.23)"
                            .into(),
                    ));
                }
                let mut parts = Vec::with_capacity(a.len());
                for m in a {
                    parts.push(m.as_str().ok_or_else(|| {
                        bad(format!("Query {k} entries must be strings (5.2.23)"))
                    })?);
                }
                vp.insert(k.into(), capped(k, parts.join(","))?);
            }
            Some(_) => {
                return Err(bad(format!(
                    "Query {k} must be a non-empty array of strings (5.2.23)"
                )))
            }
        }
    }
    if let Some(n) = q.get("joinLevel") {
        let v = n
            .as_u64()
            .filter(|v| *v >= 1)
            .ok_or_else(|| bad("Query joinLevel must be a positive integer (5.2.23)".into()))?;
        vp.insert("joinLevel".into(), v.to_string());
    }
    for k in ["entityMap", "splitEntities"] {
        match q.get(k) {
            None => {}
            Some(Value::Bool(b)) => {
                vp.insert(k.into(), b.to_string());
            }
            Some(_) => return Err(bad(format!("Query {k} must be a boolean (5.2.23)"))),
        }
    }
    if let Some(l) = q.get("entityMapLifetime") {
        // ISO 8601 duration; EntityMap lifetimes are the broker's call
        // ("possibly overriding the requested duration") — 5.14.x surface.
        if !l.is_string() {
            return Err(bad(
                "Query entityMapLifetime must be a string (5.2.23)".into()
            ));
        }
    }
    match q.get("geoQ") {
        None => {}
        Some(Value::Object(g)) => {
            for k in ["georel", "geometry", "geoproperty"] {
                match g.get(k) {
                    None => {}
                    Some(Value::String(s)) => {
                        vp.insert(k.into(), capped(k, s.clone())?);
                    }
                    Some(_) => return Err(bad(format!("geoQ {k} must be a string (5.2.13)"))),
                }
            }
            // `coordinates` is the one lifted member NOT capped in bytes: its
            // ceiling is MAX_GEO_VERTICES (1024), which a legal polygon can
            // spend more than MAX_URI_BYTES on. One cap per parameter, the
            // one that governs it.
            if let Some(c) = g.get("coordinates") {
                vp.insert(
                    "coordinates".into(),
                    match c {
                        Value::String(s) => s.clone(),
                        other => other.to_string(),
                    },
                );
            }
        }
        Some(_) => return Err(bad("geoQ must be a GeoQuery object (5.2.13)".into())),
    }
    match q.get("temporalQ") {
        None => {}
        Some(Value::Object(tq)) if temporal => temporal_q_params(tq, vp)?,
        Some(Value::Object(_)) => {
            return Err(bad(
                "temporalQ is only allowed for Query Temporal Evolution of Entities (5.2.23)"
                    .into(),
            ))
        }
        Some(_) => {
            return Err(bad(
                "temporalQ must be a TemporalQuery object (5.2.21)".into()
            ))
        }
    }
    match q.get("aggrParams") {
        None => {}
        Some(Value::Object(ap)) if temporal => {
            // 5.2.44 AggregationParams: aggrMethods + aggrPeriodDuration
            match ap.get("aggrMethods") {
                None => {}
                Some(Value::String(s)) => {
                    vp.insert("aggrMethods".into(), capped("aggrMethods", s.clone())?);
                }
                Some(Value::Array(a)) => {
                    let mut parts = Vec::with_capacity(a.len());
                    for m in a {
                        parts.push(m.as_str().ok_or_else(|| {
                            bad("aggrParams aggrMethods entries must be strings (5.2.44)".into())
                        })?);
                    }
                    vp.insert(
                        "aggrMethods".into(),
                        capped("aggrMethods", parts.join(","))?,
                    );
                }
                Some(_) => {
                    return Err(bad(
                        "aggrParams aggrMethods must be a comma separated list of strings (5.2.44)"
                            .into(),
                    ))
                }
            }
            match ap.get("aggrPeriodDuration") {
                None => {}
                Some(Value::String(s)) => {
                    vp.insert(
                        "aggrPeriodDuration".into(),
                        capped("aggrPeriodDuration", s.clone())?,
                    );
                }
                Some(_) => {
                    return Err(bad(
                        "aggrParams aggrPeriodDuration must be a string (5.2.44)".into(),
                    ))
                }
            }
        }
        Some(Value::Object(_)) => {
            return Err(bad(
                "aggrParams is only allowed for Query Temporal Evolution of Entities (5.2.23)"
                    .into(),
            ))
        }
        Some(_) => {
            return Err(bad(
                "aggrParams must be an AggregationParams object (5.2.44)".into(),
            ))
        }
    }
    match q.get("ordering") {
        None => {}
        Some(Value::Object(o)) => {
            // Table 5.2.43-1 (OrderingParams): orderBy String[] -> the 4.23
            // keys; coordinates (JSON array) + geometry (default "Point")
            // -> the dist-ordering reference (orderFrom/orderGeometry).
            // Every member lifted here takes the same MAX_URI_BYTES cap as
            // the rest of the body, for the same reason: it becomes the
            // parameter the GET twin carries in its URI. `coordinates` is
            // again the exception — its ceiling is MAX_GEO_VERTICES, the one
            // that governs a reference geometry.
            if let Some(ob) = o.get("orderBy") {
                let a = ob.as_array().ok_or_else(|| {
                    bad("ordering orderBy must be an array of strings (5.2.43)".into())
                })?;
                let mut parts = Vec::with_capacity(a.len());
                for m in a {
                    parts.push(m.as_str().ok_or_else(|| {
                        bad("ordering orderBy entries must be strings (5.2.43)".into())
                    })?);
                }
                vp.insert("orderBy".into(), capped("orderBy", parts.join(","))?);
            }
            match o.get("coordinates") {
                None => {}
                Some(Value::Array(c)) => {
                    vp.insert("orderFrom".into(), Value::Array(c.clone()).to_string());
                }
                Some(_) => {
                    return Err(bad(
                        "ordering coordinates must be a JSON array (5.2.43)".into()
                    ))
                }
            }
            match o.get("geometry") {
                None => {}
                Some(Value::String(g)) => {
                    vp.insert("orderGeometry".into(), capped("orderGeometry", g.clone())?);
                }
                Some(_) => return Err(bad("ordering geometry must be a string (5.2.43)".into())),
            }
            match o.get("collation") {
                None => {}
                Some(Value::String(c)) => {
                    vp.insert("collation".into(), capped("collation", c.clone())?);
                }
                Some(_) => return Err(bad("ordering collation must be a string (5.2.43)".into())),
            }
        }
        Some(_) => {
            return Err(bad(
                "ordering must be an OrderingParams object (5.2.43)".into()
            ))
        }
    }
    Ok(())
}

/// Percent-encode one client-controlled value for use as a query-string
/// value (RFC 3986 clause 3.4: a query is made of `pchar`, `/` and `?`).
/// Parameters reach a handler already percent-decoded, so a value spliced
/// back into a URI raw would change the query it belongs to (`&` and `=`
/// start another parameter, `%` re-decodes, `+` reads back as a space) and,
/// in a Link header, `>` would end the link-value (RFC 8288 clause 3).
pub(crate) fn query_value(s: &str) -> String {
    pct_encode(s, |b| {
        matches!(
            b,
            b'-' | b'.'
                | b'_'
                | b'~'
                | b'!'
                | b'$'
                | b'\''
                | b'('
                | b')'
                | b'*'
                | b','
                | b';'
                | b':'
                | b'@'
                | b'/'
                | b'?'
        )
    })
}

/// RFC 3986 clause 2.1: every byte the caller does not keep becomes its
/// percent-encoded triplet; ASCII letters and digits are always kept.
pub(crate) fn pct_encode(s: &str, keep: impl Fn(u8) -> bool) -> String {
    let mut out = String::with_capacity(s.len());
    for b in s.bytes() {
        if b.is_ascii_alphanumeric() || keep(b) {
            out.push(b as char);
        } else {
            out.push_str(&format!("%{b:02X}"));
        }
    }
    out
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

#[cfg(test)]
mod tests {
    use super::*;
    use antares_jsonld::Loader;
    use serde_json::json;

    const D: &str = "https://uri.etsi.org/ngsi-ld/default-context/";

    fn state() -> AppState {
        AppState::new("http://localhost:9090".into())
    }

    fn params(pairs: &[(&str, &str)]) -> HashMap<String, String> {
        pairs
            .iter()
            .map(|(k, v)| ((*k).to_owned(), (*v).to_owned()))
            .collect()
    }

    fn items(n: usize) -> Vec<Value> {
        (0..n).map(|i| json!({"id": format!("urn:{i}")})).collect()
    }

    fn ent(id: &str, attr: &str, v: Value) -> Value {
        json!({"id": id, "type": ["T"],
            format!("{D}{attr}"): [{"type": "Property", "value": v}]})
    }

    fn ids(docs: &[Value]) -> Vec<&str> {
        docs.iter().map(|d| d["id"].as_str().unwrap()).collect()
    }

    /// 4.12: clients specify a limit (page size); a next link flags remaining
    /// elements; prev enables backwards iteration; absent on the edges.
    #[test]
    fn next_and_prev_flag_remaining_elements() {
        let st = state();
        let (page, _, links) = paginate(&st, &params(&[("limit", "1")]), items(3), "/e").unwrap();
        assert_eq!(page.len(), 1);
        assert!(links.iter().any(|l| l.contains("rel=\"next\"")));
        assert!(
            !links.iter().any(|l| l.contains("rel=\"prev\"")),
            "no prev on the first page"
        );
        let (_, _, links) = paginate(
            &st,
            &params(&[("limit", "1"), ("offset", "1")]),
            items(3),
            "/e",
        )
        .unwrap();
        assert!(links.iter().any(|l| l.contains("rel=\"next\"")));
        assert!(links.iter().any(|l| l.contains("rel=\"prev\"")));
        let (_, _, links) = paginate(
            &st,
            &params(&[("limit", "1"), ("offset", "2")]),
            items(3),
            "/e",
        )
        .unwrap();
        assert!(
            !links.iter().any(|l| l.contains("rel=\"next\"")),
            "no next on the last page"
        );
        assert!(links.iter().any(|l| l.contains("rel=\"prev\"")));
    }

    /// 4.12: "define a default limit (default page size)" — applied when the
    /// client sends none.
    #[test]
    fn default_page_size_applies() {
        let st = state();
        let n = st.default_limit + 5;
        let (page, _, links) = paginate(&st, &params(&[]), items(n), "/e").unwrap();
        assert_eq!(page.len(), st.default_limit);
        assert!(links.iter().any(|l| l.contains("rel=\"next\"")));
    }

    /// 4.12 should: a hard result-size ceiling, rejected with TooManyResults
    /// (not silently clamped).
    #[test]
    fn limit_above_the_ceiling_is_too_many_results() {
        let st = state();
        let over = (st.max_limit + 1).to_string();
        let err = paginate(&st, &params(&[("limit", &over)]), items(1), "/e").unwrap_err();
        assert!(format!("{err:?}").contains("TooManyResults"));
    }

    /// 4.13: the result count is relayed "whenever this is requested by the
    /// client" — and only then.
    #[test]
    fn count_is_returned_only_on_request() {
        let st = AppState::new("http://localhost:9090".into());
        let items: Vec<Value> = (0..3).map(|i| json!({"id": format!("urn:{i}")})).collect();
        let (_, count, _) =
            paginate(&st, &params(&[("count", "true")]), items.clone(), "/e").unwrap();
        assert_eq!(count, Some(3));
        let (_, count, _) = paginate(&st, &params(&[]), items, "/e").unwrap();
        assert_eq!(count, None, "no count member unless requested");
    }

    /// 4.13: "a client can issue a query that limits to zero the number of
    /// desired results but asks for the count to be present" — limit=0 is
    /// only valid together with count.
    #[test]
    fn limit_zero_with_count_yields_an_empty_page_and_the_total() {
        let st = AppState::new("http://localhost:9090".into());
        let items: Vec<Value> = (0..7).map(|i| json!({"id": format!("urn:{i}")})).collect();
        let (page, count, links) = paginate(
            &st,
            &params(&[("limit", "0"), ("count", "true")]),
            items.clone(),
            "/e",
        )
        .unwrap();
        assert!(page.is_empty());
        assert_eq!(count, Some(7));
        assert!(links.is_empty(), "limit=0 pages have no next/prev");
        assert!(
            paginate(&st, &params(&[("limit", "0")]), items, "/e").is_err(),
            "limit=0 without count is rejected"
        );
    }

    /// 4.23.2: mixed datatypes order as Numbers < Strings < Object < Array <
    /// Boolean < Time < Date < DateTime < Null < absent.
    #[test]
    fn datatype_comparison_order() {
        let ctx = Loader::new().core();
        let mut docs = vec![
            ent("urn:null", "x", Value::Null),
            ent("urn:datetime", "x", json!("2020-01-01T00:00:00Z")),
            ent("urn:bool", "x", json!(true)),
            json!({"id": "urn:absent", "type": ["T"]}),
            ent("urn:array", "x", json!([1, 2])),
            ent("urn:string", "x", json!("abc")),
            ent("urn:date", "x", json!("2020-01-01")),
            ent("urn:object", "x", json!({"k": 1})),
            ent("urn:number", "x", json!(5)),
            ent("urn:time", "x", json!("12:00:00Z")),
        ];
        order_entities(&mut docs, "x", &params(&[]), &ctx).expect("order");
        assert_eq!(
            ids(&docs),
            vec![
                "urn:number",
                "urn:string",
                "urn:object",
                "urn:array",
                "urn:bool",
                "urn:time",
                "urn:date",
                "urn:datetime",
                "urn:null",
                "urn:absent"
            ]
        );
    }

    /// 4.23.3 EXAMPLES 8/9: dist-asc / dist-desc rank by haversine distance
    /// from the orderFrom reference; a non-GeoProperty under a dist ordering
    /// falls back to value ordering after the geo-ranked ones (4.23.2).
    #[test]
    fn distance_ordering() {
        let ctx = Loader::new().core();
        let geo = |id: &str, lon: f64, lat: f64| {
            json!({"id": id, "type": ["T"],
                "https://uri.etsi.org/ngsi-ld/location": [
                    {"type": "GeoProperty",
                     "value": {"type": "Point", "coordinates": [lon, lat]}}]})
        };
        let mut docs = vec![
            geo("urn:far", 10.0, 45.0),
            geo("urn:near", 8.01, 40.01),
            geo("urn:mid", 9.0, 41.0),
        ];
        let p = params(&[("orderFrom", "[8,40]")]);
        order_entities(&mut docs, "location;dist-asc", &p, &ctx).expect("order");
        assert_eq!(ids(&docs), vec!["urn:near", "urn:mid", "urn:far"]);
        order_entities(&mut docs, "location;dist-desc", &p, &ctx).expect("order");
        assert_eq!(ids(&docs), vec!["urn:far", "urn:mid", "urn:near"]);
        // dist without orderFrom is a violation
        assert!(order_entities(&mut docs, "location;dist-asc", &params(&[]), &ctx).is_err());
        // 4.23.2: under a distance ordering the GeoProperties rank first by
        // distance, and the non-GeoProperties after them BY VALUE — so the
        // ordering member has to be the same (core) one the geo entities use,
        // and there have to be two of them for their own order to mean
        // anything.
        let plain = |id: &str, v: &str| {
            json!({"id": id, "type": ["T"],
                "https://uri.etsi.org/ngsi-ld/location": [
                    {"type": "Property", "value": v}]})
        };
        let mut mixed = vec![
            plain("urn:plain-z", "zzz"),
            plain("urn:plain-a", "aaa"),
            geo("urn:g", 8.0, 40.0),
        ];
        order_entities(&mut mixed, "location;dist-asc", &p, &ctx).expect("order");
        assert_eq!(ids(&mixed), vec!["urn:g", "urn:plain-a", "urn:plain-z"]);
    }

    /// 4.23.3 EXAMPLE 4: a trailing [path] addresses a compound-value
    /// subitem; EXAMPLE 3: per-key directions apply sequentially.
    #[test]
    fn bracket_paths_and_sequential_keys() {
        let ctx = Loader::new().core();
        let addr = |id: &str, city: &str| ent(id, "address", json!({"city": city}));
        let mut docs = vec![addr("urn:b", "Berlin"), addr("urn:a", "Amsterdam")];
        order_entities(&mut docs, "address[city]", &params(&[]), &ctx).expect("order");
        assert_eq!(ids(&docs), vec!["urn:a", "urn:b"]);
        // name asc, then age desc among equals (EXAMPLE 3)
        let two = |id: &str, name: &str, age: i64| {
            json!({"id": id, "type": ["T"],
                format!("{D}name"): [{"type": "Property", "value": name}],
                format!("{D}age"): [{"type": "Property", "value": age}]})
        };
        let mut docs = vec![
            two("urn:x1", "same", 1),
            two("urn:x9", "same", 9),
            two("urn:a", "aaa", 5),
        ];
        order_entities(&mut docs, "name,age;desc", &params(&[]), &ctx).expect("order");
        assert_eq!(ids(&docs), vec!["urn:a", "urn:x9", "urn:x1"]);
    }

    /// 6.3.10 count-only page (`limit=0&count=true`): the pushed shape — no
    /// rows from the store plus its pre-LIMIT count — must be the same answer
    /// the full scan builds from a materialized match set, page contents,
    /// count and Links included.
    #[test]
    fn the_count_only_page_is_the_same_answer_pushed_or_scanned() {
        let st = AppState::new("antares-test".into());
        let matches: Vec<Value> = (0..7)
            .map(|i| serde_json::json!({"id": format!("urn:ngsi-ld:T:{i}"), "type": "T"}))
            .collect();
        for extra in [vec![], vec![("offset", "3")]] {
            let mut pairs = vec![("type", "T"), ("limit", "0"), ("count", "true")];
            pairs.extend(extra.iter().copied());
            let p = params(&pairs);
            let scanned = paginate(&st, &p, matches.clone(), "/ngsi-ld/v1/entities").expect("scan");
            let pushed = paginate_pre(
                &st,
                &p,
                Vec::new(),
                "/ngsi-ld/v1/entities",
                // what the store's count(*) reports for the same query
                matches.len(),
            )
            .expect("pushed");
            assert_eq!(scanned.0, pushed.0, "page contents differ: {pairs:?}");
            assert_eq!(scanned.1, pushed.1, "count differs: {pairs:?}");
            assert_eq!(scanned.2, pushed.2, "Links differ: {pairs:?}");
            assert!(
                pushed.0.is_empty(),
                "a count-only page carries no Entity: {:?}",
                pushed.0
            );
            assert_eq!(pushed.1, Some(matches.len()), "the count is the match set");
            assert!(
                !pushed.2.iter().any(|l| l.contains("rel=\"next\"")),
                "a page of zero has no next page: {:?}",
                pushed.2
            );
        }
    }

    /// 5.5.9 pagination is driven by `limit` and `offset`, and 5.5.6 answers
    /// a request for "so many results that can potentially exhaust client or
    /// server resources" with TooManyResults rather than clamping. 6.3.10
    /// takes `limit=0` only together with `count=true`. Every one of these is
    /// a client-supplied number, so the boundaries are what a client reaches
    /// for: the largest offset the store can bind, the first one it cannot,
    /// a number no integer type holds, and the negative form of both.
    #[test]
    fn the_paging_numbers_are_refused_at_their_boundaries_not_wrapped() {
        let st = AppState::new("http://localhost:9090".into());

        let (offset, _, _) = page_params(&st, &params(&[("offset", &i64::MAX.to_string())]))
            .expect("the largest offset the store can bind is servable");
        assert_eq!(offset, i64::MAX as usize);

        // one past it wraps negative when bound as `$n::bigint`, and a
        // negative OFFSET is a Postgres error, so it is a bad request here
        let over = (i64::MAX as u128 + 1).to_string();
        assert!(
            matches!(
                page_params(&st, &params(&[("offset", &over)])),
                Err(e) if matches!(e, crate::negotiate::ApiError::Ngsi(NgsiError::BadRequestData(_)))
            ),
            "an offset above i64::MAX must not reach the store"
        );

        for bad in ["-1", "1.5", "", " 1", "0x10", "99999999999999999999999999"] {
            assert!(
                matches!(
                    page_params(&st, &params(&[("offset", bad)])),
                    Err(e) if matches!(e, crate::negotiate::ApiError::Ngsi(NgsiError::BadRequestData(_)))
                ),
                "offset {bad:?} is not a number this API takes"
            );
            assert!(
                matches!(
                    page_params(&st, &params(&[("limit", bad)])),
                    Err(e) if matches!(e, crate::negotiate::ApiError::Ngsi(NgsiError::BadRequestData(_)))
                ),
                "limit {bad:?} is not a number this API takes"
            );
        }

        // 5.5.6: over the server maximum is 403, never a silent clamp
        let over_max = (st.max_limit + 1).to_string();
        assert!(
            matches!(
                page_params(&st, &params(&[("limit", &over_max)])),
                Err(e) if matches!(e, crate::negotiate::ApiError::Ngsi(NgsiError::TooManyResults(_)))
            ),
            "a limit above the maximum is TooManyResults"
        );
        let (_, limit, _) = page_params(&st, &params(&[("limit", &st.max_limit.to_string())]))
            .expect("the maximum itself is servable");
        assert_eq!(limit, st.max_limit, "the boundary value is not clamped");

        // 6.3.10: limit=0 is the count-only request and nothing else
        assert!(
            matches!(
                page_params(&st, &params(&[("limit", "0")])),
                Err(e) if matches!(e, crate::negotiate::ApiError::Ngsi(NgsiError::BadRequestData(_)))
            ),
            "limit=0 without count asks for a page of nothing"
        );
        let (_, limit, count) = page_params(&st, &params(&[("limit", "0"), ("count", "true")]))
            .expect("limit=0 with count=true is the count-only request");
        assert_eq!((limit, count), (0, true));

        // `count` is the literal "true" and nothing else is a count request
        for not_true in ["TRUE", "1", "yes", ""] {
            let (_, _, count) = page_params(&st, &params(&[("count", not_true)])).expect("params");
            assert!(!count, "count={not_true:?} is not count=true");
        }
    }

    /// Table 5.2.23-1 splitEntities: "If true it is assumed that single
    /// Entities are distributed between different Context Brokers and/or
    /// Context Sources and this has to be taken into account when applying
    /// any kind of filters" — the body member drives the same read as the
    /// query-parameter twin, so it has to reach the filter path.
    #[test]
    fn query_body_split_entities_reaches_the_filter_params() {
        let q = json!({"type": "Query", "entityMap": true, "splitEntities": true});
        let mut vp = HashMap::new();
        query_doc_params(q.as_object().expect("object"), false, &mut vp).expect("valid Query");
        assert_eq!(
            vp.get("splitEntities").map(String::as_str),
            Some("true"),
            "splitEntities must not be dropped: {vp:?}"
        );
        assert_eq!(vp.get("entityMap").map(String::as_str), Some("true"));
        // the false reading is carried through unchanged, never as "true"
        let q = json!({"type": "Query", "splitEntities": false});
        let mut vp = HashMap::new();
        query_doc_params(q.as_object().expect("object"), false, &mut vp).expect("valid Query");
        assert_eq!(vp.get("splitEntities").map(String::as_str), Some("false"));
    }

    /// 5.2.33: the `entities` EntitySelectors are a union and "id takes
    /// precedence over idPattern" PER selector. A flat filter carries one
    /// id/idPattern pair, so a member may only be emitted when it holds for
    /// every selector — otherwise the flat filter excludes Entities that a
    /// selector on its own selects.
    #[test]
    fn entity_selectors_are_a_union_not_a_flat_id_filter() {
        let mixed = json!({"type": "Query", "entities": [
            {"type": "Vehicle", "id": "urn:ngsi-ld:Vehicle:1"},
            {"type": "Building", "idPattern": "^urn:ngsi-ld:Building:"}
        ]});
        let mut vp = HashMap::new();
        query_doc_params(mixed.as_object().expect("object"), false, &mut vp).expect("valid Query");
        assert_eq!(vp.get("type").map(String::as_str), Some("Vehicle,Building"));
        assert!(
            !vp.contains_key("id"),
            "an id from one selector must not filter the other selector out: {vp:?}"
        );
        assert!(
            !vp.contains_key("idPattern"),
            "a pattern from one selector must not filter the other out: {vp:?}"
        );
        // every selector carries id → the union of ids is exact
        let all_ids = json!({"type": "Query", "entities": [
            {"type": "Vehicle", "id": "urn:ngsi-ld:Vehicle:1"},
            {"type": "Building", "id": ["urn:ngsi-ld:Building:1"]}
        ]});
        let mut vp = HashMap::new();
        query_doc_params(all_ids.as_object().expect("object"), false, &mut vp)
            .expect("valid Query");
        assert_eq!(
            vp.get("id").map(String::as_str),
            Some("urn:ngsi-ld:Vehicle:1,urn:ngsi-ld:Building:1")
        );
        assert!(!vp.contains_key("idPattern"));
        // no selector carries id and every one carries idPattern → union
        let all_pats = json!({"type": "Query", "entities": [
            {"type": "Vehicle", "idPattern": "^urn:ngsi-ld:Vehicle:"},
            {"type": "Building", "idPattern": "^urn:ngsi-ld:Building:"}
        ]});
        let mut vp = HashMap::new();
        query_doc_params(all_pats.as_object().expect("object"), false, &mut vp)
            .expect("valid Query");
        assert_eq!(
            vp.get("idPattern").map(String::as_str),
            Some("^urn:ngsi-ld:Vehicle:|^urn:ngsi-ld:Building:")
        );
        assert!(!vp.contains_key("id"));
        // a single selector keeps the 5.2.33 id-over-idPattern precedence
        let both = json!({"type": "Query", "entities": [
            {"type": "Vehicle", "id": "urn:ngsi-ld:Vehicle:1", "idPattern": "^urn:"}
        ]});
        let mut vp = HashMap::new();
        query_doc_params(both.as_object().expect("object"), false, &mut vp).expect("valid Query");
        assert_eq!(
            vp.get("id").map(String::as_str),
            Some("urn:ngsi-ld:Vehicle:1")
        );
        assert!(!vp.contains_key("idPattern"), "id wins in one selector");
    }

    /// RFC 3986 clause 3.4 + RFC 8288 clause 3: a value spliced back into a
    /// query string must not be able to start another parameter, decode a
    /// second time, or end the link-value it is carried in. The characters an
    /// NGSI-LD filter legitimately uses (`urn:`, `.`, `*`, `-`) survive, or
    /// every pagination link would run a different query than the one that
    /// produced it.
    #[test]
    fn query_value_encodes_what_would_change_the_query() {
        assert_eq!(
            query_value("urn:ngsi-ld:Building:01931.*"),
            "urn:ngsi-ld:Building:01931.*"
        );
        assert_eq!(query_value("cat>1"), "cat%3E1");
        assert_eq!(query_value(r#"cat=="a&b""#), "cat%3D%3D%22a%26b%22");
        // a value already carrying a percent must not decode twice
        assert_eq!(query_value("a%26b"), "a%2526b");
        // `+` reads back as a space in a query string
        assert_eq!(query_value("a+b"), "a%2Bb");
        assert_eq!(query_value("é"), "%C3%A9");
    }
}
