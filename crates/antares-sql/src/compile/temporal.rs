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
}
