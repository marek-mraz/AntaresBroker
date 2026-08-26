# ADR-0008 — The browser build: one crate, the same router, no fourth backend

Date: 2026-08-06 · Status: accepted

## Decision

The whole broker compiles to `wasm32-unknown-unknown` as ONE additional crate
(`antares-wasm`) exposing `handle(request) → response`; a module Service
Worker feeds it browser requests on the virtual origin path `/ngsi-ld/v1/*`.
Core crates gained **target sections and cfg fences, never behavioral
forks** — the code above the executor is byte-for-byte the code the native
binary runs.

What the browser build IS: the memory store through the store seam, `bus=local`
semantics, HTTP notification delivery via the page's own `fetch`, plus a
page sink (`onNotification`) for endpoints a page cannot host. What it is
NOT: no NATS, no MQTT, no Postgres, no roles — those are deployment shapes,
not spec surface.

## The portability ledger (what wasm32 actually required)

| Native reality | wasm32 reality | Resolution |
|---|---|---|
| tokio runtime + `tokio::spawn` | no runtime; JS microtask queue | `crate::spawn` = `tokio::spawn` / `spawn_local` |
| `std::time::Instant` | panics | `web-time` (std re-export natively) |
| moka's clock (std Instant, unconditional) | panics at cache creation | `minicache` FIFO behind the same six methods |
| process env (`ANTARES_*`) | `std::env::var` always errs | constructor options (`allowPrivateEgress`) |
| reqwest client + futures are Send | `!Send`, but axum demands Send everywhere | `send_wrapper` fences (`HttpClient`, `http_interaction`) — sound single-threaded, runtime-checked |
| DNS-pinned egress resolver, redirect cap | no DNS, no redirect control in `fetch` | native-only; the browser's CORS sandbox IS the egress boundary in a page |
| sqlx / sockets | none | `postgres` cargo feature on `antares-sql`/`antares-api`; filter types moved to the ungated `store::filter` |
| TCP listener | no inbound sockets | Service Worker `fetch` interception, Node `http` shim |

## Conformance scope

- **Node tier** (`www/node-shim.mjs`): the same `.wasm` behind
  `http.createServer` — no CORS, unrestricted outbound fetch. Every serial
  ETSI suite is in scope; this is the wasm artifact's conformance gate.
- **Browser tier** (`www/test/browser-test.mjs`): Provision / Consumption /
  CommonBehaviours / jsonldContext class of behavior, plus the demo loop
  (entity → subscription → in-page notification, cross-tab shared broker).
- **Structurally out of reach in a browser page**: Subscription's HTTP
  callbacks to external receivers, ContextSource / DistributedOperations /
  IOP federation — all need either inbound sockets or CORS-consenting peers.
  They are covered by the Node tier; the page sink covers in-page delivery.

## Alternatives rejected

- A separate "browser broker" implementation — the whole point is that the
  conformance surface is the SAME code; a second implementation drifts.
- WASI/wasm32-wasi — no browser story; the Service Worker is the deployment
  target, not a server runtime.
- Threads/atomics build — nothing needs it; single-threaded keeps the
  send_wrapper argument sound and the artifact small (2.4 MB raw).

## Confirmation

`.github/workflows/wasm.yml` builds `antares-wasm` for `wasm32-unknown-unknown` and runs the browser tests; the wasm-file cell of the full ETSI matrix.
