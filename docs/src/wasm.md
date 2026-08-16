# Browser & WebAssembly

The same broker crates compile to `wasm32-unknown-unknown` and run inside
a web page — an NGSI-LD broker with zero installation. Current artifact:
**3.99 MB raw, 1.52 MB gzipped** (budgets: 8 MB / 3 MB, the build fails
over budget). Try it: <https://antares-ngsi-ld-demo.marek-mraz.com/>.

## Two ways to use it in a page

- **Service Worker**: the worker intercepts `fetch` and answers
  `/ngsi-ld/v1/*` for the whole origin — existing NGSI-LD client code
  works unchanged against the page's own URL.
- **In-page API**: `await broker.fetch(request)` with the browser's own
  `Request`/`Response` objects — a caller cannot tell it from a network
  broker.

Stores in the browser: `memory`, or persistent via
`AntaresBroker.persistentWithHandle(...)` over an OPFS sync-access handle
(the browser's origin-private file system) — the same redb format as the
native `file` store.

## What a page cannot do (structural, not missing features)

- No inbound sockets and CORS: other systems cannot call *into* a page,
  so inbound federation and external HTTP notification callbacks are out
  of reach. Outbound notifications and forwards still leave via `fetch`.
- No MQTT, NATS, Postgres, or role-split — those need an OS process.
- `Content-Length` is a forbidden browser header: the wasm seam stamps it
  from the buffered body, since a page can never send it (CIM 009 6.3.4
  is enforced on the wire truth by whatever fronts the broker — in the
  Node tier, the shim).

## The Node tier

`www/node-shim.mjs` serves the SAME `.wasm` bytes behind a real TCP port
(Node ≥ 18) — this is how the browser artifact is conformance-tested: the
`wasm-file` CI cell runs the serial ETSI suites + IOP against five
dockerized shims over the redb file store. Per-shim env: `ANTARES_STORE`
(`memory`/`file`), `ANTARES_FILE` (redb path), `ANTARES_HOST_ALIAS`, and
`globalThis.ANTARES_PUBLIC_URL` for distributed subscriptions (wasm has no
process env — the shim wires these through JS globals).

## Build it

```bash
./dev/install-wasm-tools.sh   # wasm-bindgen (lockfile-matched) + wasm-opt
./dev/wasm-build.sh           # → www/pkg; fails if over the size budgets
node www/node-shim.mjs 9090   # the artifact behind a TCP port
./dev/wasm-test.sh            # Node smoke + headless-Chromium page test
```

`www/index.html` is the playground: create entities, subscribe, watch
notifications arrive in-page — including a loopback federation demo where
one browser tab hosts multiple context spaces federating through CSRs.
