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
  implemented       477
  informative       468
  partial             2
  robot-tagged       192
```

A `partial` names its gap in `notes:` and is the honest status for a clause
whose normative surface is not fully closed; `not-implemented` is empty.
`python3 dev/spec.py check` fails on a malformed file, a stale `robot:` list
or a count in this chapter that no longer matches the ledger; `dev/spec.py
gaps` lists leaf clauses without a TP.

## The suite

The suite directory holds 669 `.robot` files under
`TP/NGSI-LD/{CommonBehaviours, ContextInformation, ContextSource,
DistributedOperations, jsonldContext}`. Most carry ETSI's own numbering
(`002_01`, `D018_01`); the rest are additions written here for normative
surface the official set leaves untested, either clause-numbered (`566_01`
for 5.6.6, `5510_01` for 5.5.10, `4233_01` for 4.23.3) or slotted into the
ETSI family they extend, and all following the same conventions and tagged
with their clause so the ledger picks them up. Every file expands to test
cases; one full run of a native cell is 1817 test cases:

| suite | test cases |
|---|---|
| CommonBehaviours | 65 |
| Consumption | 535 |
| EntityMap | 22 |
| Provision | 403 |
| Snapshot | 5 |
| Subscription | 156 |
| ContextSource | 143 |
| DistributedOperations | 134 |
| IOP | 286 |
| jsonldContext | 68 |

Behaviour Antares defines for itself, where CIM 009 is silent, does not go
in `TP/` — that directory is run against other brokers in interoperability
campaigns, so every file in it has to assert a SHALL the spec text carries.
Those tests live in `ngsi-ld-test-suite/AntaresSpecificTests/` instead, and
each says in its own documentation that it is an Antares decision rather
than a CIM 009 requirement, with the reason the behaviour exists.

## The matrix

The same 1817 cases run once per native cell; `wasm-file` runs 1805 of
them, the twelve MQTT cases having no broker socket to run against in the
browser build. Every push gates on the quick preset; the full preset runs twice a week, on `v*` tags and on dispatch,
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

A native cell passes at 1817/1817, `wasm-file` at 1805/1805. `dev/etsi-matrix-summary.py` folds the
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
enough. `resources/variables.py` carries the suite's own compose addresses
(`scorpio1` for the broker, `172.28.0.18` for the notification and
context-source mocks), which the runners rewrite and a bare `robot` does
not, so the recipe overrides them itself:

```bash
cargo build -q -p antares-broker -j 2
ANTARES_HTTP_PORT=9377 ./target/debug/antares &
cd ngsi-ld-test-suite && robot --variable url:http://localhost:9377/ngsi-ld/v1 \
  --variable temporal_api_url:http://localhost:9377/ngsi-ld/v1 \
  --variable notification_server_host:127.0.0.1 \
  --variable context_source_host:127.0.0.1 \
  --variable context_server_host:127.0.0.1 \
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

## Spec-statement coverage

The ledger says implemented or partial per clause; this table says which
of those clauses no Robot test exercises at all. `python3 dev/spec.py
statements` counts the SHALL statements in each leaf clause's text against
the TPs tagged with its number (or its operation's number) and the
code/test anchors its evidence cites. It adds no tests; it names where the
next ones belong.

310 leaf clauses carry 1661 SHALL statements; 119 of them have no Robot TP (350 SHALLs), 64 cite no code/test anchor.

The fifteen untested clauses with the most SHALL statements:

| clause | title | SHALL | robot TPs | code/test anchors |
|---|---|---:|---:|---:|
| 6.18.3.2 | Resource methods › GET | 23 | 0 | 1 |
| 6.8.3.2 | Resource methods › GET | 17 | 0 | 0 |
| 6.5.3.1 | Resource methods › GET | 11 | 0 | 0 |
| 5.2.39 | EntityMap | 10 | 0 | 2 |
| 5.2.9 | CSourceRegistration | 9 | 0 | 2 |
| 4.2.3 | Cross Domain Ontology | 8 | 0 | 1 |
| 5.2.35 | VocabProperty | 8 | 0 | 0 |
| 5.2.38 | JsonProperty | 8 | 0 | 0 |
| 7.2 | Notification behaviour | 8 | 0 | 2 |
| 5.2.36 | ListProperty | 7 | 0 | 0 |
| 5.2.5 | Property | 7 | 0 | 0 |
| 5.2.7 | GeoProperty | 7 | 0 | 0 |
| 6.3.8 | Notification behaviour | 7 | 0 | 3 |
| 5.2.37 | ListRelationship | 6 | 0 | 0 |
| 5.2.6 | Relationship | 6 | 0 | 0 |

A SHALL count is a proxy: one sentence can carry several rules, and a
clause's unit tests (the anchors column) may assert what no TP does. The
counts are a snapshot to regenerate after ledger or suite changes, not a
gate.
