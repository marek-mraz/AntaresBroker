# Playground UI — deep analysis: what to show, and how

Analysis 2026-08-07 (AntaresBroker). Subject: `www/` — the in-browser wasm
broker playground ("context spaces"). Question asked: what can the site show
by default (icons, pages, use cases), and specifically: **whenever an entity
is displayed, always show where it came from** — e.g. an entity served
through a CSR must show the flow along the federation edge.

Everything below is grounded in what the code serves today (router inventory
in §2) — each proposal names its data source and its cost.

---

## 1. The one principle: provenance is never optional

The playground's whole point is making NGSI-LD's invisible machinery
visible. The single highest-value rule, applied everywhere:

> **Every entity rendered anywhere carries its origin, and the origin is a
> link — hover/click replays the path the data took, on the graph.**

### 1.1 What the broker can actually tell us (three provenance tiers)

| Tier | Mechanism | Precision | Status |
|---|---|---|---|
| **T0 — diff heuristic** (today) | query `local=true` vs full; `originOf()` searches cached peer entity lists | 1 hop, same-broker only; guesses; wrong for multi-source merges | shipped (`app.js`) |
| **T1 — EntityMaps (spec-native)** | clause 5.14 / 6.34: the broker records, per query, entityId → the registration(s) that served it; `NGSILD-EntityMap` header + `GET /entityMaps/{id}` | exact, per entity, per registration — even N sources per entity | **not implemented yet** — it's already a v1.0 ledger item (§5.4.8), so building it serves compliance AND the UI at once |
| **T2 — attribute-level** | `merge_candidates()`/`merge_docs()` already know which attrs came from which `Part` (aux-ordered); an entityMap row per (entity, registration) + reg's `propertyNames` narrows attribution to attribute granularity | which *attribute* came from which source | derivable client-side from T1 + the reg's `information[]`; no broker change beyond T1 |

**Decision recommendation:** implement `/entityMaps` next on the broker side
(it is the ETSI-tested provenance instrument — DistributedOperations suite
covers it, incl. the B1 csourceid-mixup regression), and design the UI
against T1 with T0 as the always-available fallback. No vendor tracing hack:
the spec already has the exact feature the UI needs.

### 1.2 How to SHOW it — concrete mechanics

**a) Origin chip on every entity row (always on).**
Today remote rows show `← 🏔 mountain-town`. Generalize into a chip grammar
used identically in the entity list, the editor header, toasts and the log:

```
🏠 local                      — lives in this space
🌐 ← 🏔 mountain-town         — 1 hop via CSR (chip border = origin's color)
🌐 ← 🏔 ⛓2                    — multi-hop (Via chain length badge)
🧩 3 sources                  — composite entity (attrs merged from N regs)
```

Chip click → selects the origin bubble; chip hover → §1.2b path replay.

**b) Edge flow replay (the "show the flow via edge" ask).**
The CSR edges are already curved SVG paths with a dash animation. Add a
**burst** mode: when a federated query returns (or an origin chip is
hovered), each contributing edge gets 3–5 particles (`<circle>` +
`animateMotion` along the same path `d`) running peer → registrant for ~1s,
plus a transient count label `#️⃣ 3 entities` at the midpoint. Data source:
T1 entityMap grouped by registration; T0 fallback: count per origin guess.
Multi-hop chains animate **staggered** (hop 1 completes, then hop 2) so the
topology reads as a route, not a blur.

**c) Provenance card in the entity detail (see §3.2).**
A small table: origin space, registration id (clickable → registry page),
mode (`inclusive ⊕ / exclusive ⊘ / redirect ↪ / auxiliary ➕`), hops (from
`Via`), and — composite case — per-attribute source rows (T2).

**d) Failure provenance is provenance too.**
`combine()` already classifies partial results (207-style). When a peer
times out or a loop is cut (508), flash the edge red + a ⚠ chip on the
querying bubble ("mountain-town unreachable — results partial"). Teaching
distributed ops honestly means showing the failure modes, not only the happy
path.

---

## 2. What the wasm broker already serves (UI-usable surface)

Router inventory (verified against `antares-api/src/lib.rs`; ALL of it ships
in the wasm build — same router, no listener):

| Endpoint group | UI element it can power |
|---|---|
| `/entities` CRUD + `/attrs` + `/attrs/{attr}/value` | everything today + per-attr editing |
| `/entityOperations/*` (create/upsert/update/delete/merge/query) | scenario presets, bulk seeding, space wipe |
| `/subscriptions` (+ status members: `status`, `timesSent`, `lastNotification`, `lastSuccess`, `lastFailure`) | a real subscriptions panel, not just a bell button |
| `/csourceRegistrations`, `/csourceSubscriptions` | registry page; csource-notification toasts when links change |
| `/temporal/entities` full set (incl. per-instance PATCH/DELETE) + `/temporal/entityOperations/query` | **sparklines and a temporal explorer — the store records history already; the UI just never asks for it** |
| `/types`, `/types/{t}`, `/attributes`, `/attributes/{a}` | auto-built legend/palette; discovery page; "what lives here" per space |
| `/jsonldContexts` | context inspector (kinds `Hosted/Cached/ImplicitlyCreated`) |
| `/info/sourceIdentity` | broker identity card (alias, version) |
| `/q/health` | shipped (header pill) |
| federation: `Via` chains, 508, inclusive/exclusive/redirect/auxiliary, attr-scoped regs | mode glyphs on edges, hop badges, loop demo |
| **missing:** `/entityMaps` | exact provenance (§1.1 T1) — the one broker-side enabler worth building |

---

## 3. Information architecture — from one canvas to views

Keep the canvas as THE home (it is the product's identity). Add views as
**slide-in panels / route-hash pages** (`#/registry`, `#/temporal/…`), not a
nav rebuild — the graph stays visible or one click away.

### 3.1 Canvas (home) — additions, all always-on
- provenance chips + edge bursts (§1.2)
- per-bubble **capability icons** under the name: 🔔×N active subs,
  📡×N CSRs, ⏱ if temporal history exists, feeding pipes ⚙×N
- edge **mode glyph** in the label: `CSR ⊕ Room` (inclusive) vs `↪` redirect
  vs `➕` auxiliary — today all CSRs render identically; the modes ARE the
  semantics (4.3.6)
- a **notification pulse**: on toast, animate a bell ring on the bubble that
  owns the subscription + a brief halo on the entity's origin bubble

### 3.2 Entity detail (drawer over the canvas, click any row)
- header: type emoji, id, provenance chip (§1.2a)
- **provenance card** (§1.2c)
- attributes table with **kind icons**: Property 📊 · Relationship 🔗 (target
  id is a link — click follows to that entity, drawing a transient edge!) ·
  GeoProperty 📍 · LanguageProperty 🌐 · JsonProperty ⟨⟩ · Vocab 📖 · List ⋯
- per-attribute **sparkline** (last N from `/temporal/entities/{id}?attrs=…`,
  `lastN` bounded) — live-updating while pipes tick
- raw JSON-LD with a **compacted ⇄ expanded toggle** (teaches @context; the
  broker does both natively)
- sysAttrs toggle (createdAt/modifiedAt chips)

### 3.3 Registry page (`#/registry`)
Table of all CSRs across spaces: from → to, mode glyph, entity types,
attr narrowing (propertyNames/relationshipNames), expiry countdown,
per-reg "🧪 test match" button (runs a query and bursts the edge). This is
where §1.2c's registration links land.

### 3.4 Subscriptions & notifications (`#/subs`)
Left: subscription cards (space, watched types/attrs, endpoint scheme icon,
status ✅/💤, `timesSent`, last delivery ago). Right: the notification
**feed** (persistent, filterable) replacing ephemeral toasts as the record —
toasts stay as the transient signal. Each feed row carries the entity's
provenance chip: *a notification caused by a federated write shows the whole
chain: pipe ⚙ → space A → CSR edge → space B → 🔔.*

### 3.5 Temporal explorer (`#/temporal`)
Pick space → entity → attribute(s) → chart (uPlot-class tiny lib or
hand-rolled SVG — no heavy dep). Range = the 206/`Content-Range` window the
API already enforces. Bonus teach: a "deleted attr" appears with `deletedAt`
when `timeproperty=deletedAt` — the suite's own trap, visualized.

### 3.6 Discovery (`#/types`) and About
`/types` + `/attributes` rendered as the legend the canvas already implies;
About card = `/info/sourceIdentity` + `/q/health` + store mode + a link to
the ETSI story ("this exact binary passes N/686 TPs").

---

## 4. Icon system — one glyph per NGSI-LD concept, used everywhere

Rule: a concept's glyph is identical in rows, edges, logs, toasts, dialogs.
(Today's entity-type emoji already do this; extend the discipline.)

| Concept | Glyph | Where |
|---|---|---|
| local entity | 🏠 | rows, detail header |
| federated entity | 🌐 | rows, toasts, feed |
| multi-hop | ⛓+n | chip suffix |
| composite (multi-source) | 🧩 | chip, detail |
| CSR inclusive / exclusive / redirect / auxiliary | ⊕ / ⊘ / ↪ / ➕ | edge labels, registry, provenance card |
| subscription / notification | 🔔 / 🔔→toast | bubble badges, subs page |
| csourceSubscription | 📡🔔 | registry page |
| temporal history exists | ⏱ | bubble badge, row affordance |
| Property / Relationship / GeoProperty / LanguageProperty / JsonProperty / VocabProperty / ListProperty | 📊 / 🔗 / 📍 / 🌐 / ⟨⟩ / 📖 / ⋯ | detail attr table |
| pipeline / simulated device | ⚙ / 🌡🚗💡 | shipped |
| partial failure / loop cut | ⚠ / 🔄🚫 | edges, feed |
| tenant/space | existing per-space emoji+color | everywhere (provenance chips reuse the origin's color — already the pattern for remote rows) |

---

## 5. Use-case presets (one click, self-narrating)

A `▶ scenarios` menu; each preset builds spaces/CSRs/pipes via the public
API (all endpoints exist) and then walks 3–5 narrated steps (spotlight +
caption; "next" advances). Each step ends on a visible provenance proof.

1. **Federated smart city** — `city` ⊕-federates `district-a`, `district-b`;
   pipes feed the districts; the walkthrough runs a query in `city` and the
   two edges burst with counts. *Teaches: inclusive CSR, tenant = peer.*
2. **Edge → cloud** — `edge` holds devices, `cloud` federates with ↪
   redirect for one type; shows the same entity id resolving in `cloud` with
   a 🌐⛓ chip. *Teaches: redirect mode, id-based forwarding.*
3. **Data marketplace (auxiliary)** — two providers, one consumer; the ➕
   auxiliary source loses merge conflicts, visible in the 🧩 per-attr card.
   *Teaches: aux merge ordering (`merge_candidates`).*
4. **Alarm chain** — subscription in `hq` on a type only produced in a
   federated peer via a pipe; the feed row shows the full ⚙→space→CSR→🔔
   chain. *Teaches: subs fire on local state; federation is query-side.*
5. **Loop & failure drill** — create A→B→A CSRs, run a query, watch the 508
   loop cut render 🔄🚫 on the cycle; delete a peer mid-scenario for the ⚠
   partial. *Teaches: Via chains, hop limits, graceful partials.*

Presets double as living documentation and as browser-test fixtures
(`browser-test.mjs` already drives the same button contract).

---

## 6. Cost map (client work unless marked)

| Feature | Size | Depends on |
|---|---|---|
| origin chips generalized (T0 data) | S | nothing — refactor of `originOf` usage |
| edge burst animation + counts | S/M | nothing (SVG already has the paths) |
| mode glyphs on edges + registry page | S | nothing (CSR docs carry `mode`) |
| notification feed + bubble bell pulse | S | nothing |
| entity detail drawer + attr-kind icons + compact⇄expand toggle | M | nothing |
| per-attr sparklines + temporal explorer | M | nothing (temporal API live) |
| discovery page + auto legend | S | nothing |
| scenario presets + narration | M | nothing |
| **exact provenance (chips/bursts driven by real reg ids, composite 🧩, B1-proof)** | M client | **broker: implement `/entityMaps` (5.14/6.34 — ledger item, ETSI-tested)** |
| per-attribute source attribution (T2) | S on top | T1 |
| failure/loop visualization | S | nothing (`combine`/508 exist; surface status in `brokerFetch` log hook) |

Sequencing that pays fastest: chips+bursts on T0 (immediate visible win) →
detail drawer + sparklines → registry/subs pages → scenarios →
`/entityMaps` on the broker → flip chips/bursts to T1 exactness.

---

## 7. Non-goals / guardrails

- **Motion is evidence, never decoration** (user rule, 2026-08-07): an edge
  animates only while data actually crossed it — a federated query that
  returned entities, a pipe tick that wrote. Idle edges are static; the dash
  pattern alone encodes the edge kind. No looping "demo" animations anywhere.
- No framework adoption; the site is deliberately zero-dependency ES
  modules + hand SVG. Everything above fits that budget.
- No vendor provenance headers or debug endpoints — T1 is the spec's own
  mechanism; the playground must showcase NGSI-LD, not a fork of it.
- The canvas stays the home; new views are panels/hash-routes, never a
  navigation maze. One glance = one city of bubbles, always.
