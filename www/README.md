# Antares playground v2 — React UI for the in-browser NGSI-LD broker

A user-friendly front-end for the same wasm broker that powers `www/`:
React + React Flow board (pan the whole canvas with the mouse, wheel-zoom,
drag bubbles), a spreadsheet view per tenant with filters, temporal history
charts, and the board features ported 1:1 (demo, template JSON, reset,
request log). `www/` stays as the zero-dependency reference page; this app
is the product-shaped one.

## Ground rules (inherited, non-negotiable)

1. **Motion is evidence, never decoration** — an edge animates only while
   data actually crossed it (a federated query that returned entities, a
   pipe tick whose write the broker accepted). Idle edges are static.
2. **Every entity always shows its origin** — 🏠 local or 🌐 + the peer it
   came from. Today's origin source is the `local=true` diff heuristic; when
   the broker implements `/entityMaps` (clause 5.14), `broker/api.js` is the
   ONLY file that changes (see `docs/playground-ui-analysis.md` §1.1).
3. **The broker is the only truth** — the UI never fabricates state; every
   count/row/edge label is derived from API responses. Structure the user
   authored (spaces, pipelines) persists in `localStorage`; data never does.
4. **Tenant names**: `A-Za-z0-9-`, max 64 — a strict subset of the broker's
   `TenantId` rule, so UI and API can never disagree.

## Architecture

```
www/
├── public/            # runtime-identical assets, copied from ./ by `npm run sync`
│   ├── pkg/           #   wasm-bindgen output (antares_wasm.js + .wasm)
│   ├── worker.js      #   the OPFS persistence host (dedicated worker)
│   └── loopback.js    #   the self.antares.internal virtual host
├── src/
│   ├── broker/
│   │   ├── transport.js   # the transport ladder: OPFS worker → in-page wasm.
│   │   │                  # Exposes brokerFetch(), mode, onNotification(),
│   │   │                  # and the request-log ring (every call: tenant,
│   │   │                  # method, path, status — the 🛰 feature).
│   │   ├── virtualhost.js # loopback.js's pattern one prefix further in:
│   │   │                  # same-origin fetch to /ngsi-ld/* routes into
│   │   │                  # brokerFetch — any client (Swagger UI, devtools
│   │   │                  # console) talks to the in-tab broker like a server.
│   │   └── api.js         # typed-ish NGSI-LD client: entities/CSRs/subs/
│   │                      # temporal/types. The ONLY module that builds
│   │                      # URLs. Origin attribution lives here too.
│   ├── model.js           # pure data: TYPES (sensor catalog), GENERATORS,
│   │                      # DEMO topology, tenant-name rule, template
│   │                      # build/apply logic. No I/O — fully unit-tested.
│   ├── state/
│   │   ├── board.js       # the one store (useSyncExternalStore): spaces,
│   │   │                  # pipes, fedView, polled links/ents, bursts.
│   │   │                  # Persistence keys are shared with www/ so both
│   │   │                  # UIs read the same board on the same origin.
│   │   └── pipes.js       # pipeline timers; a tick counts ONLY when the
│   │                      # broker accepted the write (rule 1).
│   └── components/
│       ├── App.jsx        # layout: TopBar / Board / right drawer
│       ├── Board.jsx      # React Flow: space + device nodes, CSR (dashed)
│       │                  # and pipe (dotted) edges; canvas pans, wheel
│       │                  # zooms, nodes drag; edge bursts carry counts
│       ├── TenantSheet.jsx# the spreadsheet: all entities of the clicked
│       │                  # tenant; filters: free text, type, origin
│       │                  # (local / per-peer); row click → History
│       ├── History.jsx    # temporal chart for one attribute
│       │                  # (GET /temporal/entities/{id}, lastN window)
│       ├── RequestLog.jsx # the 🛰 panel (ring buffer, toggleable)
│       └── ApiConsole.jsx # 📖 the ETSI CIM 009 OpenAPI 1.8.1 spec
│                          # (public/openapi/, vendored from forge.etsi.org
│                          # cim/ngsi-ld-openapi) in Swagger UI; "Execute"
│                          # hits the in-tab broker via virtualhost.js.
│                          # Lazy chunk. NOT RapiDoc — it hard-loops on the
│                          # spec's recursive schemas; Models stay collapsed
│                          # for the same reason.
├── test/                  # vitest: model invariants, demo idempotence,
│                          # template round-trip, sheet filtering (RTL)
└── e2e/smoke.mjs          # playwright-core against the BUILT app: boot,
                           # auto-demo, node count, sheet filters, history
```

### Layering rule

`components → state → broker → (wasm)` and `model` is importable from
anywhere; nothing imports upward. React Flow appears only inside
`Board.jsx` — swapping the graph library is a one-file change. The broker
client knows nothing about React; the vanilla `www/` page could adopt it.

### Decisions & tradeoffs

- **React Flow (`@xyflow/react`)** buys pan/zoom/drag/minimap for free —
  exactly the interaction budget the hand-rolled SVG was starting to eat.
  The evidence rule survives: edges get `animated` + a count label only
  while a burst mark is fresh, driven by the same store logic as `www/`.
- **JS + JSX, not TS** (for now): matches the sibling page, zero build
  friction; Vite strips types without checking anyway, so TS would only pay
  once a real typecheck gate exists. Upgrade path: rename `broker/` +
  `model.js` to `.ts` first — they are the API surface.
- **No SW transport tier here**: the ladder is OPFS worker → in-page. The
  Service Worker tier exists for `www/`'s no-worker browsers; this app
  requires module workers anyway, and two SWs on one scope fight. If the
  OPFS worker is unavailable the in-page broker still gives full function,
  minus persistence (banner shows ⚠ ephemeral, same as `www/`).
- **Same-origin note**: served on its own port, this app owns its own
  origin ⇒ its own OPFS file and localStorage ⇒ its own broker instance.
  It does NOT share state with `www/` unless served from the same origin
  (the intended deployment: `/AntaresBroker/` = www, `/AntaresBroker/app/`
  = this build — then they share the board AND the OPFS store).
- **History**: temporal queries hit the broker's real `/temporal/entities`
  (the store records history automatically). The chart is a hand-rolled
  SVG polyline — a charting dependency is not justified for one sparkline;
  revisit if the temporal explorer grows aggregation controls.

## Feature parity checklist (vs `www/`)

| Feature | Status |
|---|---|
| transport ladder + ⚠ ephemeral banner | ported (minus SW tier) |
| context spaces CRUD, tenant validation | ported |
| CSR federate + fed view + origin chips | ported |
| pipelines (device / copy), honest ticks | ported |
| evidence-only edge bursts with counts | ported |
| board demo (9 spaces / 7 CSRs / 13 devices / 3 copies, decidim) | ported (shared DEMO topology) |
| template JSON export / apply | ported |
| remove everything | ported |
| request log + console | ported |
| notifications → toasts | ported |
| **new:** whole-board mouse panning, wheel zoom, minimap | React Flow |
| **new:** tenant spreadsheet with filters (text / type / origin) | this app |
| **new:** temporal history chart per attribute | this app |
| **new:** 📖 API console — ETSI OpenAPI docs, try any NGSI-LD request against the in-tab broker (Postman-in-browser) | this app |

## Commands

```bash
npm install          # once
npm run sync         # copy pkg/worker/loopback from ./ (after wasm rebuilds)
npm run dev          # vite dev server
npm test             # vitest unit + component tests
npm run build        # production build (dist/)
npm run e2e          # build + headless-chromium smoke against dist/
```

## Roadmap hooks

- `/entityMaps` lands in the broker → exact per-entity provenance: replace
  `api.originOf()` internals, everything downstream (chips, sheet origin
  column, edge counts) is already keyed on its output.
- Scenario presets (§5 of the UI analysis) → additional `DEMO`-shaped
  configs in `model.js`; the runner already takes the topology as data.
- TS migration: `broker/`, `model.js` first; components last.
- Deploy: `wasm.yml` gains a second artifact (this `dist/` under `/app/`)
  once the app stabilizes — same manual dispatch gate as the main page.
