# Conformance

**Live evidence:** <https://antares-ngsi-ld-demo.marek-mraz.com/reports/latest/>
— per-store results with Robot Framework's own drill-down for every test,
rebuilt from the newest full matrix run. This page explains what those
numbers mean and how to reproduce them yourself, which is exactly what a
procurement or acceptance process should do.

## What 1713/1713 means

The [ETSI NGSI-LD test suite](https://forge.etsi.org/rep/cim/ngsi-ld-test-suite)
is the official conformance suite for ETSI GS CIM 009. Antares runs it —
plus this repository's own extension test procedures (see below) — across
a matrix of store/bus configurations. 1713 is the total per cell across
all eight suites (CommonBehaviours, Consumption, EntityMap, Provision,
Snapshot, Subscription, ContextSource, DistributedOperations, IOP,
jsonldContext); a full run is green in all six native cells:

| Cell | What it proves |
|---|---|
| `memory`, `file` | the store ladder without a database |
| `postgres`, `timescale` | production stores, PostGIS + Timescale temporal |
| `postgres-nats`, `timescale-nats` | a 10-container role-split fleet **rolling continuously under the whole suite** — HA is tested, not asserted |
| `wasm-file` | the browser artifact behind Node shims — serial suites + IOP (MQTT structurally excluded in a wasm build) |

## Beyond the official TP list

The official suite's ~686 test procedures cover part of the normative
surface. Antares therefore maintains a **per-clause conformance ledger**:
the full CIM 009 V1.9.1 text split into 947 clause files
([`docs/spec/`](https://github.com/marek-mraz/AntaresBroker/tree/master/docs/spec)),
each carrying a status (`implemented` / `informative` / …), evidence
anchors into code and tests, and the Robot TPs that exercise it. Where the
official suite had no coverage (411 Content-Length behaviour, error-path
edge cases, distributed-operation corners), this repository adds its own
extension TPs to the suite fork — the 1713 total includes them. Suspected
defects in the official suite are proven against the spec text and logged
upstream, never worked around in the broker.

`dev/spec.py status` prints the ledger; CI (`spec.py check`) fails when a
ledger entry references a nonexistent test.

## Reproduce it locally

```bash
dev/etsi-local.sh                        # workspace tests + one store mode (memory)
STORE=postgres dev/etsi-local.sh         # the mode you care about
STORE=all dev/etsi-local.sh              # the full store matrix, serially
WASM=1 WASM_DOCKER=1 STORE=file dev/etsi-pipeline.sh   # the browser-artifact cell
```

The same scripts CI runs — there is no CI-only magic. Suite vendored at
`ngsi-ld-test-suite/`; every cell's raw Robot `log.html`, per-second
CPU/RSS samples, and failure CSVs are downloadable from each run's
artifacts.

## Caveats stated plainly

- The wasm cell excludes MQTT (no MQTT stack in a browser build) and
  runs the serial suites — inbound-federation callbacks are covered by
  the Node tier, not in-browser.
- The arm64 image half is built natively but the ETSI gates run on amd64.
- ETSI CIM 009 evolves; the ledger pins V1.9.1 and records deliberate
  deviations (currently one: the 6.3.4 Content-Length check is scoped to
  HTTP/1.x, since HTTP/2 carries length in framing).
