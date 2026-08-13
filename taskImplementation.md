# taskImplementation.md — temporal q= work: what is real, what is missing

Analysis run 2026-08-13 (AntaresBroker) on request: *"is there something
missing / not implemented / faked or lazy-implemented?"* Scope: the temporal
query surface touched by the q= pushdown work (commits `b37aefb`,
suite-fork `5744_01`/`5744_02`) plus a silent-ignore sweep of every
parameter the temporal query endpoint accepts.

## Verdict in one paragraph

One real conformance gap was found and reproduced: **`scopeQ` is accepted
on temporal queries but never applied locally** — 5.7.4.4 S4 is
unimplemented while the ledger says `implemented`. Everything else checked
is either genuinely implemented (with test evidence) or a *documented*
performance ceiling with the arbiter guaranteeing correctness — slower,
never wrong. No case was found where a wrong answer can be served.

---

## 1. CONFORMANCE GAP (must fix): scopeQ ignored on temporal queries

**Spec:** 5.7.4.4 S4 (p.209): "If the Scope query is present, from S3,
select those Entities whose Entity Scope instances match the Scope query
(as mandated by clause 4.19)."

**Reproduced** against the debug memory broker, 2026-08-13:

```
POST /temporal/entities   {"id":"urn:...:scopetest","type":"Vehicle","scope":"/A/B",...}  → 201
GET  /temporal/entities?type=Vehicle&scopeQ=/A/#&timerel=after&timeAt=...  → returns it  (correct)
GET  /temporal/entities?type=Vehicle&scopeQ=/X&timerel=after&timeAt=...    → returns it  (WRONG — must be filtered out)
```

**Why it was missed:** `"scopeQ"` sits in the accepted-params whitelist
(`temporal.rs` ~1400) so no 400 is raised; `fed_query_temporal` forwards
it to remote brokers (the 5.7.4.4 ledger note even mentions that
forwarding); but the local S2/S3 evaluation loop (temporal.rs ~1634–1704)
filters on ids/idPattern/type/attrs/q/geo/window only — no
`scope_matches` call. The official 021 TP set has no scopeQ-on-temporal
case, so the Robot oracle never caught it. Ledger `docs/spec/5/5.7.4.4.md`
carries `status: implemented` — **overstated** until fixed.

**Affected endpoints:** `GET /temporal/entities` and the POST query
operation that routes through the same path.

**Proposed fix (small, matches the entities path):** in the S-loop, after
the geo check, apply `crate::scope_matches(sq, &doc)` — S4 note: scope is
judged on Entity Scope *instances*; the temporal doc keeps instance-shaped
scope arrays (`scope_instances` handling already exists in the renderer,
temporal.rs ~450). Add TP `5744_03` (scopeQ match / non-match / `/#` /
alternatives, on temporal + windowed scope instances), flip the ledger
note, one commit `5.7.4.4:`.

**Not fixed in this analysis run** — per the standing measure-vs-modify
rule this document is the deliverable; say the word and it gets built.

## 2. Silent-ignore sweep of the accepted temporal-query parameters

Checked each whitelisted parameter for "accepted but unimplemented":

| Param | State | Evidence |
|---|---|---|
| id, idPattern, type, attrs | applied | S-loop + store filter; 021 trees 156/156 |
| q | applied | 5744_01/02 27/27; parity battery |
| georel/geometry/coordinates/geoproperty | applied (S3, windowed) | 5744_02_14 |
| **scopeQ** | **ACCEPTED, IGNORED locally** | repro above — the gap |
| csf | applied | csf gate (commit 360731f), TP 5724_01 |
| timerel/timeAt/endTimeAt/timeproperty | applied | 021 trees; compile::temporal tests |
| aggrMethods/aggrPeriodDuration | applied | 4.5.19 audit, 021 aggregation TPs |
| lastN | applied | RANK() cap + API window; parity tie test |
| limit/offset/count | applied | 6.3.10/6.3.13 audits, paging pushdown tests |
| options/format, lang, pick/omit, datasetId | applied | 4.5.x audits; trepr wiring |
| local | applied | 5.5.13 audit |
| entityMap | applied | e61f6b9, TPs 5714_01/5734_01 |
| orderBy (+collation) | applied, local-only guard | TP 5741_01_06; 4171ff4 |

No other silent ignore found.

## 3. Deliberate ceilings in the q= pushdown (documented laziness, not fakes)

All of these are *correct today* because the in-memory evaluator re-runs on
every returned row (superset contract, proven by
`temporal_q_prefilter_narrows_but_never_drops`). They are listed because
each leaves performance on the table, with the upgrade path named:

1. **lastN withheld when q/geo present** (`temporal.rs` gate). The spec
   leaves lastN-vs-q ordering ambiguous (p.208 vs S2). Upgrade: resolve the
   doubt (raise upstream), then push the RANK() cap with q.
2. **Entity paging withheld when q/geo present.** `limit=1` with q still
   materializes every prefilter-passing entity. Upgrade: page in SQL when
   the prefilter compiled *exactly* (no leaf refused, no widening
   dependence) — needs an exactness flag on the compiler.
3. **No geo prefilter.** geoQ benefits from range pruning only; the
   `geo_value geometry` column on `attr_instances` sits unused by the
   prefilter. Upgrade: a windowed `EXISTS` with a PostGIS predicate in the
   same qprefilter framework (the slot is designed for it).
4. **Refused leaf shapes narrow nothing**: `!=`, `~=`/`!~=`, negated
   existence, dotted paths, `[lang]` brackets, linked hops, string
   ordering, `Or` with any refused branch. Each is a compiler extension
   candidate; each currently costs a full-candidate-set scan when it is the
   *only* filter.
5. **timeproperty=deletedAt has no column bound** (decompose fills no
   deleted_at) — text-only pruning, no index assist. Rare in practice.
6. **±48 h window widening** admits up to 4 extra days of instances into
   the reconstruction before the text predicate trims them — the price of
   offset-safety. Note: the API rejects offset stamps (4.6.3), so this
   guards only store-level data; it could be tightened to Z-only exactness
   if that invariant is ever formalized.
7. **Unpaged internal callers** (snapshot fill "all pages", EntityMap
   temporal candidates) still reconstruct full candidate sets — they
   benefit from the column bound but not from paging.

## 4. Test-coverage debt

- **scopeQ-on-temporal has zero TPs** (official and fork) — add with the
  fix (see §1). This hole is what let the gap survive an `implemented`
  audit.
- languageMap + bracket-less q (`label=="hi"`) is pinned only by the parity
  battery deriving expectation from `eval_q` itself (self-grounding); a
  spec-grounded TP would need the 4.9 doubt (is bracket-less on a
  LanguageProperty defined at all?) resolved first.
- The 4×8 CI matrix has not yet run over the new prefilter (pushed
  commits pending Mac-side) — the pg/timescale cells are where 5744_01/02
  actually exercise the SQL path.

## 5. Task checklist (proposed order)

- [ ] 1. `5.7.4.4:` implement S4 scopeQ on the temporal S-loop +
      TP 5744_03 + ledger note fix (the only *conformance* item).
- [ ] 2. Watch the next CI matrix run: 4×8 must be green over the
      prefilter + the two new TP files on pg/timescale.
- [ ] 3. Geo prefilter on `geo_value` (biggest remaining perf win for
      geo-filtered temporal queries).
- [ ] 4. Exactness flag on qprefilter → SQL paging with fully-compiled q.
- [ ] 5. Raise the lastN-vs-q ordering doubt upstream
      (docs/upstream/etsi-raises.md has the vehicle).
- [ ] 6. Optional compiler extensions: string-ordering leaves (needs a
      collation-safe strategy), `[lang]` brackets (jsonpath on
      `languageMap."en"` is expressible), `!=` (needs the p.91/92
      datatype-mismatch semantics reproduced exactly).

Items 3–6 are performance/robustness; item 1 is the only place the broker
currently gives a spec-wrong answer.
