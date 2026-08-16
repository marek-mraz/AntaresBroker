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
