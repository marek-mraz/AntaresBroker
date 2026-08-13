//! C11 — Temporal Query Language (CIM 009 clause 4.11) compiled to a
//! per-instance SQL predicate over a jsonb instance object.
//!
//! Exactness by construction: `TemporalQ::instance_matches` (antares-api)
//! compares the RAW timestamp strings byte-wise — so the SQL compares the
//! same strings with `COLLATE "C"` (byte order, locale-proof) instead of
//! casting to timestamptz (which would re-order mixed-offset forms and can
//! raise on malformed values). The member must be string-typed, exactly as
//! `Value::as_str` demands. The predicate is therefore the SAME function the
//! in-memory arbiter applies — but the arbiter still runs afterwards, so even
//! a drifting edge case only costs bytes, never a wrong answer.

/// The 4.11 window, as the API layer parsed it. `timerel` ∈
/// before|after|between|any ("any" = bare timeproperty: presence filter).
pub struct InstanceRange<'a> {
    pub timerel: &'a str,
    pub time_at: &'a str,
    pub end_time_at: Option<&'a str>,
    pub timeproperty: &'a str,
}

/// SQL predicate over one jsonb instance (`el`) + its binds, numbered from
/// `first_bind`. Bind 0 is always the timeproperty name.
pub struct CompiledRange {
    pub sql: String,
    pub binds: Vec<String>,
}

/// `None` = a shape this compiler does not reproduce (unknown timerel, or
/// between without an end) — the caller prunes nothing and the in-memory
/// window stays the arbiter.
pub fn compile_instance_range(
    r: &InstanceRange<'_>,
    el: &str,
    first_bind: usize,
) -> Option<CompiledRange> {
    let tp = format!("${first_bind}");
    let present = format!("jsonb_typeof({el} -> {tp}) = 'string'");
    let ts = format!("({el} ->> {tp}) COLLATE \"C\"");
    let mut binds = vec![r.timeproperty.to_owned()];
    let sql = match r.timerel {
        "any" => present,
        "before" => {
            binds.push(r.time_at.to_owned());
            format!("{present} AND {ts} < ${}", first_bind + 1)
        }
        "after" => {
            binds.push(r.time_at.to_owned());
            format!("{present} AND {ts} >= ${}", first_bind + 1)
        }
        "between" => {
            let end = r.end_time_at?;
            binds.push(r.time_at.to_owned());
            binds.push(end.to_owned());
            format!(
                "{present} AND {ts} >= ${} AND {ts} < ${}",
                first_bind + 1,
                first_bind + 2
            )
        }
        _ => return None,
    };
    Some(CompiledRange { sql, binds })
}

/// Widened, index-serving COLUMN bound for the 4.11 window. Returns SQL only
/// — it references `$time_bind` (and `$time_bind+1` for between), the SAME
/// binds the byte-exact text predicate uses, cast to timestamptz in place.
///
/// A SUPERSET by construction: the parsed column and the raw stamp diverge by
/// at most the two RFC 3339 offsets (±14 h each), so 48 h of slack admits
/// every row the text window keeps; the extra rows it admits are dropped by
/// the text predicate (or the API arbiter) right after. Purpose: the btree on
/// (tenant_id, entity_id, attr_id, observed_at) can serve this range — the
/// jsonb text extraction the exact predicate runs on never uses an index.
/// Only timeproperties with a parsed column compile; others prune by text
/// alone (`None`).
pub fn column_range_bound(r: &InstanceRange<'_>, alias: &str, time_bind: usize) -> Option<String> {
    let col = match r.timeproperty {
        "observedAt" => "observed_at",
        "createdAt" => "created_at",
        "modifiedAt" => "modified_at",
        _ => return None,
    };
    Some(match r.timerel {
        "before" => format!("{alias}.{col} < ${time_bind}::timestamptz + interval '48 hours'"),
        "after" => format!("{alias}.{col} >= ${time_bind}::timestamptz - interval '48 hours'"),
        "between" => {
            r.end_time_at?;
            format!(
                "{alias}.{col} >= ${time_bind}::timestamptz - interval '48 hours' \
                 AND {alias}.{col} < ${}::timestamptz + interval '48 hours'",
                time_bind + 1
            )
        }
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn range<'a>(rel: &'a str, at: &'a str, end: Option<&'a str>) -> InstanceRange<'a> {
        InstanceRange {
            timerel: rel,
            time_at: at,
            end_time_at: end,
            timeproperty: "observedAt",
        }
    }

    #[test]
    fn operators_mirror_instance_matches() {
        // before: strict <   after: >=   between: [at, end)
        let c = compile_instance_range(&range("before", "2026-01-01T00:00:00Z", None), "el", 4)
            .expect("compiles");
        assert!(c.sql.contains("< $5"), "sql: {}", c.sql);
        assert_eq!(c.binds, vec!["observedAt", "2026-01-01T00:00:00Z"]);

        let c = compile_instance_range(&range("after", "t0", None), "el", 1).expect("compiles");
        assert!(c.sql.contains(">= $2"), "sql: {}", c.sql);

        let c = compile_instance_range(&range("between", "t0", Some("t1")), "el", 1).expect("c");
        assert!(
            c.sql.contains(">= $2") && c.sql.contains("< $3"),
            "{}",
            c.sql
        );
        assert_eq!(c.binds.len(), 3);
    }

    #[test]
    fn byte_order_and_string_type_guard() {
        let c = compile_instance_range(&range("any", "", None), "el", 1).expect("compiles");
        // presence = string-typed member, exactly Value::as_str
        assert_eq!(c.sql, "jsonb_typeof(el -> $1) = 'string'");
        // ranged forms compare bytes, never timestamptz casts
        let c = compile_instance_range(&range("after", "t0", None), "el", 1).expect("c");
        assert!(c.sql.contains("COLLATE \"C\""), "{}", c.sql);
        assert!(!c.sql.contains("timestamptz"), "{}", c.sql);
    }

    #[test]
    fn unknown_shapes_refuse() {
        assert!(compile_instance_range(&range("since", "t0", None), "el", 1).is_none());
        assert!(compile_instance_range(&range("between", "t0", None), "el", 1).is_none());
    }

    #[test]
    fn column_bound_reuses_binds_and_widens_outward() {
        // after: lower bound moves DOWN, before: upper bound moves UP —
        // widening must always ADMIT more than the text window, never less
        let s = column_range_bound(&range("after", "t0", None), "ai", 5).expect("bound");
        assert_eq!(s, "ai.observed_at >= $5::timestamptz - interval '48 hours'");
        let s = column_range_bound(&range("before", "t0", None), "ai", 2).expect("bound");
        assert!(s.contains("< $2::timestamptz + interval '48 hours'"), "{s}");
        let s = column_range_bound(&range("between", "t0", Some("t1")), "ai", 2).expect("bound");
        assert!(
            s.contains(">= $2::timestamptz - interval")
                && s.contains("< $3::timestamptz + interval"),
            "{s}"
        );
    }

    #[test]
    fn column_bound_only_for_parsed_columns_and_known_relations() {
        let created = InstanceRange {
            timeproperty: "createdAt",
            ..range("after", "t0", None)
        };
        assert!(column_range_bound(&created, "ai", 1)
            .expect("bound")
            .contains("ai.created_at"));
        let deleted = InstanceRange {
            timeproperty: "deletedAt",
            ..range("after", "t0", None)
        };
        assert!(column_range_bound(&deleted, "ai", 1).is_none(), "no column");
        assert!(column_range_bound(&range("any", "", None), "ai", 1).is_none());
        assert!(column_range_bound(&range("since", "t0", None), "ai", 1).is_none());
        assert!(column_range_bound(&range("between", "t0", None), "ai", 1).is_none());
    }
}
