# Security policy

## Reporting a vulnerability

Email **contact@marek-mraz.com**. Please include a reproduction (request +
config) and the store/bus mode. You will get an acknowledgement within a few
days; coordinated disclosure preferred.

## Supported versions

Pre-1.0: only the latest `0.x` release line receives fixes — upgrade to
the newest release before reporting. From 1.0 on, the latest MINOR of the
current MAJOR is supported; anything older gets fixes only for
critical-severity issues, best effort.

| Version | Supported |
|---|---|
| latest release | yes |
| older releases | critical fixes, best effort |
| `master` / `:dev` images | yes (fixes land here first) |

## Security posture (what is in scope)

Antares draws a deliberate line: the broker
enforces **data-plane integrity** — strict input validation at every API
boundary, tenant isolation (one shared schema + `tenant_id` on every row,
Postgres Row-Level Security, `ANTARES_REQUIRE_RLS` production gate), an
egress wall for every outbound call the broker makes on user-supplied URLs
(scheme allowlist, DNS pinning, a redirect cap, per-destination breakers,
size/time bounds, and the cloud instance-metadata endpoints refused in every
IPv6 spelling whatever the configuration says), bounded resource use
(`/q/health` `limits` — body size, batch items, fan-out, response bytes), and `unsafe_code = "forbid"` with
`cargo deny` license/advisory gates.

Private, loopback and link-local destinations are **allowed** by default, so
a single-node or edge deployment federates and notifies over the local
network without configuration. An internet-facing deployment turns that off
with `ANTARES_EGRESS_ALLOW_PRIVATE=false`: it belongs in the same
pre-production checklist as `ANTARES_REQUIRE_RLS=1`, because a subscription
or registration endpoint is client-supplied and the default lets one name an
address inside your network.

### What the egress wall does not cover

A destination is judged twice: the host as WRITTEN, before the request path
commits to it, and the ADDRESS the transport is about to dial. The second
judgement is where a name is decided. With private egress allowed — the
default — a destination written as a name is not resolved at the first
check; the resolver runs once, inside the transport, where `PolicyResolver`
(every reqwest client) and the MQTT connector's own address check drop the
instance-metadata addresses whatever the configuration says, and the private
ones under the switch. Resolving at the first check as well would buy no
different verdict and would cost a second DNS query on every notification
delivery and every cold `@context` fetch.

Two consequences follow, and both are accepted:

- A notification binding a deployment registers itself is handed a
  destination the wall has cleared as written — `NotificationSink::network()`
  defaults to `true`, so a binding is policed unless it declares that it
  opens no socket — but the address filter belongs to whatever transport it
  dials with. A binding that opens a raw socket owes that filter itself.
- An embedder that hands `Loader::with_client` its own HTTP client owes it
  too: the DNS pin and the per-hop redirect cap live in
  `antares_jsonld::client_builder`, so build on that and add the proxy,
  timeouts and trust anchors to it.

Inside the workspace that invariant is a CI gate: no HTTP client is
constructed anywhere but `client_builder`.

**Out of core by standing decision:** authentication, rate limiting,
quotas, DID/VC/ODRL — the PEP/gateway in front of the broker owns them.
The broker takes no authorization decision of its own either: it exposes a
policy *seam* (ADR-0020) — one trait, one built-in allow-all engine, fail
closed — so a deployment can attach an engine as an addon crate the way it
attaches a store. Every engine lives outside `crates/` behind an
off-by-default feature; the shipped image and every CI gate run the
built-in engine, and conformance is asserted against it. The subject
headers an engine reads never travel: they are stripped from forwarded
requests and never enter a notification, a log line or a dead letter.
Deployments MUST NOT expose an Antares port to untrusted callers directly.
The NATS change stream carries every tenant's data: require auth on
JetStream and network-isolate it (the broker logs a loud warning when it
connects to an unauthenticated NATS).
