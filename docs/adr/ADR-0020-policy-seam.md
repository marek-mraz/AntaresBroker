# ADR-0020 — The policy seam: one trait, one built-in engine, every engine an addon

Date: 2026-09-02. Status: accepted, amended (the narrowing marker header).
Reverses the "no policy code in the
broker" reading of the standing PEP decision (SECURITY.md,
`docs/roadmap-1.0.md`, `docs/policies.md`), which otherwise stands.

## Context

The standing decision puts authentication, authorization, rate limiting
and quotas in the gateway in front of the broker, and `docs/policies.md`
is the design that follows it: APISIX as the PEP, OPA as the PDP,
Keycloak and a VC verifier for identity. That division holds for
everything a gateway can decide from the request line and the response
bytes.

It stops holding where the decision is about data the gateway never sees
as data:

- **A query the broker runs.** "This consumer sees only entities in scope
  `/BB/Traffic`" has to narrow `GET /entities` before the store answers.
  A gateway can only rewrite `q=` on the way in — string surgery on a
  grammar with `;`, `|` and parentheses, where a wrong parenthesisation
  widens the result instead of narrowing it — or filter the response
  after the fact, which has already paid for the rows, and which leaves
  `NGSILD-Results-Count` and the paging links describing a result set the
  caller is not allowed to see.
- **One subscription's notification.** 5.8.6 delivery is broker-initiated:
  the notification goes from the broker to the subscriber's endpoint. A
  gateway in front of the broker is not on that path at all.
- **A federated merge.** 4.3.6 merges the local answer with what the
  Context Sources returned. Only the broker holds the merged result
  before it is rendered.

The deployment that forces the question is the three-organization
federation: a registration forwarded to a peer must carry the narrowing,
or the peer answers more than the consumer may see, and 4.3.6.1's own
narrowing rules then travel with a predicate the gateway never wrote.

## Decision

The broker gains a policy **seam**, and no policy engine.

**Module `antares-api::policy`** — the trait, the types and one built-in
`AllowAll` engine. Core code: always compiled, always tested, always
shipped, no feature flag.

```
PolicyEngine: Send + Sync {
    fn name(&self) -> &str;
    fn decide(&self, subject: &Subject, op: &Operation) -> DecisionFuture;
    fn pre_notify(&self, subject: &Subject, sub: &Value,
                  notification: &mut Value) -> NotifyDecision;
}
```

`Subject { tenant, headers }` carries the identity headers named by
`ANTARES_POLICY_SUBJECT_HEADERS`, opaque to the broker — it parses no
credential and validates no token. `Operation` is the expanded request:
clause, ids, types, attrs, `q`, `scopeQ`, geo, body. `Decision` is
`Allow`, `Deny(String)` or `Filter(Filter)`; `Filter` carries a `q`, a
`scopeQ`, `omit`, `pick` and `restricted`.

**Two phases**, both already in ADR-0014's table:

| phase | when | granularity |
|---|---|---|
| `on_request` | after negotiation and expansion, before the operation and before any fan-out | once per request |
| `pre_notify` | notification built, before the egress check and the send | once per notification document per subscription |

**The answers:**

| decision | the broker does |
|---|---|
| `Allow` | nothing |
| `Deny` | 403 with ProblemDetails whose `type` is an Antares URI — Table 6.3.2-1 has no access-denied type, so this is an Antares decision with its own `AntaresSpecificTests` and a 6.3.2 ledger note |
| `Filter` | narrows: the `q` is conjoined into the query the store runs and travels on forwards, `omit`/`pick` go through the 5.2.14.1 projection the notification path already has, and a merged federated result is filtered after the merge |
| `Filter { restricted: true }` | the same, plus `Antares-Results-Restricted: true` (see the amendment) |
| `Filter` on purge (5.6.21), snapshot fill (5.16) or tenant purge | answered as `Deny` — there is no narrowed form of "delete everything" |

The conjunction is made on the `antares-ql` AST, never on the query
string, so the precedence trap a gateway rewrite has to distribute around
cannot occur. Narrowing is silent: a caller cannot tell a hidden entity
from an absent one, and a retrieve of a hidden entity is 404.

A `pick`/`omit` name is expanded against the request's own `@context`, so
an engine may write it either as a short name or as the IRI this decision
asks for; it is one Attribute name (6.5.3.1), not a 4.21 projection
expression, because that grammar reads a dot as the sub-attribute path
separator and would truncate an IRI at the first dot of its authority.

**What a notification answer may be.** `pre_notify` narrows by projection
only. `Drop` is not a failed delivery: 5.11.7 moves `notification.timesSent`
and `lastNotification` for a notification that "shall be sent", and one the
engine dropped never was — the same reading the broker already applies to a
cooldown (5.2.15) and an open circuit. A `Filter` carrying `q` or `scopeQ`
is refused as a `Drop`: the entities were selected by the subscription's own
conditions long before the seam sees them, there is nothing left to re-run a
query against, and delivering unfiltered would report a narrowing that never
happened.

**A broker-held record outlives its request.** An EntityMap (5.14) and a
Snapshot (5.16) are both a stored answer to one query, reused by later
requests, so each records the subject it was made for in `__subject`. 5.5.14
already says what to do with a map this subject cannot access — "no inference
can be made … and a new one shall be created" — so a map presented by another
subject is answered exactly as an expired one is, and the two cannot be told
apart; retrieving or deleting it is `ResourceNotFound`, because its
`entityMap` member is the id set a narrowing exists to withhold. The one
allowance the clause makes in the other direction stands: other components
"shall only be allowed to update the expiry timestamp", so 5.14.2 Update is
open to a subject that did not build the map. A snapshot's fill runs after
its request has been answered, which is why the subject is recorded there
too — and why 5.16.1, 5.16.2 and 5.16.7 sit in the whole-tenant list with
5.6.21: there is no narrowed form of "copy everything these queries match".

**Whose subject a notification is under.** 5.8.6 delivery is
broker-initiated, so no request is in flight when it is decided. The subject
is the subscriber's, taken from the creating request and stored with the
subscription in `__subject` — a broker-internal member beside the `__context`
that same clause already keeps there. The `__` prefix as a whole is the
broker's: a client can set no member under it, no served representation
carries one (5.8.3/5.8.4 serve the 5.2.12 data type, which defines none), and
the 5.8.1.4 copy forwarded to a Context Source is stripped of all of them.

**Fail closed.** An engine error, panic or timeout
(`ANTARES_POLICY_TIMEOUT_MS`) is `Deny`. `AllowAll` has no error path.

**The subject never travels.** The subject headers are stripped from
every forwarded request in `federation::forward`, and never enter a
notification, a log line, a dead letter or a peer registration. That
stripping has to be active rather than incidental: 4.3.6.5 lets a
registration name a header to copy from the triggering request
(`"urn:ngsi-ld:request"`), and a registration is client-supplied, so
whoever can register a Context Source could otherwise read the identity of
every request that fans out to it. The names in
`ANTARES_POLICY_SUBJECT_HEADERS` are dropped from that copy — dropped, not
refused at 5.9.2, because which headers a deployment made subject headers
is not something a client gets to probe by watching which registrations
fail. The one place the subject does travel is the broker's own
subscription mirror: 5.8.6 delivery is broker-initiated, and a pod that
mirrored a subscription without its subject would decide every notification
under nobody. That is the broker's own state, the same value the store row
holds, not a peer. `/q` is outside the seam: the admin surface is the
gateway's to protect.

**Every engine is an addon**, in the sense `plugin-example` is: a crate
outside `crates/`, behind an off-by-default `antares-broker` feature,
built and run by `examples.yml` alone. The shipped image, `ci`, `full`,
`strict` and every ETSI cell run `AllowAll`. No core crate names an
engine crate.

A policy engine's rule language, identity model and data source are
deployment choices. A broker that shipped one would make conformance a
function of its configuration — the argument `surface.rs` already makes
for the `/x/` prefixes. Conformance is therefore asserted against the
built-in engine, and a deployment running another engine makes no
conformance claim.

## Amendment: the narrowing marker is an Antares header

This decision first named the marker `NGSILD-Results-Restricted`. It is
sent as `Antares-Results-Restricted` instead.

`NGSILD-` is ETSI's prefix, and clause 6.3 defines what it carries:
`NGSILD-Tenant` (6.3.1), `NGSILD-EntityMap`, `NGSILD-Results-Count` and
`NGSILD-Warning`. A broker-invented header under that prefix claims a name
CIM 009 has not assigned, and a later version that assigns it to something
else turns every Antares deployment into a client-visible conflict. The
same reasoning already puts the access-denied error type on an Antares URI
rather than under `https://uri.etsi.org/ngsi-ld/errors/`: where the
standard is silent, Antares answers in its own namespace and says so.

Nothing else changes — the header is still advisory, still sent only when
the engine sets `restricted`, and a narrowing is otherwise silent.

## Consequences

- ADR-0014's phase table gains no row: the seam is a named user of two
  phases that already exist, with fail-closed policy per rule 2.
- It does take a stated exception to ADR-0014's rule 1. `pre_notify`
  fires once per matched subscription, not once per drained batch,
  because the decision is *about* the subscription: the subscriber is the
  subject, and one verdict for a batch of subscriptions with different
  subscribers has no meaning. The marshalling cost that rule 1 protects
  against is bounded by the delivery it precedes — the seam is crossed
  once per notification the broker was going to serialise and send over
  the network anyway.
- The gateway keeps everything it had. Authentication, token validation,
  rate limiting and quotas do not move; the broker still takes no
  authorization decision of its own, it asks the engine and obeys.
  `docs/policies.md` stays the reference for what an engine outside the
  tree looks like.
- Deployments that run `AllowAll` — the default, the image, every gate —
  pay one virtual call per request and one per notification.

## Confirmation

- `run_policy_contract` (behind `test-kit`, the contract an addon
  engine's own tests call): a `Filter` never widens — the engine's answer
  is run through the projection and asserted a subset of the unfiltered
  one; the whole-tenant operations answer `Deny`; the timeout path
  answers `Deny`.
- A counting engine, reachable from no socket, walks every route of
  `router()` and asserts each operation hits the gate exactly once. A
  handler that skips the gate is a red test, not a review finding.
- `cargo tree -p antares-broker` with default features names no engine
  crate, and the release gate proves no addon is in the shipped image.
  `policy_shelf_tests` in `antares-broker` asserts the shelf of a build
  without the addon feature is exactly `allow-all`, that no selection is
  that engine, and that an unknown name is fatal rather than wide open.
- The seam is proven from outside `crates/`, which is what makes it a seam:
  `examples/plugin-example/src/policy.rs` is an engine written the way a
  deployment's would be — static per-tenant rules from a JSON document —
  and `examples/plugin-example/tests/policy.rs` runs `run_policy_contract`
  against it and then answers real requests through `router()`: a denied
  Entity type is 403 `urn:antares:error:AccessDenied` while the next type
  in the same tenant is 201, and the engine's `q` and omit list narrow a
  query the store ran. `AntaresSpecificTests/policy_engine.robot` repeats
  it against a running broker built with that engine, adding the
  notification projection and the Context Source forward; `examples.yml`
  runs the conformance suite on `allow-all` and that folder on the addon,
  in the same job.
- `tests/policy_subject_on_the_wire_p6.rs`: a registration that asks for
  the subject header by the 4.3.6.5 copy value does not get it while an
  ordinary header it asks for still arrives; a 5.8.1.4 forwarded
  subscription copy carries no internal member and no subject; a change
  event carries none; no response echoes one back; and the subscription
  mirror keeps it, on purpose, with the reason in the test.
- `/q/health` names the active engine and its timeout.
- `tests/policy_maps_snapshots_5_14_5_16.rs`: a map built for one subject is
  not reused by another and is answered like an expired one; retrieve and
  delete of another subject's map are `ResourceNotFound` while the 5.5.14
  expiry update stays open; no served map or snapshot carries an internal
  member; a narrowing answer to 5.16.1, 5.16.2, 5.16.7 or 5.6.21 is a 403
  while an empty filter is an allow.
- `tests/policy_notify_5_8_6.rs`: a dropped notification is no attempt at
  all (no POST, no `timesSent`, no `lastNotification`, no `lastFailure`)
  while a delivered one still counts; the projection removes what the engine
  named and nothing else, named as a short name or as an IRI; a `Filter`
  carrying a `q` is dropped; the engine is asked about the subscriber and
  not about whoever's write triggered the notification; and the stored
  subject appears in no representation and can be set by no client.
- `tests/policy_filter_5_7.rs`: what the raw read answers, what the
  narrowed one answers, and what is not in it — over the entity query, the
  retrieve, the attribute retrieve, the batch query, the temporal query and
  retrieve, the EntityMap candidate set and a merged federated result; the
  marker header on each read it narrowed.
