// SPDX-License-Identifier: EUPL-1.2
//! The temporal query of a request: `TemporalQ` parsed from the
//! timerel/timeAt/endTimeAt/timeproperty parameters (5.2.21, Table
//! 5.2.21-1) and the instance match it defines (4.11).

use antares_jsonld::parse_datetime;
use antares_model::{dt_key, NgsiError};
use serde_json::Value;
use std::collections::HashMap;

#[derive(Clone)]
pub struct TemporalQ {
    pub timerel: String,
    pub time_at: String,
    pub end_time_at: Option<String>,
    pub timeproperty: String,
}

impl TemporalQ {
    /// The 4.11 Temporal Query from its request parameters: `timerel` decides
    /// which of `timeAt`/`endTimeAt` are required, and `required` says
    /// whether the operation demands one at all (5.7.4 does, 5.7.3 does not).
    /// `GeoQuery::from_params` is the same convention for a different
    /// parameter family, not the same parser.
    pub fn from_params(
        params: &HashMap<String, String>,
        required: bool,
    ) -> Result<Option<Self>, NgsiError> {
        let bad = NgsiError::BadRequestData;
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

    pub(crate) fn instance_matches(&self, inst: &Value) -> bool {
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

#[cfg(test)]
mod tests {
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
