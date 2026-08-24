//! Query-filter shapes shared by every backend (the pushdown contract),
//! plus the pure pieces of the geo and temporal query shapes the filters
//! reference. Pure data — no SQL, no I/O; the memory arm consumes these
//! filters too, it just never gets a `decided` outcome.

use serde_json::Value;

pub use antares_ql::geo::{GeoSpec, Rel, LOCATION_IRI};

/// The 4.11 temporal window as the API already validated it.
pub struct InstanceRange<'a> {
    pub timerel: &'a str,
    pub time_at: &'a str,
    pub end_time_at: Option<&'a str>,
    pub timeproperty: &'a str,
}

pub struct EntityFilter<'a> {
    /// exact entity ids (`id=` / the ids of a batch query)
    pub ids: Option<&'a [&'a str]>,
    /// Entity Type Selection (4.17) as OR-of-AND groups, expanded IRIs
    pub types: Option<&'a [Vec<String>]>,
    /// `attrs=`: the entity must carry at least one, expanded IRIs
    pub attrs: Option<&'a [String]>,
    /// `q=` AST; compiled when its shape is exactly reproducible, else skipped
    pub q: Option<&'a antares_ql::QNode>,
    /// `scopeQ=` verbatim (4.19); compiled over the `scopes` column
    pub scope_q: Option<&'a str>,
    /// `georel`/`geometry`/`coordinates`/`geoproperty` (4.10), compiled over
    /// the extracted `location` column
    pub geo: Option<&'a GeoSpec<'a>>,
    /// term → IRI, the request context's expander (the AST holds terms)
    pub expand: &'a dyn Fn(&str) -> String,
    /// Pagination pushdown: applied ONLY when every present predicate
    /// compiled exactly (`decided`) — otherwise the caller's evaluator still
    /// has rows to drop and a SQL LIMIT would page over the wrong set. The
    /// caller passes it only when its own store-invisible filters (idPattern,
    /// federation, orderBy) are absent.
    pub page: Option<Page>,
    /// Projection pushdown (4.21 `pick`, top-level): keep these expanded
    /// attr IRIs + every non-attribute member. Applied only when `decided` —
    /// a projected doc can no longer answer a q= re-check.
    pub keep_attrs: Option<&'a [String]>,
    /// Projection pushdown (`omit`, top-level entries only): drop exactly
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
    pub range: Option<InstanceRange<'a>>,
    /// `lastN`: per-(attr, datasetId) RANK() cap — ties all kept, so the
    /// per-attr lastN the API applies afterwards always finds its instances
    pub last_n: Option<i64>,
    /// ordering key for the lastN cap (the request's timeproperty)
    pub timeproperty: &'a str,
    /// Entity-level LIMIT/OFFSET pushdown (without it a temporal query
    /// materializes the tenant's ENTIRE history). Passed only when the
    /// caller has no store-invisible entity filters (idPattern, q, geo) —
    /// when honoured, SQL also applies the caller's entity-qualification rule
    /// (≥1 instance, in-window when a range is given), so the paged set is
    /// exactly the set the evaluator would keep.
    pub page: Option<Page>,
    /// 5.7.4.4 S2 prefilter: the `q=` AST. The Pg arm compiles the leaves it
    /// can reproduce into windowed EXISTS predicates and treats everything
    /// else as TRUE — always a SUPERSET of the eval_q verdict, so the API
    /// arbiter (which always re-runs when q is present) never changes an
    /// answer, only sees fewer rows. Requires the matching `expand`.
    pub q: Option<&'a antares_ql::QNode>,
    /// term → IRI, the request context's expander (the AST holds terms).
    /// `Sync` so a filter alive across an await keeps the handler future Send.
    pub expand: &'a (dyn Fn(&str) -> String + Sync),
    /// 5.7.4.4 S3 prefilter: the geoquery plus the EXPANDED geoproperty IRI
    /// whose windowed instances the EXISTS checks (per-instance rows carry
    /// extracted geometries for EVERY geoproperty, not just `location`).
    /// Superset like `q` — `GeoQuery::matches` stays the arbiter; rows with
    /// an unextracted `geo_value` always survive.
    pub geo: Option<(&'a GeoSpec<'a>, &'a str)>,
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
            q: None,
            expand: &|t: &str| t.to_owned(),
            geo: None,
        }
    }
}

/// 4.22 transient storage: is this doc/instance past its `expiresAt`?
/// Parses both stamps to instants (so a non-UTC-Z offset expiresAt is judged
/// correctly, matching the SQL `expires_at`/timestamptz path); byte compare is
/// only the fallback when a stamp is unparseable.
///
/// 4.6.3 also allows a comma as the seconds-fraction separator, which RFC 3339
/// does not; a comma cannot appear anywhere else in such a stamp, so the first
/// one is rewritten to a point before parsing. Without that the comma form
/// always fell into the byte fallback, where ',' (0x2C) sorts before both '.'
/// and 'Z' and a live instance reads as expired.
pub fn expired_at(v: &Value, now: &str) -> bool {
    let Some(e) = v.get("expiresAt").and_then(Value::as_str) else {
        return false;
    };
    let instant = |s: &str| chrono::DateTime::parse_from_rfc3339(&s.replacen(',', ".", 1)).ok();
    match (instant(e), instant(now)) {
        (Some(exp), Some(n)) => exp < n,
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
    use serde_json::json;

    const NOW: &str = "2026-08-08T12:00:00.000Z";

    /// Defaults push nothing down: a filter built with `..Default::default()`
    /// must not silently add a predicate, and its expander is the identity
    /// (terms stay terms until a request context replaces it).
    #[test]
    fn entity_filter_default_pushes_nothing_down() {
        let f = EntityFilter::default();
        assert!(f.ids.is_none());
        assert!(f.types.is_none());
        assert!(f.attrs.is_none());
        assert!(f.q.is_none());
        assert!(f.scope_q.is_none());
        assert!(f.geo.is_none());
        assert!(f.page.is_none(), "no LIMIT until the caller proves decided");
        assert!(f.keep_attrs.is_none());
        assert!(f.drop_attrs.is_none());
        assert_eq!((f.expand)("Vehicle"), "Vehicle");
    }

    /// The default lastN ordering key is `observedAt` (4.11); a different
    /// default would silently rank instances by another time property.
    #[test]
    fn temporal_filter_default_orders_by_observed_at() {
        let f = TemporalFilter::default();
        assert_eq!(f.timeproperty, "observedAt");
        assert!(f.ids.is_none());
        assert!(f.types.is_none());
        assert!(f.attrs.is_none());
        assert!(f.range.is_none(), "no window means no instance pruning");
        assert!(f.last_n.is_none());
        assert!(f.page.is_none());
        assert!(f.q.is_none());
        assert!(f.geo.is_none());
        assert_eq!((f.expand)("speed"), "speed");
    }

    /// 4.22: expiry has PASSED only when the stamp lies strictly before now —
    /// the same strictness as the `expires_at < now()` reaping predicate, so a
    /// read and the sweep never disagree at the boundary instant.
    #[test]
    fn expiry_exactly_at_now_has_not_passed() {
        assert!(!expired_at(&json!({ "expiresAt": NOW }), NOW));
        // same instant, coarser spelling: still not expired
        assert!(!expired_at(
            &json!({"expiresAt": "2026-08-08T12:00:00Z"}),
            NOW
        ));
        // one millisecond earlier is
        assert!(expired_at(
            &json!({"expiresAt": "2026-08-08T11:59:59.999Z"}),
            NOW
        ));
    }

    /// 4.6.3 DateTime accepts a comma as the fraction separator, so a stored
    /// `expiresAt` may carry one. Judging it by bytes puts ',' (0x2C) before
    /// '.' (0x2E) and calls a still-live stamp expired — the comparison must
    /// stay on instants.
    #[test]
    fn comma_fraction_expiry_is_judged_by_instant() {
        assert!(
            !expired_at(&json!({"expiresAt": "2026-08-08T12:00:00,500Z"}), NOW),
            "half a second in the future is not expired"
        );
        assert!(expired_at(
            &json!({"expiresAt": "2026-08-08T11:59:59,999Z"}),
            NOW
        ));
    }

    /// A comma-fraction expiry still in the future must not cost the instance
    /// its place in the document.
    #[test]
    fn a_live_comma_fraction_instance_is_not_stripped() {
        let mut doc = json!({
            "id": "urn:x", "type": ["T"],
            "https://a/attr": [{"value": 1, "instanceId": "i1",
                                "expiresAt": "2026-08-08T12:00:00,500Z"}]
        });
        assert!(!strip_expired(&mut doc, NOW));
        assert_eq!(doc["https://a/attr"].as_array().map(Vec::len), Some(1));
    }

    /// A non-UTC offset is judged by instant: 13:30+02:00 is 11:30Z, expired
    /// against a 12:00Z now even though its bytes sort after it.
    #[test]
    fn offset_expiry_is_judged_by_instant_not_bytes() {
        assert!(expired_at(
            &json!({"expiresAt": "2026-08-08T13:30:00+02:00"}),
            NOW
        ));
        assert!(!expired_at(
            &json!({"expiresAt": "2026-08-08T11:30:00-02:00"}),
            NOW
        ));
    }

    /// Hostile `expiresAt` shapes decide without panicking: a non-string is no
    /// expiry at all, an unparseable string falls back to the byte compare.
    #[test]
    fn a_hostile_expiry_never_panics() {
        for v in [
            json!({ "expiresAt": 1 }),
            json!({ "expiresAt": null }),
            json!({"expiresAt": {"@value": "2020-01-01T00:00:00Z"}}),
            json!({ "expiresAt": ["2020-01-01T00:00:00Z"] }),
            json!({ "expiresAt": true }),
            json!({}),
        ] {
            assert!(!expired_at(&v, NOW), "a non-string expiresAt is no expiry");
        }
        // unparseable strings stay decidable through the byte fallback
        assert!(expired_at(&json!({ "expiresAt": "" }), NOW));
        assert!(expired_at(
            &json!({"expiresAt": "2026-08-08T11:00:00"}),
            NOW
        ));
        assert!(!expired_at(
            &json!({"expiresAt": "9999-99-99T99:99:99Z"}),
            NOW
        ));
    }

    /// Multi-instance (4.5.5) attribute: the expired instance must be gone
    /// from the document entirely, not merely reordered or emptied.
    #[test]
    fn an_expired_instance_never_survives_a_multi_instance_attribute() {
        let mut doc = json!({
            "id": "urn:x", "type": ["T"],
            "https://a/attr": [
                {"value": 1, "instanceId": "i1", "datasetId": "urn:d:1"},
                {"value": 2, "instanceId": "i2", "datasetId": "urn:d:2",
                 "expiresAt": "2026-08-08T11:00:00Z"}
            ]
        });
        assert!(!strip_expired(&mut doc, NOW));
        let text = serde_json::to_string(&doc).expect("serialize");
        assert!(
            !text.contains("i2"),
            "expired instance still present: {text}"
        );
        assert!(!text.contains("urn:d:2"));
        assert!(text.contains("i1"));
    }

    /// Only attribute arrays are instance-filtered: an already-empty array is
    /// left in place (it lost nothing), and a doc that is not an object is
    /// simply not expired.
    #[test]
    fn strip_expired_leaves_untouched_what_it_must_not_remove() {
        let mut doc = json!({
            "id": "urn:x", "type": ["T"], "scope": ["/a"],
            "https://a/empty": []
        });
        assert!(!strip_expired(&mut doc, NOW));
        assert!(doc.get("https://a/empty").is_some(), "empty array kept");
        assert_eq!(doc["scope"], json!(["/a"]));

        let mut not_an_object = json!(["urn:x"]);
        assert!(!strip_expired(&mut not_an_object, NOW));
        assert_eq!(not_an_object, json!(["urn:x"]));
    }

    /// An entity expiry outranks the instance pass: the caller drops the whole
    /// document, so a live instance inside it must never reach a response.
    #[test]
    fn an_expired_entity_is_dropped_before_any_instance_survives() {
        let mut doc = json!({
            "id": "urn:x", "type": ["T"], "expiresAt": "2026-08-08T11:00:00Z",
            "https://a/attr": [{"value": 1, "instanceId": "i1",
                                "expiresAt": "2999-01-01T00:00:00Z"}]
        });
        assert!(strip_expired(&mut doc, NOW));
    }

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
