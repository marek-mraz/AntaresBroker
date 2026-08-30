// SPDX-License-Identifier: EUPL-1.2
//! Temporal Query Language (CIM 009 clause 4.11) compiled to a
//! per-instance SQL predicate over a jsonb instance object.
//!
//! Exactness by construction: `TemporalQ::instance_matches` (antares-api)
//! compares CANONICAL keys — the trailing `Z` dropped and the 4.6.3 seconds
//! fraction (`.` or `,`) zero-padded — so that equal instants written in
//! different fraction forms hit the bounds exactly. The SQL builds the same
//! key with `dt_key_sql` and compares it with `COLLATE "C"` (byte order,
//! locale-proof) instead of casting to timestamptz (which would re-order
//! mixed-offset forms and can raise on malformed values). The member must be
//! string-typed, exactly as `Value::as_str` demands.
//!
//! The store PRUNES on this predicate, so it may never be stricter than the
//! arbiter: a raw byte compare made `…00.000Z` sort after `…00Z` ('.' is
//! 0x2E, 'Z' is 0x5A) and silently dropped instances 4.11 requires to be
//! returned.

/// The 4.11 window, as the API layer parsed it. `timerel` ∈
/// before|after|between|any ("any" = bare timeproperty: presence filter).
pub use antares_store::filter::InstanceRange;

// The compiled range is the shared fragment shape; its bind 0 is always the
// timeproperty name.
use antares_ql::sql::CompiledSql;

/// 4.6.3 DateTime → canonical lexicographic key, the SQL twin of the
/// arbiter's `dt_key`: for a `Z`-terminated stamp of at least 19 characters,
/// the `Z` is dropped and the optional seconds fraction (`.` or `,`
/// separator) is zero-padded to six digits; anything else is compared as it
/// stands. String order over the key is temporal order across spellings, so
/// `…00Z`, `…00.000Z` and `…00,0Z` are one instant on both sides.
///
/// Total by construction — no cast, so a malformed stored stamp can never
/// raise; the nested `CASE` is only reached for stamps long enough to slice.
pub fn dt_key_sql(e: &str) -> String {
    // the arbiter takes the fraction only after a '.'/',' and otherwise
    // treats it as absent — junk between the seconds and the 'Z' is dropped
    let frac = format!(
        "(CASE WHEN substr({e},20,1) IN ('.', ',') \
         THEN substr({e},21,length({e})-21) ELSE '' END)"
    );
    // zero-pad to six WITHOUT truncating: rpad would shorten a nanosecond
    // fraction the arbiter keeps in full, which turns a near-tie into a tie
    format!(
        "(CASE WHEN right({e},1) = 'Z' AND length({e}) >= 20 \
         THEN substr({e},1,19) || '.' || {frac} || repeat('0', greatest(0, 6 - length{frac})) \
         ELSE {e} END) COLLATE \"C\""
    )
}

/// `None` = a shape this compiler does not reproduce (unknown timerel, or
/// between without an end) — the caller prunes nothing and the in-memory
/// window stays the arbiter.
pub fn compile_instance_range(
    r: &InstanceRange<'_>,
    el: &str,
    first_bind: usize,
) -> Option<CompiledSql> {
    let tp = format!("${first_bind}");
    let present = format!("jsonb_typeof({el} -> {tp}) = 'string'");
    let ts = dt_key_sql(&format!("({el} ->> {tp})"));
    // the bound is keyed too — keying one side only is what made the
    // pushdown stricter than the arbiter
    let at = |n: usize| dt_key_sql(&format!("${n}::text"));
    let mut binds = vec![r.timeproperty.to_owned()];
    let sql = match r.timerel {
        "any" => present,
        "before" => {
            binds.push(r.time_at.to_owned());
            format!("{present} AND {ts} < {}", at(first_bind + 1))
        }
        "after" => {
            binds.push(r.time_at.to_owned());
            format!("{present} AND {ts} >= {}", at(first_bind + 1))
        }
        "between" => {
            let end = r.end_time_at?;
            binds.push(r.time_at.to_owned());
            binds.push(end.to_owned());
            format!(
                "{present} AND {ts} >= {} AND {ts} < {}",
                at(first_bind + 1),
                at(first_bind + 2)
            )
        }
        _ => return None,
    };
    Some(CompiledSql { sql, binds })
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
    // deleted_at is nullable AND was unfilled before migration 0009's era —
    // its bound must let NULL rows through to the text predicate (which
    // decides membership either way); the other columns are NOT NULL since
    // schema 0003, so their bounds stay bare.
    let (col, nullable) = match r.timeproperty {
        "observedAt" => ("observed_at", false),
        "createdAt" => ("created_at", false),
        "modifiedAt" => ("modified_at", false),
        "deletedAt" => ("deleted_at", true),
        _ => return None,
    };
    let bound = match r.timerel {
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
    };
    Some(if nullable {
        format!("({alias}.{col} IS NULL OR ({bound}))")
    } else {
        bound
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
        assert!(c.sql.contains(" < (CASE"), "sql: {}", c.sql);
        assert!(c.sql.contains("$5::text"), "sql: {}", c.sql);
        assert_eq!(c.binds, vec!["observedAt", "2026-01-01T00:00:00Z"]);

        let c = compile_instance_range(&range("after", "t0", None), "el", 1).expect("compiles");
        assert!(c.sql.contains(" >= (CASE"), "sql: {}", c.sql);
        assert!(c.sql.contains("$2::text"), "sql: {}", c.sql);

        let c = compile_instance_range(&range("between", "t0", Some("t1")), "el", 1).expect("c");
        assert!(
            c.sql.contains(" >= (CASE") && c.sql.contains(" < (CASE"),
            "{}",
            c.sql
        );
        assert!(c.sql.contains("$2::text") && c.sql.contains("$3::text"));
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

    /// 4.11 bounds are inclusive/exclusive on the INSTANT, and 4.6.3 spells
    /// one instant several ways (`…00Z`, `…00.000Z`, `…00,0Z`). The store
    /// prunes on this predicate, so it must key BOTH operands the way the
    /// arbiter's `dt_key` does — keying only the stored stamp made
    /// `"…00.000Z" >= "…00Z"` false in bytes and dropped an instance that
    /// `?timerel=after&timeAt=…00Z` must return.
    #[test]
    fn both_operands_are_canonically_keyed() {
        let c = compile_instance_range(&range("after", "2017-12-13T14:20:00Z", None), "el", 1)
            .expect("compiles");
        // one key expression per operand, and the raw jsonb text is never
        // compared directly against the bind
        assert_eq!(c.sql.matches("repeat('0', greatest(0, 6 -").count(), 2);
        assert!(
            !c.sql.contains("(el ->> $1) COLLATE \"C\" >= $2"),
            "raw byte compare survived: {}",
            c.sql
        );
        // the fraction separator the arbiter accepts is accepted here too
        let key = dt_key_sql("x");
        assert!(key.contains("IN ('.', ',')"), "{key}");
        // no cast: a malformed stored stamp must not be able to raise
        assert!(!key.contains("::timestamp"), "{key}");
    }

    #[test]
    fn unknown_shapes_refuse() {
        assert!(compile_instance_range(&range("since", "t0", None), "el", 1).is_none());
        assert!(compile_instance_range(&range("between", "t0", None), "el", 1).is_none());
    }

    /// `timeproperty` and the stamps are client strings: the first is a bind,
    /// the stamps are binds, and the only identifiers in the statement are the
    /// caller's own alias and this module's fixed column table.
    #[test]
    fn client_text_never_reaches_the_statement() {
        let hostile = "observedAt' OR 1=1 --";
        let r = InstanceRange {
            timerel: "between",
            time_at: "2026-01-01T00:00:00Z'; DROP TABLE attr_instances; --",
            end_time_at: Some("2026-02-01T00:00:00Z"),
            timeproperty: hostile,
        };
        let c = compile_instance_range(&r, "el", 1).expect("compiles");
        for needle in ["observedAt", "DROP", "TABLE", "--", "OR 1=1"] {
            assert!(!c.sql.contains(needle), "{needle:?} leaked: {}", c.sql);
        }
        assert!(c
            .sql
            .starts_with("jsonb_typeof(el -> $1) = 'string' AND (CASE"));
        // every identifier in the statement is this module's own, and the
        // only slots are $1..$3
        for n in ["$1", "$2", "$3"] {
            assert!(c.sql.contains(n), "{n} missing: {}", c.sql);
        }
        assert!(!c.sql.contains("$4"), "overshoot: {}", c.sql);
        assert_eq!(c.binds[0], hostile);
        // an unknown timeproperty has no column, so no identifier is ever
        // derived from client text
        assert!(column_range_bound(&r, "ai", 1).is_none());
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
        // deleted_at is nullable and historically unfilled — its bound must
        // carry the IS NULL escape so old rows reach the text predicate
        let deleted = InstanceRange {
            timeproperty: "deletedAt",
            ..range("after", "t0", None)
        };
        let s = column_range_bound(&deleted, "ai", 1).expect("bound");
        assert!(s.starts_with("(ai.deleted_at IS NULL OR ("), "{s}");
        assert!(column_range_bound(&range("any", "", None), "ai", 1).is_none());
        assert!(column_range_bound(&range("since", "t0", None), "ai", 1).is_none());
        assert!(column_range_bound(&range("between", "t0", None), "ai", 1).is_none());
    }
}
