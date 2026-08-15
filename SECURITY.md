# Security policy

## Reporting a vulnerability

Email **contact@marek-mraz.com**. Please include a reproduction (request +
config) and the store/bus mode. You will get an acknowledgement within a few
days; coordinated disclosure preferred.

## Security posture (what is in scope)

Antares draws a deliberate line (docs/deep-analysis.md §16): the broker
enforces **data-plane integrity** — strict input validation at every API
boundary, tenant isolation (one shared schema + `tenant_id` on every row,
Postgres Row-Level Security, `ANTARES_REQUIRE_RLS` production gate), an
egress wall for every outbound call the broker makes on user-supplied URLs
(scheme allowlist, private-range deny by default, per-destination breakers,
size/time bounds), bounded resource use (`/q/health` `limits` — body size,
batch items, fan-out, response bytes), and `unsafe_code = "forbid"` with
`cargo deny` license/advisory gates.

**Out of core by standing decision:** authentication, authorization, rate
limiting, DID/VC/ODRL — the PEP/gateway in front of the broker owns them.
Deployments MUST NOT expose an Antares port to untrusted callers directly.
The NATS change stream carries every tenant's data: require auth on
JetStream and network-isolate it (the broker logs a loud warning when it
connects to an unauthenticated NATS).

The full audit trail: [docs/security-audit-2026-08-04.md](docs/security-audit-2026-08-04.md)
and the production-readiness audit + 2026-08-15 re-audit in
[docs/production-readiness-audit-2026-08-09.md](docs/production-readiness-audit-2026-08-09.md).
