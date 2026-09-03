// SPDX-License-Identifier: EUPL-1.2
//! The policy seam: one trait, one built-in engine, every engine an addon
//! (ADR-0020).
//!
//! The broker takes no authorization decision of its own. It asks the
//! engine a deployment gave it and obeys, and what an engine may answer is
//! deliberately narrow: allow, deny, or narrow the operation. Nothing here
//! parses a credential or validates a token — authentication, rate limiting
//! and quotas stay in the gateway in front of the broker.
//!
//! Everything in this module is core code, compiled and tested in every
//! build. The only engine the broker ships is [`AllowAll`], and conformance
//! is asserted against it; any other engine is an addon crate outside
//! `crates/`, behind an off-by-default `antares-broker` feature.
//!
//! The seam fails closed: an engine that errors, panics or runs past
//! [`TIMEOUT`] denies. A deployment that wires in a broken engine loses
//! service, never its access rules.

use antares_ql::geo::GeoQuery;
use antares_ql::QNode;
use axum::http::HeaderMap;
use serde_json::Value;
use std::future::Future;
use std::pin::Pin;
use std::sync::LazyLock;
use std::time::Duration;

/// What [`PolicyEngine::decide`] hands back. Boxed because an engine that
/// asks a policy server has to await, and the trait must stay object-safe:
/// the broker holds one `Arc<dyn PolicyEngine>` chosen at startup.
pub type DecisionFuture<'a> = Pin<Box<dyn Future<Output = Decision> + Send + 'a>>;

/// How long an engine has to answer before the broker stops waiting and
/// denies. Deployment knob (`ANTARES_POLICY_TIMEOUT_MS`), read once at
/// first use. The default is short on purpose: the seam sits in front of
/// every request, and an engine that cannot answer inside it is an outage
/// either way — failing closed at 250 ms is the difference between a 403
/// and a broker that stops accepting.
pub static TIMEOUT: LazyLock<Duration> = LazyLock::new(|| {
    let ms = std::env::var("ANTARES_POLICY_TIMEOUT_MS")
        .ok()
        .and_then(|v| v.parse().ok())
        .filter(|n: &u64| *n > 0)
        .unwrap_or(250);
    Duration::from_millis(ms)
});

/// The reasons the seam denies on its own, rather than on an engine's word.
/// Fixed strings: they reach the client in a ProblemDetails `detail`, and
/// an engine's own text about why it failed is not the client's business.
pub const ENGINE_TIMED_OUT: &str = "the policy engine did not answer in time";
pub const ENGINE_FAILED: &str = "the policy engine failed";

/// The clauses whose operations act on everything the tenant holds. There
/// is no narrowed form of "delete every entity" or "snapshot the tenant",
/// so a [`Decision::Filter`] on one of them is answered as a deny
/// ([`resolve`]).
pub const WHOLE_TENANT: [&str; 4] = ["5.6.21", "5.16.1", "5.16.2", "5.16.7"];

/// The clauses whose handlers read the [`Filter`] an engine returns: the
/// reads that can serve less than they were asked for. Every other clause
/// takes its operation whole — a create either writes the Entity or does
/// not — so a [`Decision::Filter`] there would be dropped, and the broker
/// would perform in full an operation the engine believed it had narrowed.
/// That is the one direction the seam may not fail in, so those clauses
/// answer a narrowing as a deny ([`resolve`]).
pub const FILTERABLE: [&str; 6] = ["5.7.1", "5.7.2", "5.7.3", "5.7.4", "5.14.4", "5.14.5"];

/// Who is asking. The headers are the ones a deployment names for the
/// seam to carry, copied verbatim and never interpreted: the broker does
/// not know what a token is. They never leave this process — stripped from every
/// forwarded request, absent from notifications, dead letters and logs,
/// which is why [`std::fmt::Debug`] here prints names and not values.
#[derive(Clone)]
pub struct Subject {
    pub tenant: antares_model::TenantId,
    pub headers: Vec<(String, String)>,
}

impl std::fmt::Debug for Subject {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Subject")
            .field("tenant", &self.tenant)
            .field(
                "headers",
                &self.headers.iter().map(|(k, _)| k).collect::<Vec<_>>(),
            )
            .finish()
    }
}

/// What is being asked for, after negotiation and expansion: the request
/// the broker is about to run, in the terms the operation itself uses.
/// Every name is expanded, so an engine writes its rules against IRIs and
/// not against whatever short name the caller's `@context` happened to use.
pub struct Operation<'a> {
    /// The CIM 009 clause of the operation, e.g. `"5.6.1"`.
    pub clause: &'static str,
    /// The Entity identifiers the operation names, as the handler holds
    /// them; empty for a query that names none.
    pub ids: &'a [&'a str],
    pub types: &'a [String],
    pub attrs: &'a [String],
    pub q: Option<&'a QNode>,
    pub scope_q: Option<&'a str>,
    pub geo: Option<&'a GeoQuery>,
    /// The request body of a write, expanded. `None` for a read.
    pub body: Option<&'a Value>,
}

/// Shape and counts, never the payload: an `Operation` carries the
/// caller's data, and a log line is not where it belongs.
impl std::fmt::Debug for Operation<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Operation")
            .field("clause", &self.clause)
            .field("ids", &self.ids.len())
            .field("types", &self.types.len())
            .field("attrs", &self.attrs.len())
            .field("q", &self.q.is_some())
            .field("scope_q", &self.scope_q.is_some())
            .field("geo", &self.geo.is_some())
            .field("body", &self.body.is_some())
            .finish()
    }
}

impl Operation<'_> {
    /// The operation with nothing but its clause: what a handler that
    /// carries no ids, types or attributes passes, and the base every other
    /// call site fills in with struct-update syntax.
    pub const fn new(clause: &'static str) -> Operation<'static> {
        Operation {
            clause,
            ids: &[],
            types: &[],
            attrs: &[],
            q: None,
            scope_q: None,
            geo: None,
            body: None,
        }
    }
}

/// What an engine may answer. There is no third state: an operation is
/// allowed, refused, or allowed over less than it asked for.
#[derive(Debug, Clone, PartialEq)]
pub enum Decision {
    Allow,
    /// Refused, with the engine's own reason. Answered 403 with a
    /// ProblemDetails whose type is an Antares URI — Table 6.3.2-1 names no
    /// access-denied error, so this is an Antares decision.
    Deny(String),
    /// Allowed over less. The caller cannot tell a hidden entity from an
    /// absent one.
    Filter(Filter),
}

/// How much less. Every member narrows and none widens: the `q` and
/// `scopeQ` are conjoined with the caller's own, `pick` keeps a subset of
/// the members, `omit` removes some.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Filter {
    /// Conjoined into the query the store runs, on the AST — never on the
    /// query string, where the 4.9 precedence of `;` against `|` would have
    /// to be re-derived by whoever writes the rule.
    pub q: Option<QNode>,
    /// Conjoined the same way with the request's own `scopeQ` (4.18).
    pub scope_q: Option<String>,
    /// Members removed from every document served.
    pub omit: Vec<String>,
    /// If non-empty, the only members served beside the document frame.
    pub pick: Vec<String>,
    /// Answer [`RESTRICTED_HEADER`]`: true`, so a client can know the
    /// answer was narrowed. Narrowing is otherwise silent.
    pub restricted: bool,
}

/// What identifies the document rather than describing it: 5.2.4 makes
/// `id` and `type` mandatory members of an Entity, and an answer without
/// them is not an Entity at all. A projection never removes these.
const FRAME: [&str; 3] = ["id", "type", "@context"];

impl Filter {
    /// Apply `pick`/`omit` to one document by member name. This is the
    /// reference semantics `run_policy_contract` holds an engine's answer
    /// against; a served document is projected through the request's own
    /// 5.5.2 representation instead (`repr::narrow_projection`), where the
    /// names are expanded against the request `@context` first.
    pub fn project(&self, doc: &mut Value) {
        let Some(obj) = doc.as_object_mut() else {
            return;
        };
        if !self.pick.is_empty() {
            obj.retain(|k, _| FRAME.contains(&k.as_str()) || self.pick.iter().any(|p| p == k));
        }
        for name in &self.omit {
            if FRAME.contains(&name.as_str()) {
                continue;
            }
            obj.remove(name);
        }
    }

    /// True when the filter would change nothing, which is how an engine
    /// that means "allow" can say so with an empty `Filter`.
    pub fn is_empty(&self) -> bool {
        self.q.is_none() && self.scope_q.is_none() && self.omit.is_empty() && self.pick.is_empty()
    }

    /// The request's own query parameters, narrowed by this decision.
    ///
    /// The `q` conjunction is made on the AST and rendered back once
    /// (`antares_ql`'s renderer parenthesises an `Or` inside an `And`, so
    /// 4.9's `;`-over-`|` precedence is the renderer's problem and not the
    /// rule writer's). Every consumer below reads the narrowed parameters:
    /// the store push-down, the local re-check that 5.7.2.4 runs over
    /// merged results, and the query the request is forwarded with.
    ///
    /// A `scopeQ` narrowing is set when the request carries none, and
    /// intersected with the request's own when it carries one. 4.19's `and`
    /// is over independent per-pattern predicates, so it distributes over
    /// the `,`/`|` disjunction and the intersection is itself a Scope
    /// Query — `antares_ql::scope::intersect_scope_q` writes it. Where the
    /// product cannot be written down the seam answers the only way that is
    /// not wider than the engine decided: it refuses.
    /// Say the answer was narrowed, when the engine asked for it to be
    /// said. Narrowing is otherwise silent: a caller cannot tell an Entity
    /// it may not see from one that is not there.
    pub fn mark_restricted(&self, headers: &mut HeaderMap) {
        if self.restricted {
            headers.insert(
                RESTRICTED_HEADER,
                axum::http::HeaderValue::from_static("true"),
            );
        }
    }

    pub fn narrow_params(
        &self,
        params: &std::collections::HashMap<String, String>,
    ) -> Result<std::collections::HashMap<String, String>, Denied> {
        let mut out = params.clone();
        if let Some(extra) = &self.q {
            let narrowed = match out.get("q").map(|q| antares_ql::parse_q(q)).transpose() {
                Ok(Some(own)) => QNode::And(vec![own, extra.clone()]),
                Ok(None) => extra.clone(),
                // the request's own `q` is parsed and refused long before a
                // filter reaches it; an unparsable one here is a caller that
                // narrowed something it never validated
                Err(_) => return Err(Denied(ENGINE_FAILED.to_owned())),
            };
            out.insert("q".to_owned(), narrowed.to_string());
        }
        if let Some(scope) = &self.scope_q {
            let narrowed = match out.get("scopeQ") {
                None => scope.clone(),
                Some(own) => antares_ql::scope::intersect_scope_q(own, scope)
                    .ok_or_else(|| Denied(SCOPE_NOT_NARROWABLE.to_owned()))?,
            };
            out.insert("scopeQ".to_owned(), narrowed);
        }
        Ok(out)
    }
}

/// The response header a `Filter { restricted: true }` adds. It is not in
/// the `NGSILD-` namespace: that prefix is ETSI's, carries the headers
/// clause 6.3 defines (`NGSILD-Tenant`, `NGSILD-EntityMap`,
/// `NGSILD-Results-Count`, `NGSILD-Warning`), and a broker-invented header
/// under it would collide with whatever a later version puts there — the
/// same reason a refusal answers `urn:antares:error:AccessDenied` rather
/// than an invented `uri.etsi.org` type.
pub const RESTRICTED_HEADER: &str = "Antares-Results-Restricted";

/// The refusal a `scopeQ` narrowing gets when its intersection with the
/// request's own cannot be written as a Scope Query — one side selects
/// nothing, or the distributed product is too large to express (see
/// [`Filter::narrow_params`]).
pub const SCOPE_NOT_NARROWABLE: &str =
    "the request's own scope query cannot be narrowed by the policy engine";

/// What an engine may answer about one notification, for one subscription.
#[derive(Debug, Clone, PartialEq)]
pub enum NotifyDecision {
    Deliver,
    /// Send it, narrowed: the same `pick`/`omit` projection the query path
    /// applies, over the entities of `data`. A notification `Filter` that
    /// carries `q` or `scopeQ` is refused as a [`NotifyDecision::Drop`]:
    /// the entities were selected by the subscription's own conditions long
    /// before this point, there is nothing left to re-run the query against,
    /// and delivering the notification unfiltered would tell the engine a
    /// narrowing was applied that never was.
    Filter(Filter),
    /// Do not send. 5.8.6 counts this as no attempt at all — the
    /// notification was never sent, so it is neither a success nor a
    /// failure, and `timesSent` does not move.
    Drop,
}

/// The seam. One implementation ships with the broker; every other one is
/// an addon crate a deployment builds itself.
pub trait PolicyEngine: Send + Sync {
    /// The name a deployment selects the engine by, and the name
    /// `/q/health` reports.
    fn name(&self) -> &str;

    /// Fires once per request, after negotiation and expansion, before the
    /// operation and before any fan-out (ADR-0014 `on_request`).
    fn decide<'a>(&'a self, subject: &'a Subject, op: &'a Operation<'a>) -> DecisionFuture<'a>;

    /// Fires once per notification document per subscription, before the
    /// egress check and the send (ADR-0014 `pre_notify`). Synchronous: it
    /// sits inside the delivery the broker is about to make, and an engine
    /// that has to ask a server for every notification is a design the seam
    /// declines to make easy.
    fn pre_notify(
        &self,
        subject: &Subject,
        sub: &Value,
        notification: &mut Value,
    ) -> NotifyDecision;
}

/// The engine the broker ships, and the one conformance is asserted
/// against: it decides nothing, so the broker behaves exactly as it did
/// before the seam existed.
pub struct AllowAll;

impl PolicyEngine for AllowAll {
    fn name(&self) -> &str {
        "allow-all"
    }

    fn decide<'a>(&'a self, _subject: &'a Subject, _op: &'a Operation<'a>) -> DecisionFuture<'a> {
        Box::pin(std::future::ready(Decision::Allow))
    }

    fn pre_notify(&self, _s: &Subject, _sub: &Value, _n: &mut Value) -> NotifyDecision {
        NotifyDecision::Deliver
    }
}

/// A `Filter` only means something where the broker can serve less than it
/// was asked for, and [`FILTERABLE`] is that list. Everywhere else the
/// narrowing would be silently dropped: "purge every entity, but only the
/// ones you may see" is a different operation from the one the client
/// asked for and would delete less than the 204 claims, and a create writes
/// its Entity or does not. Those operations take the strict reading: narrow
/// means refuse. An empty `Filter` asks for nothing and is an allow.
pub fn resolve(clause: &str, decision: Decision) -> Decision {
    match decision {
        Decision::Filter(f) if !FILTERABLE.contains(&clause) => {
            if f.is_empty() {
                Decision::Allow
            } else if WHOLE_TENANT.contains(&clause) {
                Decision::Deny(format!(
                    "{clause} acts on everything the tenant holds and cannot be narrowed"
                ))
            } else {
                Decision::Deny(format!(
                    "{clause} is performed whole or not at all and cannot be narrowed"
                ))
            }
        }
        other => other,
    }
}

/// Ask the engine, and fail closed. An engine that panics, or that runs
/// past [`TIMEOUT`], denies: a seam that waved the request through on its
/// own failure would turn a broken addon into an open door.
///
/// The panic is caught in place rather than on a spawned task, because the
/// task boundary would demand `'static` of the operation, which borrows the
/// request. It is caught TWICE, because an engine has two places to fail:
/// `decide` returns a boxed future, and a synchronous engine does its whole
/// decision in the call that builds that future — the reference engine is
/// `Box::pin(ready(self.judge(..)))` — so guarding only the future guards
/// the half that does nothing.
///
/// [`TIMEOUT`] can only race the future. Work done before the future exists
/// holds the executor thread, and no timer inside the same task can
/// interrupt it; an engine that blocks is a deployment's own bug, and the
/// bound that catches it is the request timeout in front of the broker.
pub async fn decide(engine: &dyn PolicyEngine, subject: &Subject, op: &Operation<'_>) -> Decision {
    #[cfg(not(target_arch = "wasm32"))]
    {
        use futures_util::FutureExt as _;
        let built =
            std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| engine.decide(subject, op)));
        let Ok(fut) = built else {
            tracing::error!("policy engine {} panicked; denying", engine.name());
            metrics::counter!("antares_policy_failures_total", "reason" => "panic").increment(1);
            return Decision::Deny(ENGINE_FAILED.to_owned());
        };
        let guarded = std::panic::AssertUnwindSafe(fut).catch_unwind();
        match tokio::time::timeout(*TIMEOUT, guarded).await {
            Ok(Ok(d)) => resolve(op.clause, d),
            Ok(Err(_)) => {
                tracing::error!("policy engine {} panicked; denying", engine.name());
                metrics::counter!("antares_policy_failures_total", "reason" => "panic")
                    .increment(1);
                Decision::Deny(ENGINE_FAILED.to_owned())
            }
            Err(_) => {
                tracing::error!(
                    "policy engine {} did not answer within {:?}; denying",
                    engine.name(),
                    *TIMEOUT
                );
                metrics::counter!("antares_policy_failures_total", "reason" => "timeout")
                    .increment(1);
                Decision::Deny(ENGINE_TIMED_OUT.to_owned())
            }
        }
    }
    // The browser build has no timer to race against and aborts on panic;
    // it also loads no addon, so the engine is always the built-in one.
    #[cfg(target_arch = "wasm32")]
    {
        resolve(op.clause, engine.decide(subject, op).await)
    }
}

/// Ask the engine about one notification, and fail closed: an engine that
/// panics drops the notification. The document it was handed may already be
/// half-edited, and the broker cannot know which half — so it is not sent.
pub fn pre_notify(
    engine: &dyn PolicyEngine,
    subject: &Subject,
    sub: &Value,
    notification: &mut Value,
) -> NotifyDecision {
    match std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        engine.pre_notify(subject, sub, notification)
    })) {
        Ok(d) => d,
        Err(_) => {
            tracing::error!(
                "policy engine {} panicked on a notification; dropping it",
                engine.name()
            );
            metrics::counter!("antares_policy_failures_total", "reason" => "panic").increment(1);
            NotifyDecision::Drop
        }
    }
}

/// The ProblemDetails `type` and `title` of a refusal. Table 6.3.2-1 names
/// no access-denied error, so none is invented under the ETSI namespace: a
/// refusal is answered with this broker's own URN, documented in the book
/// and noted in the 6.3.2 ledger entry. A client that reads error types can
/// tell a policy refusal from a spec error by the namespace alone.
pub const ACCESS_DENIED_TYPE: &str = "urn:antares:error:AccessDenied";
pub const ACCESS_DENIED_TITLE: &str = "AccessDenied";

/// The request headers copied into a [`Subject`], comma-separated
/// (`ANTARES_POLICY_SUBJECT_HEADERS`), matched case-insensitively and read
/// once at first use. Empty by default: an engine that wants an identity
/// names the header that carries it, and a broker that copied every header
/// into the subject would be handing an addon the whole request.
pub static SUBJECT_HEADERS: LazyLock<Vec<String>> = LazyLock::new(|| {
    std::env::var("ANTARES_POLICY_SUBJECT_HEADERS")
        .unwrap_or_default()
        .split(',')
        .map(|s| s.trim().to_ascii_lowercase())
        .filter(|s| !s.is_empty())
        .collect()
});

/// Who this request is from, as far as the seam is concerned: the tenant it
/// addresses and the headers a deployment named. A header the request does
/// not carry is simply absent — the seam invents nothing.
pub fn subject_of(tenant: &antares_model::TenantId, headers: &HeaderMap) -> Subject {
    let mut carried = Vec::new();
    for name in SUBJECT_HEADERS.iter() {
        for value in headers.get_all(name.as_str()) {
            if let Ok(v) = value.to_str() {
                carried.push((name.clone(), v.to_owned()));
            }
        }
    }
    Subject {
        tenant: tenant.clone(),
        headers: carried,
    }
}

/// The member a Subscription carries its creator's subject in. A
/// broker-internal member like `__context` and `__via`: a client can
/// neither set it nor read it back, it is stripped from every served
/// representation and from the 5.8.1.4 copy forwarded to a Context Source,
/// and 5.2.12 defines no such member — the whole `__` prefix is the
/// broker's, so a member added later inherits every one of those rules
/// instead of a new list to forget.
pub(crate) const SUBJECT_MEMBER: &str = "__subject";

/// The stored form of a subject: only the headers a deployment named, since
/// the tenant is where the subscription already lives. `None` when the
/// subject carries nothing, so a deployment that named no header stores no
/// member at all.
pub(crate) fn subject_member(subject: &Subject) -> Option<Value> {
    (!subject.headers.is_empty())
        .then(|| serde_json::to_value(&subject.headers).ok())
        .flatten()
}

/// The subject a notification is delivered under: the one stored when the
/// subscription was created. A subscription created before the deployment
/// named its headers — or by the broker itself, for the 5.8.1.4 internal
/// copies — simply has none, and the engine is asked about a subject with
/// no headers rather than about the wrong one.
pub(crate) fn stored_subject(tenant: &antares_model::TenantId, sub: &Value) -> Subject {
    Subject {
        tenant: tenant.clone(),
        headers: sub
            .get(SUBJECT_MEMBER)
            .cloned()
            .and_then(|v| serde_json::from_value(v).ok())
            .unwrap_or_default(),
    }
}

/// Remove every broker-internal member from a document about to be served.
///
/// The `__` prefix is the broker's: the notification `@context` (5.8.6), the
/// 6.3.18 Via chain, a snapshot's synthetic tenant and [`SUBJECT_MEMBER`]
/// all live under it, and no NGSI-LD data type defines a member there. One
/// predicate rather than a list per document type, so a member added later
/// is hidden by construction instead of by remembering every serve point.
pub(crate) fn strip_internal(doc: &mut Value) {
    if let Some(o) = doc.as_object_mut() {
        o.retain(|k, _| !k.starts_with("__"));
    }
}

/// Whether a stored document was made by this subject. Compared on the
/// headers alone: the tenant is where the document already lives, and a
/// deployment that named no header has no subjects to tell apart — every
/// request is the same one, and everything it stored is its own.
pub(crate) fn belongs_to(doc: &Value, subject: &Subject) -> bool {
    stored_subject(&subject.tenant, doc).headers == subject.headers
}

/// A refusal on its way out of a handler. Its own type rather than an
/// `ApiError`, so this module names nothing above it and stays a leaf; the
/// `?` in a handler converts it through `From`.
#[derive(Debug, Clone, PartialEq)]
pub struct Denied(pub String);

/// The one call a handler makes. Every operation passes through it exactly
/// once, which is what `every_route_asks_the_policy_engine_once` asserts by
/// walking the router with a counting engine.
///
/// The returned [`Filter`] is what the answer has to be narrowed by; an
/// allow returns the empty filter, which narrows nothing.
pub async fn gate(
    engine: &dyn PolicyEngine,
    tenant: &antares_model::TenantId,
    headers: &HeaderMap,
    op: &Operation<'_>,
) -> Result<Filter, Denied> {
    match decide(engine, &subject_of(tenant, headers), op).await {
        Decision::Allow => Ok(Filter::default()),
        Decision::Filter(f) => Ok(f),
        Decision::Deny(why) => Err(Denied(why)),
    }
}

/// The members of a JSON object, or nothing when it is not one.
#[cfg(any(test, feature = "test-kit"))]
fn members(v: &Value) -> Vec<String> {
    v.as_object()
        .map(|o| o.keys().cloned().collect())
        .unwrap_or_default()
}

/// The contract every engine has to hold, so an addon's own tests can call
/// it. It asserts the three things an engine can actually get wrong — it
/// stops answering, it hands back an answer the seam has to override, or it
/// puts something into a notification that was not there — and it asserts
/// them through the seam, so an engine that passes here passes as the
/// broker will call it.
///
/// The core runs it against [`AllowAll`]; `examples/plugin-example` runs it
/// against the reference engine.
#[cfg(any(test, feature = "test-kit"))]
pub async fn run_policy_contract(engine: &dyn PolicyEngine) {
    let name = engine.name();
    assert!(!name.is_empty(), "an engine answers to a name");

    let subject = Subject {
        tenant: antares_model::TenantId::default(),
        headers: vec![("X-Subject".into(), "someone".into())],
    };
    let doc = serde_json::json!({
        "id": "urn:ngsi-ld:Vehicle:1",
        "type": "Vehicle",
        "speed": {"type": "Property", "value": 10},
        "brand": {"type": "Property", "value": "Skoda"}
    });

    // The engine answers, and the answer is its own: a deny carrying one of
    // the seam's own reasons means it timed out or panicked instead.
    for clause in ["5.6.1", "5.7.2", "5.8.1"] {
        match decide(engine, &subject, &Operation::new(clause)).await {
            Decision::Allow => {}
            Decision::Deny(why) => assert!(
                why != ENGINE_TIMED_OUT && why != ENGINE_FAILED,
                "{name}: {clause} was denied by the seam, not by the engine: {why}"
            ),
            Decision::Filter(f) => {
                let mut narrowed = doc.clone();
                f.project(&mut narrowed);
                let before = members(&doc);
                for key in members(&narrowed) {
                    assert!(
                        before.contains(&key),
                        "{name}: the filter for {clause} added {key:?} to the answer"
                    );
                }
            }
        }
    }

    // An operation the broker performs whole is allowed or refused, never
    // done to less than it says: WHOLE_TENANT because there is no narrowed
    // form of "delete everything", and the writes because no handler there
    // reads a Filter at all (FILTERABLE).
    for clause in WHOLE_TENANT
        .iter()
        .chain(["5.6.1", "5.6.6", "5.8.1", "5.9.2"].iter())
    {
        assert!(
            !matches!(
                decide(engine, &subject, &Operation::new(clause)).await,
                Decision::Filter(_)
            ),
            "{name}: {clause} was answered with a filter"
        );
    }

    // `pre_notify` holds the notification by `&mut`, which is the one place
    // an engine can widen rather than narrow: a member it puts there is a
    // member no subscriber asked for and no store answered with.
    let sub = serde_json::json!({"id": "urn:ngsi-ld:Subscription:1", "type": "Subscription"});
    let mut notification = serde_json::json!({
        "id": "urn:ngsi-ld:Notification:1",
        "type": "Notification",
        "subscriptionId": "urn:ngsi-ld:Subscription:1",
        "notifiedAt": "2026-01-01T00:00:00Z",
        "data": [doc.clone()]
    });
    let before = members(&notification);
    let entity_members = members(&doc);
    if pre_notify(engine, &subject, &sub, &mut notification) != NotifyDecision::Drop {
        for key in members(&notification) {
            assert!(
                before.contains(&key),
                "{name}: pre_notify added {key:?} to the notification"
            );
        }
        let served = notification
            .get("data")
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default();
        for entity in &served {
            for key in members(entity) {
                assert!(
                    entity_members.contains(&key),
                    "{name}: pre_notify added {key:?} to a notified Entity"
                );
            }
        }
    }

    // And the seam stops waiting for this engine, whatever it does.
    #[cfg(not(target_arch = "wasm32"))]
    {
        let slow = Slow(engine);
        assert_eq!(
            decide(&slow, &subject, &Operation::new("5.6.1")).await,
            Decision::Deny(ENGINE_TIMED_OUT.to_owned()),
            "{name}: the seam waited past its timeout instead of denying"
        );
    }
}

/// The same engine, one timeout slower: what the contract wraps it in to
/// prove the seam stops waiting.
#[cfg(all(any(test, feature = "test-kit"), not(target_arch = "wasm32")))]
struct Slow<'e>(&'e dyn PolicyEngine);

#[cfg(all(any(test, feature = "test-kit"), not(target_arch = "wasm32")))]
impl PolicyEngine for Slow<'_> {
    fn name(&self) -> &str {
        self.0.name()
    }

    fn decide<'a>(&'a self, subject: &'a Subject, op: &'a Operation<'a>) -> DecisionFuture<'a> {
        Box::pin(async move {
            tokio::time::sleep(*TIMEOUT * 2 + Duration::from_millis(50)).await;
            self.0.decide(subject, op).await
        })
    }

    fn pre_notify(&self, s: &Subject, sub: &Value, n: &mut Value) -> NotifyDecision {
        self.0.pre_notify(s, sub, n)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use futures_util::FutureExt as _;
    use serde_json::json;

    fn subject() -> Subject {
        Subject {
            tenant: antares_model::TenantId::default(),
            headers: vec![("X-Subject".into(), "a-token-shaped-string".into())],
        }
    }

    /// Run the contract and report whether it refused the engine.
    async fn contract_holds(engine: &dyn PolicyEngine) -> bool {
        let hook = std::panic::take_hook();
        std::panic::set_hook(Box::new(|_| {}));
        let outcome = std::panic::AssertUnwindSafe(run_policy_contract(engine))
            .catch_unwind()
            .await;
        std::panic::set_hook(hook);
        outcome.is_ok()
    }

    #[tokio::test]
    async fn the_built_in_engine_holds_the_contract() {
        run_policy_contract(&AllowAll).await;
    }

    /// An engine that narrows is what the seam is for, and the contract
    /// must not stand in its way.
    struct Narrowing;

    impl PolicyEngine for Narrowing {
        fn name(&self) -> &str {
            "narrowing"
        }
        fn decide<'a>(&'a self, _s: &'a Subject, _o: &'a Operation<'a>) -> DecisionFuture<'a> {
            Box::pin(std::future::ready(Decision::Filter(Filter {
                omit: vec!["brand".into()],
                restricted: true,
                ..Filter::default()
            })))
        }
        fn pre_notify(&self, _s: &Subject, _sub: &Value, n: &mut Value) -> NotifyDecision {
            if let Some(data) = n.get_mut("data").and_then(Value::as_array_mut) {
                for entity in data {
                    if let Some(o) = entity.as_object_mut() {
                        o.remove("brand");
                    }
                }
            }
            NotifyDecision::Filter(Filter {
                omit: vec!["brand".into()],
                ..Filter::default()
            })
        }
    }

    #[tokio::test]
    async fn an_engine_that_only_narrows_holds_the_contract() {
        assert!(contract_holds(&Narrowing).await);
    }

    /// The contract has teeth or it proves nothing: this engine puts a
    /// member into a notification nobody asked for.
    struct Widening;

    impl PolicyEngine for Widening {
        fn name(&self) -> &str {
            "widening"
        }
        fn decide<'a>(&'a self, _s: &'a Subject, _o: &'a Operation<'a>) -> DecisionFuture<'a> {
            Box::pin(std::future::ready(Decision::Allow))
        }
        fn pre_notify(&self, _s: &Subject, _sub: &Value, n: &mut Value) -> NotifyDecision {
            if let Some(o) = n.as_object_mut() {
                o.insert("stowaway".into(), json!(true));
            }
            NotifyDecision::Deliver
        }
    }

    #[tokio::test]
    async fn the_contract_refuses_an_engine_that_widens_a_notification() {
        assert!(
            !contract_holds(&Widening).await,
            "the contract passed an engine that added a member to a notification"
        );
    }

    #[test]
    fn a_pick_keeps_the_frame_and_drops_the_rest() {
        let mut doc = json!({"id": "urn:ngsi-ld:Vehicle:1", "type": "Vehicle",
                             "speed": 1, "brand": "Skoda"});
        Filter {
            pick: vec!["speed".into()],
            ..Filter::default()
        }
        .project(&mut doc);
        assert_eq!(
            doc,
            json!({"id": "urn:ngsi-ld:Vehicle:1", "type": "Vehicle", "speed": 1})
        );
    }

    #[test]
    fn an_omit_cannot_remove_what_makes_it_an_entity() {
        let mut doc = json!({"id": "urn:ngsi-ld:Vehicle:1", "type": "Vehicle", "speed": 1});
        Filter {
            omit: vec!["id".into(), "type".into(), "speed".into()],
            ..Filter::default()
        }
        .project(&mut doc);
        assert_eq!(
            doc,
            json!({"id": "urn:ngsi-ld:Vehicle:1", "type": "Vehicle"})
        );
    }

    #[test]
    fn a_pick_of_something_absent_adds_nothing() {
        let mut doc = json!({"id": "urn:ngsi-ld:Vehicle:1", "type": "Vehicle", "speed": 1});
        Filter {
            pick: vec!["mileage".into()],
            ..Filter::default()
        }
        .project(&mut doc);
        assert_eq!(
            doc,
            json!({"id": "urn:ngsi-ld:Vehicle:1", "type": "Vehicle"})
        );
    }

    #[test]
    fn a_projection_leaves_a_non_object_alone() {
        let mut doc = json!(["not", "an", "object"]);
        Filter {
            pick: vec!["speed".into()],
            omit: vec!["brand".into()],
            ..Filter::default()
        }
        .project(&mut doc);
        assert_eq!(doc, json!(["not", "an", "object"]));
    }

    #[test]
    fn narrowing_a_whole_tenant_operation_is_a_refusal() {
        let narrowed = Decision::Filter(Filter {
            omit: vec!["speed".into()],
            ..Filter::default()
        });
        for clause in WHOLE_TENANT {
            assert!(matches!(
                resolve(clause, narrowed.clone()),
                Decision::Deny(_)
            ));
        }
        // and every other clause outside FILTERABLE, because the handler
        // there never reads the Filter
        for clause in ["5.6.1", "5.6.6", "5.8.1", "5.9.2", "5.13.2", "5.7.5"] {
            assert!(
                matches!(resolve(clause, narrowed.clone()), Decision::Deny(_)),
                "{clause} dropped a narrowing instead of refusing it"
            );
        }
    }

    /// The reads that do read it keep it: the seam refuses what would be
    /// dropped, and nothing more.
    #[test]
    fn a_filterable_read_keeps_its_narrowing() {
        let narrowed = Decision::Filter(Filter {
            omit: vec!["speed".into()],
            ..Filter::default()
        });
        for clause in FILTERABLE {
            assert_eq!(resolve(clause, narrowed.clone()), narrowed, "{clause}");
        }
    }

    #[test]
    fn an_empty_filter_on_a_whole_tenant_operation_is_not_a_refusal() {
        assert_eq!(
            resolve("5.6.21", Decision::Filter(Filter::default())),
            Decision::Allow
        );
    }

    struct Panicking;

    impl PolicyEngine for Panicking {
        fn name(&self) -> &str {
            "panicking"
        }
        fn decide<'a>(&'a self, _s: &'a Subject, _o: &'a Operation<'a>) -> DecisionFuture<'a> {
            Box::pin(async { panic!("the engine is broken") })
        }
        fn pre_notify(&self, _s: &Subject, _sub: &Value, _n: &mut Value) -> NotifyDecision {
            panic!("the engine is broken")
        }
    }

    /// The other half of the same failure: a synchronous engine decides in
    /// the call that BUILDS the future, so a guard around the future alone
    /// never sees it. The reference engine has exactly this shape.
    struct PanickingEagerly;

    impl PolicyEngine for PanickingEagerly {
        fn name(&self) -> &str {
            "panicking-eagerly"
        }
        fn decide<'a>(&'a self, _s: &'a Subject, _o: &'a Operation<'a>) -> DecisionFuture<'a> {
            panic!("the engine is broken")
        }
        fn pre_notify(&self, _s: &Subject, _sub: &Value, _n: &mut Value) -> NotifyDecision {
            NotifyDecision::Deliver
        }
    }

    #[tokio::test]
    async fn an_engine_that_panics_before_it_returns_a_future_denies_too() {
        let hook = std::panic::take_hook();
        std::panic::set_hook(Box::new(|_| {}));
        let decided = decide(&PanickingEagerly, &subject(), &Operation::new("5.6.1")).await;
        std::panic::set_hook(hook);
        assert_eq!(decided, Decision::Deny(ENGINE_FAILED.to_owned()));
    }

    #[tokio::test]
    async fn an_engine_that_panics_denies_rather_than_allows() {
        let hook = std::panic::take_hook();
        std::panic::set_hook(Box::new(|_| {}));
        let decided = decide(&Panicking, &subject(), &Operation::new("5.6.1")).await;
        let mut notification = json!({"type": "Notification"});
        let notified = pre_notify(&Panicking, &subject(), &json!({}), &mut notification);
        std::panic::set_hook(hook);
        assert_eq!(decided, Decision::Deny(ENGINE_FAILED.to_owned()));
        assert_eq!(notified, NotifyDecision::Drop);
    }

    /// Real time, not a paused clock: the workspace tokio is built without
    /// `test-util`, so the test pays the timeout it asserts.
    #[tokio::test]
    async fn an_engine_that_never_answers_denies() {
        struct Never;
        impl PolicyEngine for Never {
            fn name(&self) -> &str {
                "never"
            }
            fn decide<'a>(&'a self, _s: &'a Subject, _o: &'a Operation<'a>) -> DecisionFuture<'a> {
                Box::pin(std::future::pending())
            }
            fn pre_notify(&self, _s: &Subject, _sub: &Value, _n: &mut Value) -> NotifyDecision {
                NotifyDecision::Deliver
            }
        }
        assert_eq!(
            decide(&Never, &subject(), &Operation::new("5.6.1")).await,
            Decision::Deny(ENGINE_TIMED_OUT.to_owned())
        );
    }

    #[test]
    fn the_subject_never_prints_its_header_values() {
        let printed = format!("{:?}", subject());
        assert!(printed.contains("X-Subject"), "{printed}");
        assert!(
            !printed.contains("a-token-shaped-string"),
            "a header value reached a log line: {printed}"
        );
    }

    #[test]
    fn an_operation_never_prints_the_payload() {
        let body = json!({"id": "urn:ngsi-ld:Vehicle:1", "plate": "BB-123-XY"});
        let printed = format!(
            "{:?}",
            Operation {
                body: Some(&body),
                ..Operation::new("5.6.1")
            }
        );
        assert!(!printed.contains("BB-123-XY"), "{printed}");
    }
}
