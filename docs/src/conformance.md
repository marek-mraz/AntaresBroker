# Conformance

Antares implements ETSI GS CIM 009 V1.9.1. Conformance is tracked in two
places that check each other: the clause ledger in `docs/spec/`, and the
ETSI Robot Framework suite vendored at `ngsi-ld-test-suite/`, run in a
matrix of store modes on every push.

## The ledger

`docs/spec/` holds one file per clause of CIM 009, 947 files, each
carrying the clause text, the PDF pages it came from, and three
hand-maintained fields: `status`, `evidence` (code and test anchors) and
`notes`. The `robot:` list is generated: `python3 dev/spec.py robot`
scans the suite for `[Tags]` in the clause form (`5_6_6`) and writes the
matching TP names into the clause file.

| status | meaning |
|---|---|
| `implemented` | every SHALL of the clause holds, with evidence anchors |
| `partial` | a named gap in `notes` |
| `not-implemented` | audited, not yet built |
| `staged-v1x` | deferred to a later spec version by decision |
| `informative` | a heading, an umbrella clause or an informative annex; the requirements are audited in the leaf clauses it delegates to |

Current counts (`python3 dev/spec.py status`):

```text
947 sections
  implemented       479
  informative       468
  robot-tagged      190
```

Zero `partial`, zero `not-implemented`. `python3 dev/spec.py check`
fails on a malformed file or a stale `robot:` list; `dev/spec.py gaps`
lists leaf clauses without a TP.

## The suite

The suite directory holds 666 `.robot` files under
`TP/NGSI-LD/{CommonBehaviours, ContextInformation, ContextSource,
DistributedOperations, jsonldContext}`. 555 carry ETSI's numbering
(`002_01`, `D018_01`); 111 are clause-numbered additions written here for
normative surface the official set leaves untested (`566_01` for 5.6.6,
`5510_01` for 5.5.10, `586_01` for 5.8.6), following the same conventions
and tagged with their clause so the ledger picks them up. Every file
expands to test cases; one full run is 1784 test cases:

| suite | test cases |
|---|---|
| CommonBehaviours | 65 |
| Consumption | 529 |
| EntityMap | 22 |
| Provision | 396 |
| Snapshot | 5 |
| Subscription | 153 |
| ContextSource | 136 |
| DistributedOperations | 132 |
| IOP | 278 |
| jsonldContext | 68 |

## The matrix

The same 1784 cases run once per cell. Every push gates on the quick
preset; the full preset runs twice a week, on `v*` tags and on dispatch,
and is what the report page and the badges render.

| cell | store | preset | what it adds |
|---|---|---|---|
| file | redb file store | quick, full | durability across restart |
| postgres | PostGIS | quick, full | the production current-state path |
| timescale | PostGIS + TimescaleDB | quick, full | hypertable history, columnstore |
| memory | in-RAM | full | the zero-dependency binary |
| postgres-nats | PostGIS + NATS JetStream | full | ten containers in split roles, rolled during the run |
| timescale-nats | as above on TimescaleDB | full | same, on the temporal-heavy backend |
| wasm-file | browser artifact over the file store | full | five Node shims driving the WebAssembly build; MQTT excluded, the browser has no broker socket |

A cell passes at 1784/1784. `dev/etsi-matrix-summary.py` folds the
per-cell results into one table and lists every failure across the
matrix; a release requires that list to be empty.

## Running a suite locally

One store mode per run, the one the change touches:

```bash
dev/etsi-local.sh                                  # workspace tests + memory cell
STORE=timescale dev/etsi-local.sh                  # one cell, all suites
STORE=all dev/etsi-local.sh                        # the quick trio, serially
STORE=postgres STOP_ON_ERROR=1 dev/etsi-pipeline.sh    # halt at the first red TP
STORE=file SUITES=Consumption,Subscription SKIP_BUILD=1 dev/etsi-pipeline.sh
```

`dev/etsi-pipeline.sh` knobs: `STORE`, `STOP_ON_ERROR` (default 1, CI
sets 0), `SKIP_BUILD` (reuse the local image), `SUITES` (comma list),
`MQTT=1` (include the `058_*` MQTT cases), `KEEP_UP=1` (leave the stack
running), `MEM_LIMIT_MB` (per-broker peak-RSS gate), `ROLES_SPLIT=1
ROLL_DURING_RUN=1` (reproduce a `-nats` cell), `WASM=1 WASM_DOCKER=1
STORE=file` (reproduce the wasm cell). Results land in `results/$STORE`
with Robot's own `log.html` per suite.

For a single clause during development, one broker without Docker is
enough:

```bash
cargo build -q -p antares-broker -j 2
ANTARES_HTTP_PORT=9377 ./target/debug/antares &
cd ngsi-ld-test-suite && robot --variable url:http://localhost:9377/ngsi-ld/v1 \
  --outputdir /tmp/robot-566 TP/NGSI-LD/ContextInformation/Provision/Entities/DeleteEntity/566_01.robot
```

## Suite and spec defects

A red TP is proven against the clause text before any broker change. When
the text says the TP or the spec is wrong, the finding goes to
`docs/upstream/etsi-raises.md` as a ready-to-file issue, and the fork
carries the fix. The current list:

| # | target | finding |
|---|---|---|
| 1 | suite | `D018_01` asserts 508 Loop Detected for an inclusive registration |
| 2 | suite | nine official `_exc` TPs create exclusive registrations that 4.3.6.3 forbids |
| 3 | suite | `LdContextNotAvailable` fixtures assert 503; V1.9.1 mandates 504 |
| 4 | spec | 5.3.4 SnapshotNotification: member naming conflict and a phantom `snapshotReady` |
| 5 | spec | Table 6.6.3.2-2 (Update Attributes 207): Data Type and Remarks conflict |
| 6 | spec | 5.7.4.4 / Table 5.2.21-1: `lastN` versus values-filter ordering is unspecified |
| 7 | spec | CIM 029 A.5.2.26 cites clause 5.15.4, which does not exist in CIM 009 V1.9.1 |
| 8 | openapi | v1.8.1 temporal GET operations declare the `options` parameter twice |

Filing is manual; each entry carries the clause quotation and the
proposed fix so it can be pasted into the ETSI tracker as is.
