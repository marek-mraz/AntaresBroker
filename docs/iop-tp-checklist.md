# IOP TP checklist — 118 new spec-grounded interoperability tests

Written 2026-08-14 (AntaresBroker) on request: "at least 100 different IOP
tests, always based on specs, also edge cases". Every item cites its ETSI
CIM 009 V1.9.1 clause — the clause is the requirement source, never the
suite (claude.md rule 2). None duplicates the 102 existing IOP_TP cases
(inventory checked 2026-08-14: CNF_01-04, EXT_QRY_01/02, EXT_RET_01/02,
EXT_ADV_01/02, EXT_TMP_01/02, EXT_UPD_01/02).

Conventions: one file per group under
`IOP_TP/NGSI-LD/Interoperability/<area>/`, tags carry the clause
(`4_3_6_2` form), `[Documentation]` quotes the clause requirement briefly.
Harness: the 5-broker `dev/run-five.sh` stack (b1..b5 on 9090-9094) —
every item below is runnable on it locally (rule 8) unless marked CI-only.
Each TP keeps at least one negative assertion (what must NOT be in the
response). Work order: one group = one commit (`IOP_EXT_<GRP>:` prefix),
red-first where the behaviour is new.

## The /goal prompt — copy-paste this to write all 118 TPs to completion

```
/goal Work docs/iop-tp-checklist.md top-to-bottom until every checkbox is
[x] with commit hash + green-run evidence recorded next to it. One group =
one commit (`IOP_EXT_<GRP>:` prefix). Per item, full claude.md §0.3
discipline: PDF SPEC FIRST (hard rule, user 2026-08-14) — READ the cited
clause's actual PDF pages via mempalace_get_pdf_pages (mempalace_search
only locates them) and quote the requirement verbatim in [Documentation]
BEFORE writing the TP; the checklist item itself is a POINTER, not the
requirement — if the PDF text contradicts the item's wording, the PDF
wins and the item is corrected in the same commit. Never write a TP from
the ledger body, this checklist, memory, the suite, or the broker's
current behaviour; follow IOP_TP conventions (clause tags in
4_3_6_2 form, fixtures json.tool-validated, at least one NEGATIVE
assertion per TP — what must NOT be in the response). Validate every TP
locally per rule 8: --dryrun while iterating, then the REAL run on the
5-broker dev/run-five.sh stack (b1..b5_url 9090-9094,
notification_server_host:127.0.0.1, context_source_host:127.0.0.1) — a
red run blocks the group's commit. Where the broker is wrong the TP is
the red-first proof: fix the broker in the same commit (rule-9 scope test
applies) or prove a spec doubt and log it in error.md +
testsuite-doubts.md — never bend a TP to broken behaviour. The three
flagged known-red items (CAS_01_04 508-on-inclusive-loop, ERR_01_06/07
cooldown-vs-breaker posture, SUB_02_01/02 splitEntities merge) get broker
fixes with their own clause-prefixed commits + ledger updates. Use
ponytail throughout. Mac-side pushes + CI watch: list for the user, never
block. DONE = 118/118 [x] with evidence and the full IOP_TP tree green
locally on run-five.
```

## A. Registration semantics & modes — IOP_EXT_REG_01 (4.3.6.2/4.3.6.3, 5.2.9, 5.9.2.4/5.9.3.4, 4.20)

**DONE 2026-08-14** — TP file `IOP_TP/NGSI-LD/Interoperability/Registration/IOP_EXT_REG_01.robot`, 14/14 green on run-five. Red-first broker fixes in the same commit: FedReg::query_op (4.20 — retrieveEntity-only sources no longer receive query forwards) + CsrSpec.geo location gate (5.2.9). Repo commit 826afac.

- [x] REG_01_01 Registration without `mode` behaves as inclusive — remote data merged with local (5.2.9: mode default inclusive)
- [x] REG_01_02 Creating an exclusive CSR whose Attributes already exist on a local entity → 409 Conflict (5.9.2.4 — citation corrected from 5.9.4: PDF p.227-228, 5.9.4 is Delete CSR)
- [x] REG_01_03 Creating a redirect CSR when an existing local Entity already matches → 409 Conflict (5.9.2.4, p.228 — citation corrected from 5.9.4)
- [x] REG_01_04 Updating a CSR to exclusive when local conflicting Attributes exist → 409 Conflict (5.9.3.4, p.229 — citation corrected from 5.10.4: Update CSR is 5.9.3)
- [x] REG_01_05 Two overlapping redirect CSRs: the operation is distributed to BOTH context sources (4.3.6.3, p.42)
- [x] REG_01_06 Auxiliary data used ONLY where local/inclusive data is absent — auxiliary attr fills a gap, never shadows (4.3.6.2 — citation corrected: auxiliary is an additive mode, defined in 4.3.6.2)
- [x] REG_01_07 Auxiliary vs inclusive both matching the same attr: inclusive instance is served, auxiliary is NOT in the response (4.3.6.2 — citation corrected as above)
- [x] REG_01_08 Attribute-scoped CSR (propertyNames): query with attrs outside the registration is NOT forwarded (4.3.6.1 narrowing)
- [x] REG_01_09 relationshipNames-scoped CSR: only the registered names forward, forward narrowed to them (5.2.10 + 4.3.6.1 — reworded: prop-vs-rel discrimination of one name is not expressible at forward time; the PDF grounds name-scoped narrowing)
- [x] REG_01_10 idPattern-scoped CSR: retrieve of a non-matching id is served locally without a forward (5.2.9 entities.idPattern)
- [x] REG_01_11 CSR with `operations:["retrieveEntity"]`: queryEntity is NOT forwarded to it, retrieveEntity is (4.20)
- [x] REG_01_12 CSR with an operation GROUP (`"redirectionOps"`) expands to its member operations (4.20 Table)
- [x] REG_01_13 Default operations = `"federationOps"` — createEntity is NOT in it, so a create never forwards to a default-ops CSR (4.20, 5.6.1.4)
- [x] REG_01_14 Geo-scoped CSR (location member): only a geo query intersecting the registered geometry forwards (5.2.9 location)

## B. Cascading, loops, Via — IOP_EXT_CAS_01 (4.3.6.4, 6.3.17, 6.3.18, 5.2.34)

**DONE 2026-08-14** — TP file `IOP_TP/NGSI-LD/Interoperability/Cascading/IOP_EXT_CAS_01.robot` (fork commit db4cfea), 8/8 green on run-five. The flagged CAS_01_04 known-red resolved by broker commit 43ad459 (`6.3.17:` peer NGSILD-Warning propagation on forwarded reads + single-proxy 508 passthrough in combine_attr_parts — red-first via CAS_01_02/04/05).

- [x] CAS_01_01 Forwarded request carries a Via header naming the forwarding broker; two hops → two Via entries in order (6.3.18 Table 6.3.18-2 + 5.2.40 — citation corrected from 6.3.17: the Via/hostAlias mandate lives in 6.3.18, p.279)
- [x] CAS_01_02 Exclusive registration redirecting back onto the broker itself → 508 Loop Detected on unsafe methods (6.3.17, p.278 — the 508/504/404/502 list sits in the unsafe-methods paragraph; standing audit posture keeps reads on Table 6.3.17-1 warnings)
- [x] CAS_01_03 Redirect self-loop → 508 Loop Detected (6.3.17)
- [x] CAS_01_04 INCLUSIVE loop (b1→b2→b1) → NGSILD-Warning 199 + local data served, NOT 508 (6.3.17 — 508 is exclusive/redirect-only; edge the audit flagged as over-applied)
- [x] CAS_01_05 Auxiliary loop → NGSILD-Warning 199, response 200 (6.3.17)
- [x] CAS_01_06 Three-broker chain b1→b2→b3: entity found only on b3 reaches b1's client exactly once (4.3.6.4 dedup)
- [x] CAS_01_07 Diamond topology (b1→b2→b4, b1→b3→b4): b4's entity appears once in the union (4.3.6.4 duplicates)
- [x] CAS_01_08 localOnly on the forwarded leg: b2 answers from its own storage, b3 is never contacted — assert b3's access log empty (4.3.6.4, 5.2.34 localOnly)

## C. contextSourceInfo — IOP_EXT_CSI_01 (4.3.6.5, 4.3.6.6, 6.3.19)

**DONE 2026-08-14** — TP file `IOP_TP/NGSI-LD/Interoperability/ContextSourceInfo/IOP_EXT_CSI_01.robot` (fork commit 5aa07d5), 6/6 green on run-five first run — no broker gaps.

- [x] CSI_01_01 contextSourceInfo key/value pair becomes an HTTP header on the forwarded request — assert at the mock CS (4.3.6.5)
- [x] CSI_01_02 Multiple contextSourceInfo pairs → all present as headers, values verbatim (4.3.6.5)
- [x] CSI_01_03 `jsonldContext` key is PRE-PROCESSED: applied as the @context of the forward, NOT sent as a literal header (4.3.6.6)
- [x] CSI_01_04 `Authorization` pair reaches the CS; assert it is NOT echoed back to the client response (4.3.6.5 + negative)
- [x] CSI_01_05 contextSourceInfo on an inclusive reg applies to query forwards; absent pairs → no extra headers (negative baseline) (4.3.6.5)
- [x] CSI_01_06 Header-name case-insensitivity: key `accept` does not break the forward's own content negotiation (4.3.6.6 edge)

## D. Unitary query/retrieve — IOP_EXT_UNI_01 (4.3.6.7, 5.7.1.4, 5.7.2.4)

**DONE 2026-08-14** — TP file `IOP_TP/NGSI-LD/Interoperability/Unitary/IOP_EXT_UNI_01.robot` (fork commit f86607b), 8/8 green on run-five first run — no broker gaps.

- [x] UNI_01_01 limit=1 on a federated query returns ONE assembled entity even when its parts live on two brokers (4.3.6.7 unitary)
- [x] UNI_01_02 A split entity counts ONCE toward NGSILD-Results-Count (5.7.2.4 + 6.3.13)
- [x] UNI_01_03 q filter over a split entity: predicate satisfied only by the COMBINATION of local+remote attrs matches (5.7.2.4 aggregate filter, splitEntities=true) — written as a THREE-way split (b1+b2+b3) since the two-way case is already IOP_EXT_QRY_02_03
- [x] UNI_01_04 geo filter applied post-aggregation: geometry lives remote, q attr lives local, both must hold (5.7.2.4)
- [x] UNI_01_05 pick projection applied AFTER aggregation — picked attr exists only remotely, survives (5.7.2.4)
- [x] UNI_01_06 attrs= selector over the union: entity qualifying only via a remote attr is included (5.7.2.4 S-order)
- [x] UNI_01_07 Stable pagination: entity created remotely mid-walk does not appear in later pages of the pinned map (4.3.6.7, 5.14.1.1)
- [x] UNI_01_08 Retrieve unitary merge: same attr same datasetId — newest observedAt wins regardless of which broker holds it (4.3.6.3 conflict rule, both directions in one TP with two datasetIds)

## E. Distributed provision — IOP_EXT_PRV_01/02 (5.6.1.4–5.6.10.4, 5.6.17.4, 4.20)

**DONE 2026-08-14** — TP files `IOP_TP/NGSI-LD/Interoperability/Provision/Distributed/IOP_EXT_PRV_01/02.robot` (fork commit 8d24c27), 16/16 green on run-five; full IOP_TP tree 154/154. Red-first broker fixes in the repo commit: distributed batch S/E arrays per 5.2.16 (were opaque combine parts), batch single-op fallbacks no longer inherit ?options (the PRV_02_06 400), DELETE ?type gates CSR matching (5.6.6.4/4.17), inline request @context now travels in forwarded bodies as ld+json (5.6.1.4/6.3.5). Related: cd4bc5a (parallel session) fixed the 5.8.5.4 delete-vs-create race the crate suite surfaced mid-group.

- [x] PRV_01_01 Create forwarded to a redirect CSR supporting createEntity: entity lives on b2 only — assert b1 local=true query does NOT return it (5.6.1.4)
- [x] PRV_01_02 Create matching a redirect CSR NOT supporting createEntity → 409 Conflict (5.6.1.4, p.160)
- [x] PRV_01_03 Create matching an inclusive CSR without createEntity support: forward skipped silently, local create 201 (5.6.1.4)
- [x] PRV_01_04 Forwarded create carries the request @context — remote stores expanded terms identically (5.6.1.4 + 6.3.5)
- [x] PRV_01_05 Update Attributes split across brokers: local part + redirect part both applied; 204 when all succeed (5.6.2.4 attr-parts)
- [x] PRV_01_06 Update where the remote part fails → 207 multi-status naming the failed attrs (5.6.2.4)
- [x] PRV_01_07 Append (noOverwrite) on a remotely-held attr → remote 207/appended; local copy still absent (negative) (5.6.3.4)
- [x] PRV_01_08 Partial attribute update targeting a redirect-held attr forwards; unknown attr on both sides → 404 (5.6.4.4)
- [x] PRV_02_01 Delete entity spanning local+redirect: both halves gone; re-retrieve → 404 (5.6.6.4)
- [x] PRV_02_02 Delete with ?type not matching the redirect-held entity: nothing deleted anywhere (4.17 + 5.6.6.4 negative)
- [x] PRV_02_03 deleteAttribute of a remote-only attr forwards and returns 204; other attrs untouched (5.6.5.4)
- [x] PRV_02_04 Batch create with items matching different redirect CSRs: each lands on its broker (5.6.7.4)
- [x] PRV_02_05 Batch create where one remote item fails → 207 BatchOperationResult with success[] and errors[] correctly split (5.6.7.4, 5.2.16)
- [x] PRV_02_06 Batch upsert update-mode over a redirect-held entity forwards as update (5.6.8.4)
- [x] PRV_02_07 Batch delete spanning brokers: 207 lists the remote 404 item, local ones deleted (5.6.10.4)
- [x] PRV_02_08 Merge entity (PATCH /entities/{id}) with a redirect-held attr forwards the merge fragment (5.6.17.4)

## F. Distributed subscriptions — IOP_EXT_SUB_01/02 (5.8.1.4–5.8.6, 5.2.12)

**DONE 2026-08-14** — TP files `IOP_TP/NGSI-LD/Interoperability/Subscription/IOP_EXT_SUB_01/02.robot` (fork commit 0f9c87a), 14/14 green on run-five. The flagged SUB_02_01/02 known-red resolved by broker commit 1ad0fdf (`5.8.6:` split-merged notifications shaped/compacted like local ones + `5.8.1.4:` absent watchedAttributes narrowed to registered names — red-first via SUB_02_01/SUB_01_03). splitEntities merge itself had landed earlier (cb385a8); the gap was presentation. Official HTTP Subscription tree 125/125 (058_x MQTT = CI-only).

- [x] SUB_01_01 Subscription update on b1 (new q) re-narrows the remote sub — old-q change no longer notifies (5.8.2.4)
- [x] SUB_01_02 isActive=false on b1 pauses the chain: remote change produces NO notification; reactivation resumes (5.8.2.4, 5.2.12)
- [x] SUB_01_03 watchedAttributes reduced copy: remote sub watches only the CSR-scoped attrs — unrelated remote attr change does not notify (5.8.1.4)
- [x] SUB_01_04 timeInterval (periodic) subscription over federated data: each tick queries remote and notifies the union (5.8.1.4 + 5.2.12)
- [x] SUB_01_05 Notification data from the remote broker arrives unmodified at the subscriber (values byte-equal) (5.8.6)
- [x] SUB_01_06 notification fires when a remote entity ENTERS the q filter (5.8.6 — reworded: Table 5.3.1-1 defines no triggerReason on entity Notifications; that member is CSourceNotification-only, 5.3.2/5.3.3)
- [x] SUB_01_07 no notification when the remote update LEAVES the filter; re-entry notifies (5.8.6 — reworded, same 5.3.1 ground)
- [x] SUB_01_08 Entity deleted on b2 → deletedAt-only notification through the chain (5.8.6, 5.2.12, 4.5.7 — corrected: default notificationTrigger excludes deletions, the sub must list entityDeleted)
- [x] SUB_02_01 splitEntities=true: one merged notification entity assembled from b2+b3 parts (5.8.6 merge block; deployment default off — enable per-sub)
- [x] SUB_02_02 splitEntities merge honours q on the AGGREGATE (5.8.6 + 5.2.23 splitEntities row)
- [x] SUB_02_03 Sub created BEFORE the CSR: registering the CSR later wires the chain (registration-triggered sub propagation, 5.8.1.4)
- [x] SUB_02_04 CSR deleted → remote leg dismantled; remote change stops notifying while local changes still do (5.8.5.4 negative pair)
- [x] SUB_02_05 Two subs sharing one CSR: deleting one keeps the other's chain alive (5.8.5.4 refcount edge)
- [x] SUB_02_06 Remote notification whose subscriptionId is unknown (stale/forged) → dropped, no client delivery (5.8.6 inbound gate + security negative)

## G. CSR subscriptions & notifications — IOP_EXT_CSN_01 (5.11.2–5.11.7, 5.3.2, 6.3.9)

**DONE 2026-08-14** — TP file `IOP_TP/NGSI-LD/Interoperability/CSourceSubscription/IOP_EXT_CSN_01.robot` (fork commit df4a012), 8/8 green on run-five first run — no broker gaps.

- [x] CSN_01_01 Subscribe to CSRs (POST /csourceSubscriptions); creating a matching CSR on the same broker notifies with the CSR body (5.11.2, 5.3.2)
- [x] CSN_01_02 Initial csourceNotification carries ALL currently-matching CSRs (5.11.2)
- [x] CSN_01_03 CSR update fires a csourceNotification with the changed registration (5.11.3)
- [x] CSN_01_04 CSR deletion notifies; subsequent CSR churn does not (5.11.6 + negative)
- [x] CSN_01_05 csourceSubscription entities filter: CSR for a different type does NOT notify (5.11.2 negative)
- [x] CSN_01_06 csourceNotification format members (id, type=ContextSourceNotification, subscriptionId, notifiedAt, data[]) exact (5.3.2 table — assert no extra members)
- [x] CSN_01_07 Update csourceSubscription watched type; old type stops, new type starts notifying (5.11.3)
- [x] CSN_01_08 Query + retrieve csourceSubscriptions return the stored subs; unknown id → 404 ResourceNotFound (5.11.4/5.11.5)

## H. Distributed temporal edges — IOP_EXT_TMP_03 (5.7.3.4, 5.7.4.4, 4.18)

- [ ] TMP_03_01 aggrMethods (avg) computed over the MERGED series, not per-broker (5.7.4.4 post-aggregation + 4.5.19)
- [ ] TMP_03_02 scopeQ on a federated temporal query: remote instances filtered validity-aware (5.7.4.4 S4 + 4.18)
- [ ] TMP_03_03 timeproperty=modifiedAt window against remote instances (5.7.4.4 + 5.2.21)
- [ ] TMP_03_04 Temporal pagination over a merged series: pages partition the union, no instance repeats (5.7.4.4 + 6.3.10)
- [ ] TMP_03_05 Deleted attribute instance (urn:ngsi-ld:null marker) merged from remote appears as deletion (4.5.7 + 5.7.3.4)
- [ ] TMP_03_06 Remote temporal 404 with local instances present: local series served + NGSILD-Warning (5.7.3.4 + 6.3.17)
- [ ] TMP_03_07 CSR with observationInterval matching only part of the window: forward happens only when intervals intersect (5.2.9)
- [ ] TMP_03_08 POST /temporal/entityOperations/query federates like GET (6.24 binding parity over the dist path)
- [ ] TMP_03_09 lastN over merged series where remote alone has >lastN instances — cap applies after merge (5.7.4.4 + 5.2.21 window-scoped)
- [ ] TMP_03_10 Temporal instance datasetId collision across brokers, same timestamp → one instance survives the merge (4.5.5.x dedup; mirrors the 4.5.5.3 same-slot rule)

## I. EntityMaps distributed — IOP_EXT_MAP_01 (5.14, 5.7.2.4, 6.3.18)

- [ ] MAP_01_01 EntityMap from a federated query lists per-entity source brokers ("@none" for local) (5.2.39)
- [ ] MAP_01_02 Expired map presented via NGSILD-EntityMap → fresh map recreated, response 201-style new map id (5.14.1.1)
- [ ] MAP_01_03 Pinned map keeps serving an entity DELETED remotely mid-walk (map fixes the id set) — with the entity's current absence tolerated per 5.7.2.4 map usage
- [ ] MAP_01_04 Map built from a scoped query never leaks non-matching remote ids on later pages (5.7.2.4 "created based on S4" analogue + negative)
- [ ] MAP_01_05 PATCH /entityMaps/{id} expiresAt extends a federated map's life; other members rejected (5.14.3)
- [ ] MAP_01_06 DELETE /entityMaps/{id} → later use of the header on that id recreates instead of 404-ing the query (5.14.4 + 5.14.1.1)
- [ ] MAP_01_07 Temporal federated query creates a temporal-mode map; reuse serves identical instance sets (5.7.4.4 map arm)
- [ ] MAP_01_08 Map id in the NGSILD-EntityMap RESPONSE header is a Location-style path — client strips to the last segment (6.4.3.2-2 trap regression)

## J. Tenancy across brokers — IOP_EXT_TEN_01 (4.14, 6.3.14, 5.2.9 tenant)

- [ ] TEN_01_01 Client tenant header propagates on the forward when the CSR has no tenant override (6.3.14)
- [ ] TEN_01_02 CSR tenant member OVERRIDES the client tenant on the forward — assert remote received the override, not the client's (5.2.9 tenant)
- [ ] TEN_01_03 Remote NonexistentTenant (404) degrades to local data + NGSILD-Warning, not a client-visible 404 (6.3.14 + 6.3.17)
- [ ] TEN_01_04 Default-tenant client + tenant-scoped CSR: data written remotely lands in the CSR's tenant only (negative: default tenant on b2 stays empty) (5.2.9)
- [ ] TEN_01_05 Same entity id in two tenants across brokers never merges (4.14 isolation + 4.3.6.3)
- [ ] TEN_01_06 Distributed subscription chain preserves tenant end-to-end: notification carries the origin tenant (5.8.1.4 + 6.3.14)

## K. Errors, timeouts, resilience — IOP_EXT_ERR_01 (6.3.2, 6.3.17, 5.2.34, §16.7 posture)

- [ ] ERR_01_01 Peer answering 500 on query: union = local + warning; the 500 body is NOT merged into results (6.3.17 + negative)
- [ ] ERR_01_02 Peer timeout (stalling socket) on query → local data + NGSILD-Warning 199 within the deadline budget (6.3.17)
- [ ] ERR_01_03 Redirect-only retrieve with the CS down → 503/504 class error to the client (nothing local to serve) (5.7.1.4 + Table 6.3.2-1)
- [ ] ERR_01_04 Malformed JSON from a peer (mock returns garbage 200): treated as source failure, warning, local served (5.7.2.4 robustness)
- [ ] ERR_01_05 management.timeout on the CSR bounds the forward — 1 ms timeout forces the warning path even on a fast peer (5.2.34)
- [ ] ERR_01_06 management.cooldown: after a failure, a re-query within cooldown does NOT contact the CS (assert mock hit-count static), after it, does (5.2.34)
- [ ] ERR_01_07 A responding-but-erroring CS keeps being attempted on every request — no breaker starvation (6.3.8 posture, commit 50bcefb regression TP)
- [ ] ERR_01_08 NGSILD-Warning format: `199 <origin> "<msg>"` parseable, one warning per failed source, none per healthy source (6.3.17)
- [ ] ERR_01_09 Forwarded op returning 401/403 from the CS surfaces as warning on inclusive, error passthrough on redirect (4.3.6.2 asymmetry)
- [ ] ERR_01_10 Distributed delete where remote succeeds and local entity absent → 204 (remote-only success is success) (5.6.6.4)
- [ ] ERR_01_11 207 response schema on partial dist update: updated[] + notUpdated[] with attributeName+reason exact (5.2.19 — assert no extra members)
- [ ] ERR_01_12 Fed 404 on retrieve with NO local data and inclusive reg → 404 to client WITH the warning preserved (6.3.17, mirrors TP 6317_01 on the IOP stack)

## L. Cross-cutting — IOP_EXT_MSC_01 (csf, GeoJSON, lang, snapshots, counts)

- [ ] MSC_01_01 csf selects by CSR property (csf=endpoint=="...") — only matching CSRs consulted; assert the excluded mock got no hit (5.2.23 csf + negative)
- [ ] MSC_01_02 csf with timerel over CSR sysAttrs (registered-after) gates forwards (5.2.23)
- [ ] MSC_01_03 GeoJSON federated RETRIEVE of a split entity: geometry from remote default GeoProperty, properties merged (4.5.16 + 5.7.1.4)
- [ ] MSC_01_04 lang=* on a remote LanguageProperty returns the full languageMap through the merge (4.5.18)
- [ ] MSC_01_05 Snapshot fill (5.16.1.4) executes its snapshotQueries over the DISTRIBUTED path — snapshot contains remote entities; status=success
- [ ] MSC_01_06 Snapshot fill with one dead source → status=partial with ExecutionResultDetails naming it (5.16.1.4, 5.2.42)
- [ ] MSC_01_07 count=true on a three-broker union equals the deduped entity count, not the sum of broker counts (6.3.13 + 4.3.6.4)
- [ ] MSC_01_08 options=sysAttrs on a federated query: remote createdAt/modifiedAt survive; the FORWARDING broker's own timestamps do NOT replace them (4.5.2 + negative)

## Tally

| Group | Items | | Group | Items |
|---|---|---|---|---|
| REG | 14 | | CSN | 8 |
| CAS | 8 | | TMP_03 | 10 |
| CSI | 6 | | MAP | 8 |
| UNI | 8 | | TEN | 6 |
| PRV | 16 | | ERR | 12 |
| SUB | 14 | | MSC | 8 |

**Total: 118** — all new, all clause-cited, edge cases throughout (every
group carries its error-path and negative-assertion items; CAS_01_04,
REG_01_02-04, ERR_01_* and SUB_02_06 exist ONLY as edge cases).

Known broker gaps these TPs will hit red-first (fix or doubt-log per
§0.3): 508 over-application on inclusive loops (CAS_01_04 — audit
finding), management.cooldown semantics vs the §16.7 breaker posture
(ERR_01_06/07 — reconcile per-reg cooldown with the timeout-only breaker,
the clause's own MAY allows override but the TP must pin the chosen
posture), splitEntities notification merge default-off (SUB_02_01/02).
