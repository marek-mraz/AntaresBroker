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
