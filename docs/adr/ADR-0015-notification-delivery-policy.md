# ADR-0015 — Notification delivery policy: one attempt by default, retries as transport, dead letters in the store

Date: 2026-08-26. Status: accepted, implemented.

## Context

A notification POST that failed was tried once and forgotten; only the
egress breaker and `lastFailure` recorded it. CIM 009 5.8.6 mandates no
retry: it says the notification is sent, `timesSent` moves by one, and the
outcome is booked as `lastSuccess` or `lastFailure` + `status`. Operators
running flaky subscribers still lose notifications on transient failures
with no replay path.

## Decision

1. `DeliveryPolicy { attempts, backoff, jitter, max_age }` in
   `antares-notifier`, default `attempts = 1` — the clause as written, and
   the only behaviour the ETSI suite sees. Read once at startup from
   `ANTARES_NOTIFY_*`; a malformed value fails the process.
2. Retries are transport under ONE notification. The first attempt is
   booked exactly as before, the moment it resolves. Further attempts run
   on a spawned task, never on the change-consumer or request path, so a
   backoff on one subscription cannot delay another. A retry that succeeds
   sets `lastSuccess` and `status: "ok"`; `timesSent` and `timesFailed`
   never move again for that notification. A subscription deleted
   meanwhile ends its retries.
3. An exhausted policy writes a dead letter as a doc kind
   (`Kind::DeadLetter`, table `dead_letters`, ADR-0012 shape, RLS-belted,
   redb table in file mode) under the subscription's tenant: the exact
   request (endpoint, headers, payload) plus attempt history. The admin
   surface lists, replays (one attempt, same binding, current egress
   policy) and deletes them, per tenant, never under `/ngsi-ld`.
4. No cargo feature. With the default policy the send path is the same
   code and the same single attempt; a feature would double the build
   matrix to prove a difference that does not exist. Egress refusals are
   never retried: a policy verdict is not a transport failure.

## Consequences

- Deployments that want at-least-once delivery towards flaky subscribers
  set `ANTARES_NOTIFY_ATTEMPTS`; everyone else keeps the spec's behaviour.
- Dead letters are persisted data (`0002_dead_letters.sql`); changing
  their encoding later is a data migration, which is why this is an ADR.
- A tenant purge (ADR-0001 amendment) deletes the tenant's dead letters
  with everything else.

## Confirmation

`cargo test -p antares-notifier policy_tests` (schedule, defaults, env
parsing), `cargo test -p antares-api -- delivery_policy dead_letter`
(one notification per retry chain, dead letter on exhaustion, isolation
between subscriptions and tenants, restart survival), and the Robot TP
586_01 in the Subscription suite, which holds under any
`ANTARES_NOTIFY_ATTEMPTS` value.
