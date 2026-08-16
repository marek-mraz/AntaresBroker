# ADR-0010 — Private-range egress allowed by default

Date: 2026-08-08. Status: accepted, implemented.

## Context

The `EgressPolicy` (notifications, @context fetches, federation forwards)
shipped with deny-by-default for loopback/link-local/
RFC 1918/metadata destinations and `ANTARES_EGRESS_ALLOW_PRIVATE=true` as
the opt-out. Live testing of the `dev-47` image showed the practical
consequence: on any dev box, compose stack, or cluster-internal deployment
— i.e. every place a broker actually runs today — the first subscription's
notifications are silently swallowed until the operator discovers the
switch. The ETSI/IOP suites, the wasm playground, and every workspace
compose file already had to set it.

A refusal also left the subscription reporting `status: ok`/`lastSuccess`
(the optimistic writeback was stamped before the policy check and the
refusal path returned early), which made the swallow invisible even to a
client that checked its subscription.

## Decision

1. **Allow-by-default.** `EgressPolicy::from_env` defaults
   `allow_private: true`; `ANTARES_EGRESS_ALLOW_PRIVATE=false` (or `0`)
   turns the deny on. Internet-exposed deployments — the SSRF-relevant
   case, which always sit behind a deliberate deployment config anyway —
   set one env var to get the full lockdown. The scheme allowlist,
   redirect cap, DNS pinning, response-size caps and breakers are
   unconditional and unchanged.
2. **Refusal is a delivery failure for bookkeeping.** When the deny is on,
   a refused notification takes the same path as a failed send: subscription
   `status: failed`, `lastFailure` stamped, the optimistic `lastSuccess`
   rolled back, `antares_notifications_failed_total` incremented. Breaker
   state is untouched — the verdict says nothing about endpoint health.

## Consequences

- Notifications work everywhere out of the box; no compose file or test
  stack needs the flag anymore (setting `=true` stays harmless).
- A hardened deployment must now opt IN to the private-range deny; the
  deployment docs and any future hardening checklist must name
  `ANTARES_EGRESS_ALLOW_PRIVATE=false`.
- `security_regression.rs` still sets `=true` explicitly, which is now a
  no-op but keeps the test independent of this default.
