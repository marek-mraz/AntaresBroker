# ETSI NGSI-LD Test Suite — Issues & Doubts

Running list of issues encountered in the ETSI NGSI-LD Test Suite (the
official ISG CIM conformance suite at
`https://forge.etsi.org/rep/cim/ngsi-ld-test-suite`) while running it
against swBroker. Some are bugs in the test code, some in the test
framework (`robotframework-httpctrl`), some are environmental
assumptions. Intended as input for upstream issue reports.

Each entry: **what we hit** · **where** · **why it's wrong** · **impact** ·
**workaround / fix wanted**.

---

## 1. `Get Stub Count` int vs Robot literal string

**Hit:** Tests do
```robot
${stub_count}=    Get Stub Count    POST    /...
Should Be Equal   ${stub_count}    1
```
and fail with `1 (integer) != 1 (string)`.

**Where:** `Get Stub Count` is implemented in
`libraries/robotframework-httpctrl/src/HttpCtrl/__init__.py:1162` —
returns a Python `int`. Robot's `${...}` literal parsing makes the
right-hand `1` a string. Robot's `Should Be Equal` is type-strict by
default.

**Why it's wrong:** The test author wanted "stub was hit exactly once".
The expected idiom is `Should Be Equal As Integers ${stub_count} 1` or
`Should Be Equal ${stub_count} ${1}`.

**Impact:** Tests that pattern-match. Confirmed in our partial run on
nine DistOps batch / replace tests:
`D012_01_inc / _red`, `D013_01_inc`, `D014_01_red / _02_red`,
`D015_01_exc / _inc / _red`, `D016_01_inc / _red`, `D007_01_red`.
The broker is doing the right thing — stub IS hit once — but the
assertion fails on type alone.

**Workaround / fix wanted:** Patch each test to use one of the
type-relaxed forms above, OR change `get_stub_count` to return `str`.
Upstream issue: file against `forge.etsi.org/rep/cim/ngsi-ld-test-suite`.

---

## 2. HttpCtrl stub-server doesn't reply to broker forwards

**Hit:** Tests register a stub via `Set Stub Reply`, then expect the
broker to forward to the mock and receive that stubbed reply. The
broker's forward TCP-connects and sends the request, but the mock
either closes the connection or never replies. Broker side sees
`SWC_ERR_CLOSED` after ~5 s, treats CSR as failed, surfaces 404 / [].

**Reproduced isolated:** with a tiny standalone Python `BaseHTTPRequestHandler`
on `0.0.0.0:8086` mimicking what `Set Stub Reply` should do, the broker
forwards correctly, gets 200 + body, and the test scenario passes
end-to-end. So the broker side is provably correct.

**Where:** `libraries/robotframework-httpctrl/src/HttpCtrl/http_handler.py`
`__default_handler` and `http_stub.py` `__is_satisfy`. The stub matcher
has odd URL-prefix semantics (lines 79–141 in `http_stub.py`):
- For GETs with `?` in the URL it does substring matching of stub
  params against criteria params, which can both false-match and
  false-miss depending on param order.
- For requests with `urn` in the URL it does a `for elements in id:`
  scan that is brittle.
- The `if "urn" in stub.criteria.url:` branch never returns False on a
  no-match path that has `?` — it returns False above. The control flow
  isn't easy to reason about.

When `__is_satisfy` returns False for a request that the test author
thought it would match, the handler falls through to
`ResponseStorage().pop()` (line 88-92), which **blocks indefinitely**
waiting for an explicit `Wait For Request` + `Reply By` keyword —
neither of which the test calls. Broker times out and treats the CSR as
unreachable.

**Impact:** Many DistOps tests where the broker-to-CSR URL doesn't
literal-match the stub URL exactly: timeouts (8 tests), `204 != 404`
(7 tests), `200 != 404` (5 tests), `Lengths are different: 1 != 0`
(6 tests). Confirmed by running the same broker against our own
python-`http.server` mock — passes.

**Workaround / fix wanted:** Either
1. Replace the stub matcher with strict literal `(method, url)` tuple
   matching, or
2. Document the matcher's exact semantics and have the tests construct
   stub URLs that match what NGSI-LD brokers produce on the wire.

A minor side-issue: the handler's blocking fallback is unfriendly —
when no stub matches, returning a default 404 immediately would let
broker-side error handling exercise (and would be obvious in test
logs).

---

## 3. Mock server started AFTER CSR registered

**Hit:** Standard test setup is
```robot
${response1}=    Create Context Source Registration With Return  ...
Check Response Status Code  201  ${response1.status_code}
Start Context Source Mock Server     # only AFTER the POST returned
```

**Where:** Most DistOps test `Setup Registration And Start Context Source Mock Server`
keywords (e.g. D003_01_red.robot, D004_01_red.robot, ...).

**Why it's wrong:** The broker probes `<endpoint>/info/sourceIdentity`
at CSR-registration time (TS 104-175 § 15 / TS 104-175 § 5.2.6.5.6). If the mock isn't up yet,
the probe fails. Our broker treats probe failure as benign (CSR still
active, reactive Via-loop detection still works — see
`swNgsild/ldRegCache.c:402-418`), but a stricter implementation could
mark the CSR unreachable and refuse forwards.

**Impact:** None on swBroker thanks to lenient probe handling, but
flaky against any broker that relies on probe success. Wastes ~5 s per
test on the failed probe (configurable; we use a short default).

**Fix wanted:** Either start the mock first, or have a `Wait For
Server Up` keyword in the setup before the CSR POST.

---

## 4. Aggregation fixture timestamps in canonical seconds form

**Hit:** Aggregated-temporal expectation files use `2020-08-01T12:03:00Z`
(no fractional second). Brokers that emit `2020-08-01T12:03:00.000Z`
fail string-equality.

**Where:** Examples
- `data/expectations/temporalEntities/vehicle-temporal-representation-aggregated-avg-PT1H.json`
- The `ObservedAtPropertyOperator` in
  `libraries/assertionUtils.py:58` does relax DateTime comparisons —
  but only for fields whose JSON path ends in `['observedAt']`.
  Aggregation bucket tuples are `[value, t-start, t-end]` — paths like
  `root[0]['speed']['avg'][0][1]` — and don't get the relaxed match.

**Why it's wrong:** ISO 8601 / RFC 3339 both allow optional fractional
seconds. Both forms are spec-conformant. String-strict equality on
DateTime is fragile.

**Impact:** Aggregation tests `020_11_*` and `021_*` series, all temporal
retrieve / query tests that don't go through `observedAt`. We worked
around it by trimming `.000` from broker output when sub-second is zero,
but a broker that always emits ms (which is also valid) would still fail.

**Fix wanted:** Apply DateTime-aware comparison (parse as datetime,
compare instants) to ALL DateTime fields, not just `observedAt`. The
existing operator infrastructure makes that straightforward; matcher
predicate just needs a wider net.

---

## 5. Compound `@context` fetched from forge.etsi.org over the internet

**Hit:** Tests Link-header-reference
`https://raw.githubusercontent.com/marek-mraz/ngsi-ld-test-suite/main/resources/jsonld-contexts/ngsi-ld-test-suite-compound.jsonld`
and expect the broker to fetch + cache it.

**Where:** `resources/variables.py:3` defines `ngsild_test_suite_context`
to that absolute forge URL.

**Why it's wrong:** Tests are not hermetic — a broker behind a
restrictive firewall, or run when forge.etsi.org is down, will fail.
The test suite repo *contains* the @context document at
`resources/jsonld-contexts/ngsi-ld-test-suite-compound.jsonld`, but
points the broker at the forge URL.

**Impact:** First-run failures on isolated networks. Cache poisoning if
forge.etsi.org serves a different version than what the test fixture
expects.

**Fix wanted:** Have a local-server step in the suite setup that
serves the in-repo `@context` files at a `localhost:<port>/contexts/`
URL, and rewrite `ngsild_test_suite_context` to point there.

---

## 6. Some tests expect 206 for temporal queries that aren't truncated

**Hit:** `021_15_04 / _05 / _06 / _07 / _08`, `020_05_01 / _02`,
`020_13_01 / _06 / _07 / _08 / _09`, `021_16_01` — all expect 206 when
returning the full Temporal Evolution of an Entity. `021_03_01` (with
`?lastN=4`) expects 200.

**Where:** `TP/NGSI-LD/ContextInformation/Consumption/TemporalEntity/...`

**Why it's wrong:** TS 104-176 § 6.4.7 defines 206 + Content-Range as **conditional**
on truncation: "if the implementation is not able to respond with the
full representation at once". A response that fits within the
implementation cap is a normal 200. The ETSI fixtures contradict this
in two directions at once: 021_15 expects 206 for a 20-instance result
(no truncation by any reasonable cap), 021_03 expects 200 for a
`lastN=4` result (which our reading of the spec says SHOULD be 206 — a
client-requested truncation is still a truncation).

**Impact:** Inconsistent — there's no way for an implementation to
match all of these tests simultaneously without divergent logic per URL
shape. Currently 11 fail with `206 != 200` (broker correct, test
expectation drift); 1 fails the other way.

**Fix wanted:** Pick a coherent rule and propagate to all temporal
fixtures. Our reading: 206 whenever any truncation happened, including
client-requested via `?lastN`; 200 otherwise. TS 104-176 § 6.4.7 should be amended
to make this unambiguous.

---

## 7. ETSI compound `@context` defines `Vehicle` to a non-default IRI

**Hit:** With Link-header `compound`, `?type=Vehicle` expands to
`https://ngsi-ld-test-suite/context#Vehicle`. CSR registrations that
re-use the same compound `@context` register types under that IRI;
mocks that send entities with bare `"type":"Vehicle"` and no `@context`
get the entity expanded under the broker's request context, NOT under
the mock's implicit default. This caused us a long debug session
because the failure mode (silent post-merge type drop) is invisible
without logs.

**Where:** Implicit assumption across the DistOps test suite.

**Why it's noteworthy:** If the broker were strict — "a CSR response
without @context defaults to core, and core-Vehicle is a different IRI
than compound-Vehicle, so don't merge" — every DistOps test would fail.
swBroker now handles this by expanding the CSR response in the user's
context (commits `95fc80d` swJsonld + `676b267` swBroker). But the spec is ambiguous about
which @context wins for an unannotated CSR response.

**Fix wanted:** TS 104-175 § 5.3.2 should pick a side. Either "CSR response without
@context inherits the request's @context" (what we and ETSI do
implicitly) or "always defaults to core". One of the two needs to be
explicit in the spec.

---

## 8. CSR registration in test fixtures has `"endpoint": "http://my.csource.org:1026"`

**Hit:** The literal CSR fixture files (e.g.
`data/csourceRegistrations/context-source-registration-vehicle-complete.jsonld`)
hardcode `endpoint: http://my.csource.org:1026`, a non-resolving
hostname.

**Where:** Most CSR fixtures.

**Why it's noteworthy:** Tests rely on
`Prepare Context Source Registration From File` (in
`resources/ApiUtils/ContextSourceRegistration.resource`) to rewrite the
endpoint to `http://${context_source_host}:${context_source_port}`
(=0.0.0.0:8086 by default) before posting. Anyone who copies a fixture
and forgets the rewrite step gets unhelpful failures.

**Fix wanted:** Either drop the bogus URL from fixtures (use
`__REPLACE__` or the variable form directly), or document the rewrite
step prominently.

---

## 9. `020_17 / 020_18 / 020_19 / 020_20` (and similar) `Test Setup` is incomplete — POST `/temporal/entities` is not enough to make `DELETE /entities/{id}/attrs/{name}` succeed

**Hit:** `020_17_01..03`, `020_18_01..03`, `020_19_01`, `020_20_01`,
`020_15_01/02`, `020_12_02`, `021_09_01/02` — every test in the
"deleted attribute leaves a `deletedAt` tombstone" cluster.

**Where:** `TP/NGSI-LD/ContextInformation/Consumption/TemporalEntity/RetrieveTemporalEvolutionOfEntity/020_{15,17,18,19,20}*.robot`
(plus 020_12_02 in the same family, plus 021_09 on the multi-entity side).

**What the tests do:**

```
Test Setup           Create Temporal Entity   # → POST /temporal/entities only

[Test body]:
    Delete Entity Attributes ...              # → DELETE /entities/{id}/attrs/{name}
    Retrieve Temporal Representation Of Entity timeproperty=deletedAt
    expect: the deleted attribute appears with `deletedAt`
```

**Why it's wrong:** `POST /temporal/entities` (TS 104-175 § 11.2.2) creates a
*Temporal Evolution of an Entity* — instances in the temporal store.
`DELETE /entities/{id}/attrs/{name}` (TS 104-175 § 10.2.7) is a *current-state*
operation that requires the entity to exist in the current-state
representation. The two stores are architecturally separate: a
temporal-only entity has no current-state attribute to delete, so
the spec-strict response is **404 Not Found**, and no `deletedAt`
tombstone is written.

The test fixtures assume — implicitly and undocumented — that the
implementation under test treats current-state and temporal as a
single unified entity (so a `POST /temporal/entities` magically also
creates the corresponding current-state entity, even though
TS 104-175 § 11.2.2.4 only says "create the provided Temporal Evolution of an
Entity", with no mention of current-state mirroring).

**Impact:** 13 tests fail in this family. Our broker keeps the two
representations separate by design; we will not mirror, so these
tests will keep failing.

**Fix wanted:** The `Test Setup` should also `POST /entities` (or
otherwise ensure the entity has a current-state representation with
the attribute to be deleted) before the test body runs. With that
change the `deletedAt`-tombstone path can be exercised faithfully,
without forcing the implementation into a unified-entity architecture
that the spec doesn't mandate.

---

## 10. `043_01_*` + `028_07_02` expects HTTP 503 for `LdContextNotAvailable`; spec says 504

**Status:** RESOLVED — fixed in MR !269.

**Hit:** `043_01_01`, `043_01_02`, `043_01_03`, `043_01_04`, `043_01_05`
— "Verify receiving 503 - LdContextNotAvailable error if remote
JSON-LD @context cannot be retrieved" across Create Entity / Create
Subscription / Create Temporal Representation / Batch Create / CSR
Create. Plus `028_07_02` (subscription create with unreachable
`jsonldContext` URL — same pattern, asserts 503). All hard-code:

```
${expected_status_code}=        503
```

**Where:** `TP/NGSI-LD/CommonBehaviours/CommonResponses/VerifyLdContextNotAvailable/043_01.robot`

**Why it's wrong:** TS 104-176 § 6.3.2 (Table "Mapping of error types to HTTP status codes") explicitly maps
`LdContextNotAvailable` to HTTP **504**, not 503:

```
https://uri.etsi.org/ngsi-ld/errors/LdContextNotAvailable    504
```

TS 104-176 § 10.3.3.2 (Delete and Reload) likewise pairs the same problem type
with 504 Gateway Timeout. There's no place in v1.9.1 that maps it to
503.

**Impact:** All five tests fail with `expected 503, got 504`. Our
broker also previously failed silently (returned 201) because the
URL-prefix shortcut for the broker's own core context was loose
enough to swallow `ngsi-ld-core-context-non-existing.jsonld` —
that's now fixed; we emit the spec-correct 504 + ProblemDetails.

**Fix wanted:** The fixture should set `${expected_status_code}=504`
and rename the test docstring to match. Until then the suite keeps
failing 5 tests for a spec-correct broker.

---

## 11. `051_07_01` extracts the full URL field instead of the localId for DELETE

**Status:** RESOLVED — fixed in this MR (caller-side + [[#19]] keyword-side).

**Hit:** `TP/NGSI-LD/jsonldContext/Provision/DeleteContext/051_07.robot`
"Delete A ImplicitlyCreated @contexts With A Valid Id And Reload Set To
True". Expects 400 BadRequestData; broker returns 404 ResourceNotFound.

**Where:** the test's setup keyword extracts the identifier with
```robot
${data}=    Get From List    ${response.json()}    0
${implicit_id}=    Get From Dictionary    ${data}    URL
```

That `URL` field in the List-@contexts response is the full broker URL
(e.g. `http://localhost:8080/ngsi-ld/v1/jsonldContexts/urn:ngsi-ld:Context:1-NNN`).
The companion test `051_06` does it correctly:
```robot
${implicit_id}=    Get From Dictionary    ${response.json()}    jsonldContext
${implicit_id}=    Evaluate    '${implicit_id}'.split('/')[-1]
```
i.e. takes the last path segment (the locally unique identifier).

**Why it's wrong:** TS 104-175 § 13.5.3 says the operation takes "the locally
unique identifier that identifies the desired @context in the broker's
internal storage. For @contexts of kind 'Cached' this can also be the
original URL the broker downloaded the @context from." The URL form is
explicitly NOT supported for Hosted/Implicit, only Cached. So the test
ought to extract the localId (e.g. via the `localId` field of the list
entry, or by splitting `URL`).

**Impact:** broker correctly 404s (the URL-encoded full URL doesn't
match any cache key for an Implicit context), but the test expects 400
which would only fire after a successful lookup. One PASS→FAIL.

**Fix wanted:** mirror `051_06`'s pattern — take the `localId` field, or
split `URL` on `/` and take the last segment.


## 12. `019_09 / 019_10 / 019_11` use a self-intersecting polygon test fixture

**Hit:** the `Setup Initial Entities` keyword in
`TP/NGSI-LD/.../QueryEntities/019_09.robot`,
`019_10.robot`, `019_11.robot` (and any test that depends on
`building-location-polygon.jsonld` / `building-location-polygon-second.jsonld`)
creates a Building entity whose `location` GeoProperty is a Polygon with
crossing non-adjacent edges:

```
[[13.2865906,52.5648645],[13.2879639,52.5648645],
 [13.2797241,52.4988679],[13.477478,52.4712703],
 [13.5049438,52.5373084],[13.2865906,52.5648645]]
```

MongoDB's 2dsphere index rejects it with *"Loop is not valid: Edges 1
and 4 cross"*; per TS 104-175 § 7.2.4 a self-intersecting polygon is not a valid
GeoJSON, so the broker now rejects the entity create with 400
BadRequestData (see swNgsild commit "ldCheckGeo: reject
self-intersecting polygons up front"). The test setup then fails and
every test in the suite is marked `FAIL`.

**Why it's wrong:** TS 104-175 § 7.2.4 / RFC 7946 require polygon rings to be
simple (no self-intersection). The fixture is invalid GeoJSON and was
only "working" against brokers that didn't validate.

**Impact:** every 019_09 / 019_10 / 019_11 test fails on setup. Same
for `019_11_06`, where the *query* polygon is also self-intersecting —
that one would have failed even with a valid fixture.

**Fix wanted:** replace the polygon coordinates with a valid simple
polygon (e.g. a non-crossing pentagon roughly enclosing the same area
in central Berlin). Same for the query polygons in `019_11_06` and
similar.


## 13. `028_01_01 / 029_05_* / 029_06_01 / 030_03_01` deep-diff response against request — disallows spec defaults

**Status:** RESOLVED — fixed in MR !269.

**Hit:** these Subscription Create / Update / Retrieve tests build a
request payload, send it, then compare the response body to the
request via `deep_diff` and assert the result is empty.

**Why it's wrong:** TS 104-175 § 5.2.6.5.2 makes `isActive` a 0..1 member with
default `true`. TS 104-176 § 6.4.6 shows it on every Subscription representation
example. The broker injects `isActive: true` on create when the user
didn't supply one, and emits it on retrieve — so the spec-conformant
response contains a member the (minimal) request didn't.

The `deep_diff` then reports `dictionary_item_added: ["root['isActive']"]`
and the test fails. Same pattern would break for `status` (TS 104-175 § 5.2.6.5.2 too
— always emitted by the broker since it's computed, not user-supplied)
and any other defaulted member.

**Impact:** four tests flip PASS→FAIL purely from the broker emitting
`isActive: true`. Reverting the injection would also break the CSR-Sub
retrieve test that relies on the field being present, so the broker
side is correct.

**Fix wanted:** the assertion should ignore members that are spec
defaults / computed (`isActive`, `status`, `subscriptionName` when
auto-derived, etc.) — either with a `deep_diff` `exclude_paths`
allowlist or by switching to "subset" semantics ("response includes
every key the request asked for, with the same value").


## 14. `046_24 / 046_28` expectation files hard-code `createdAt`/`modifiedAt`

**Hit:** the JSON expectation files for `046_24_01` and `046_28_01`
(`entity-created-name-attribute-join-flat-sysAttrs.json`,
`entity-created-name-attribute-join-inline-sysAttrs.json`) include
fixed `createdAt`/`modifiedAt` ISO timestamps — e.g.
`"2025-05-19T15:20:16.418984Z"` — alongside the linked-entity members
the test cares about.

When `deep_diff` runs, the broker's *current-time* timestamps trip
`values_changed` and the test fails — even though the field that the
test was meant to verify (sysAttrs being present + linked entities
appearing) is satisfied.

**Why it's wrong:** the broker stamps `createdAt`/`modifiedAt` from
the request clock; no broker can satisfy a fixture that demands a
specific past datetime. TS 104-175 § 5.3.2.4 / TS 104-175 § 5.2.6.4.2 are explicit that these
are computed.

**Fix wanted:** strip `createdAt`/`modifiedAt` from the expectation
JSON files for these tests, OR move them through `deep_diff`'s
`exclude_regex_paths` so the timestamps are ignored.



## 15. `046_34_04` vs `046_37_01` LanguageProperty null-marker shape

**Hit:** when emitting the TS 104-175 § 10.5.7 null-marker for a deleted
LanguageProperty, the two fixtures disagree on the wire form:

  * `046_34_04` (attribute-delete) expects
    ```json
    "street": { "type": "LanguageProperty",
                "languageMap": { "@none": "urn:ngsi-ld:null" } }
    ```
  * `046_37_01` (entity-delete + showChanges) expects
    ```json
    "street": { "type": "LanguageProperty",
                "languageMap": "urn:ngsi-ld:null",
                "previousLanguageMap": { fr: ..., nl: ... } }
    ```

Spec TS 104-175 § 10.5.7 only says "the value (or object) shall be set to
`urn:ngsi-ld:null`". The bare-string form is the natural fit for
a single-value scalar key like `value` / `object` / `vocab` /
`json`, but for `languageMap` (whose normal shape is a JSON object)
both forms are defensible. The fixtures should pick one and stick
with it.

**Our call:** the broker emits `{@none: null}` on attribute-delete
notifications and bare `"urn:ngsi-ld:null"` on entity-delete
notifications, matching each fixture exactly. Both are spec-allowed.

**Fix wanted:** fixtures should agree (preferably both bare null,
since other types use bare null and TS 104-175 § 10.5.7 reads more naturally
that way). Once aligned, the broker's two code paths can be
unified.


## 16. `051_02_01`, `051_04_02/03`, `053_03_01` — missing resource imports

**Hit:** these tests reference `${ERROR_TYPE_RESOURCE_NOT_FOUND}` in
`Check Response Body Containing ProblemDetails Element`, but their
`*** Settings ***` only import `jsonldContext.resource` and
`AssertionUtils.resource` (and HttpUtils for some). None of those
define the variable.

`${ERROR_TYPE_RESOURCE_NOT_FOUND}` is defined in
`Common.resource`, `ContextInformationConsumption.resource`, and
several others — just not in the ones these tests pull in.

Robot fails the test with `Variable '${ERROR_TYPE_RESOURCE_NOT_FOUND}'
not found.` before even checking the broker behaviour.

**Our call:** broker behaviour is correct (404 on unknown context-id);
nothing to fix on our side.

**Fix wanted:** add `Resource ${EXECDIR}/resources/ApiUtils/Common.resource`
(or any resource that defines the variable) to those four tests'
`*** Settings ***` blocks.


## 17. `051_02_01` — random numeric string is not a URI

**Hit:** the test generates a 16-digit random string with `Generate
Random String 16 [NUMBERS]` and DELETEs `/jsonldContexts/<digits>`,
expecting 404 ResourceNotFound. Per TS 104-175 § 13.5.4 a context-id that is
not a valid URI must yield 400 BadRequestData — the broker correctly
returns 400.

**Our call:** broker enforces the URI check.

**Fix wanted:** the test should generate a URI (e.g.
`urn:test:<random>` or `http://example.org/<random>`) so the 404
path is exercised instead of the 400 (non-URI) path.


## 18. `051_05_01`, `053_05_01` — wants 503, spec says 504

**Status:** RESOLVED 2026-08-12 — fork fixtures now assert 504 /
"Gateway Timeout" (also `043_01`, `028_07`, which pinned the V1.8-era
503); the broker mapping was restored to the V1.9.1 Table 6.3.2-1
value (504) in the same change.

**Hit:** both tests stop the local @context server, then hit the
broker with a path that triggers `LdContextNotAvailable` (e.g. DELETE
?reload=true on a Cached context whose source is gone). They assert
`Check Response Status Code 503` with reason "Service Unavailable".

ETSI GS CIM 009 v1.9.1 TS 104-176 § 6.3.5 (and the table in ch6 line 260) maps
`LdContextNotAvailable` to **504** — `Gateway Timeout`. The broker
emits 504 per spec.

**Our call:** broker returns spec-mandated 504.

**Fix wanted:** the fixtures should assert 504 / "Gateway Timeout".


## 19. `051_07_01` — Robot's URL stripping mangles the implicit URL

**Status:** RESOLVED — fixed in this MR (keyword now anchors on absolute URLs too).

**Hit:** the `Delete a @context` keyword strips the prefix
`/ngsi-ld/v1/jsonldContexts/` from an absolute URL. When the broker
returns the implicit context's URL as
`http://localhost:8080/ngsi-ld/v1/jsonldContexts/<id>`, the strip
leaves `http://localhost:8080<id>`, which Robot then URL-encodes and
sends as the path component. The broker decodes this, looks up an id
that doesn't exist, returns 404 — but the test expected 400 (reload
on Implicit).

**Our call:** broker behaves correctly given the input it receives.

**Fix wanted:** the strip should anchor on the absolute prefix
(e.g. regex `^https?://[^/]+/ngsi-ld/v1/jsonldContexts/`) or use the
last `/`-segment of the URL.


## 20. `051_08_*`, `051_09_*` — fixture `core_context` ≠ broker's actual core

**Hit:** `resources/variables.py` sets
`core_context = 'https://uri.etsi.org/ngsi-ld/v1/ngsi-ld-core-context-v1.6.jsonld'`.
The broker's actual core (compile-time) is v1.9 / v1.9.1. Per spec
(TS 104-175 § 13.5.4), only the implementation's *actual* core
context is undeletable / un-reloadable — older or unrelated core URLs
are user contexts to the API and may be deleted normally.

So when the test does `Delete a @context ${core_context}` expecting
400 (or 200 for 051_09 reload), the broker (correctly) returns 204 /
404 because v1.6 is just a regular Cached/Implicit context to it, not
the core.

**Our call:** keep the spec-strict semantics. Don't extend the
"undeletable core" classification to alternate-version core URLs.

**Fix wanted:** `variables.py` should be set per-implementation, e.g.
`core_context = 'https://uri.etsi.org/ngsi-ld/v1/ngsi-ld-core-context-v1.9.jsonld'`,
or the fixture should derive it from a broker-served endpoint.


## 21. DistOps — `application/ld+json` + `Link` header in body-bearing POSTs

**Hit:** the Robot keyword `Query Entities Via POST` (and the related
batch-query helpers used across `D011_*`, `D016_*`) sets
`Content-Type: application/ld+json` AND a `Link: <ctx>; rel=…/json-ld#context`
header, with no `@context` in the body. The broker returns 400
BadRequestData ("Missing @context") and the test fails with
"expected 200, got 400" (~6 tests).

**Spec:** TS 104-176 § 6.2.4 is explicit:
> "the presence of a JSON-LD Link header in the incoming HTTP request
>  when the Content-Type header is application/ld+json shall result
>  in an HTTP error response of type BadRequestData."

So broker behaviour is mandatory.

**Our call:** broker stays spec-strict — both "ld+json without body
@context" and "ld+json + Link header" raise 400.

**Fix wanted:** the test framework's POST keywords should pick ONE
of two consistent shapes:
  - Content-Type `application/json` + Link header (no body @context); or
  - Content-Type `application/ld+json` + body @context (no Link header).
Currently they emit a hybrid that the spec mandates rejecting.


## 22. DistOps — HttpCtrl stub literal-match doesn't match forwarded URL

**Hit:** `Set Stub Reply <method> <url> <status> <body>` registers an
HttpCtrl stub keyed on the URL string `/broker1/ngsi-ld/v1/...` (or
similar). The broker, when forwarding a distributed operation to the
mocked Context Source, appends URL parameters mandated by TS 104-175 § 10.4.3 /
TS 104-175 § 10.2.x — typically `?type=<expanded>&pick=<expanded>&sysAttrs=true`,
plus `id=` lists for queries. HttpCtrl matches stubs by exact URL
string, so the request misses the stub. The mock then either:
  - returns nothing (timeout → 7 tests fail with `Timeout: request
    was not received`), or
  - returns a default 404/empty (broker reports CS forward failed →
    207 instead of 204, 502 instead of 200 — ~30+ tests).

**Spec:** TS 104-175 § 10.4.3 entitles the broker to attach the matching CSR's
property/relationship constraints as query params on the forward
(`pick`, `omit`, `type`, etc.). TS 104-175 § 5.3.2 / TS 104-175 § 10.4.2 say sysAttrs and
similar shall be honoured end-to-end. Broker is correct.

**Our call:** broker emits canonical distop URLs.

**Fix wanted:** stubs should match by URL prefix or use a regex.
Robotframework-httpctrl supports `Set Stub Reply` with regex match
in newer versions — adopting that across the DistOps suite would
flip ~30 tests without touching the broker.


## 23. DistOps — `Get Stub Count 1 (integer) != 1 (string)` (~12 tests)

**Hit:** Robot tests do `Should Be Equal ${stub_count} 1`.
HttpCtrl's `Get Stub Count` returns a Python int; the literal `1`
in the .robot file is a string. Robot's `Should Be Equal` is type-
strict and fails.

**Spec:** N/A.

**Our call:** broker uninvolved.

**Fix wanted:** use `Should Be Equal As Numbers` / `Should Be Equal
As Integers`, or `Should Be True ${stub_count} == 1`.


## 24. DistOps — `No keyword with name 'Get Request Url Params' found`

**Hit:** three tests reference `Get Request Url Params`, a keyword
that doesn't exist in the version of robotframework-httpctrl
installed for the test runner. Tests crash before the broker ever
sees a request.

**Our call:** broker uninvolved.

**Fix wanted:** pin a robotframework-httpctrl version that exposes
this keyword, or rewrite the affected tests with what the installed
version offers.


## 25. TS 104-175 § 12.3.3.5 / 037_08, 037_09_*, 037_10_02 — ETSI fixtures expect the un-filtered RegistrationInfo set

**Hit:** Eight `GET /csourceRegistrations?…` tests (037_05_01,
037_08_01, 037_09_01..04, 037_10_02, 037_11_*) assert against an
expectation file that contains *every* `information[]` entry of the
created CSR, regardless of whether the entry's `entities` / property
names matched the discovery filter.

The broker today returns only the matching entries (i.e. `entry.entities
∩ ?type/?id/?attrs ≠ ∅`), so the response is a subset of what the
fixture lists → `Compare Dictionaries Ignoring Keys` reports the
missing entries as `Item …['information'][N] removed from iterable`.

**Spec:** TS 104-175 § 12.3.3.5 says implementations **should** return filtered
registrations — only matching RegistrationInfo elements. "Should",
not "shall" — so both shapes are spec-compliant.

**Our call:** keep the filtered behaviour by default — it's the
spec-recommended thing and a cleaner client experience (no irrelevant
entries returned). Provide `--testConformance/-tc` so ETSI runs can
opt into the un-filtered shape and pass these eight tests.

**Fix wanted:** ETSI fixtures should accept either shape, or — better
— the spec should add a URL param to let the client choose (see
spec-doubts § 26 for the proposal).


## 26. TS 104-175 § 12.3.3.4 / 037_10_01 — three separate bugs in one test (one was on broker side, two on fixture side)

The test calls `GET /csourceRegistrations?id=<csr1>,<csr3>` and expects 200 + two specific CSRs.

### 26a. Too-wide rejection on `?id=` alone — RESOLVED broker-side

TS 104-175 § 12.3.3.4 lists `type / attrs / q / geoQ` as the required sufficient selectors. `id` and `idPattern` are not in that list verbatim, but they bound the candidate set at least as tightly. Broker now accepts `?id=` and `?idPattern=` as sufficient selectors for CSR Discovery, mirroring the same relaxation already applied to `/entities`. Match function handles comma-separated `?id=A,B` as OR-of-any.

Spec gap still worth raising with ETSI TC DATA — TS 104-175 § 12.3.3.4 should mention id / idPattern.

### 26b. `?id=` semantic — fixture confuses CSR's own id with EntityInfo.id

Per TS 104-175 § 12.3.3, `?id` filters on `EntityInfo.id` (entities the CSR claims to know about), **not** on the CSR's own identifier. The CSRs in this test are created with `entities: [{ "type": "Building" }]` — no `id` field — so the EntityInfo-id constraint is "any". Filtering on `?id=urn:ngsi-ld:ContextSourceRegistration:X` doesn't narrow anything; the broker returns all CSRs with matching information.

The test author has mistaken the EntityInfo-id filter for a CSR-id filter. Either:
- the fixture should query by something the EntityInfo actually has (a specific entity id, a type), or
- spec should be amended to also support a filter on the CSR's own id (proposal-worthy — convenient for fetch-by-id when caller has discovered them earlier).

Broker is spec-correct here.

### 26c. Expected body is malformed — template never substituted

The expectation file looks like Robot string-formatted `${first_id},${third_id}` straight into a JSON dict key, producing one key `"urn:ngsi-ld:CSR:A,urn:ngsi-ld:CSR:B"` (with literal comma) instead of two keys. Adjacent entries also show unfilled `urn:ngsi-ld:ContextSourceRegistration:randomUUID` placeholders. **No broker can pass this body assertion** — the fixture is shipped broken.

**Fix wanted:** regenerate the expectation file with proper id substitution; once that's done, fix 26b by changing the query to a meaningful filter (e.g. `?type=Building`).


## 27. TS 104-175 § 7.4 / 037_11_01 + 037_11_02 — pagination expects offset to index page-by-page, not item-by-item

**Hit:** Setup creates 3 CSRs with two different fixtures; `?type=Building` matches 2 of them. The two tests then query with limit/offset:

- 037_11_01: `limit=1, offset=2` expects **1** result.
- 037_11_02: `limit=2, offset=2` expects **1** result.
- 037_11_03: `limit=15, offset=0` expects **2** results — passing.

The 03 fixture confirms the matching-set size is 2. With TS 104-175 § 7.4's zero-based item offset:
- offset=2, limit=1 → skip 2 of 2 items → 0 results.
- offset=2, limit=2 → same → 0 results.

For the tests to expect 1, the fixtures must either (a) count all three CSRs as matching `?type=Building` (one of them has no Building EntityInfo so this is wrong), or (b) interpret offset as a page index multiplied by limit (which contradicts TS 104-175 § 7.4).

**Our call:** broker is spec-strict on offset.

**Fix wanted:** rewrite the fixtures with an offset and matching-set count that line up, or — if 037_11_03's "expects 2" is actually correct — pick offset values that don't exceed the matching-set size.


## 28. TS 104-175 § 10.4.3.4 / D011_03_inc_01/02 + D011_04_inc_01/02 — `?id=urn:…` alone rejected as "too wide"

**Hit:** Four distributed-query tests call `GET /entities?id=urn:ngsi-ld:Vehicle:<uuid>` (and the queryBatch POST equivalent) with no `type`, `attrs`, `q`, geoQ, `scopeQ`, or `local=true`. Expects 200 with the entity from the matching CSR.

**Spec:** TS 104-175 § 10.4.3.4 enumerates exactly five sufficient selectors:
- (a) selector of Entity Types
- (b) list of Attribute names with at least one non-system Attribute
- (c) NGSI-LD Query with at least one non-system Attribute
- (d) NGSI-LD GeoQuery
- (e) local scope

`id` and `idPattern` are NOT in that list. The broker rejects 400 "too wide query (TS 104-175 § 10.4.3.4 — id / idPattern alone is too wide)" per `ldParamsValidate.c`.

**Broker:** stays spec-strict — the listing in TS 104-175 § 10.4.3.4 is enumerative ("the following input data shall be provided"), and `id` is conspicuously absent. A `getEntity by id` operation already exists (`GET /entities/{id}` — single entity, no query semantics); the queryEntities path is for filtering, where unbounded id-filter has no use.

**Our call:** broker correct, tests wrong. The CSR Discovery variant of this (§ 26a) WAS relaxed because there the `id` filter applies to EntityInfo.id, not to the resource being listed — different semantics.

**Fix wanted:** the four tests should each add a `&type=Vehicle` or use `GET /entities/{id}` directly. Either passes a sufficient selector while still exercising the CSR forward.


## 29. TS 104-175 § 7.4 / 041_03_01..03 — `?page=` is not a NGSI-LD pagination parameter

**Status:** RESOLVED — fixed in this MR.

**Hit:** Tests 041_03_01, 041_03_02, 041_03_03 query CSR subscriptions with `GET /csourceSubscriptions?limit=1&page=2` (and `page=3`). Expect 200 with the entity at that page.

**Spec:** TS 104-175 § 7.4 / TS 104-176 § 6.4.6 define pagination via `limit` (page size) and `offset` (zero-based item index). There is no `page` URL parameter anywhere in the NGSI-LD 1.9.1 spec — pagination is item-index-based, not page-number-based.

**Broker:** rejects 400 InvalidRequest "Unknown/unsupported URL parameter: page". Correct per TS 104-176 § 6.2.2 (unknown URL params must be rejected for body-bearing endpoints; for read endpoints the broker is also strict, which matches TS 104-175 § 7.4's enumerated list).

**Fix wanted:** the test fixtures should use `offset=2` / `offset=3` / etc. — actual NGSI-LD pagination — instead of `page=N`.


## 30. D001_04_inc — Robot keyword typo `Generate Random Vehice Id`

**Status:** RESOLVED — fixed upstream in MR !263.

**Hit:** Test D001_04_inc setup calls `Generate Random Vehice Id`. Robot reports: `No keyword with name 'Generate Random Vehice Id' found. Did you mean: 'Generate Random Vehicle Entity Id'`.

**Spec:** N/A — pure typo.

**Fix wanted:** rename the call to `Generate Random Vehicle Entity Id` (the actual keyword in `Common.resource`).


## 31. D001_02_exc / D002_02_exc / D012_01_exc — HttpCtrl `OSError` starting mock server

**Hit:** Setup fails with bare `OSError`. The traceback (in the log.html) points at `HttpCtrl.Server.Start Server` — likely the mock can't bind its port (timing / leftover socket from a previous test).

**Spec:** N/A.

**Fix wanted:** make `Start Context Source Mock Server` resilient to bind failures (retry, or rebind on a different port). Today a single port-clash in any test cascades the entire setup. Same root cause as §22 (HttpCtrl mock fragility).


## 32. 047_02_01 — fixture assertion compares two different CSR ids

**Hit:** Setup-phase `Should Contain` checks that one CSR id list contains another id; they're unrelated (`[A]` should contain `B`). The setup fails immediately. The test is in ContextSourceRegistrationSubscriptionNotificationBehaviour, which has multiple Setup keywords that each generate fresh CSR ids — the assertion appears to use the wrong variable.

**Spec:** N/A — fixture variable bug.

**Fix wanted:** trace the variable indirection in the setup keyword chain and align the asserted ids.


## 33. 053_06_01 — fixture sends path-only context id `/api/v1/context.jsonld`

**Status:** RESOLVED — fixed upstream in MR !263.

**Hit:** Setup retrieves an implicitly-created context via path `/api/v1/context.jsonld` (no scheme, no host). Broker (correctly per TS 104-175 § 13) rejects with 400 BadRequestData "context id '/api/v1/context.jsonld' is not a valid URI". Fixture expects 200.

**Spec:** TS 104-175 § 13 mandates context ids be URIs (scheme://...). A bare path is not a URI.

**Fix wanted:** the fixture should generate the full URL (broker base + path) before issuing the GET.


## 34. 019_11_* — fixture polygon is self-intersecting per S2 geometry

**Hit:** Suite setup of `QueryEntities.019 11` creates an entity with a
5-vertex polygon whose `BC` and `EA` edges cross near vertex `A`. Mongo's
2dsphere index (which uses S2 geometry) rejects with "Can't extract geo
keys"; the entity creation returns 400 (was 500 before §34's broker fix)
and all 8 sub-tests in the suite cascade-fail.

Polygon coordinates: `[[13.2865906,52.5648645], [13.2879639,52.5648645],
[13.2797241,52.4988679], [13.477478,52.4712703], [13.5049438,52.5373084],
[13.2865906,52.5648645]]`. The first two vertices are essentially collinear
at y=52.5648645, then BC goes far SW and EA (the closing edge) cuts back
across BC just before reaching A.

**Spec:** TS 104-175 § 5.2.6.11.1 says polygons "should" not be self-intersecting (SHOULD,
not MUST), so technically valid input — but any storage layer using S2
(which most do) will refuse to index it.

**Fix wanted:** pick a different polygon for these geo-query tests, or
explicitly state the polygon contract is "S2-valid GeoJSON". Until then
the seven 019_11_* sub-tests all fail at suite-setup.


## 35. 046_24_01 / 046_28_01 — fixture compares createdAt/modifiedAt as literals

**Hit:** Notification-with-linked-entity tests compare the broker's
response against an expectation file containing hardcoded
`"createdAt": "2025-05-19T15:20:16.418984Z"` / `"modifiedAt": "..."`. Since
the test creates the entity at test-run time, the broker's actual timestamps
will always differ — both at the entity level and inside the embedded
`name`/`locatedAt` attributes.

**Spec:** N/A.

**Fix wanted:** the expectation file should mark these fields as
"any value" / regex, or the assertion utility should skip them.


## 36. 038_01_01 / 039_01_01 / 040_01_01 — CSR-Subscription fixtures
don't expect notificationTrigger or notification stats

**Hit:** Create/Update/Retrieve of a CSR-Subscription. ETSI expectation
for the retrieved sub:
- `isActive: true` present (broker now suppresses defaults; 028_06 expects suppression)
- No `notificationTrigger` (broker now default-emits per TS 104-175 § 5.2.6.5.2)
- No `notification.timesSent`/`timesFailed`/etc. (broker emits stats per
  TS 104-175 § 5.2.6.7.3)

028_06 (regular Subscription) expects the OPPOSITE: no isActive, yes
notificationTrigger default. So ETSI's own fixtures disagree on the
spec-default-emission rules across the two endpoints.

**Spec:** TS 104-175 § 5.2.6.5.2 — defaults aren't supposed to be persisted as user
fields; TS 104-175 § 5.2.6.7.3 — notification counters are part of the Subscription
representation.

**Fix wanted:** decide and apply consistently across all subscription
retrieve fixtures (regular + CSR). Both endpoints share the Subscription
resource type, so behaviour shouldn't diverge.


## 37. Robot teardowns clean current-state but NOT temporal history

**Hit:** Run-to-run variance of ±10–15 tests on the full ETSI suite,
all concentrated in the **021_* (temporal)** cluster, all showing as
`200 != 206` from the broker. Cherry-picking a single failing
021_* test in isolation makes it pass — the failure only reproduces
inside the full suite run.

**Why:** every `Test Teardown` in the ETSI Robot suite does
`DELETE /ngsi-ld/v1/entities/{id}` (current-state cleanup) but does
NOT also `DELETE /ngsi-ld/v1/temporal/entities/{id}` (history
cleanup). TRoE rows from every prior test linger for the entire run.
By the time the temporal tests run, the per-entity attribute-instance
history (across all tests sharing the broker's timescale DB) exceeds
the broker's `-troeCap` (default 100, TS 104-176 § 6.4.7). The TRoE driver
correctly returns 206 + Content-Range; the test expected 200.

**Spec:** TS 104-176 § 6.4.7 — 206 + Content-Range is the right answer for a
truncated temporal result. Spec-correct broker, fixture-side problem.

**Why this matters beyond the count drift:** the suite is no longer
order-independent. Cherry-picking a single 021_* test to debug a
failure works (clean DB → fits under the cap → passes), but the
exact same test fails when run inside the suite (cap blown by prior
fixtures). That makes triage of any temporal failure unnecessarily
expensive.

**Fix wanted:** add `DELETE /ngsi-ld/v1/temporal/entities/{entity_id}`
to every test teardown that creates temporal data, paired with the
existing entity-delete call. The broker fully supports the temporal
DELETE; teardown order doesn't matter (current and temporal are
independent stores).

**Workaround on the broker side:** start the broker with
`-troeCap 100000` (or larger) for ETSI baseline runs — pushes the
cap above what any reasonable accumulation hits. We're not enabling
this by default in our broker because the spec-correct cap is part
of what the suite ought to exercise.

## 38. 041_04_02 / 041_04_03 — fixture uses non-spec `?page=` param

**Test:**
[041_04.robot](https://forge.etsi.org/rep/cim/ngsi-ld-test-suite/-/blob/develop/TP/NGSI-LD/ContextSource/RegistrationSubscription/QueryContextSourceRegistrationSubscriptions/041_04.robot) — "Check that one cannot query context source registration subscriptions with invalid page and limit parameters".

```
041_04_01 Invalid Limit            limit=-5   page=2
041_04_02 Invalid Page             limit=2    page=-3
041_04_03 Invalid Limit And Page   limit=0    page=0
```

All three assert `400 BadRequestData`.

**Broker:** returns `400 InvalidRequest` for the two cases that
include `?page=` (test names ending _02 and _03), because `page` is
not a registered URL parameter and the broker correctly classifies an
unknown URL parameter as InvalidRequest per TS 104-176 § 6.3.6.

**Spec:** NGSI-LD defines two pagination parameters — **`limit`**
(TS 104-176 § 6.4.7.2 Table "Pagination: limit parameter") and **`offset`** (TS 104-176 § 6.4.7.2 Table "Pagination: offset parameter").
There is no `page` parameter. TS 104-176 § 6.3.6: "If an HTTP request for an
operation contains parameters that are incompatible with the
operation … an HTTP error response of type InvalidRequest should be
returned." So `?page=…` is unknown → InvalidRequest, not
BadRequestData.

**Fix wanted:** rewrite the fixture to exercise only spec-defined
pagination parameters. The intent ("reject negative pagination") is
correct and worth keeping, but it should use `offset=-3` (the actual
NGSI-LD analogue of "page") to validate the broker's value-range
rejection on a known parameter, which IS BadRequestData per TS 104-175 § 7.4.
Alternatively, the test can keep `?page=` and reclassify its expected
error to InvalidRequest.

**Note:** 041_04_01 uses only `limit=-5` (no `page=`) and expects
BadRequestData. That's consistent with the spec and our broker
returns BadRequestData for it.

## 39. 059_01_* + 007_02_02 + 015_01_01 (AddAttribute) + 050_02_02 — BadRequestData vs InvalidRequest split

**Test pattern:** ETSI tests across several sections assert
**InvalidRequest** for what TS 104-175 § 8.2.3 / TS 104-176 § 6.3.6 classify as
syntactic/transport errors (as opposed to semantic errors in a
parsed NGSI-LD payload, which are BadRequestData):

- **059_01_xx** — `?invalidParams=invalidValue` on every endpoint
  family. TS 104-176 § 6.3.6: unknown URL param → InvalidRequest.
- **007_02_02 Create A Temporal Entity With An Empty Json** —
  empty request body. TS 104-175 § 8.2.3: "not a valid JSON document" →
  InvalidRequest.
- **015_01_01 (setup) Append Attribute With Empty Body** — same as
  above.
- **050_02_02 Checking Wrong JSON** — POST `/jsonldContexts` with
  malformed JSON. TS 104-175 § 8.2.3 again — not a valid JSON document.
- **050_02_01 Checking Incorrect Payload** — POST
  `/jsonldContexts` with a JSON object whose shape is wrong
  (missing required key). Our broker returns InvalidRequest here
  because the payload is rejected at the syntactic-shape gate
  ("payload must be a JSON object") *before* any semantic
  @context-content validation. The test agrees.

No fixture changes needed — this entry exists so future readers
understand the rationale for the broker behaviour. The
failures for these tests cleared after
the broker stopped over-using BadRequestData on transport-layer
issues.

## 40. HttpCtrl mock — `Set Stub Reply` URL is exact-match (path + query string)

**Library:** `robotframework-httpctrl` (used by all DistOps tests via
`resources/MockServerUtils.resource` → `Start Context Source Mock Server`).

**Behaviour:** in `HttpCtrl/http_stub.py::HttpStubCriteria.__eq__` the
stub is matched against an incoming request only when both `method`
*and* `url` are case-insensitively equal. The "url" is the request's
raw target — path **and** query string, no normalisation. A stub
registered as `Set Stub Reply  POST  /a/b/attrs/` will *not* match a
request to `/a/b/attrs` (missing trailing slash) or
`/a/b/attrs/?sysAttrs=true` (extra query string).

**Concrete impact on the swBroker DistOps tests:**

1. **Trailing-slash mismatch on the attribute-list endpoint.** TS 104-176 § 7.4.3
   TS 104-176 § 7.4.2 (Table "URI variables") shows the URI template as `/entities/{entityId}/attrs/`
   (with trailing slash).

   Many tests historically stubbed the trailing-slash form;
   our broker used to send `/attrs` (no slash) on forwarded
   requests and broke them:
   D003_01_red, D004_01_red, D006_02_exc, D014_01_red, D014_02_red.
   **Worked around** on the broker side (postEntityAttrs /
   patchEntityAttrs / postEntityTemporalAttrs now emit the
   trailing slash to match the spec).

   The opposite case also occurs: tests that stub
   `…/attrs` WITHOUT the trailing slash — these miss now
   that the broker forwards the spec-correct `…/attrs/`.
   Affects D004_01_inc (`Set Stub Reply PATCH
   /ngsi-ld/v1/entities/<id>/attrs 204`). Broker forwards
   `…/attrs/` per TS 104-176 § 7.4.3, HttpCtrl misses, forward fails,
   `anyCsrSucceeded` stays false, entity not found locally
   → 404 instead of the test-expected 207. The fix is on
   the test side: align every stub with the spec-template
   trailing slash.

   In every case the underlying HttpCtrl strict-match bug
   stays.

   **Test-suite-side ask:** stubs (and the matching `Get
   Stub Count` calls) should accept both `…/attrs` and
   `…/attrs/` — the spec wording is ambiguous enough that
   brokers MAY emit either. Personal preference: drop the
   trailing slash everywhere (terser, matches the rest of
   the attribute-level URI templates in TS 104-176 § 7.4).

2. **Forward URL carries `?sysAttrs=true` and `&type=…`.** For
   retrieveEntity through CSRs the broker has to ask the upstream for
   sysAttrs (createdAt / modifiedAt are needed at the merge tiebreaker
   per TS 104-175 § 8.5.3) and the entity type is added when the CSR's
   RegistrationInfo specifies one. Both are correct per TS 104-175 § 10.4.2 / §
   4.3.6.3. The ETSI stubs are registered with `Set Stub Reply  GET
   /ngsi-ld/v1/entities/{id}  200  …` — no query string — so the
   stub never matches the broker's request.

   This affects:
     * D010_01_inc, D010_01_red (single-CSR retrieve —
       inclusive / redirect)
     * D010_01_exc (exclusive retrieve where the local
       entity is missing the attrs the CSR owns — when
       the forward fails the broker can't satisfy the
       request and returns 502 instead of 200; visible
       in the test as `200 != 502`)
     * D010_03_inc_01..03 (chained retrieve)

   **Not worked around** — dropping `sysAttrs=true` from the forward
   would silently break the spec-mandated merge for multi-source
   reads. The fix belongs on the testsuite side.

3. **Forward URL carries an `options=` / similar query for
   write ops.** Several spec clauses require propagating the
   original `options=` (and `deleteAll=`, etc.) URL param to
   the upstream CP so the same mode applies on both legs.
   ETSI stubs at the bare path miss HttpCtrl's strict match
   → broker sees a transport-level fail (Request timeout or
   immediate Connection closed) and rolls up to 207 with a
   Bad-Gateway entry per entity.

   This affects:
     * D013_02_exc, D013_02_inc — POST upsertBatch with
       `?options=update` (TS 104-175 § 10.3.4.4)
     * D013_01_inc — POST upsertBatch with `?options=replace`
       (the default mode)
     * D003_02_red — POST appendAttrs with
       `?options=noOverwrite` (TS 104-175 § 10.2.4.4). Broker correctly
       forwards `/attrs/?options=noOverwrite`; stub at the
       bare URL misses. Visible in the `KtDistOpRequest=400`
       trace.
     * D006_02_inc — DELETE attrs with `?deleteAll=true`
       (TS 104-175 § 10.2.7.4). Broker forwards
       `/attrs/speed?deleteAll=true`; stub at the bare URL
       misses. Surfaces in the 207 body's
       `error.detail="forward failed: Request timeout"`.

   Confirmed end-to-end with a Python mock that ignores the
   query string: broker returns the test-expected status.
   Only HttpCtrl's strict match breaks these.

**Fix wanted upstream:** either
  (a) loosen `HttpStubCriteria.__eq__` to compare just the path
      (with optional `?…` glob support), or
  (b) update every affected `Set Stub Reply` to register both the
      canonical URL and a `?sysAttrs=true` variant (cumbersome), or
  (c) switch tests that need precise URL matching to `Wait For
      Request` + manual reply instead of the stub mechanism.



## 41. `020_14_01/02` — default temporal page size is not in the spec

**Hit:** 60-instance fixture (`speed` Jan 1, `fuelLevel` Jan 2)
queried with `timerel=after&timeAt=2019-01-01`. Test expects
`fuelLevel` to come back empty — the implicit assumption is the
broker applies a default pagination on temporal retrieves that
sorts the union of all instances by `observedAt` and truncates
at some threshold < 118.

**Spec:** TS 104-176 § 6.4.7 doesn't pin a default temporal page size. swBroker
returns all 118 instances → the `Check Data Is Empty` assertion on
`fuelLevel` fails.

**Why it's wrong:** two defensible interpretations exist; the
fixture is committed to one without spec backing.

**Fix wanted:** either nail down a default page size in the spec,
or rewrite the test to assert pagination via an explicit `lastN=`.


## 42. `021_25` — expectation file missing one of the entities

**Hit:** `Setup Initial Temporal Entities` creates `Vehicle:021-06-A`
and `Vehicle:021-06-B`. The query `?local=true&timerel=after&timeAt=
2020-07-01` matches both. Expectation file
`data/temporalEntities/expectations/vehicles-temporal-representation-021-06.jsonld`
contains only entity A.

Test is tagged `not-implemented` already, so the author knew it
was WIP. Broker correctly returns both → deepdiff complains
`Vehicle:021-06-B added to dictionary`.

**Fix wanted:** add entity B to the expectation file (or drop the
test's `not-implemented` tag once the fixture is repaired).


## 43. `047_03/04/08/09/16` — CSR-sub vs CSR fixture entity-type mismatch

**Hit:** the CSR-subscription notification tests pair
`csourceSubscriptions/subscription.jsonld` (`entities: [type:
Building]`) with `csourceRegistrations/context-source-registration.jsonld`
(`entities: [type: Vehicle], [type: OffStreetParking]`). The CSR
doesn't overlap with the sub's entity scope, so per TS 104-175 § 12.4.7 no
`newlyMatching` notification should fire.

**Broker:** correctly fires nothing in isolation → test times out
("Timeout: request was not received"). In the full suite the test
sometimes "succeeds" structurally because a Building CSR created by
another test lingers in the regCache and its id ends up in the
notification body, then deepdiff complains `<old-id> does not
contain <expected-new-id>`.

**Fix wanted:** rework the fixture pair so the CSR entities match
the subscription's `entities[type:Building]` (or drop the `entities`
filter from the sub if the intent is "any new CSR").


## 44. `038_08_03 InvalidQuery` — bare attribute name is a valid q

**Hit:** fixture `csourceSubscriptions/subscription-invalid-query.jsonld`
sets `q: "invalidQuery"` and the test asserts 400 BadRequestData.

**Spec:** TS 104-175 § 7.2.3 — a bare attribute name in q is the "attribute
exists" predicate. Perfectly valid.

**Broker:** correctly returns 201.

**Fix wanted:** rewrite the fixture's `q` to something genuinely
invalid (mismatched parentheses, unsupported operator, etc.), or
drop the test.


## 45. `041_01_01 / 041_02_03 / 038_02_01` — fixture omits `notificationTrigger` (API-version drift)

**Hit:** CSR-subscription create/retrieve fixtures omit
`notificationTrigger` in both the POSTed body and the retrieval
expectation file.

**Spec:** `notificationTrigger` is a TS 104-175 § 5.2.6.5.2 field that appears
in newer NGSI-LD spec revisions (post-1.6.1). When omitted, the
default is `["attributeCreated", "attributeUpdated"]`. Brokers
surface that default on retrieve so the user sees what's in force.

**Broker:** does that — adds `notificationTrigger:
["attributeCreated","attributeUpdated"]` on retrieve when the
user didn't provide one. The deepdiff fails with
`'notificationTrigger' added to dictionary`.

**Why it's a doubt:** the test fixtures look authored against an
older spec revision (1.6.1 or earlier) where the field didn't
exist. Newer-broker correctness collides with older-test
expectations.

**Fix wanted:** update the affected fixtures + expectations to
the current spec, OR drop deep-diff in favour of asserting only
user-set fields.


## 46. `002_02_01 / 054_02_02 / 056_02_02` — confused expectations for verbs on `/entities/`

**Hit:** three fixtures hit the entity collection endpoint with a
write verb and no id, each expecting a different status:

- `002_02_01` — `DELETE /entities/` — expects **405**
- `054_02_02` — `PUT /entities/`    — expects **400**
- `056_02_02` — `PATCH /entities/`  — expects **400**

**Spec:**
- `DELETE /entities` IS a real endpoint (TS 104-176 § 7.2.3.3 — Purge
  Entities). Purge was added post-1.6.1, so the test fixture
  (likely authored against pre-purge spec) treats it as
  non-existent and expects 405. The broker correctly handles
  purge and returns 400 with "requires at least one of
  id/type/idPattern/q/geoquery/scopeQ or ?local=true" — the user
  supplied no filter.
- `PUT /entities` and `PATCH /entities` are NOT defined on the
  collection — only on the individual `/entities/{id}` resource.
  HTTP convention says 405 Method Not Allowed.

**Broker:** consistent and correct on all three (400 for purge,
405 for the two undefined verbs).

**Fix wanted:** the three fixtures need consistent + spec-aware
expectations:
- `002_02_01`: change to 400 (purge endpoint, missing filter) —
  OR keep at 405 and add a filter so the response really IS 405.
- `054_02_02`, `056_02_02`: change to 405.


## 47. `016_02_06` — PATCH temporal attr instance with empty attr name

**Hit:** `PATCH /temporal/entities/{id}/attrs//{instanceId}` (note
the empty attr-name segment). Test asserts **405**.

**Broker:** routes the request via wildcard matching to the
partial-update-temporal-attr handler, validates the empty attr
name as a TS 104-175 § 5.2.2.3 violation, returns **400** "invalid attribute
name '' (TS 104-175 § 5.2.2.3)".

**Why it's a doubt:** both interpretations are defensible — 405
from a strict router perspective (URL doesn't match a valid
route shape), 400 from a "URL matched, but the name slot is
invalid" perspective. 400 is more diagnostic.

**Fix wanted:** decide on a convention and update the test or
the broker. Same logic should also apply to other
attrs/{empty}/... routes (016_02_05 is the same shape but uses
`invalid(Id` instead of empty and asserts 400 — broker matches).


## 48. `034_05_01` — fixture uses real JSON null to unset `expiresAt`

**Hit:** `PATCH /csourceRegistrations/{id}` with body
`{ "expiresAt": null }` (real JSON null). Test expects **204**
and afterwards `expiresAt` to be gone from the registration.

**Where:**
`data/csourceRegistrations/fragments/context-source-registration-null-expiresAt.json`.

**Spec:** TS 104-175 § 5.2.6.4.8 introduces the explicit sentinel
`"urn:ngsi-ld:null"` to mean "delete this member" in a merge
patch. Real JSON null is NOT the agreed signal for that — and
§ 7.4.4 of JSON-LD (Expansion algorithm) explicitly drops
members whose value is `null` *before* the request body is
interpreted. So a JSON-LD-conformant pre-processor sees
**no** `expiresAt` member at all — the merge becomes a no-op.

**Broker:** rejects with **400** "'expiresAt' must be a DateTime
string". Correct per TS 104-175 § 5.2.6.4.8 — null is not a valid
DateTime, and the sentinel `"urn:ngsi-ld:null"` was designed
precisely to make this case unambiguous.

**Fix wanted:** the fixture should send
`{ "expiresAt": "urn:ngsi-ld:null" }` (with `Content-Type:
application/json` — the sentinel survives JSON-LD expansion
because it is a string, not null). The broker is correct.


## 49. `001_05_02 / 003_04_02 / 003_05_02 / 010_04_01 / 033_05_01` — fixtures key on the expanded IRI in plain-JSON responses

**Hit:** the test code reads response keys with the fully-
expanded JSON-LD IRI:
```python
${response_body['ngsi-ld:default-context/almostFull']}
${response_body['https://ngsi-ld-test-suite/context#almostFull']}
${response_body['ngsi-ld:default-context/attribute_to_be_added']}
${response_body['ngsi-ld:default-context/Building']}
```
Each fails with `KeyError` because the broker compacts the
response back to short names (`almostFull`, `Building`) as
spec wants.

**Spec:** TS 104-176 § 6.2.4 — `application/json` responses are compacted
using the user @context. Short names are correct on the wire.

**Broker:** correct (compacts).

**Fix wanted:** Robot tests should access
`${response_body['almostFull']}`, `Building`, etc. — i.e.,
the short names that JSON-LD compaction produces.


## 50. `020_05_02 / 020_13_06..09 / 021_15_05..07` — temporal 206 vs 200

**Hit:** broker emits **206 Partial Content** + `Content-Range`
when the temporal slice exceeds the per-entity instance cap
(`--troeInstanceCap`, default 20 per TS 104-176 § 6.4.7). Tests expect
**200** in cases where the slice still fits, OR **206** in
cases where the slice fits but the test predates the cap rule.

The result: each test in this family encodes one specific
interpretation of when 206 should fire, but the broker (using
a uniform "instance count > cap" rule) crosses the test's
expectation at different points.

**Spec:** TS 104-176 § 6.4.7 / TS 104-176 § 6.2.4 — the broker SHALL emit 206 +
Content-Range when an entity's instance count exceeds the
configured pageSize. The configured pageSize is broker-
dependent; the test suite hard-codes one specific value.

**Broker:** correct under its own configured cap. Same behaviour
as doubt #41 (`020_14_01/02` — default temporal page size not
in the spec).

**Fix wanted:** either the test suite needs a documented
expected-cap that brokers honour, or the assertion accepts
both 200 and 206 + Content-Range for these cases.


## 51. `054_01_01 Replace An Existing Entity` — modifiedAt jitter on exact-string compare

**Hit:** the assertion compares `modifiedAt` strings byte-for-
byte:
```
2026-05-20T15:22:04.844856025Z != 2026-05-20T15:22:04.823397351Z
```
The two timestamps differ by ~20 ms because the broker records
`modifiedAt` at the moment of write, then the test reads back
and compares to a fixture timestamp generated at a different
moment.

**Spec:** TS 104-175 § 5.2.6.10.5 — `modifiedAt` is broker-assigned; the
client cannot predict its exact value.

**Broker:** correct (writes its own timestamp).

**Fix wanted:** the test should compare with tolerance (e.g.
ignore `modifiedAt` and other server-side timestamps in the
deep-diff), or assert "is a valid DateTime, is close to now".


## 52. `047_03_01 / 047_04_01 / 047_08_01 / 047_09_01 / 047_16_01` — hardcoded CSR ids in assertion

**Hit:** these CSR-subscription tests assert against a CSR id
that's hardcoded in the expectation file, e.g.:
```
[ urn:ngsi-ld:ContextSourceRegistration:5902198644784923 ] does not contain value
'urn:ngsi-ld:ContextSourceRegistration:5902198644784923'
```
But the setup generates a fresh random CSR id each run
(`Generate Random CSR Id`). The expectation file is never
re-written with the random id; the comparison can therefore
never match.

**Spec:** N/A — purely a test-fixture wiring issue.

**Broker:** correct (notification fires with the actually-
registered CSR id).

**Fix wanted:** the assertion needs to substitute the
generated CSR id into the expectation (the same trick the
test does for entity ids elsewhere), or the expectation file
needs to be templated.


## 53. `047_05_01 / 047_06_01` — fixture predates `timesFailed` and `status` fields

**Hit:**
```
Item root['timesFailed'] added to dictionary.
Value of root['status'] changed from "ok" to "failed".
```
The broker now emits `timesFailed` (and reflects the success/
failure of the last notification attempt in `status`). Older
expectation files don't list these.

Same shape as doubt #45 (`041_01_01 / 041_02_03 / 038_02_01`
— fixtures omit `notificationTrigger`).

**Broker:** correct (per TS 104-175 § 12.4 the CSR-sub maintains delivery
stats; the spec doesn't forbid surfacing them).

**Fix wanted:** regenerate the expectations against a current-
spec broker.


## 54. `051_04_03` — fixture references an undeclared Robot variable

**Hit:** assertion fails with
```
Variable '${ERROR_TYPE_RESOURCE_NOT_FOUND}' not found.
```
The variable isn't declared in any of the imported `.resource`
files. The test cannot run at all.

**Broker:** never even reached.

**Fix wanted:** declare `${ERROR_TYPE_RESOURCE_NOT_FOUND}` in
`resources/ApiUtils/Common.resource` (the rest of the suite
defines a parallel set of `ERROR_TYPE_*` variables there).


## 55. `051_08_01 / 051_08_02 / 051_09_01` — Delete-Core-@context behaviour is spec-undefined

**Hit:** tests call `DELETE /jsonldContexts/{core-context-id}`
and assert specific status codes (variously 204 / 404 / 400).
The broker treats the core context as immutable and rejects
with 400 ("core context cannot be deleted").

**Spec:** TS 104-176 § 7.3 — does not define what should happen when a
client tries to delete the core JSON-LD @context. The tests
disagree with each other on which status to expect (204 from
one, 404 from another), confirming the spec gap.

**Broker:** consistent (always 400 with a descriptive detail).

**Fix wanted:** spec needs to mandate one of: 400 (delete
forbidden), 405 (method not allowed on the core resource), or
204 + reload-from-disk. Once chosen, the three tests should
all assert the same code.


## 56. `019_11_01..08 / 021_09_02` — fixture polygon is self-intersecting at vertex A

**Hit:** suite setup creates a Building with
`building-location-polygon.jsonld`, whose polygon is:
```
A = (13.2865906, 52.5648645)
B = (13.2879639, 52.5648645)
C = (13.2797241, 52.4988679)
D = (13.477478,  52.4712703)
E = (13.5049438, 52.5373084)
```

A and B share the same latitude and B is only 0.0014° east of
A. Edge BC drops straight down from B while edge EA closes the
polygon back to A — and in the tiny x-interval [13.2866,
13.2880] those two edges cross (BC dips below EA's near-A end,
then the polygon "wraps" around). The broker's planar self-
intersection check (mirroring MongoDB 2dsphere strictness)
correctly returns 400 at setup time, which cascades the entire
019_11_* suite plus the related polygon test 021_09_02.

**Spec:** TS 104-175 § 5.2.6.4.5 — Polygons must be simple (non-self-
intersecting). TS 104-176 § 7.3.3 — bad geometry → 400 BadRequestData.

**Broker:** correct.

**Fix wanted:** replace the fixture polygon with one whose
near-A edge has even modest separation in either lat or lon —
e.g. shifting B east by another ~0.005° eliminates the
self-intersection without changing the test's intent.


## 57. `020_17_0[1-3] / 020_18_0[1-3]` — fixture deletes via the current-state endpoint on a temporal-only entity

**Hit:** the test setup is
```robot
Test Setup    Create Temporal Entity
```
which calls `POST /temporal/entities`. The test body then does
```robot
Delete Entity Attributes
   entityId=${temporal_entity_representation_id}
   attributeId=${attr_name}
```
which expands to `DELETE /entities/{id}/attrs/{name}` — i.e.
the current-state endpoint. Then the test retrieves the
temporal representation with `?timeproperty=deletedAt` and
expects to see the deleted attribute with a `deletedAt`
timestamp and `urn:ngsi-ld:null` as the value-marker.

The broker returns 404 on the DELETE because the entity does
not exist in current state — and so no deletion event is
logged into the temporal store. The follow-up GET returns
`{id, type}` with no attributes, so deep-diff reports the
expected attribute as "removed from dictionary".

**Spec:** TS 104-175 § 10.4.3.1 (Create Temporal Representation):
> "If the corresponding Entity does not already exist in the
>  current state, the Context Broker shall NOT create the
>  Entity in the current state."

So the test's assumption that `POST /temporal/entities` will
populate current state is contrary to the spec.

**Broker:** correct.

**Fix wanted:** the test should either
- create the entity in both stores (call `Create Entity`
  before `Create Temporal Entity`), or
- use `DELETE /temporal/entities/{id}/attrs/{name}` — the
  temporal-side delete that exists for exactly this purpose
  (TS 104-175 § 10.4.3.4). The `Delete Attribute From Temporal Entity`
  keyword is already defined in
  `resources/ApiUtils/TemporalContextInformationProvision.resource`.

**Note:** 020_17_01 and 020_18_01 (the Property subtests)
occasionally pass in the full suite due to cross-test state
leak — when run in isolation, all six subtests fail with the
same fixture root cause.


## 58. `033_01_02` — fixture leaks the previous test's CSR into the next test's `exclusive`-mode conflict check

**Hit:** test setup for 033_01_02 creates an `exclusive`-mode
CSR with the same entity scope (`Vehicle:A456` + `OffStreetParking`
idPatterns) as 033_01_01's earlier (default `inclusive`) CSR.
The suite has `Suite Teardown    Delete Created Context Source
Registrations` but only cleans up the most-recently-saved
`${registration_id}` variable — 033_01_01's CSR stays in the
broker between tests.

Per TS 104-175 § 12.2.2.4: "An exclusive registration shall conflict with
another registration covering the same Entity scope (regardless
of mode)." Two CSRs with overlapping entityInfo, at least one
exclusive → 409 Conflict on the second create.

**Broker:** returns 409 — correct per spec.

**Test:** asserts 201. Fails.

**Verified:** when 033_01_02 is run truly alone (broker DB
wiped, broker restarted), it passes with 201. Failure only
appears once 033_01_01 has populated the broker.

**Fix wanted:** add a `Test Teardown` that deletes the test's
CSR (not just a suite-level teardown for the last one). The
suite has 4 subtests, each creating a separate CSR; only one
gets removed today.


## 59. `047_03_01` — `Check Notification Data Entities` indexes `type` as an array

**Hit:** the helper keyword in `NotificationUtils.resource`:
```robot
FOR    ${registration_information}    IN    @{notification_data_information}
    Append To List    ${notification_data_entities}
    ...    ${registration_information}[entities][0][type][0]
END
```
The expression `[type][0]` takes the first ELEMENT of `type`.
Python string indexing makes `"Building"[0] == "B"`. So when
the broker correctly returns `"type": "Building"` (string),
the test collects `"B"` into the list and the assertion fails
with `Index 0: Building != B`.

**Spec:** TS 104-175 § 5.2.6.6.2 defines `EntityInfo.type` as "an NGSI-LD
attribute name" — singular, scalar string. TS 104-176 § 6.2.4 compaction
returns a single value as a string, not an array.

**Broker:** correct (returns `"type": "Building"`). Verified by
037_05_01 / 037_05_02 (GET /csourceRegistrations) whose
expectation files have `"type": "Vehicle"` as a string and pass.

**Fix wanted:** drop the trailing `[0]` from the assertion:
```robot
Append To List    ${notification_data_entities}
...    ${registration_information}[entities][0][type]
```

**Broker improvements landed alongside this triage:** the CSR-
sub notification now (a) filters `information[]` to only the
entries matching the subscription's entity scope (TS 104-175 § 12.4.7
SHOULD), and (b) compacts using the subscription's @context so
attribute IRIs come back as short names. Both were genuine
broker bugs visible regardless of the assertion typo.


## 60. `008_01_01` — fixture's expected temporal slice matches neither TRoE history nor current-state shape

**Hit:** suite does two consecutive `POST /temporal/entities/`
with the same id:

setup body:
```
speed:     [{val:120, obs:12:03}, {val:80, obs:12:05}]
fuelLevel: [{val:67, obs:12:03}, {val:53, obs:13:05},
            {val:40, obs:14:07, datasetId:"12345-fuel"}]
```
update body:
```
speed:     [{val:121, obs:12:03}, {val:80, obs:12:05},
            {val:100, obs:12:07}]
fuelLevel: [{val:67, obs:12:03}, {val:53, obs:13:05},
            {val:40, obs:14:07}]   ← no datasetId
```
Then `GET /temporal/entities/{id}` and the expectation has
**3 speed + 4 fuelLevel** entries: the update's three speed
instances, and (3 update fuel + 1 setup datasetId-bearing fuel
that wasn't covered by the update).

That matches an implicit dedup-by-(datasetId, observedAt) rule
where the second POST replaces same-key instances and merges
new ones — applied to the temporal layer.

**Spec — two layers, two semantics:**

- **TRoE** (what the endpoint queried by the test serves):
  TS 104-175 § 10.3.6 / TS 104-175 § 11.2.2 say a second `POST /temporal/entities/`
  on an existing entity ADDS the new instances to the history.
  Strict spec reading: the TRoE rows after both POSTs are the
  union — 5 speed + 6 fuelLevel.
- **Current state**: in our broker, `POST /temporal/entities/`
  on an entity that doesn't already exist in current state
  intentionally skips current-state creation (TS 104-175 § 10.4.3.1),
  so a follow-up `GET /entities/{id}` returns 404 here. A
  broker that DID create the entity in current state would, on
  the second POST, REPLACE the no-datasetId instances at the
  current-state layer (because current state stores at most
  one instance per (attr, datasetId)) — landing at exactly the
  3 speed + 4 fuelLevel shape the test expects. But that
  isn't where the test is looking.

**Broker:** spec-correct on the TRoE layer (appends). Returns
5+6 from the temporal endpoint.

**Verdict:** the test fixture conflates the two layers. The
expected shape is the current-state outcome of the upsert, but
the assertion reads from the TRoE endpoint. Either the
expected file should be the union (5+6) — matching what the
temporal endpoint truly produces per spec — or the test should
GET the current-state endpoint (which then also needs a setup
that puts the entity into current state to begin with, since
`POST /temporal/entities/` against a non-existent current
entity skips creation per TS 104-175 § 10.4.3.1).


## 61. `047_16_01 / 047_16_03` — PATCH /csourceSubscriptions has no @context, so the new entity-selector terms expand differently than the CSR's terms

**Hit:**

Setup (in the same suite):
- CSR1 (Vehicle) and CSR2 (Bus) are POSTed with
  `Content-Type: application/ld+json` + an `@context` array
  pointing at the test-suite-compound. The compound's nested
  `test-suite.jsonld` has an explicit mapping
  `"Vehicle" → "https://ngsi-ld-test-suite/context#Vehicle"`,
  so each CSR's `information[0].entities[0].type` is stored
  as `https://ngsi-ld-test-suite/context#Vehicle`.
- The CSR-subscription is initially created the same way
  (entities[].type = `Building`, expanded against
  test-suite-compound to `https://ngsi-ld-test-suite/context#Building`).

Test body:
```robot
PATCH /csourceSubscriptions/{id}    json={"entities":[{"type":"Vehicle"}]}
```
which `requests` sends as `Content-Type: application/json`
with NO Link header. Per TS 104-176 § 6.2.4 the broker resolves `Vehicle`
against the core context's `@vocab` because no @context
information arrives with the request. The PATCH ends up
overwriting `entities[0].type` in the cached subscription as
`https://uri.etsi.org/ngsi-ld/default-context/Vehicle`.

Now the cached subscription's entity-selector IRI is
`uri.etsi.org/ngsi-ld/default-context/Vehicle`, but CSR1's
entityInfo IRI is `ngsi-ld-test-suite/context#Vehicle`. Same
token "Vehicle" but expanded under two different @contexts.
They don't compare equal, the sub doesn't match CSR1, and the
post-PATCH `newlyMatching` notification never fires.

**Why 047_16_02 (Bus) accidentally passes:**

`test-suite.jsonld` defines `Vehicle`, `Building`,
`OffStreetParking` and a handful of attribute terms — but NOT
`Bus`. When CSR2 was POSTed, the broker walked the compound
@context looking for `Bus`, didn't find it, and fell through
to the core `@vocab` of `https://uri.etsi.org/ngsi-ld/default-context/`.
So CSR2.entityInfo[0].type was stored as
`https://uri.etsi.org/ngsi-ld/default-context/Bus` — exactly
the same IRI the PATCH later produces for `Bus`.

047_16_01 (Vehicle) hits the asymmetry; 047_16_03 covers both
Vehicle and Bus and fires the notification with only the Bus
half (`data[1]` missing).

**Spec:** broker behaviour is correct for the wire it sees. The
PATCH carries no @context, so `Vehicle` is a locally undefined
term and the broker's resolution against core `@vocab` is the
only spec-aware option.

**Fix wanted:** the test PATCH should send the same @context
that created the resources — either `Content-Type:
application/ld+json` with the `@context` array in the body, or
`Content-Type: application/json` with a Link header naming the
test-suite-compound. Without one of those, `Vehicle` is
locally undefined and the broker has nothing to align it
against.

**Broker improvement idea (future work):** on PATCH of an
existing resource with a stored `_jcResolved`/`jsonldContext`,
fall back to the stored @context if the request omits one.
That would mask this fixture bug, but it changes semantics the
spec doesn't actually mandate, and would also need an opt-out
for clients that intentionally want default-vocab semantics.
Not pursuing for now.


## 62. `020_14_01 / 020_14_02` — "cut at attribute boundary" for temporal pagination is broker-specific

**Hit:** the fixture has 59 `speed` instances on
`2020-01-01T01:01..01:59` and 59 `fuelLevel` instances on
`2020-01-02T01:01..01:59` (the two attributes have
non-overlapping time ranges, hence "unsynchronized").

- **020_14_01** — `timerel=after timeAt=2019-01-01Z`:
  asks for everything. Expects the response `fuelLevel` to be
  **empty** — broker is supposed to truncate at the cap and
  cut at the attribute boundary, so only the first attribute
  (`speed`) survives.
- **020_14_02** — `lastN=100 timerel=before timeAt=2021-01-01Z`:
  asks for the 100 most-recent instances. Expects the response
  `speed` to be **empty** — the same cut-at-boundary, but in
  reverse (lastN sorts descending so fuelLevel is the "first"
  attribute and survives, while speed is cut).

**Broker:** does not cut at the attribute boundary. Returns
instances of BOTH attributes (the prefix that fits under the
configured `--troeInstanceCap`, default 20 per TS 104-176 § 6.4.7),
interleaved. So both attributes end up with some instances and
both assertions fail.

**Spec:** TS 104-176 § 6.4.7 says when the result exceeds the broker's
configured page size, the response is 206 with a Content-Range
header. The spec is **not explicit** about "cut at attribute
boundary" — that is one valid pagination strategy among
several (uniform per-attribute cut, time-window cut, attribute-
boundary cut, etc.). The ETSI suite encodes the boundary-cut
choice without spec backing.

Same family as doubt #41 (`020_14` is "since v1.5.1" — and the
v1.5.1 wording introduced the cap concept without nailing the
cut algorithm).

**Verdict:** the broker's interleave-within-cap approach is
spec-compliant; the suite's expectation requires a specific
implementation. Either the spec adopts boundary-cut as the
mandated algorithm (and 020_14 gets explicit normative
backing) or the suite needs to assert observably (e.g.
"total instance count ≤ cap" rather than "this attribute is
empty").


## 63. `047_07_01 / 047_07_02` — flaky in full-suite due to cross-test state leak; pass in isolation

**Hit:** when run alone, both subtests pass — the broker
correctly skips notifications for a sub patched to
`isActive: false` (paused) or `expiresAt` in the past
(expired). When the full ETSI suite runs them in suite-order,
`Wait for no notification` (5s) sees a stray
`POST /notify` arrive and the assertion fails.

**Root cause:** the suite's `Start Local Server` /
`Stop Local Server` pair is per-test, but the HTTP listener
on port 1111 is the SAME port the previous tests in the 047_*
folder used as their notification endpoint. If an earlier
test's broker is still draining notifications at the moment
047_07's `Wait for no notification` window opens, the new
listener catches them — they're leftovers from a prior sub /
CSR, not from 047_07's paused/expired sub at all.

**Spec:** broker behaviour is correct in both isolated and
suite mode. The flakiness is purely test-framework
sequencing.

**Broker:** runs both subtests cleanly when invoked alone via
`robot --test '047_07_*' …`. Verified: 047_07_01 / 02 both
PASS in isolation; both FAIL in the full-folder run with a
stray-notification "request was received".

**Fix wanted:** the suite needs either
(a) per-test cleanup that drains the local listener queue
    before exiting, or
(b) a longer stabilisation delay between tests in the 047_*
    folder, or
(c) per-test ports for the local listener so leftover
    notifications can't bleed across.

**Also tagged:** 047_01_01 — same family, but it flips the
other direction (PASSes in full-suite, FAILs in isolation).
This direction-flip across the suite is the strongest
indicator that the suite's listener/teardown sequencing is
the actual problem, not the broker.


## 64. `D011_02_exc_02 / D011_02_red_02` — queryBatch test sends `Content-Type: application/ld+json` with `@context` in Link header instead of body

**Hit:** Two DistOps tests do `POST /entityOperations/query` with:
- `Content-Type: application/ld+json`
- `Link: <…compound.jsonld>; rel="http://www.w3.org/ns/json-ld#context"; type="application/ld+json"`
- Body: `{"type":"Query","entities":[{"type":"Vehicle"}]}` — **no `@context` field**

Expects 200 with the entity from the CSR forward.

**Spec:** TS 104-176 § 6.2.4 (Use of @context) — when `Content-Type` is
`application/ld+json`, `@context` *shall* be carried in the
payload body and *shall not* be supplied as a Link header.
The reverse (Link header) is only valid with
`Content-Type: application/json`.

The broker enforces this in `ldContextResolve` /
`ldRequestParse` and rejects 400 `BadRequestData` "Missing
@context — @context is mandatory for Content-Type
application/ld+json" before the request ever reaches the
query / forward logic.

**Broker:** correct per TS 104-176 § 6.2.4 — the two
`Content-Type` choices are mutually exclusive about where
the `@context` may live, and ld+json forbids the Link form.

**Why the tests trigger it:** Robot's `Query Entities Via
POST` keyword always wraps the body in a fresh `&{body}` dict
that contains `type` + `entities` (etc.) but never appends
`@context`, while the caller passes `content_type=
${CONTENT_TYPE_LD_JSON}` and `context=
${ngsild_test_suite_context}` — the latter is then placed in
the Link header. The combination is invalid by TS 104-176 § 6.2.4.

**Fix wanted:** either
  (a) when `content_type == application/ld+json`, `Query
      Entities Via POST` should inline `@context` into the body
      (alongside the existing `type`/`entities`/… fields) and
      drop the Link header, or
  (b) the two test cases should explicitly pass
      `content_type=${CONTENT_TYPE_JSON}` so the existing Link
      header path remains valid (this is what the matching
      GET-variant tests in this family already do — only the
      `_02` POST variants regress).

**Not the same as #28:** that one is about `?id=` alone being
flagged "too wide" by TS 104-175 § 10.4.3.4 and affects the `_01` GET
variants of the same family (`D011_03_inc_01`,
`D011_04_inc_01`). #64 here is purely the body/Link @context
shape mismatch and is the dominant 400 for the POST `_02`
variants.


## 65. `D017_01_inc / D017_01_red / D017_01_exc` — `Purge Entities` keyword joins URL without a slash

**Status:** RESOLVED — fixed upstream in MR !263.

**Hit:** All three D017_01 Purge-Entities tests fail at the
first assertion: `204 != 404`. The broker emits a clean 404
ResourceNotFound. The forward to the CSR never happens.

**Why:** the Robot keyword `Purge Entities` in
`resources/ApiUtils/ContextInformationConsumption.resource`
(line 389) builds the request URL as:

```robot
    url=${final_url}${ENTITIES_ENDPOINT_PATH}
```

— **no `/` between the two parts**. With `${final_url} =
"http://localhost:1026/ngsi-ld/v1"` and
`${ENTITIES_ENDPOINT_PATH} = "entities/"`, the resulting URL
is `http://localhost:1026/ngsi-ld/v1entities/?type=Vehicle`
— a route that does not exist. The broker returns 404
"ResourceNotFound", which is what the test sees as the
"actual" status.

Every other keyword in the same file (`Query Entities`,
`Retrieve Entity`, `Batch Upsert Entities`, etc.) uses the
correct join `${url}/${ENTITIES_ENDPOINT_PATH}`.

**Broker:** correct — there is no `/ngsi-ld/v1entities/`
endpoint by any spec reading; 404 is the right answer.

**Confirmed end-to-end:** issuing `DELETE
/ngsi-ld/v1/entities?type=Vehicle` directly (correct path)
through curl returns 204 No Content, with the broker
forwarding `DELETE /ngsi-ld/v1/entities?type=Vehicle` to the
CSR endpoint as expected (verified via swBroker trace level
`KtDistOpRequest=400`).

**Fix wanted:** change the line in
`ContextInformationConsumption.resource` to include the
separator slash — matching the surrounding keywords:

```robot
    url=${final_url}/${ENTITIES_ENDPOINT_PATH}
```


## 66. `D002_01_red` — Create-Entity stub URL includes the entity id (which POST /entities never carries)

**Status:** RESOLVED — fixed upstream in MR !263.

**Hit:** `D002_01_red Delete Entities On Both Context Sources`
fails its very first assertion: the test's `Create Entity` call
returns 409 Conflict with body
`{"success":[],"errors":[{"entityId":"…","error":{"…title":
"Bad Gateway","detail":"forward failed …"}}]}` instead of the
expected 201.

**Why:** the test sets these stubs on the HttpCtrl mock:

```robot
Set Stub Reply  POST  /broker1/ngsi-ld/v1/entities/${entity_id}   201
Set Stub Reply  POST  /broker2/ngsi-ld/v1/entities/${entity_id2}  201
```

But TS 104-175 § 10.2.2 (Create Entity) defines exactly one URL for POST:
`POST /ngsi-ld/v1/entities` — with the body carrying the entity
(including its `id`). The entity id is **not** in the URL on
create; it only appears in the `Location` header of the
response (and on subsequent Retrieve / Update / Delete URLs).

The broker correctly forwards
`POST http://0.0.0.0:8086/broker1/ngsi-ld/v1/entities` (verified
via swBroker trace level `KtDistOpRequest=400`). HttpCtrl
matches on the full URL incl. query string (per §40), so the
stub at `…/entities/<id>` never fires. The forward times out at
the broker's distOpTimeout → broker reports it as transport
failure → with redirect-mode CSRs there is no local creation
either, so `anySucceeded == false` and TS 104-175 § 5.2.6.8.2 emits 409 with
the BatchOperationResult shape (see postEntities.c line 783).

**Broker:** correct. The 409 + `errors[].title=Bad Gateway`
response shape is what TS 104-175 § 5.2.6.8.2 prescribes when every CSR leg
of a Create-Entity fails and there is no local leg (redirect
mode).

**Fix wanted:** the two `Set Stub Reply` lines should drop the
`/${entity_id}` suffix:

```robot
Set Stub Reply  POST  /broker1/ngsi-ld/v1/entities    201
Set Stub Reply  POST  /broker2/ngsi-ld/v1/entities    201
```

The same test then sets up DELETE stubs *with* the id, which is
correct for DELETE — `/entities/{id}` is the right URL there
(TS 104-175 § 10.2.8).


## 67. `D002_02_01_inc / D002_02_02_inc` — broker treats CSR's 404 on DELETE forward as idempotent success, not as an error

**Hit:** Both tests do `Delete Entity` against an inclusive CSR
where one of the two legs (local or remote) is supposed to
return 404. The test then expects:

```
Check Response Status Code              207   ${response.status_code}
Check JSON Value In Response Body  ['status']  404
```

— i.e. 207 Multi-Status with a per-entity error entry
reporting the 404. The broker instead returns 204 No Content
(success).

**Broker logic:** in `deleteEntity.c` line 135 and `purgeEntities.c`
line 330, the broker explicitly skips adding an entry to
`errorsArrayP` when a CSR forward returns 404 — the rationale
is the standard HTTP-DELETE idempotency: "404 on delete means
the resource is already gone, which is exactly what we wanted".
The same is done for the local leg when the entity is absent
locally but at least one CSR succeeded.

**Spec:** TS 104-175 § 10.2.8.4 lists the possible response codes but does
not prescribe whether a per-CSR 404 must surface as a 207-style
error or roll up into the parent success. TS 104-176 § 6.4.11 (Status
Codes) and TS 104-175 § 5.2.6.8.2 (BatchOperationResult) are also silent on
this specific case.

**Our reading:** broker is defensibly correct — once any leg
confirms the entity is deleted (or already absent), the
end-state is what the client asked for; surfacing the 404 as an
"error" is noise. Same reading underlies our purge and
batch-delete behaviour.

**Fix wanted:** either
  (a) clarify in the spec that per-CSR 404 must be reported in
      BatchOperationResult.errors[] regardless of overall
      success — at which point we'd flip the `sc != 404`
      check in deleteEntity.c and purgeEntities.c to always
      record, or
  (b) accept the broker's idempotent reading and adjust these
      tests (and any matching siblings) to expect 204 / 207
      based on whether a true error occurred, not just whether
      a 404 was encountered.

This same interpretation gap likely affects sibling tests in
the D002 / D004 / D006 / D017 families that mix-and-match
local-vs-CSR delete legs.


## 68. `D011_01_*` / `D011_02_red_01` family — query-response stubs supply a single entity object instead of an array

**Hit:** The six `D011_01_*` distributed-query tests
(`D011_01_01_inc`, `_03_inc`, `_04_inc`, `_05_inc`, `_exc`,
`_red`) plus `D011_02_red_01` configure the CSR mock with
`Set Stub Reply  GET  …/entities?type=Vehicle  200
${entity_body}`, where `${entity_body}` is one entity dict
loaded from `vehicle-simple-attributes.{json,jsonld}`.
TS 104-175 § 10.4.3.4 defines the queryEntities response as an Array of
NGSI-LD Entities.

**Broker:** historically discarded any forward response whose
parsed root wasn't a JSON array — those tests reported zero
entities, length / content mismatches, etc. The broker now
also accepts a bare entity object and re-wraps it as a
one-element array, matching TS 104-176 § 6.3.4 (JSON-LD compaction's
unwrap rule for single-member arrays).

**Why the broker can't recover from this on its own:** the
fixture passes a Python `dict` (from `Load Entity` →
`Load JSON From File`) directly into HttpCtrl's
`Set Stub Reply`. At response time `HttpHandler.__send_response`
runs `self.send_header("Content-Length", str(len(body)))` —
with body being a dict, that gives `len(dict)` = number of
keys (3 for a Vehicle entity) — flushes the headers, then
calls `self.wfile.write(body)` which raises
`TypeError: a bytes-like object is required, not 'dict'`. The
exception is caught by the broad `try/except` around
`__send_response` (it just logs "Response was not sent…") and
the socket closes. The broker has the headers but the
promised body never arrives.

Trace level `KtDistOpRequest=400` on swBroker — together with
the multi-engine errorDetail enrichment landed in the same
fix — prints this exactly:

```
forward response: status=0, bodyLen=0,
  error=Connection closed by peer (n=0, errno=…, bufLen=63,
        head=HTTP/1.0 200 OK|~…|~Content-Length: 3|~|~)
```

`bufLen=63` is just headers, no body bytes, then EOF.
`swRestClientResponseComplete` correctly reports
"incomplete: promised 3 bytes, got 0 before close" and the
multi engine flags the connection as malformed.

Confirmed end-to-end: a Python mock that announces
`Content-Length: 3` and then closes without sending the body
reproduces the broker's exact failure signature; a Python
mock that ships a properly-stringified JSON array returns
the entity through the broker and lights up the JSON-LD-
unwrap codepath as expected.

**Fix wanted upstream:** serialise the body before stubbing
so HttpCtrl gets a `str` it can `.encode('utf-8')`:

```robot
@{entity_list}=    Create List               ${entity_body}
${entity_json}=    Convert JSON To String    ${entity_list}
Set Stub Reply     GET    /ngsi-ld/v1/entities?type=Vehicle    200    ${entity_json}
```

That gives HttpCtrl a string body with a matching
`Content-Length`. The broker reads the full response and the
single-object branch of TS 104-176 § 6.3.4 (JSON-LD compaction) is no
longer reached at all.

**Independent HttpCtrl bug worth filing upstream:**
`__send_response` should reject (or auto-serialise) non-bytes
non-string `body` BEFORE sending the headers, instead of
silently swallowing the TypeError after `Content-Length` is
on the wire. That'd surface as a clear "stub body must be
str/bytes" failure rather than a mysterious half-response
and broker-side "transport failure".


## 70. `D003_01_exc` — Append-Attrs fragment sent as `application/json` with no Link header, so attribute names don't expand to the CSR's

**Hit:** `D003_01_exc Append Entity Attribute` fails the
post-forward stub-count check (`0 != 1`): the broker never
forwards the request to the CSR, so the mock counter stays at
zero. The request itself returns 204 (broker correctly updated
the local copy).

**Why:** the fixture posts
`vehicle-speed-isParked-fragment.json` — a bare attrs object,
no `@context` field, no `id`/`type` — using the
`Append Entity Attributes` keyword
(`resources/ApiUtils/ContextInformationProvision.resource`),
which sets `Content-Type: application/json` and **does not**
attach the `Link: <…>` header that carries the user
`@context`.

So when the broker parses the request body it has only the
core context to expand attribute names against. `speed` and
`isParked` resolve to
`https://uri.etsi.org/ngsi-ld/default-context/speed` /
`…/isParked` (the default-vocab fallback). The CSR was
registered earlier with `Content-Type: application/ld+json`
and the test-suite compound context — its
`propertyNames=["speed"]` got expanded to
`https://ngsi-ld-test-suite/context#speed`.

In `ldEntityFragmentForInfo` the `nameInList(curP->name,
riP->propertyNamesV)` check compares those two URIs, fails,
and the per-CSR fragment ends up empty
(`matched == 0 → return NULL`). The dispatch loop in
`postEntityAttrs.c` then skips this CSR entirely — verified
via swBroker trace level `KtDistOpRequest=400`:

```
postEntityAttrs dispatch: entityId=urn:…:Vehicle:… type=(none) excl=1 redir=0 incl=0
```

— the CSR matches by id but the fragment-extract drops every
attribute, so no `forward: POST …` trace follows.

**Broker:** correct. With two different `@context`s on the
two requests, the broker has no way to know that
default-vocab `speed` and test-suite `speed` are the same
property.

**Fix wanted upstream:** the `Append Entity Attributes`
keyword should accept (and forward) a `context` argument the
way the matching consumption keywords do. The test then
calls it with `context=${ngsild_test_suite_context}` and
broker / CSR agree on the expansion.

Practical fix in the keyword:

```robot
Append Entity Attributes
    [Arguments]    ${id}    ${fragment_filename}    ${content_type}    ${context}=${EMPTY}
    &{headers}=    Create Dictionary    Content-Type=${content_type}
    IF    '${context}' != ''
        ${context_link}=    Build Context Link    ${context}
        Set To Dictionary   ${headers}    Link=${context_link}
    END
    ...
```

— and the test passes `context=${ngsild_test_suite_context}`
alongside `${CONTENT_TYPE_JSON}`.


## 71. `D004_01_exc` — PATCH-attrs body carries an id that doesn't match the URL id

**Hit:** `D004_01_exc Create Entity and Registration And Start
Context Source Mock Server` returns 400 BadRequestData where
the test expects 204:

```json
{
  "type": "https://uri.etsi.org/ngsi-ld/errors/BadRequestData",
  "title": "Entity Id Mismatch",
  "status": 400,
  "detail": "Entity 'id' in body ('urn:ngsi-ld:Vehicle:randomUUID')
             does not match URL ('urn:ngsi-ld:Vehicle:<random>')"
}
```

**Why:** the fixture
`data/entities/fragmentEntities/vehicle-speed-different-attribute.jsonld`
carries `"id": "urn:ngsi-ld:Vehicle:randomUUID"` and
`"type": "Vehicle"` alongside the `speed` attribute it wants
to update. The `Update Entity Attributes` keyword
(`ContextInformationProvision.resource` line 316) reads the
file verbatim via `Get File` and PATCHes it to
`…/entities/<test-generated-id>/attrs/` — the URL's
`<test-generated-id>` is a fresh `urn:ngsi-ld:Vehicle:<digits>`,
so the body id is a real entity id but it's *not* the same
entity id the URL targets.

TS 104-175 § 5.2.6.4.3 lets an EntityFragment omit id/type, but when an id
*is* supplied the broker has to enforce that it matches the
URL — otherwise the request is ambiguous about which entity
to mutate. `ldCheckEntity.c::isUpdateOp` covers exactly
PATCH-style ops (`LdOpUpdateEntity`, `LdOpAppendAttrs`,
`LdOpMergeEntity`); the rejection is by design.

**Broker:** correct. TS 104-175 § 10.2.3 mandates the URL id is
authoritative for partial-update ops and the body MUST NOT
disagree.

**Fix wanted:** either
  (a) strip `id` from the fixture (the `type` can stay — per
      TS 104-175 § 10.2.9.4 it's unioned into the entity's type set, so
      sending the existing `Vehicle` is a no-op), or
  (b) make the `Update Entity Attributes` keyword swap the
      fixture's id for `${id}` before PATCHing, the way
      `Create Entity` and `Prepare Context Source Registration
      From File` already do:

      ```robot
      ${fragment_payload}=  Load JSON From File  …/${fragment_filename}
      ${fragment_payload}=  Update Value To JSON  ${fragment_payload}  $.id  ${id}
      ```

Variant (a) is the smaller fix — `id` simply isn't part of
the EntityFragment semantics for partial updates; only the
URL id matters.


## 72. `D001_02_inc` / `D001_03_03_inc` / `D011_02_inc` — flaky in full-suite due to leftover Vehicle entities in current state

**Hit:** Three tests query
`Query Entities entity_types=Vehicle local=true` near the end
and assert about the set:
  * D001_02_inc — `Should Be Empty` (after rejecting a
    malformed-id Create, no Vehicle should remain locally).
  * D001_03_03_inc — `Check Response Body Containing Entities
    URIS set to [${entity_id}]` (the just-created entity is
    the only Vehicle).
  * D011_02_inc — `Should Be Empty` (with `local=true` and
    no local entity created, the broker must skip the CSR
    forward and return an empty array).

In a full-folder run the broker has Vehicles in `etsi.entities`
from earlier DistOps tests that never deleted them (their
teardowns drop the CSR + the entity-by-id, but tests that
generate Vehicles transitively or skip the teardown leave
strays). The query returns one or more extra entities and
the assertion fails — `'[…]' should be empty` for D001_02_inc
or `Lengths are different: 1 != N` for D001_03_03_inc.

**Verified:** both tests PASS in isolation via
`robot --test 'D001_02_inc*' …` and
`robot --test 'D001_03_03_inc*' …`. Broker behaviour is
correct in both runs.

**Same class as §63** (`047_07_*` notification leftovers):
the broker is right, the test-suite needs sturdier
isolation.

**Fix wanted upstream:** either
  (a) wrap each test in a Vehicle-only purge at setup
      (`DELETE /entities?type=Vehicle&local=true`), or
  (b) add a suite-level `DELETE /entities?type=Vehicle&local=true`
      between tests that mutate the Vehicle pool, or
  (c) rephrase the assertion to "MUST contain
      `${entity_id}`" instead of "MUST be exactly that one"
      — the test's narrative doesn't actually need
      exclusivity, only presence.

Variant (c) is the smallest change; (b) is the cleanest
isolation.


## 73. `037_07_02` — Within Polygon URL params carry LineString-shaped coordinates

**Hit:** `037_07_02 Within Polygon` issues
`GET /csourceRegistrations?georel=within&geometry=Polygon&coordinates=[[-13.503,47.202],[6.541,52.961],[20.37,44.653],[9.46,32.57],[-15.23,21.37]]&geoproperty=location`
and expects 200 + the CSR registered at setup. The broker
correctly answers 400 BadRequestData ("GeoJSON polygon ring
must have at least 4 positions").

**Why:** GeoJSON (RFC 7946 § 3.1.6 / § 3.1.10) defines Polygon
coordinates as an Array of LinearRings, i.e. **triple-nested**
positions:

```json
[ [ [lon1,lat1], [lon2,lat2], ..., [lonN,latN], [lon1,lat1] ] ]
```

— an outer array of rings, each ring a closed sequence of ≥4
positions. The test fixture sends a **double-nested** array
(`[[lon,lat], [lon,lat], …]`) which is the LineString shape.
NGSI-LD TS 104-175 § 7.2.4 references GeoJSON for the `geometry` /
`coordinates` URL-param pair, so the same shape constraint
applies.

The broker's geo-check (`ldCheckGeo.c::checkPolygonCoords`)
iterates the outer array as rings and finds the first
"ring" has only 2 children (the lat/lon pair) — under the
spec's 4-position minimum.

**Broker:** correct per RFC 7946 / TS 104-175 § 7.2.4.

**Fix wanted:** the fixture's coordinates should be the
spec-correct triple-nested form, with the polygon explicitly
closed:

```python
coordinates=[[[-13.503,47.202],[6.541,52.961],[20.37,44.653],[9.46,32.57],[-15.23,21.37],[-13.503,47.202]]]
```

Or the geometry param should switch to `LineString` (with
the existing coordinate shape) — but then the test is no
longer a "Within Polygon" test, so option 1 is the right
fix.

## 74. `D018_01` / `D018_03` — forwarded-create reply not stubbed (sibling `D018_02` stubs it), so the inclusive forward yields 207

**Status:** RESOLVED — fixed in MR !270.

**Hit:** `D018_01` (loop detection) and `D018_03` (Via-header
forwarding) fail at setup — `Create Entity` returns **207**, the
test asserts **201** — so they never reach their actual assertion
(loop-detection 508 / Via comparison).

**Where:** both register an **inclusive** CSR pointing at the
HttpCtrl mock and then call `Create Entity`, but neither stubs a
reply for the forwarded create. Their sibling `D018_02` does
(`Set Stub Reply POST /broker1/ngsi-ld/v1/entities 201`) and
passes. `D018_03` even stubs the DELETE forward
(`Set Stub Reply DELETE …/entities/${entity_id} 204`) but not the
POST.

**Why it's wrong:** with an inclusive registration the create is
also forwarded to the registered source. If that forward isn't
answered, a broker that surfaces the unanswered forward as a
partial success returns 207. The test assumes 201 — i.e. it
implicitly assumes the forward is answered — but never arranges
the answer. Whether a *failed* inclusive-write forward should
downgrade the operation to 207 at all is itself a spec ambiguity
(see `spec-doubts-2.md` #90 in the TS 104-175 repo), so the test
must not depend on a broker's choice there; it should stub the
forward like `D018_02`.

**Impact / broker:** the broker returns 201 when the inclusive
forward succeeds (proven by `D018_02`) and 207 when it fails —
defensible either way. The inconsistency is in the test, not the
broker.

**Fix wanted:** add `Set Stub Reply POST <path> 201` before
`Create Entity` in `D018_01` and `D018_03`, mirroring `D018_02` —
path `/ngsi-ld/v1/entities` for `D018_01` (default endpoint) and
`/broker1/ngsi-ld/v1/entities` for `D018_03` (`endpoint=/broker1`).
Verified: with the stub, the create + 201 check pass and the test
proceeds exactly like `D018_02` (it then reaches the actual
loop-detection / Via assertions). Implemented in branch
`fix/d018-stub-forwarded-create`.


## 75. `047_10`–`047_15` — CSource-notification validator carries three fixture bugs

**Status:** RESOLVED — fixed in MR !271.

**Hit:** the CSource-notification tests `047_10_01`, `047_11_01`,
`047_11_02`, `047_12_01`, `047_13_01`, `047_14_01`, `047_15_01` all fail.
They share the keyword `Wait for notification and validate it` in
`resources/NotificationUtils.resource`, which validates the received
`ContextSourceNotification` against three things that are each wrong in
the fixture — each one masked the next until fixed:

1. **Type literal misspelled.** `${notification_type}` read
   `ContextSource Notfication` (a stray space plus a missing `i`). The
   `type` member emitted by a conformant broker is `ContextSourceNotification`
   (single token, per NGSI-LD TS 104-175 § 5.2.x), so the comparison could
   never pass. → correct the literal.

2. **`notifiedAt` validated with a fixed second-precision format.** The
   check was `Is Date  ${notification}[notifiedAt]  ${date_format}` with
   `${date_format}=%Y-%m-%dT%H:%M:%SZ` — no fractional seconds. `notifiedAt`
   is an RFC 3339 timestamp and brokers legitimately emit milliseconds
   (e.g. `2026-05-27T10:28:22.317Z`), which `strptime` then rejects. → use
   the millis-tolerant `Parse Ngsild Date` (the same parser already used
   for `createdAt`/`modifiedAt`, and the one hardened on `develop` for
   arbitrary fractional precision). The now-unused `${date_format}` /
   `${date_format_with_millis}` variables are removed.

3. **Registration id looked up as `@id`.** The data loop read
   `${notification}[data][${index}][@id]`, but the notification is normal
   compacted NGSI-LD JSON — the registration identifier key is `id`
   (the broker emits `id`, `type`, `information`, `endpoint`, all
   compacted; `@id` is the expanded JSON-LD form, not present). → look up
   `id`.

**Impact / broker:** none — the broker is correct on all three; the
defects are entirely test-side.

**Fix wanted:** all three corrections in the shared keyword in
`resources/NotificationUtils.resource`. **Verified:** with the three fixes
the seven tests pass (full broker run; the same dir in isolation is partly
masked by HttpCtrl-mock `Wait For Request` timeouts, but the seven are not
among the timeouts and pass cleanly).


## 76. `042_02_01` / `042_03_01` — Delete CSR-subscription keyword raises on the 4xx it is meant to assert

**Status:** RESOLVED — fixed in MR !272.

**Hit:** `042_02_01` (delete with invalid URI, expects **400**
`BadRequestData`) and `042_03_01` (delete unknown id, expects **404**
`ResourceNotFound`) both fail with an `HTTPError` exception
(`400 Client Error` / `404 Client Error`) raised *before* the
`Check Response Status Code` assertion is reached.

**Why:** the keyword `Delete Context Source Registration Subscription` in
`resources/ApiUtils/ContextSourceRegistrationSubscription.resource` issues
its `DELETE` without `expected_status=any`, so RequestsLibrary raises on
any status ≥ 400. These two are negative-path tests that *expect* a 4xx,
so the keyword aborts before they can assert it. Every sibling keyword in
the same file already carries `expected_status=any` (lines 37, 70, 89,
100, 111), as does the regular `Delete Subscription` keyword — the Delete
keyword here is simply the one that was missed.

**Impact / broker:** none — the broker correctly returns 400 for an
invalid URI and 404 for an unknown subscription, with the right
ProblemDetails. The defect is entirely test-side.

**Fix wanted:** add `...    expected_status=any` to the `DELETE` call in
`Delete Context Source Registration Subscription`, matching the other
keywords in the file. **Verified:** with the one-line fix both tests pass
(and `042_01_01`, the happy-path delete, still passes).

## 77. `039_05_01` — invalid-JSON test expects BadRequestData, but spec § 8 says InvalidRequest

**Hit:** `039_05_01 Update Context Source Registration Subscription With Invalid JSON Fragment` sends a fragment with a syntactic JSON error (`,,`) and asserts `ProblemDetails.type == https://uri.etsi.org/ngsi-ld/errors/BadRequestData`. A spec-conformant broker returns `InvalidRequest`, so the test fails.

**Spec:** TS 104-175 § 8: "If the request payload body is not a valid JSON document then an error of type **InvalidRequest** shall be raised. If the data included by the JSON-LD document is not syntactically correct, according to the @context or the API data type definitions, then an error of type **BadRequestData** shall be raised." So malformed JSON → InvalidRequest; valid JSON with bad data → BadRequestData.

**Impact / broker:** none. Every other invalid-JSON test in the suite (`001_02`, `003_03`, `004_06`, `005_04`, `006_03`, `007_02`, `014_04`, `028_02`, `057_03`) correctly expects `INVALID_REQUEST`. 039_05_01 is the outlier — looks like the author copy-pasted from a *bad-data* test by mistake.

**Fix wanted:** swap the assertion to `ERROR_TYPE_INVALID_REQUEST`. One-line change.

**Status:** RESOLVED — fixed in this MR.

## 78. Test-infra housekeeping bundle: missing keywords, variables, fixture cleanups

Two small, independent test-suite issues that each prevented one or a few
tests from reaching the broker. Bundled into one MR for low-noise review.

**78a) `038_05_01` — undefined `${date_format}` variable**

`038_05.robot` referenced `${date_format}` in its body but had no
corresponding Variables-section entry. **Fix:** added the standard
`%Y-%m-%dT%H:%M:%SZ` definition in that file's Variables section, matching
sibling test files (`046_07/11`).

**78b) `053_03_01 / 051_04_02 / 051_04_03` — `Variable '${ERROR_TYPE_RESOURCE_NOT_FOUND}' not found`**

`${ERROR_TYPE_RESOURCE_NOT_FOUND}` was defined in some resource files but not
in `resources/ApiUtils/jsonldContext.resource` (which the three failing tests
use exclusively). **Fix:** added the standard mapping
`https://uri.etsi.org/ngsi-ld/errors/ResourceNotFound` to that file's
Variables section, alongside the existing `ERROR_TYPE_BAD_REQUEST_DATA`.

**Status:** RESOLVED — both items fixed in this MR.

**Items removed from the original bundle after MR review (2026-05-30):**

- `001_05_02 / 003_05_02 almostFull` — broker-side bug (@vocab over-stripping
  when the user @context defines the same short name to a different IRI).
  The expected `ngsi-ld:default-context/almostFull` form is correct per the
  JSON-LD compaction algorithm. Filed as a broker issue.
- `038_01_01 / 040_01_01 notificationTrigger` — broker-side bug. TS
  104-175 § 5.2 Subscription table is explicit: `notificationTrigger`
  *"is not applicable and shall be ignored if the Subscription data type
  is used for a Context Source Registration Subscription"*. The broker
  shouldn't be auto-populating it on CSR subs; filed separately.
- `021_15_* / 029_10_01 DateTime keyword resolution` — likely env-delta
  on the reporter side (Robot 7.4.1 / Python 3.14.4) rather than a
  genuinely missing import; the tests have been passing on Forge CI for
  years. Pulled from the MR pending re-verification on a clean checkout.

## 79. `D011_02_exc_01` — mock stub URL keys on `attrs=` but broker forwards `pick=`

**Files:**
- `TP/NGSI-LD/DistributedOperations/Consumption/Entity/QueryEntities/D011_02_exc.robot`

**Symptom:** broker query forward returns 200 with the entity skeleton but
no `speed` attribute; `Should Have Value In Json $.speed` fails.

**Cause:** the test sets the CSR mock stub URL to
`${url}?type=Vehicle&id=${entity_id}&attrs=speed` and only replies with the
speed-bearing payload when that exact URL is hit. The broker forwards the
query with `pick=speed` (the current-spec attribute-projection parameter,
since `attrs=` is the deprecated v1.6 form). The stub URL doesn't match, so
the mock returns its default empty reply and the assertion fails.

This was found on swBroker, where the @vocab over-stripping bug previously
masked the issue by causing a different failure mode upstream. Once the
broker bug was fixed, the stub-URL mismatch surfaced.

All NGSI-LD payloads in this test use the same compound test-suite context
(`ngsi-ld-test-suite-compound.jsonld`), so the parameter mismatch is purely
`attrs` vs `pick`, not a context-resolution issue:

- entity create — `vehicle-simple-attributes.jsonld` carries the compound context
- CSR registration — same compound context, `propertyNames:["speed"]`
- query — `context=${ngsild_test_suite_context}` resolves to the same URL
- mock stub reply (`vehicle-speed-attribute.json`) has no `@context` at all

**Fix:** change the stub URL in the `GET` branch to use `pick=speed`
instead of `attrs=speed`. Check `D011_02_red_01` / `D011_02_inc_01` and
any other sibling that builds stub URLs from `attrs=` for the same pattern.

**Spec angle:** § 5.7.2 (Query Entities) and § 6.3.2 list `pick` and `omit`
as the projection parameters; `attrs=` is not in the v1.9 parameter table.
The spec could be clearer that distributed-operation forwards must
translate any legacy `attrs=` from a 1.6-era CSR into `pick=` so that mock
test stubs written against the modern spec match the forwarded URL.

## 80. `046_22_08` — `deleteAll=${true}` serializes as `deleteAll=True` (capital T)

**Files:**
- `TP/NGSI-LD/ContextInformation/Subscription/SubscriptionNotificationBehaviour/046_22_08.robot`

**Symptom:** broker returns 400 BadRequestData on the DELETE attribute
call:
```
"detail": "'deleteAll' must be 'true' or 'false' (lowercase); got 'True'"
```

**Cause:** the test passes `deleteAll=${true}` (Robot Framework boolean
literal) to the `Delete Entity Attributes` keyword. The keyword in
`ContextInformationProvision.resource:248` appends it to the URL as
`deleteAll=${true}` → which Robot serializes to the string `"True"`
(capital T). The broker correctly rejects this per the strict-case URL
parameter rule.

NGSI-LD § 4 and RFC 3986 § 6.2.2.1 both require URL parameter values to
be case-sensitive; the spec uses the literal lowercase tokens `true` /
`false` everywhere. swBroker enforces this strictly (parseBool in
`ldUrlParams.c`); accepting `True` / `TRUE` / etc. would be a silent
deviation from the spec.

All ten sibling tests `046_22_01`..`046_22_07`/`09`..`11` pass
`deleteAll=${EMPTY}` (omit the parameter entirely), so this is a
one-off bug isolated to `046_22_08`.

**Fix:** change `deleteAll=${true}` to the literal string `deleteAll=true`
in `046_22_08.robot:30`. The keyword's existing serialization will then
produce the lowercase form the broker (and the spec) expect.

**Related:** when this is fixed, the test will progress past the URL-param
rejection and into the actual notification-trigger logic — which is what
the test is intended to exercise. The current 400 short-circuits the
whole assertion.

**80b) `Create Subscription And Entity` keyword — `${entity_id_suffix}=None` default never falls through to `Generate Random Building Entity Id`**

The trace for 046_22_08 (and all sibling 046_22_* tests) shows the
entity URL as `urn:ngsi-ld:Building:None` — a literal "None" suffix
rather than the intended random-uuid. Source in
`resources/SubscriptionUtils.resource:14`:

```robot
[Arguments]    ${subscription_payload_file_path}    ${building_filename}    ${entity_id_suffix}=None

IF    '${entity_id_suffix}' == None
    ${entity_id}=    Generate Random Building Entity Id
ELSE
    ${entity_id}=    Catenate    ${BUILDING_ID_PREFIX}${entity_id_suffix}
END
```

Two layered bugs:

1. The argument default `${entity_id_suffix}=None` makes the default
   value the literal string `"None"`, not Robot's `${None}` (Python
   `None`). To get the actual `None` object the default would need to
   be `${entity_id_suffix}=${None}`.

2. The condition `'${entity_id_suffix}' == None` compares the
   stringified `'None'` against Python's `None` (built-in identifier in
   Robot evaluator). Strings never compare equal to NoneType in
   Python, so the IF branch (which calls
   `Generate Random Building Entity Id`) never fires — every caller
   that omits the suffix ends up with `urn:ngsi-ld:Building:None`.

The tests "work" because each setup creates the entity with that fixed
id and the teardown deletes it again — a single test's create / use /
delete cycle is consistent. But:

- Any test parallelism would break with a collision on `:None`.
- Cross-test state leak between siblings is hidden by the matching
  delete in teardown — but the entity briefly exists for two adjacent
  tests at the same time during the Robot run, which is unintentional.
- The keyword's `Generate Random Building Entity Id` branch is dead
  code today.

**Fix:** change the default and the condition to be consistent. Either:
- `${entity_id_suffix}=${None}` + `IF '${entity_id_suffix}' == 'None'` (compare against the string), OR
- `${entity_id_suffix}=${EMPTY}` + `IF '${entity_id_suffix}' == ''`

The second form matches the convention already used elsewhere in the
suite (`deleteAll=${EMPTY}` etc.).

## 82. `D010_01_exc` / `D002_02_exc` / `D003_02_exc` / `D004_01_exc` — distop tests surfaced as test-side after § 9.3.3 enforcement

The triage of four distop "later error" failures (from the `lowercase
boolean URL params` MR (#81)) found four distinct test-side problems
behind them. Brokers that strictly validate exclusive registrations per
§ 9.3.3 and `application/ld+json` body shape per § 5.6.2 will reject
the tests' fixtures and keyword setup. Each item below stands on its
own; group them here because the same audit pass surfaced all four.

**82a) `D010_01_exc` — stub-count assertion checks the wrong URL pattern**

- Stub registered at `/broker1/ngsi-ld/v1/entities/${entity_id}` (by-id)
- Assertion at line 34 counts hits at `/broker1/ngsi-ld/v1/entities?type=Vehicle` (by-type listing)

The broker correctly forwards a by-id retrieve for an exclusive CSR
that pins a specific entity id — the by-id stub matches. The by-type
stub-count URL never matches by construction and always returns 0, so
`Should Be True ${stub_count} > 0` always fails. **Fix:** change the
`Get Stub Count` URL in line 34 to the by-id pattern actually
registered at line 29.

**82b) `D002_02_exc` — fixture invalid for exclusive mode (§ 9.3.3)**

The test uses
`csourceRegistrations/context-source-registration-vehicle-redirection-ops.jsonld`
as an exclusive CSR. The fixture's `information[0].entities[0]`
declares only `{"type": "Vehicle"}` — a type-only group selector. Per
§ 9.3.3, exclusive registrations "shall always relate to specific
Attributes found on a single Entity" — both a specific entity id AND
attribute names are mandatory. swBroker (and any spec-strict broker)
now rejects the CSR creation with **400 BadRequestData**; the test's
setup then fails (`Check Response Status Code 201` doesn't see 201).

The same fixture, used as a redirect CSR (e.g. `D010_02_red.robot`),
is fine — § 9.3.3 only constrains exclusive mode. The fix is to give
this test its own fixture (or extend
`vehicle-redirection-ops.jsonld` keeping the original redirect-mode
behaviour intact) that adds `propertyNames` matching what the test
actually exercises. The test name "Delete Entity Without Redirection
Operations" is also slightly misleading — per § 9.2,
`redirectionOps` includes `deleteEntity`, so the premise that "this
CSR doesn't support delete" doesn't hold with the listed operation
set.

**82c) `D003_02_exc` — `application/json` body without Link header doesn't propagate the test-suite context to attribute matching**

The test POSTs `data/entities/fragmentEntities/vehicle-speed-isParked-fragment.json`
to `/entities/{id}/attrs/` with `Content-Type: application/json` and
no Link header. The fragment carries no `@context`, so the broker
expands `speed` against the **core** context →
`https://uri.etsi.org/ngsi-ld/default-context/speed`. The CSR was
registered with `${ngsild_test_suite_context}` so its `propertyNames:
["speed"]` claim expanded to a different IRI. The two don't match,
the broker doesn't forward to the CS, and `Wait for redirected request`
times out.

The test should either send the body as `application/ld+json` with a
root `@context`, OR pass a Link header carrying
`${ngsild_test_suite_context}`, so the broker's attr expansion lines
up with the CSR's. Same pattern probably affects other
`Append Entity Attributes With Parameters` callers.

**82d) `D004_01_exc` — fragment placeholder `randomUUID` is never substituted**

`Update Entity Attributes` (`ContextInformationProvision.resource:248`)
reads the fragment file (`vehicle-speed-different-attribute.jsonld`)
verbatim and PATCHes it. The fixture's root carries
`"id": "urn:ngsi-ld:Vehicle:randomUUID"` — never replaced with the
actual `${entity_id}` the test set up. The broker correctly returns
**400 BadRequestData** with `"Entity Id Mismatch"` because the body id
doesn't match the URL id.

**Fix:** mirror what `Create Entity` does — substitute the body's
`$.id` with the actual `${entity_id}` before PATCHing. Or omit the
`id` from the fragment entirely (PATCH attrs doesn't need it).


## 75. `D018_01` — asserts **508** while registering `mode=inclusive`

**Status:** fixed in this fork (setup now registers `mode=redirect`).

**Hit:** `D018_01 Loop Detection With Via Header` — the DELETE replaying the
broker's own `Via` pseudonym returns **204**, the test asserts **508**. Only
failing TP of the 4×8 matrix, identical in every store mode.

**Where:** the Test Setup calls `Prepare Context Source Registration From File
… mode=inclusive`. (The fixture name — `…-vehicle-redirection-ops.jsonld` —
refers to `operations: ["redirectionOps"]`, an operation group, and does not
set a mode.)

**Spec:** 6.3.17 p.278 returns 508 only "in the case of an **exclusive** or
**redirect** registration, where all of the data is held outside of the Context
Broker and held in a single registered source … registered to redirect back on
to the Context Broker". For **inclusive** the clause prescribes 207 on source
errors, and Table 6.3.18-2 p.279 makes the Via listing "used when determining
matching registrations" — the looping registration drops out and the operation
completes locally, i.e. 204.

Supporting signal: `D018_02` and `D018_03` are tagged `additive-inclusive`;
`D018_01` is not, yet it is the one that pins the mode to inclusive.

**Fix wanted upstream:** `mode=redirect` (or `exclusive`) in `D018_01`'s setup,
so the scenario matches the clause the assertion cites. Alternatively, if ETSI
intends *any* replayed self-alias to be 508 regardless of mode, 6.3.17 needs
amending — the current text cannot support the assertion.

---

## 2026-08-10 — `_exc` setups violate 4.3.6.3's exclusive-registration rules

4.3.6.3 p.41 mandates that an exclusive registration define BOTH an entity id
("an id pattern or Entity type defining a group of entities is not supported
for exclusive registrations") AND Attributes. Nine `_exc` TPs created
registrations breaking this and asserted 201:

- `D001_02_exc`, `D002_02_exc`: entity id but no Attributes
  (`vehicle-redirection-ops` fixture).
- `D012_01/D013_01/D013_02/D014_01/D014_02/D015_01/D016_01_exc`:
  `idPattern=urn:ngsi-ld:Vehicle:*` (`vehicle-speed-with-batch-ops` fixture).

A broker enforcing 4.3.6.3/5.9.2 (400 on the malformed exclusive, 409 on a
proxied overlap with an existing exclusive) fails these setups through no
fault of its own.

**Fix applied in this fork:** D001_02_exc/D002_02_exc use the
`vehicle-speed-with-redirection-ops` fixture (id + propertyNames); the seven
batch `_exc` setups register the two concrete entity ids instead of the
pattern. Assertions untouched — each test still proves its stated subject.
**Fix wanted upstream:** same, or ETSI relaxes 4.3.6.3 if pattern-scoped
exclusives are intended to be legal (the current text cannot support them).

## 2026-08-11 — 5.2.3 served @context: literal wording vs ecosystem practice

Clause 5.2.3 (V1.9.1, p.105): "an array (flattened to a single string if
necessary), containing a user @context where present, and the core @context
(as described in clause 4.4) shall be included as a special member of the
corresponding JSON-LD Object."

Read literally, every application/ld+json response body should carry
`[user-context…, core-context]`. The official suite pins the OPPOSITE in 68
expectation files (strict `StringOrSingleListContextOperator` comparison,
e.g. `data/entities/expectations/building-language-property-fr-filter.jsonld`
= suite context alone), and the suite is validated against Scorpio/Stellio —
i.e. the whole ecosystem treats the core context as implicit (4.4) and
echoes only the user context. Antares follows the ecosystem reading
(negotiate.rs `served_context`); flipping to the literal reading would fail
all 68 expectations. Not fork-fixed — raise as a wording clarification
upstream instead. Strictly-JSON-LD consumers that need core terms resolvable
must apply the core context themselves, exactly as 4.4 describes.

## 2026-08-11 — 5.2.8 EntityInfo type cardinality vs the IOP fixture

5.2.8 (V1.9.1, p.111): type has Cardinality 1 — "it is not possible to
register what Entities can be provided by a Context Source just by their id
or idPattern (i.e. without specifying their type)". The official
interoperability suite registers exactly that:
`data/csourceRegistrations/interoperability/context-source-registration-exclusive-1.jsonld`
carries `{"id": "urn:ngsi-ld:OffStreetParking:2"}` with no type, and the IOP
job depends on it being accepted. Antares stays tolerant (typeless
EntityInfo restricts by id/idPattern only) so the official IOP suite runs;
strictly enforcing the cardinality would 400 that fixture. Raise upstream —
either the fixture or the table note should change.

## 2026-08-11 — 033_01_02 fixture: exclusive mode with idPattern EntityInfos

`data/csourceRegistrations/context-source-registration-with-mode-and-operations.jsonld`
registered `mode: exclusive` with two idPattern-only EntityInfos. 4.3.6.3:
an exclusive registration "relates to a specific Entity" — Antares (commit
0990944) rejects exclusive registrations whose EntityInfos use idPattern or
type-only groups, same argument as the nine `_exc` TPs fork-fixed on
2026-08-10. Fork fix: the fixture's mode becomes `redirect` (equally
non-default, legal with idPattern), keeping the TP's purpose — a non-default
mode with an operations list. Raise upstream together with the `_exc` batch.

## 2026-08-12 — 021_23: temporal orderBy by attribute contradicts 5.7.4.4

`021_23` (fork extension, init import) asserted 200 + ordering for temporal
queries with `orderBy=name`, `createdAt`, `name.subProperty`, `type,name`,
etc. CIM 009 V1.9.1 clause 5.7.4.4 is explicit: "If the ordering parameter
is present and refers an entity name other than \"id\", then an error of
type BadRequestData shall be raised" — the temporal query, unlike 5.7.2
(entity query, any member per 4.23), permits ordering ONLY by `id`.
Fork fix: cases rewritten — orderBy `id` / `id;desc` keep the 200+order
assertions; every non-id member now expects 400 BadRequestData via the new
`Query Temporal Entities With Rejected OrderBy` keyword.

## 2026-08-13 — Official dist-ops/IOP setups create entities before exclusive/redirect registrations (5.9.2.4 Conflict)

**Suite defect (14 official files + 1 IOP).** CIM 009 V1.9.1 5.9.2.4 (p.227-228)
added registration-vs-entity conflict rules: an **exclusive** registration
"shall" be refused with Conflict if an Entity already exists for the supplied
Entity ID carrying any registered Attribute; a **redirect** registration
"shall" be refused if ANY existing Entity matches it. The official `_exc`/`_red`
TPs (D005_01_exc, D006_01/02_red, D009_01_exc, D014_01/02_exc, D015_01_exc,
IOP RetrieveEntity IOP_CNF_04_02) pre-create the very entities their setup then
registers exclusive/redirect over — a spec-conformant broker must answer 409 and
the setup dies asserting 201. The redirect TPs scoped `idPattern=urn:ngsi-ld:Vehicle:*`
(D013_01/02_red, D014_01/02_red, D015_01_red, D016_01_red) additionally conflict
with ANY residual Vehicle from earlier tests, including ones earlier PASSING
tests legitimately leave behind (e.g. D014_01_inc's updated entities) — so they
fail even with perfect per-test hygiene. Seen in CI matrix run 2026-08-13 on all
three stores identically (65 failures, deterministic).

**Broker:** correct — `csource::check_entity_conflict`, message cites 5.9.2.4;
clause text verified verbatim against the PDF.

**Fork fix:** (a) genuine violators: setup order swapped — registration first,
then the local copies with `local=true` (entity creation carries no 5.9.2.4
restriction, so the same test state is reached legally); (b) the
`idPattern`-scoped `_red` setups start with `Purge Entities type=Vehicle
local=true` so residual Vehicles cannot legally 409 the registration;
(c) IOP RetrieveEntity IOP_CNF_04_02: B's local copy created after B's two
redirect registrations, `local=true`. D001_01/02/03_03_inc needed no edit —
their failures were pure cascade from the leaked setups. All touched trees
36/36 + 17/17 green against the strict broker locally. Status: to be raised
upstream (the official TPs predate the V1.9.1 conflict rules; their `since_v1.6.1`
tags corroborate).

## 2026-08-13 — 436_02 aux fixture declared redirectionOps (5.9.2.4 aux ops limit)

Fork extension TP 436_02 registered `mode=auxiliary` from the redirectionOps
fixture. 5.9.2.4 (p.228): auxiliary operations must be one of
retrieveOps/retrieveEntity/queryEntity "or a combination thereof", else
BadRequestData — the premise "aux reg whose ops would allow a write" is not
representable on a conformant broker. Fixture switched to
context-source-registration-vehicle-retrieve-ops.jsonld; the negative
(aux + write op → 400) is already pinned by extension TP 5922_01.

## 2026-08-13 — 053_07_01 timescale-cell flake: fixtures depend on live smartdatamodels.org

**Suite/environment defect.** CI matrix runs (6) and (9) failed
`053_07_01` in the timescale cell only. Decompressed robot log shows the
truth: the setup's `POST /subscriptions` answered **504 LdContextNotAvailable
"fetching http://smartdatamodels.org/context.jsonld: error sending request"**
— then `Retrieve Subscription` 404s and `Get From Dictionary jsonldContext`
raises KeyError on the ProblemDetails body (the setup keyword checks no
status). Mechanism: every sibling test's teardown deletes the Cached
@context row (`Delete a @context` over the fixture's URL list), and the
persisted row is the broker's one existence truth for Cached entries — so
each next test forces a LIVE refetch of smartdatamodels.org; one transient
network failure lands the 504 on whichever test draws it (the ts cell, being
the last of three parallel cells to hammer the host, drew it twice).

**Broker:** correct — LdContextNotAvailable 504 per Table 6.3.2-1 is exactly
what a failed remote @context fetch must produce.

**Fork fix:** the three fixtures naming smartdatamodels.org
(`subscription-with-implicitlycreated-contexts.jsonld`,
`@context-cached-valid.json`, `@context-cached-one-valid.json`) now use the
fork's own GitHub-raw-hosted context instead — the suite already depends on
raw.githubusercontent.com, so no new infrastructure. Test semantics
preserved (the tests need "a fetchable remote @context", not that specific
host). jsonldContext tree 63/63 locally on timescale (the 2 remaining local
fails are the standing 050_04/051_03 9090-hardcode artifacts, green in CI).

---

## 19. No TP can cover bracket-less q on a LanguageProperty (4.9 gap)

4.9 (p. 90) defines LanguageProperty matching only for `attr[lang]` and
`attr[*]`; bracket-less `attr=="x"` on a LanguageProperty is undefined in
the clause, so no spec-grounded TP is writable for it. Antares answers
"no match" (both evaluator and SQL prefilter, parity-pinned). Raised in
the broker repo's docs/upstream/etsi-raises.md context (error.md
2026-08-13 entry); do not add a TP asserting either behaviour until the
clause defines the case.

## 2026-08-17 — 047_06 fixture missed timesFailed (fixed in fork)

Table 5.2.14.2-1 defines `timesFailed` ("Number of times an unsuccessful
response (or timeout) has been received when delivering the notification")
as an output-only NotificationParams member, and 5.11.7 reuses
NotificationParams for csource registration subscriptions. The fixture
`1-timesSent-failed.json` asserted the failed-notification shape WITHOUT
it, so a broker emitting the spec-defined member failed the strict diff.
Fixture now carries `"timesFailed": 1` (deterministic: exactly one failed
send in the scenario).
