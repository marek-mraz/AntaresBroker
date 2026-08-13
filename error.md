# ETSI test-suite tool bugs (never hack the broker around these)

Log of defects in the ETSI Robot suite itself, per the testing guide: prove
the suite contradicts the spec, record it here, leave the broker correct.
Each entry: TP id, what the suite does, why it is wrong (spec clause), status.

## Known (inherited from the Scorpio campaign in this workspace)

### IOP QueryEntities 04_01 / 04_02 — duplicate-id setup → self-inflicted 409

The suite's own setup creates two payloads with the same entity id on one
broker; the second create correctly gets 409 AlreadyExists (CIM 009 5.6.1),
which the test then reports as a failure. Broker behaviour is spec-correct.
Status: open upstream; excluded from gating conclusions. (See memory
`multi-broker-fed-stack.md`.)

## New entries

### 2026-08-05 — forge.etsi.org serves an incomplete TLS chain (infra, not a TP)

`forge.etsi.org` presents ONLY its leaf certificate (`CN=*.etsi.org`, issuer
`GeoTrust TLS RSA CA G1`) with no intermediate in the handshake. Strict chain
builders — rustls/webpki (the broker), OpenSSL (curl), python `ssl` (the
Robot suite itself) — all fail with "unable to get local issuer certificate";
browsers mask it via AIA chasing. Effect: every `@context` fetch from the
forge and every suite-side resolve fails, turning the whole run red with
`LdContextNotAvailable` (first seen as 33/33 CommonBehaviours failures; runs
on 2026-08-04 were green, so the server changed in between).

Not a broker bug and not hacked around: verification stays on. Fix was a
deliberate trust-anchor addition of the PUBLIC DigiCert intermediate
(`dev/ca-extra.pem`): brokers via `ANTARES_EXTRA_CA_FILE` (compose), the
suite + curl via `REQUESTS_CA_BUNDLE`/`CURL_CA_BUNDLE`/`SSL_CERT_FILE`
(pipeline).

**RESOLVED 2026-08-05, same day**: ETSI fixed their server — `openssl
s_client -connect forge.etsi.org:443 -showcerts` now returns the full
3-cert chain (leaf + GeoTrust TLS RSA CA G1 + DigiCert Global Root G2).
Both wirings and `dev/ca-extra.pem` removed. The `ANTARES_EXTRA_CA_FILE`
knob itself stays in the broker — it is the documented mechanism for
private CAs (§16.4).

### 2026-08-05 — MqttUtils launches mosquitto with no readiness wait (tool bug, fixed in fork)

`Start Mqtt Server` (resources/mqttUtils/MqttUtils.resource) does
`docker rm -f` + `docker run -d` and returns immediately; the test's first
MQTT connect races the mosquitto daemon start. On a cold docker daemon the
race is lost reliably: `058_02_02` fails with
`ConnectionRefusedError: [Errno 111]` while `058_02_01` (image already warm)
passes. Fixed in the fork: `Wait Until Keyword Succeeds` polling the broker
port (15 s / 0.5 s) after the `docker run`, before the keyword returns. Also
load-bearing: the mosquitto container must be the ONLY occupant of
`compose-files_default` (it is addressed by the hardcoded 172.29.9.2 mapping)
— the ETSI compose therefore keeps the db containers on their own `dbs`
network, and the pipeline creates `compose-files_default` for every run since
the compose now references it as external.

### 2026-08-06 — ContextSource keywords read `Location` case-sensitively (tool quirk, absorbed in the K2 LB config)

The ContextSource suite's create-without-id keywords extract the new
resource id from the `Location` response header via a case-SENSITIVE lookup
(plain dict, not requests' CaseInsensitiveDict — the Provision/Consumption
keywords are case-insensitive and unaffected). HTTP header names are
case-insensitive per RFC 9110 §5.1, so this is a suite quirk, not a broker
contract. It only surfaces behind the K8 LB: the broker emits `Location`
title-cased, haproxy's HTX normalizes it to lowercase, the keyword misses it,
the id becomes the string `None`, and the un-deletable orphan
registration/subscription then cascades (12 of the 15 reds in the first K8
run: wrong 047_16 match counts, 041_x query counts, D015/D017 forwarding
207s to the orphan's `my.csource.org` endpoint). Absorbed in OUR overlay,
not hacked around in the broker: `haproxy-ha.cfg` now carries
`h1-case-adjust location Location` + `option h1-case-adjust-bogus-client`,
which merely preserves the case the broker sent. The remaining 3 reds were a
real HA bug (per-instance Cached-@context registry split-brain), fixed by
making the persisted `jsonld_contexts` rows the single existence truth.

### 2026-08-06 — forge `develop` @context files churned mid-day (environment, not broker)

The suite's context URLs point at forge's LIVE `develop` branch. Mid-session
ETSI edited them: the compound gained a RELATIVE nested ref
(`"ngsi-ld-test-suite.jsonld"` — which exposed a real broker gap, fixed:
JSON-LD 1.1 relative context IRIs now resolve against the referencing
document's URL in `merge_entry_based`), and the nested context shrank to 15
keys that no longer agree with the `easy-global-market` context the payload
files use (`availableSpotsNumber` vs `availableSpotNumber`, `providedBy`
gone). Result: entities expand attributes under
`https://ngsi-ld-test-suite/context#…` at create time and the query-side
context can no longer compact them — 7 TPs fail with expanded-IRI diffs in
EVERY tier, native included, until forge settles or the two context families
re-converge: Consumption 019_01_02/019_02_02/019_02_04/023_01_01, plus
ContextSource 036_03_01/036_05_01 and DistributedOperations D003_01_inc
(same `isParked` class, measured on the 13:07 node-tier re-proof —
1018/1025 with ZERO broker-caused reds). Morning runs (K8 1037/1037, N7a 1025/1025) predate the edit and
were consistent. Do NOT chase these 4 as broker bugs; re-measure when the
upstream contexts stabilise.

### 2026-08-06 — N7 wasm-tier lessons (all fixed in-tree)

- Node shim: `dns.setDefaultResultOrder("ipv4first")` (suite mocks are
  IPv4-only, Node prefers ::1 → every federation forward 502'd), a
  node:http-based `fetch` replacement that PRESERVES header case both ways
  (JS `Headers` lowercases; ContextSource/receiver keywords are
  case-sensitive — same class as the K8 haproxy `h1-case-adjust`), and
  `Object.defineProperty(resp,"url",…)` (reqwest-wasm does
  `Url::parse(resp.url()).expect("url parse")` and a constructed Response
  has url "").
- Five shims must carry DISTINCT `hostAlias` values or Via loop-detection
  508s every broker-to-broker forward.
- wasm egress needs its own deadlines (`io_deadline`, gloo-timers race):
  browser fetch has no client-level timeout and reqwest's AbortController
  timer does not arm in a dedicated worker; a pending fetch holds its
  resolve permit and eventually stalls ALL context resolution.
- `page.evaluate` requests from the ETSI proxy carry a 30 s deadline and
  `worker.onerror` now fails-fast every pending waiter — a silently dead
  OPFS worker previously froze robot (python-requests has no timeout).
- Batch delete (5.6.10) now mirrors temporal deletion per id like single
  delete (5.6.6) — without it the between-suite reset's batch delete left
  every prior suite's entities alive in the temporal store (the 021_23
  "3 != 13" stray-Buildings cascade).
- Self-inflicted trap to remember: a leftover diagnostic HTTP server on the
  suite's context-server port (8087) makes `Start Server` block robot
  FOREVER and poisons context fetches with stub content. Sweep probe
  processes before every measured run.

### 2026-08-09 — PurgeEntities 060_05_01 filters by `id` alone (contradicts 5.6.21.4 + Table 6.4.3.3-1)

`TP/NGSI-LD/ContextInformation/Provision/PurgeEntities/060_05.robot` calls
`Purge Entities  id=${entity_id}  keep=name  context=…` and asserts **204**.
That request carries no qualifying filter, so a spec-conformant broker must
answer **400 BadRequestData**.

Proof, two independent places in CIM 009 V1.9.1:

1. **5.6.21.4 (p.194)** — "At least one of the following input data shall be
   provided: a) selector of Entity Types; b) list of Attribute names, including
   at least one non-system Attribute; c) NGSI-LD Query, including at least one
   non-system Attribute; d) NGSI-LD GeoQuery; e) local scope (see clause
   5.5.13). **If none of the above is provided, then an error of type
   BadRequestData shall be raised (too wide query).**"
   Reinforced by 5.6.21.3: "it is not possible to purge a set of entities by
   only specifying desired Entity identifiers".
2. **Table 6.4.3.3-1 (p.286-287)** — the Purge URL-parameter table states, in
   the Remarks of `geometry`, `q` and `type`: "At least one among: **type,
   attrs, q, or georel** shall be present, unless the execution of the request
   is limited to local scope". `keep` and `drop` are defined in the same table
   purely as projection ("every Entity within the payload body is reduced…"),
   and neither appears in that constraint.

So `keep` does not qualify as 5.6.21.4(b)'s "list of Attribute names" — that
maps to `attrs`. (Editorial note: Table 6.4.3.3-1 references `attrs` three
times in its Remarks but has no `attrs` row — an apparent copy-paste slip from
Table 6.4.3.2-1. It does not affect the conclusion, since `keep`/`drop` are
excluded either way.)

**Action taken:** the broker is left correct — `entities.rs` now implements the
five conditions exactly, and `id`/`idPattern` filter but never qualify (this is
the fix for the unauthenticated `DELETE /entities?idPattern=.*` tenant wipe).
The TP was minimally corrected in this fork to add `type=Building`, which keeps
its actual subject — `keep` projection semantics — intact while making the
request spec-legal. Status: to be raised upstream.

## 2026-08-09 — CIM 009 V1.9.1 internal inconsistency: Table 6.6.3.2-2 (Update Attributes 207)

Table 6.6.3.2-2 (p.297) declares the 207 Multi-Status response body **Data Type
= UpdateResult**, but its own Remarks cell says distributed errors are returned
"in a **BatchOperationResult** structure". Every sibling table — 6.6.3.1-2
(Append, p.296), 6.7.3.1-2 (Partial Update, p.299), 6.7.3.2-2 (Delete, p.300),
6.7.3.3 (Replace) — says UpdateResult in both places, and 5.2.18 (p.122) defines
UpdateResult as the result of attribute update operations "regardless of whether
local or distributed". The Data Type column and 5.2.18 govern; the Remarks cell
is a copy-paste error in the spec. Antares returns an UpdateResult on all five
/attrs methods (audit V-15). Not a test-suite issue (no TP asserts either
shape), recorded here because ics.yaml cites it.

## 2026-08-10 — ETSI suite defect: `D018_01` asserts 508 for an **inclusive** registration

**Hit:** the only failing TP of CI run 31326666159 — identical in all four
store modes: `DistributedOperations → D018_01 Loop Detection With Via Header`,
`508 != 204`.

**What the test does.** Test Setup registers the CSR through
`Prepare Context Source Registration From File … mode=inclusive` (fixture
`csourceRegistrations/context-source-registration-vehicle-redirection-ops.jsonld`,
whose `operations: ["redirectionOps"]` names an operation GROUP, not a mode).
It then creates the entity, reads the `Via` pseudonym off the forwarded create
(`1.1 antares1`), replays it on `DELETE /entities/{id}` and asserts **508**.

**Spec.** 6.3.17 (p.278) scopes that status precisely:

> In the case of an **exclusive** or **redirect** registration, where all of
> the data is held outside of the `Context Broker` and held in a single
> registered source, the following errors shall be returned: 508 Loop
> Detected — if the single registered source and tenant is registered to
> redirect back on to the `Context Broker`.

For an **inclusive** registration the same clause prescribes 207 when sources
return errors, and Table 6.3.18-2 (p.279) makes the Via listing "used when
determining matching registrations" — so the looping registration drops out of
matching and the DELETE executes locally. 204 is the correct answer; 508 would
fail an operation the broker can serve from its own store.

Corroborating: siblings `D018_02` and `D018_03` carry the `additive-inclusive`
tag and `D018_01` does not, yet only `D018_01` hardcodes `mode=inclusive` —
the mode looks like a slip in a test whose assertion belongs to the
exclusive/redirect path.

**Broker:** correct. `federation::handle_via_loop` raises 508 only for a
single proxied registration; unit test
`loop_508_only_for_a_single_proxy_registration`, end-to-end test
`crates/antares-api/tests/federation_loop.rs`.

**Action taken:** `D018_01`'s setup switched to `mode=redirect` in this fork,
which puts the scenario in the clause its assertion cites and leaves the
subject of the test (Via replay ⇒ loop detected) intact. Verified first as a
Rust test (`single_redirect_source_looping_back_is_508`) before touching the
fixture. Status: to be raised upstream.

## 2026-08-10 — Official `_exc` TPs create exclusive registrations 4.3.6.3 forbids

**Suite defect (9 files).** Clause 4.3.6.3 p.41: "An exclusive registration
shall always relate to specific Attributes found on a single Entity. Thus,
the registration shall define both: • An entity id (i.e. an id pattern or
Entity type defining a group of entities is not supported for exclusive
registrations). • Attributes." Yet:

- `D001_02_exc`, `D002_02_exc` register `mode=exclusive` from the
  attribute-less `context-source-registration-vehicle-redirection-ops.jsonld`
  (entity id, **no propertyNames/relationshipNames**) and assert 201.
- `D012_01_exc`, `D013_01_exc`, `D013_02_exc`, `D014_01_exc`, `D014_02_exc`,
  `D015_01_exc`, `D016_01_exc` register `mode=exclusive` with
  `idPattern=urn:ngsi-ld:Vehicle:*` (the batch-ops fixture) and assert 201 —
  an id pattern is exactly what the clause rules out.

**Broker:** correct as of 2026-08-10. `csource::validate_exclusive` raises
BadRequestData 400 for exclusive registrations missing an entity id or
Attributes (or using idPattern), and `csource::check_proxied_overlap` raises
409 (AlreadyExists mapping — Table 6.3.2-1 has no Conflict type) when a new
exclusive/redirect covers the same (Entity ID, Attributes) combination as an
existing exclusive (4.3.6.3 + 5.9.2 p.227).

**Action taken:** fork fixed. D001_02_exc/D002_02_exc switched to the
attribute-carrying `vehicle-speed-with-redirection-ops` fixture; the 7 batch
`_exc` setups replace the idPattern with the two concrete generated entity
ids (`Update Value To JSON $.information[0].entities`). Semantics of every
test preserved; all 9 + the whole BatchEntities tree green locally against
the strict broker. Extension TP `436_03` pins the negative cases upstream
skips. Status: to be raised upstream.

## 2026-08-12 — 021_23 temporal orderBy (fork TP corrected)

**TPs:** 021_23_01/03..09 (fork extension TP, QueryTemporalEvolutionOfEntities)

**Claim:** temporal query with `orderBy=<attribute>` returns 200 ordered.

**Spec:** 5.7.4.4 (p.208): "If the ordering parameter is present and refers
an entity name other than \"id\", then an error of type BadRequestData shall
be raised." Ordering by arbitrary members exists only on the entity query
(5.7.2.4/4.23); the temporal query restricts it to `id`.

**Action taken:** fork TP rewritten — id-based cases keep their ordering
assertions, non-id members assert 400. Broker enforces the 400 as of the
5.7.4 audit commit.

## 2026-08-12 — LdContextNotAvailable is 504, not 503 (fork fixtures fixed)

**TPs:** 043_01 (5 cases), 028_07, 051_05_01, 053_05_01

**Claim:** the official fixtures assert 503 "Service Unavailable" for
`LdContextNotAvailable` — a V1.8-era expectation.

**Spec:** V1.9.1 Table 6.3.2-1 (p.269, verified in the PDF) maps
`https://uri.etsi.org/ngsi-ld/errors/LdContextNotAvailable` to **504**.

**Action taken:** broker mapping restored to 504 (it had been flipped to
503 to match the suite — the exact anti-pattern §2 forbids); the four
fork fixtures now assert 504 / "Gateway Timeout". testsuite-doubts #18
marked RESOLVED. Status: to be raised upstream.

## 2026-08-12 — 5.3.4 SnapshotNotification internal inconsistencies (spec doubt)

**Where:** CIM 009 V1.9.1 Table 5.3.4-1 vs Table 5.2.41-2.

**Doubt 1 — member naming:** the Snapshot datatype (5.2.41) calls the
temporal details list `snapshotTemporalQueriesDetails`, but the
SnapshotNotification table (5.3.4) calls the same list
`temporalSnapshotQueriesDetails`. Decision: each datatype's own table
governs its payload — the broker stores/serves the 5.2.41 name on the
Snapshot resource and emits the 5.3.4 name in notifications (asserted both
ways in tests/snapshots_5_16.rs clause_5_16_6_snapshot_notification).

**Doubt 2 — phantom member:** the `expiresAt` row's description says "In
this case, snapshotReady shall be set to false", but no `snapshotReady`
member is defined anywhere in Table 5.3.4-1 (or 5.2.41). Looks like a
leftover from an earlier draft. Antares emits no `snapshotReady` (negative
assertion in the same test). No deletion notifications are sent — the
expiresAt-before-notifiedAt encoding only matters for implementations that
evict and notify, which the 5.5.15 MAY does not require.

**Action:** raise upstream with the other V1.9.1 doubts; no fork TP change
needed (the official suite has no Snapshot coverage — 5161_01 is ours).

## 2026-08-13 — CI matrix (6): 5.9.2.4 registration-vs-entity conflicts vs official dist-ops setups (fork fixed)

**TPs:** D005_01_exc, D006_01/02_red, D009_01_exc, D013_01/02_red,
D014_01/02_exc+red, D015_01_exc+red, D016_01_red, 436_02 (fork TP),
IOP RetrieveEntity IOP_CNF_04_02 — plus cascade victims D001_01/02/03_03_inc
and IOP QueryEntities "Via POST" (bare-string entities). 65 failures across
file/postgres/timescale, deterministic.

**Spec (verified verbatim, p.227-228):** 5.9.2.4 — exclusive: Conflict if an
Entity with a registered id already carries a registered Attribute; redirect:
Conflict if ANY existing Entity matches; auxiliary: operations limited to
retrieveOps/retrieveEntity/queryEntity else BadRequestData. The official TPs
(since_v1.6.1) create their entities BEFORE registering exclusive/redirect over
them, which a V1.9.1-conformant broker must refuse.

**Broker:** correct (`csource::check_entity_conflict`, c8ec8ea). The
DistributedOperations tree was not re-run locally after c8ec8ea — the 2026-08-12
resource-binding sweep list omitted it, which is how this reached CI first.

**Action taken (fork):** setup order swapped in the genuine violators
(registration first, then `local=true` creates — entity creation carries no
5.9.2.4 restriction); `Purge Entities type=Vehicle local=true` guard in the
idPattern-scoped `_red` setups (residual Vehicles from earlier PASSING tests
legally 409 those registrations); 436_02 aux fixture → retrieve-ops (negative
already pinned by 5922_01); IOP_CNF_01_02 POST query sends EntitySelector
objects per 5.2.33 instead of bare id strings; 5243_01_05 rewritten — collation
is HONOURED since the ICU work (4171ff4), so de-u-co-phonebk now asserts 200 +
"Ähre" before "Zebra" (codepoint order proven to flip it), and a new 5243_01_06
pins the 400 for an unparseable collation tag. Local runs: 36/36 + 17/17 green.
Multi-broker IOP validation stays CI-only. To be raised upstream with the other
V1.9.1 doubts (the D0xx setups predate the 5.9.2.4 conflict rules).

**Still open from matrix (6):** postgres 5814_01_01 Wait-For-Request timeout
(distsub on the new pg DistSub doc-kind tables — needs a docker-pg repro) and
timescale 053_07_01 (ImplicitlyCreated @context listed as Cached — suspected
in-process context-cache state carried across suites in the serial cell).
