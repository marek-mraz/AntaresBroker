//! Query-filter shapes shared by every backend (C10/C11 pushdown contract).
//!
//! Pure data — no sqlx, no I/O — split out of `pg_entity`/`pg_temporal` (N2)
//! so the wasm32 build (no `postgres` feature) keeps the same `AnyStore`
//! query surface: the memory arm consumes these filters too, it just never
//! gets a `decided` outcome.

use serde_json::Value;

pub struct EntityFilter<'a> {
    /// exact entity ids (`id=` / the ids of a batch query)
    pub ids: Option<&'a [&'a str]>,
    /// Entity Type Selection (4.17) as OR-of-AND groups, expanded IRIs
    pub types: Option<&'a [Vec<String>]>,
    /// `attrs=`: the entity must carry at least one, expanded IRIs
    pub attrs: Option<&'a [String]>,
    /// `q=` AST; compiled when its shape is exactly reproducible, else skipped
    pub q: Option<&'a antares_ql::QNode>,
    /// `scopeQ=` verbatim (4.19); compiled over the `scopes` column (C11)
    pub scope_q: Option<&'a str>,
    /// `georel`/`geometry`/`coordinates`/`geoproperty` (4.10), compiled over
    /// the extracted `location` column (C11b)
    pub geo: Option<&'a crate::compile::geo::GeoSpec<'a>>,
    /// term → IRI, the request context's expander (the AST holds terms)
    pub expand: &'a dyn Fn(&str) -> String,
    /// C11 pagination pushdown: applied ONLY when every present predicate
    /// compiled exactly (`decided`) — otherwise the caller's evaluator still
    /// has rows to drop and a SQL LIMIT would page over the wrong set. The
    /// caller passes it only when its own store-invisible filters (idPattern,
    /// federation, orderBy) are absent.
    pub page: Option<Page>,
    /// C11 projection pushdown (4.21 `pick`, top-level): keep these expanded
    /// attr IRIs + every non-attribute member. Applied only when `decided` —
    /// a projected doc can no longer answer a q= re-check.
    pub keep_attrs: Option<&'a [String]>,
    /// C11 projection pushdown (`omit`, top-level entries only): drop exactly
    /// these attr IRIs. Same `decided` gate.
    pub drop_attrs: Option<&'a [String]>,
}

impl Default for EntityFilter<'_> {
    fn default() -> Self {
        Self {
            ids: None,
            types: None,
            attrs: None,
            q: None,
            scope_q: None,
            geo: None,
            expand: &|t: &str| t.to_owned(),
            page: None,
            keep_attrs: None,
            drop_attrs: None,
        }
    }
}

/// One page: OFFSET/LIMIT in row units, ORDER BY id (the store's stable
/// default order, same as the memory snapshot).
pub struct Page {
    pub offset: i64,
    pub limit: i64,
}

/// What `query` produced. `decided` = SQL applied every present predicate
/// exactly, so re-evaluation cannot drop a row; `paged` = LIMIT/OFFSET
/// happened in SQL (implies `decided`), `total` = the pre-LIMIT match count.
pub struct QueryOutcome {
    pub rows: Vec<Value>,
    pub decided: bool,
    pub paged: bool,
    pub total: Option<i64>,
}

pub struct TemporalFilter<'a> {
    /// exact entity ids
    pub ids: Option<&'a [&'a str]>,
    /// flat OR list of expanded type IRIs (temporal query has no AND groups)
    pub types: Option<&'a [String]>,
    /// `attrs=`: the entity must carry at least one, expanded IRIs
    pub attrs: Option<&'a [String]>,
    /// the 4.11 window; `None` = no instance pruning
    pub range: Option<crate::compile::temporal::InstanceRange<'a>>,
    /// `lastN`: per-(attr, datasetId) RANK() cap — ties all kept, so the
    /// per-attr lastN the API applies afterwards always finds its instances
    pub last_n: Option<i64>,
    /// ordering key for the lastN cap (the request's timeproperty)
    pub timeproperty: &'a str,
    /// Entity-level LIMIT/OFFSET pushdown (audit 2026-08-08: a temporal query
    /// used to materialize the tenant's ENTIRE history). Passed only when the
    /// caller has no store-invisible entity filters (idPattern, q, geo) —
    /// when honoured, SQL also applies the caller's entity-qualification rule
    /// (≥1 instance, in-window when a range is given), so the paged set is
    /// exactly the set the evaluator would keep.
    pub page: Option<Page>,
}

impl Default for TemporalFilter<'_> {
    fn default() -> Self {
        Self {
            ids: None,
            types: None,
            attrs: None,
            range: None,
            last_n: None,
            timeproperty: "observedAt",
            page: None,
        }
    }
}

/// 4.22 transient storage: is this doc/instance past its `expiresAt`?
/// Parses both stamps to instants (so a non-UTC-Z offset expiresAt is judged
/// correctly, matching the SQL `expires_at`/timestamptz path); byte compare is
/// only the fallback when a stamp is unparseable.
pub fn expired_at(v: &Value, now: &str) -> bool {
    let Some(e) = v.get("expiresAt").and_then(Value::as_str) else {
        return false;
    };
    // Compare instants so a non-UTC-Z offset expiresAt is judged correctly;
    // fall back to the byte compare only if either stamp is unparseable.
    match (
        chrono::DateTime::parse_from_rfc3339(e),
        chrono::DateTime::parse_from_rfc3339(now),
    ) {
        (Ok(exp), Ok(n)) => exp < n,
        _ => e < now,
    }
}

/// Apply 4.22 invalidity to a read: `true` = the ENTITY is expired (caller
/// drops it entirely); otherwise expired attribute INSTANCES are stripped in
/// place (an attribute left with zero instances disappears).
pub fn strip_expired(doc: &mut Value, now: &str) -> bool {
    if expired_at(doc, now) {
        return true;
    }
    if let Some(obj) = doc.as_object_mut() {
        let mut empty: Vec<String> = Vec::new();
        for (k, v) in obj.iter_mut() {
            if let Some(arr) = v.as_array_mut() {
                let before = arr.len();
                arr.retain(|inst| !expired_at(inst, now));
                if before > 0 && arr.is_empty() {
                    empty.push(k.clone());
                }
            }
        }
        for k in empty {
            obj.remove(&k);
        }
    }
    false
}

/// What a temporal query produced. `paged` = LIMIT/OFFSET (and the
/// entity-qualification EXISTS) ran in SQL; `total` = pre-LIMIT match count.
pub struct TemporalOutcome {
    pub rows: Vec<Value>,
    pub paged: bool,
    pub total: Option<i64>,
}

#[cfg(test)]
mod tests {
    use super::*;

    const NOW: &str = "2026-08-08T12:00:00.000Z";

    #[test]
    fn expired_entity_is_dropped_whole() {
        let mut doc = serde_json::json!({
            "id": "urn:x", "type": ["T"], "expiresAt": "2026-08-08T11:00:00Z",
            "https://a/attr": [{"value": 1, "instanceId": "i1"}]
        });
        assert!(strip_expired(&mut doc, NOW));
    }

    #[test]
    fn expired_instances_are_stripped_and_empty_attrs_disappear() {
        let mut doc = serde_json::json!({
            "id": "urn:x", "type": ["T"], "expiresAt": "2026-08-09T00:00:00Z",
            "https://a/keep": [
                {"value": 1, "instanceId": "i1"},
                {"value": 2, "instanceId": "i2", "expiresAt": "2026-08-08T11:00:00Z"}
            ],
            "https://a/gone": [{"value": 3, "instanceId": "i3",
                                "expiresAt": "2026-08-08T00:00:00Z"}]
        });
        assert!(!strip_expired(&mut doc, NOW));
        assert_eq!(doc["https://a/keep"].as_array().map(Vec::len), Some(1));
        assert!(doc.get("https://a/gone").is_none(), "emptied attr removed");
        // meta arrays (type) are never instance-filtered
        assert_eq!(doc["type"].as_array().map(Vec::len), Some(1));
    }

    #[test]
    fn no_expiry_means_untouched() {
        let mut doc = serde_json::json!({
            "id": "urn:x", "type": ["T"],
            "https://a/attr": [{"value": 1, "instanceId": "i1"}]
        });
        let before = doc.clone();
        assert!(!strip_expired(&mut doc, NOW));
        assert_eq!(doc, before);
    }
}
