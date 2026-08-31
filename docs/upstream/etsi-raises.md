# Upstream raises — ready to file

Issue texts prepared for filing. Two targets:

- **Suite issues** → https://forge.etsi.org/rep/cim/ngsi-ld-test-suite
  (issues 1–3). Our fork already carries the fixes; each issue links the
  fork commit so the patch can be cherry-picked.
- **Spec feedback** → ETSI ISG CIM issue tracker for GS CIM 009 V1.9.1
  (issues 4–6).

---

## 1. [suite] D018_01 asserts 508 Loop Detected for an *inclusive* registration

**Title:** D018_01 uses mode=inclusive but asserts the exclusive/redirect
508 loop behaviour

**Body:**

Test `D018_01 Loop Detection With Via Header` registers its Context Source
with `mode=inclusive` (fixture
`context-source-registration-vehicle-redirection-ops.jsonld` — note its
`operations: ["redirectionOps"]` names an operation *group*, not a mode),
replays the broker's own `Via` pseudonym on `DELETE /entities/{id}`, and
asserts **508**.

GS CIM 009 V1.9.1 clause 6.3.17 (p. 278) scopes 508 to the case where
"all of the data is held outside of the Context Broker and held in a
single registered source" — i.e. **exclusive or redirect** registrations.
For an inclusive registration, Table 6.3.18-2 (p. 279) makes the Via
listing an input to registration *matching*: the looping source drops out
of matching and the DELETE executes locally, so **204** is the conformant
answer. A broker that returns 508 here refuses an operation it can serve
from its own store.

Corroborating: siblings D018_02/D018_03 carry the `additive-inclusive`
tag; D018_01 does not, yet is the only one hardcoding `mode=inclusive` —
the mode looks like a slip in a test whose assertion belongs to the
redirect path.

**Proposed fix (in our fork):** switch D018_01's setup to `mode=redirect`,
which puts the scenario inside the clause its assertion cites and keeps
the subject of the test (Via replay ⇒ loop detected) intact.

---

## 2. [suite] Nine official `_exc` TPs create exclusive registrations clause 4.3.6.3 forbids

**Title:** `_exc` variants register mode=exclusive without Attributes /
with idPattern, both ruled out by 4.3.6.3

**Body:**

Clause 4.3.6.3 (p. 41): "An exclusive registration shall always relate to
specific Attributes found on a single Entity. Thus, the registration shall
define both: an entity id (i.e. an id pattern or Entity type defining a
group of entities is not supported for exclusive registrations) [and]
Attributes." Yet:

- `D001_02_exc`, `D002_02_exc` register `mode=exclusive` from the
  attribute-less `context-source-registration-vehicle-redirection-ops`
  fixture (entity id, no propertyNames/relationshipNames) and assert 201.
- `D012_01_exc`, `D013_01_exc`, `D013_02_exc`, `D014_01_exc`,
  `D014_02_exc`, `D015_01_exc`, `D016_01_exc` register `mode=exclusive`
  with `idPattern=urn:ngsi-ld:Vehicle:*` and assert 201 — an id pattern is
  exactly what the clause rules out.

A broker enforcing 4.3.6.3 (BadRequestData 400) fails all nine setups.

**Proposed fix (in our fork):** D001_02_exc/D002_02_exc use the
attribute-carrying `vehicle-speed-with-redirection-ops` fixture; the seven
batch `_exc` setups replace the idPattern with the concrete generated
entity ids. The semantics of every test are preserved. An extension TP
(`436_03`) pins the negative cases the official suite skips.

---

## 3. [suite] LdContextNotAvailable fixtures assert 503; V1.9.1 mandates 504

**Title:** 043_01 / 028_07 / 051_05_01 / 053_05_01 assert 503 for
LdContextNotAvailable — V1.8-era expectation

**Body:**

GS CIM 009 V1.9.1 Table 6.3.2-1 (p. 269) maps
`https://uri.etsi.org/ngsi-ld/errors/LdContextNotAvailable` to **504
Gateway Timeout**. The listed fixtures still assert 503 Service
Unavailable, which was the V1.8 mapping. Suggest updating status code and
reason-phrase assertions to 504 / "Gateway Timeout" (done in our fork).

---

## 4. [spec] 5.3.4 SnapshotNotification: member naming conflict + phantom `snapshotReady`

**Title:** GS CIM 009 V1.9.1 clause 5.3.4 — two internal inconsistencies
in Table 5.3.4-1

**Body:**

1. **Member naming.** The Snapshot data type (Table 5.2.41-2) names the
   temporal details list `snapshotTemporalQueriesDetails`; the
   SnapshotNotification table (5.3.4-1) names the same list
   `temporalSnapshotQueriesDetails`. One of the two should be aligned —
   implementations currently have to pick (we emit each datatype's own
   spelling in its own payload).
2. **Phantom member.** The `expiresAt` row of Table 5.3.4-1 says "In this
   case, snapshotReady shall be set to false", but no `snapshotReady`
   member is defined in Table 5.3.4-1 (or anywhere in 5.2.41) — it reads
   like a leftover from an earlier draft. Either define the member or
   drop the sentence (the expiresAt-before-notifiedAt encoding already
   carries the deletion signal).

Additionally, the prose of 5.16.1.4 sets the initial status to
"preparation" while Table 5.2.41-2's vocabulary is "preparing" — worth
aligning in the same pass.

---

## 5. [spec] Table 6.6.3.2-2 (Update Attributes 207): Data Type vs Remarks conflict

**Title:** GS CIM 009 V1.9.1 Table 6.6.3.2-2 — 207 body declared
UpdateResult but Remarks say BatchOperationResult

**Body:**

Table 6.6.3.2-2 (p. 297) declares the 207 Multi-Status response body Data
Type = `UpdateResult`, but its own Remarks cell says distributed errors
are returned "in a BatchOperationResult structure". Every sibling table —
6.6.3.1-2 (Append), 6.7.3.1-2 (Partial Update), 6.7.3.2-2 (Delete),
6.7.3.3 (Replace) — says UpdateResult in both places, and 5.2.18 (p. 122)
defines UpdateResult as the result of attribute update operations
"regardless of whether local or distributed". The Remarks cell looks like
a copy-paste from the batch chapter; suggest correcting it to
UpdateResult.

---

## 6. [spec] 5.7.4.4 / Table 5.2.21-1: lastN vs values-filter ordering is unspecified

**Title:** GS CIM 009 V1.9.1 — interaction of `lastN` with the values
filter (and geoquery) on temporal queries is undefined

**Body:**

Table 5.2.21-1 (p. 123) defines `lastN` as "Only the last n instances,
per Attribute, per Entity (under the specified time interval) shall be
retrieved" — window-scoped, but silent on its ordering relative to the
other filters. Clause 5.7.4.4 S2 (p. 209) says the values filter "shall
be checked against all the Attribute instances resulting from the
initial filtering performed by the temporal query"; whether the lastN
cap is part of that "initial filtering" is unspecified. The two readings
give different answers whenever a matching instance is in the window but
not among the last n:

- lastN-first: the entity may FAIL the values filter (the matching
  instance was capped away);
- filter-last (lastN as a presentation cap): the entity matches, and
  lastN trims the returned instances.

The same question arises for the geoquery (S3) and the Scope query (S4,
with the 4.18 validity semantics additionally trimming instances after
S-selection). Suggest an explicit sentence in 5.7.4.4, e.g. "the lastN
restriction is applied to the instances of the final result set, after
S4" (which matches the S-chain's structure — none of S1-S4 mentions
lastN). Until then, implementations differ silently; ours applies lastN
after all S-filters (API-side presentation cap) and withholds any
store-level lastN optimization when q/geoQ/scopeQ is present.

## 7. [spec] CIM 029 A.5.2.26 cites clause 5.15.4, which does not exist in CIM 009 V1.9.1

**Title:** GS CIM 029 V2.1.1 annex A table A.5.2.26 (`/temporal/entityMaps/`)
cites a non-existent CIM 009 clause

**Body:**

Table A.5.2.26 of GS CIM 029 V2.1.1 (the ICS pro forma table for the
`/temporal/entityMaps/` resource) cites clause **5.15.4** as its CIM 009
reference. In GS CIM 009 V1.9.1 clause 5.15 is "Context Source Identity
Information" and contains only 5.15.1 — there is no 5.15.4. The clause
that carries the table's own subject (entity-map retrieval/deletion for
temporal queries) is **5.14.5**, with the HTTP binding in 6.35.3.1 /
6.35.3.2.

Presumably the citation predates a renumbering between the CIM 009
version the pro forma was authored against (V1.6.1 per clause 2) and
V1.9.1. Suggest correcting the citation to 5.14.5 in the next CIM 029
revision, and re-checking the neighbouring temporal tables for the same
drift.

## 8. [openapi] v1.8.1 temporal GET operations declare the `options` parameter twice

**Title:** ngsi-ld-openapi v1.8.1: `GET /temporal/entities` and
`GET /temporal/entities/{entityId}` carry two parameter components both
named `options`

**Body:**

In `openapi-3.1.0/ngsi-ld-api.yaml` at tag v1.8.1, the operations
`GET /temporal/entities` and `GET /temporal/entities/{entityId}` each
list two `$ref`s to parameter components that resolve to the same
parameter name `options` (in the same `query` location). The OpenAPI 3.1
specification requires parameter uniqueness by name+location, so strict
validators (e.g. openapi-spec-validator) reject the document with
"Duplicate parameter 'options'". Suggest merging the two component
definitions (their enum sets appear to have been split between temporal
and non-temporal option values) or renaming one.

## 9. [suite] three temporal TPs assert a Content-Range unit the clause does not define

**Title:** ngsi-ld-test-suite: `Content-Range` unit asserted as `date-time`,
CIM 009 6.3.10 mandates `DateTime`

**Body:**

GS CIM 009 V1.9.1 clause 6.3.10 (p. 275) specifies the temporal Partial
Content response verbatim as: `"unit"` shall be equal to `"DateTime"`.

Three test purposes assert the lowercase spelling instead:

- `TP/NGSI-LD/ContextInformation/Consumption/TemporalEntity/RetrieveTemporalEvolutionOfEntity/020_13.robot:91`
- `TP/NGSI-LD/ContextInformation/Consumption/TemporalEntity/QueryTemporalEvolutionOfEntities/021_15.robot:88`
- `TP/NGSI-LD/ContextInformation/Consumption/TemporalEntity/QueryTemporalEvolutionOfEntities/021_16.robot:46`

IETF RFC 7233 clause 4.2 makes the range unit a token matched literally, so
the two spellings are not interchangeable: an implementation that follows
6.3.10 fails all three TPs, and one that passes them emits a unit the clause
does not define. Suggest correcting the three assertions to `DateTime`, or,
if the lowercase form is the intent, correcting 6.3.10.
