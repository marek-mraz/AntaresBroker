// SPDX-License-Identifier: EUPL-1.2
//! The authorization seam: a policy engine written the way one from outside
//! this repository would be (ADR-0020).
//!
//! Static rules from a JSON document, one entry per tenant. It is
//! deliberately the simplest engine that is USEFUL rather than the most
//! capable: no policy server, no token parsing, no rule language. What it
//! shows is the shape of the seam — that an engine outside `crates/` can
//! deny an operation, narrow a query, project a document and a notification,
//! and that none of it needs a line of core code.
//!
//! ```json
//! {
//!   "acme": {
//!     "denyTypes": ["Secret"],
//!     "omit": ["price"],
//!     "q": "speed<100"
//!   }
//! }
//! ```
//!
//! A tenant with no entry is unrestricted. `denyTypes` refuses any operation
//! that names one of those Entity types; `omit` drops those Attributes from
//! every document served and every notification sent; `q` is conjoined into
//! every query the tenant runs.

use antares_api::policy::{
    Decision, DecisionFuture, Filter, NotifyDecision, Operation, PolicyEngine, Subject,
};
use serde_json::Value;
use std::collections::HashMap;

/// The name this engine is selected by (`ANTARES_POLICY=example`).
pub const POLICY_NAME: &str = "example";

/// Where the rules document is read from.
pub const RULES_ENV: &str = "ANTARES_POLICY_RULES";

/// What one tenant may do.
#[derive(Default)]
struct Rule {
    deny_types: Vec<String>,
    omit: Vec<String>,
    q: Option<antares_ql::QNode>,
}

impl Rule {
    /// Whether this rule refuses an Entity type.
    ///
    /// The broker hands an engine EXPANDED names (ADR-0020), while a rules
    /// document is written by a person. Both forms match, so a deployment
    /// can write `"Secret"` and mean the type its `@context` expands that
    /// way — or write the IRI and mean exactly it.
    fn denies(&self, iri: &str) -> bool {
        let short = iri.rsplit(['/', '#']).next().unwrap_or(iri);
        self.deny_types.iter().any(|d| d == iri || d == short)
    }

    fn filter(&self) -> Option<Filter> {
        (!self.omit.is_empty() || self.q.is_some()).then(|| Filter {
            q: self.q.clone(),
            omit: self.omit.clone(),
            // the caller is told the answer was narrowed; ADR-0020 leaves
            // this to the engine, and an engine that hides a row silently
            // is harder to operate than one that says so
            restricted: true,
            ..Filter::default()
        })
    }
}

/// The engine. Rules are read once, at startup: a policy that could change
/// under a running request is a different design, and this one is meant to
/// be readable rather than dynamic.
pub struct ExamplePolicy {
    rules: HashMap<String, Rule>,
    /// Set when the rules could not be read. ADR-0020: a deployment that
    /// wires in a broken engine loses service, never its access rules — so
    /// this engine refuses everything rather than allowing everything.
    broken: Option<String>,
}

impl ExamplePolicy {
    /// The shelf entry: read [`RULES_ENV`], parse it, and fail closed if
    /// either step does not work. Selecting this engine without a rules
    /// document is itself the failure — an engine with no rules that allowed
    /// every request would be a wide-open broker wearing a policy's name.
    pub fn from_env() -> Self {
        let broken = |why: String| Self {
            rules: HashMap::new(),
            broken: Some(why),
        };
        let Ok(path) = std::env::var(RULES_ENV) else {
            return broken(format!("{RULES_ENV} is not set"));
        };
        let raw = match std::fs::read_to_string(&path) {
            Ok(raw) => raw,
            Err(e) => return broken(format!("{RULES_ENV}={path}: {e}")),
        };
        match serde_json::from_str::<Value>(&raw) {
            Ok(v) => match Self::from_value(&v) {
                Ok(engine) => engine,
                Err(e) => broken(format!("{RULES_ENV}={path}: {e}")),
            },
            Err(e) => broken(format!("{RULES_ENV}={path}: {e}")),
        }
    }

    /// The rules as a parsed document, which is how the tests build one.
    pub fn from_value(doc: &Value) -> Result<Self, String> {
        let obj = doc
            .as_object()
            .ok_or_else(|| "the rules document must be a JSON object".to_owned())?;
        let mut rules = HashMap::new();
        for (tenant, v) in obj {
            let strings = |key: &str| -> Vec<String> {
                v.get(key)
                    .and_then(Value::as_array)
                    .map(|a| {
                        a.iter()
                            .filter_map(Value::as_str)
                            .map(str::to_owned)
                            .collect()
                    })
                    .unwrap_or_default()
            };
            let q = match v.get("q").and_then(Value::as_str) {
                Some(q) => Some(
                    antares_ql::parse_q(q)
                        .map_err(|e| format!("tenant {tenant:?}: q {q:?} does not parse: {e}"))?,
                ),
                None => None,
            };
            rules.insert(
                tenant.clone(),
                Rule {
                    deny_types: strings("denyTypes"),
                    omit: strings("omit"),
                    q,
                },
            );
        }
        Ok(Self {
            rules,
            broken: None,
        })
    }

    /// Why this engine refuses everything, if it does.
    pub fn broken(&self) -> Option<&str> {
        self.broken.as_deref()
    }

    /// The Entity types one operation names: the request's own type
    /// selector, plus the type of a write's body — a create that names no
    /// type in its query string still creates an Entity of one.
    fn types_of(op: &Operation<'_>) -> Vec<String> {
        let mut out: Vec<String> = op.types.to_vec();
        if let Some(t) = op.body.and_then(|b| b.get("type")) {
            match t {
                Value::String(s) => out.push(s.clone()),
                Value::Array(a) => {
                    out.extend(a.iter().filter_map(Value::as_str).map(str::to_owned))
                }
                _ => {}
            }
        }
        out
    }
}

impl PolicyEngine for ExamplePolicy {
    fn name(&self) -> &str {
        POLICY_NAME
    }

    fn decide<'a>(&'a self, subject: &'a Subject, op: &'a Operation<'a>) -> DecisionFuture<'a> {
        Box::pin(std::future::ready(self.judge(subject, op)))
    }

    fn pre_notify(&self, subject: &Subject, _sub: &Value, _n: &mut Value) -> NotifyDecision {
        if let Some(why) = &self.broken {
            log_broken(why);
            return NotifyDecision::Drop;
        }
        match self.rules.get(subject.tenant.as_str()) {
            // A notification is narrowed by projection only (ADR-0020), so
            // the tenant's `q` has no meaning here and is left off: it
            // selected which Entities the SUBSCRIPTION matches, which the
            // subscription's own conditions already did.
            Some(rule) if !rule.omit.is_empty() => NotifyDecision::Filter(Filter {
                omit: rule.omit.clone(),
                ..Filter::default()
            }),
            _ => NotifyDecision::Deliver,
        }
    }
}

impl ExamplePolicy {
    /// The decision, split out so it is readable without the boxed future.
    fn judge(&self, subject: &Subject, op: &Operation<'_>) -> Decision {
        if let Some(why) = &self.broken {
            log_broken(why);
            return Decision::Deny(format!("policy rules unavailable: {why}"));
        }
        let Some(rule) = self.rules.get(subject.tenant.as_str()) else {
            return Decision::Allow;
        };
        if let Some(t) = Self::types_of(op).into_iter().find(|t| rule.denies(t)) {
            return Decision::Deny(format!("entity type {t} is not available to this subject"));
        }
        match rule.filter() {
            Some(f) => Decision::Filter(f),
            None => Decision::Allow,
        }
    }
}

/// One line per refusal is one line per request; the broker's own log is
/// noisy enough without it, so the reason is stated once.
fn log_broken(why: &str) {
    use std::sync::Once;
    static ONCE: Once = Once::new();
    ONCE.call_once(|| {
        eprintln!("antares-plugin-example: refusing every operation — {why}");
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn subject(tenant: &str) -> Subject {
        Subject {
            tenant: antares_model::TenantId::new(tenant).expect("tenant"),
            headers: Vec::new(),
        }
    }

    fn engine() -> ExamplePolicy {
        ExamplePolicy::from_value(&json!({
            "acme": {"denyTypes": ["Secret"], "omit": ["price"], "q": "speed<100"}
        }))
        .expect("rules")
    }

    /// A tenant the document does not name is not restricted: the engine
    /// narrows what it was told to and nothing else.
    #[test]
    fn an_unnamed_tenant_is_unrestricted() {
        let e = engine();
        assert!(matches!(
            e.judge(&subject("other"), &Operation::new("5.7.2")),
            Decision::Allow
        ));
        assert!(matches!(
            e.pre_notify(&subject("other"), &json!({}), &mut json!({})),
            NotifyDecision::Deliver
        ));
    }

    /// Both the short name a person writes and the IRI the broker passes.
    #[test]
    fn a_denied_type_matches_the_short_name_and_the_iri() {
        let e = engine();
        for t in [
            "Secret".to_owned(),
            "https://uri.etsi.org/ngsi-ld/default-context/Secret".to_owned(),
        ] {
            let types = [t.clone()];
            let op = Operation {
                types: &types,
                ..Operation::new("5.7.2")
            };
            assert!(
                matches!(e.judge(&subject("acme"), &op), Decision::Deny(_)),
                "{t} was allowed"
            );
        }
        let types = ["https://uri.etsi.org/ngsi-ld/default-context/Vehicle".to_owned()];
        let op = Operation {
            types: &types,
            ..Operation::new("5.7.2")
        };
        assert!(matches!(
            e.judge(&subject("acme"), &op),
            Decision::Filter(_)
        ));
    }

    /// A create names its type in the body, not in a type selector.
    #[test]
    fn a_write_is_judged_by_the_type_of_its_body() {
        let e = engine();
        let body = json!({"id": "urn:ngsi-ld:Secret:1", "type": ["Secret"]});
        let op = Operation {
            body: Some(&body),
            ..Operation::new("5.6.1")
        };
        assert!(matches!(e.judge(&subject("acme"), &op), Decision::Deny(_)));
    }

    /// The narrowing the rules describe, and nothing more.
    #[test]
    fn the_filter_carries_the_rules_and_says_so() {
        let e = engine();
        let Decision::Filter(f) = e.judge(&subject("acme"), &Operation::new("5.7.2")) else {
            panic!("a tenant with rules is narrowed");
        };
        assert_eq!(f.omit, vec!["price".to_owned()]);
        assert!(f.q.is_some());
        assert!(f.restricted, "a narrowed answer says it was narrowed");
        assert!(f.pick.is_empty() && f.scope_q.is_none());
    }

    /// A notification is narrowed by projection only.
    #[test]
    fn a_notification_carries_the_projection_and_not_the_query() {
        let e = engine();
        let NotifyDecision::Filter(f) = e.pre_notify(&subject("acme"), &json!({}), &mut json!({}))
        else {
            panic!("a tenant with an omit list narrows its notifications");
        };
        assert_eq!(f.omit, vec!["price".to_owned()]);
        assert!(
            f.q.is_none(),
            "a query on a notification is a narrowing the broker cannot apply"
        );
    }

    /// Rules that cannot be read are not rules that allow everything.
    #[test]
    fn an_engine_without_rules_refuses_everything() {
        let e = ExamplePolicy {
            rules: HashMap::new(),
            broken: Some("no rules".into()),
        };
        assert!(matches!(
            e.judge(&subject("acme"), &Operation::new("5.7.2")),
            Decision::Deny(_)
        ));
        assert!(matches!(
            e.pre_notify(&subject("acme"), &json!({}), &mut json!({})),
            NotifyDecision::Drop
        ));
    }

    /// A rules document the engine cannot make sense of is an error at load
    /// time, not a rule that quietly does nothing.
    #[test]
    fn a_rules_document_that_does_not_parse_is_rejected() {
        assert!(ExamplePolicy::from_value(&json!([])).is_err());
        assert!(
            ExamplePolicy::from_value(&json!({"acme": {"q": "(unbalanced"}})).is_err(),
            "a q that does not parse would narrow nothing"
        );
    }
}
