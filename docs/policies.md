# Granular Access Policies for Federated Digital Twins

Status: design documentation, 2026-08-14
Scope: the access-control layer in front of NGSI-LD Context Brokers —
policy model, identity stack, pre-query discovery, enforcement wiring,
and the open problems. Companion to the requirements document
(R1–R43 + MIM-derived Part II) and its ADRs 001–011.

Antares's role in this design is deliberately small: the broker stays
policy-free (standing decision).
Everything below lives in the gateway (APISIX PEP), the PDP (OPA), and
the identity components — the broker only stores, serves, and notifies.

---

## 1. Identity stack (IDM decision)

**Decision: keep Keycloak, demoted to one role — OIDC for humans/services
plus Verifiable Credential issuance. Verification and authorization live
elsewhere.**

| Role | Component | Rationale |
|---|---|---|
| OIDC IdP + VC issuer | Keycloak 26.x | Native OID4VCI since the 26.6 realm-model rewrite; reference IAM of the FIWARE Data Space Connector; FIWARE plugins (`keycloak-vc-issuer`, `keycloak-jades-vc-issuer` for eIDAS-grade JAdES) |
| VC/VP verification | FIWARE VCVerifier | Keycloak issues but does not verify presentations for API access. VCVerifier speaks OID4VP/SIOPv2 and exchanges a verified presentation for a plain JWT that the APISIX `openid-connect` plugin consumes unchanged |
| Trust anchor | FIWARE Trusted Issuers Registry | "May this did:web mint this credential type" — the ADR 001 admission rule and R24 |
| PDP | OPA | evaluates the NGSI-LD `Policy` model (R3); see §4 |

Key consequence: **peer FDTs never get accounts in Keycloak.** Their
identity is did:web; trust flows through the issuers registry. 100
federated twins ≠ 100 realms.

Alternatives considered and rejected as the primary IDM:
WSO2 Identity Server 7.3 (only other full IDM with native OID4VCI +
agent identity; heavy, no FIWARE glue), walt.id (best pure VC stack,
not an IDM), Zitadel/Authentik/Ory (good OIDC, zero VC support),
FIWARE Keyrock (legacy XACML/AuthZForce path — avoid for new builds).

**Do not use Keycloak Authorization Services / UMA for the policies.**
Its model cannot express attribute-level grants or `q`/`scopeQ`/`geoQ`/
`temporalQ` conditions, token-embedded permissions go stale against
revocation, and it welds the platform to one vendor's engine, killing
the ODRL round-trip (R26).

Token hygiene: short-lived access tokens, RFC 8693 token exchange for
service-to-service hops, DPoP sender-constraining at the edge. Tokens
carry identity only (`sub`, did) — never permissions.

## 2. The policy model

There is no ratified standard for NGSI-LD access policies. XACML is
legacy (dying ecosystem, no NGSI-LD awareness); W3C ODRL 2.2 is a
rights-*expression* standard — right for exchange, too loose for
enforcement; Rego is an OPA implementation detail.

**Internal model (ADR 002): an NGSI-LD `Policy` entity whose granularity
vocabulary is the spec's own** — `EntityInfo` (CIM 009 clause 5.2.8),
`RegistrationInfo` (Table 5.2.10-1), operation names from the
distributed-operations vocabulary (R8), matching semantics identical to
clause 5.12 CSR matching. **External face: ODRL via the ADR 003 mapper.**
FIWARE's DSBA/i4Trust PDP takes the same approach (policies naming
NGSI-LD types and attributes), so this aligns with ecosystem precedent.

### 2.1 Examples (keyValues form; stored normalized)

Broad read with attribute projection — Alice may query all ParkingSpots
in BB but sees only three attributes:

```json
{
  "id": "urn:ngsi-ld:Policy:bb:parking-read:a-017",
  "type": "Policy",
  "assignee": "did:web:users.bb.sk:alice",
  "assigner": "did:web:bb.sk",
  "operations": ["queryEntity", "retrieveEntity", "createSubscription"],
  "information": [{
    "entities": [{ "type": "ParkingSpot" }],
    "propertyNames": ["status", "occupancy", "location"]
  }],
  "scopeQ": "/geo/SK/BB"
}
```

Entity-specific write grant — Bob maintains exactly one streetlight and
may change exactly two attributes:

```json
{
  "id": "urn:ngsi-ld:Policy:bb:lamp-maint:b-042",
  "type": "Policy",
  "assignee": "did:web:users.bb.sk:bob",
  "assigner": "did:web:bb.sk",
  "operations": ["retrieveEntity", "updateAttrs"],
  "information": [{
    "entities": [{ "id": "urn:ngsi-ld:Streetlight:bb:district4:L-0042",
                   "type": "Streetlight" }],
    "propertyNames": ["status", "maintenanceNote"]
  }],
  "q": "managedBy==\"urn:ngsi-ld:Org:bb:public-works\""
}
```

Enforcement of the second policy: `GET .../L-0042` works, projected to
the granted attributes; `GET .../L-0043` returns **404, not 403** (R20 —
no existence disclosure); a PATCH touching `status` succeeds; a PATCH
touching `status` **and** `powerRating` is rejected whole with a
ProblemDetails body naming `powerRating`. Writes never silently narrow
(asymmetric with reads, deliberately). For a fleet, swap `id` for an
anchored `"idPattern"` (ADR 001 URN prefixes).

### 2.2 The algebra

- **Permit-only, default deny** (R5 fail-closed). No deny rules, ever —
  deny rules buy XACML's combining-algorithm misery for nothing.
- Effective rights = **OR across a user's policies**, each policy's own
  constraints AND-ed inside it (R12 — no cross-policy bleed, ADR 006).
- **Revocation = delete or expire the policy.**
- "What attributes can I change" = `propertyNames`/`relationshipNames`
  of policies whose `operations` contain a write op. Read grants and
  write grants are separate policy entries.
- Time-bound grants: `validFrom`/`validTo` properties evaluated by the
  PDP — expiry logic stays out of the broker.

### 2.3 ODRL face (cross-boundary, ADR 003)

```json
{
  "@context": "http://www.w3.org/ns/odrl.jsonld",
  "@type": "Agreement",
  "uid": "urn:ngsi-ld:Policy:bb:lamp-maint:b-042",
  "assigner": "did:web:bb.sk",
  "assignee": "did:web:users.bb.sk:bob",
  "permission": [{
    "target": "urn:ngsi-ld:Streetlight:bb:district4:L-0042",
    "action": "ngsi-ld:updateAttrs",
    "constraint": [
      { "leftOperand": "ngsi-ld:attrs", "operator": "isAnyOf",
        "rightOperand": ["status", "maintenanceNote"] }
    ]
  }]
}
```

NGSI-LD operations and `attrs` come from a small ODRL Profile (the
standard extension mechanism). Round-trip is lossless: `operations`→
`action`, `entities`→`target`, attribute lists and `q`/`scopeQ`→
`constraint` (R26).

## 3. Policies live in the broker

Policies are stored in a **dedicated policy tenant on the same broker**.

- **Bootstrap meta-policy**: every authenticated identity may
  `queryEntity`/`retrieveEntity`/`createSubscription` on type `Policy`;
  the gateway injects `q=assignee=="<own did>"` using the same R11
  rewriting machinery. The policy store secures itself with its own
  mechanism.
- **Users and agents subscribe to their own policies** and learn about
  every grant, change, and revocation automatically:

```json
{
  "type": "Subscription",
  "entities": [{ "type": "Policy" }],
  "notificationTrigger": ["entityCreated", "attributeUpdated", "entityDeleted"],
  "notification": { "endpoint": { "uri": "https://…/my-policy-inbox",
                                  "accept": "application/ld+json" } }
}
```

  **TRAP: `notificationTrigger` must list `entityDeleted` explicitly.**
  The clause 5.2.12 default excludes deletion triggers, and a revocation
  *is* a deletion — the default subscription would announce new grants
  while staying silent about lost ones.
- One subscription mechanism serves three consumers: the portal updates
  its permission matrix live, agents re-plan (MIM3-R11), and the **PEP
  invalidates its compiled policy cache on the same trigger** (R40) —
  revocation latency is one notification hop, not a TTL.
- Free from the broker: the temporal API records policy history, so
  "who could touch what, when" (R42) is an ordinary temporal query.
- Guardrails: the policy tenant never gets a CSR (policies cross
  boundaries only as ODRL through the mapper); policies never carry
  secrets.

## 4. Pre-query knowledge ("know before you ask", R16–R22)

Industry survey — five recurring patterns:

| Pattern | Exemplars | Fit here |
|---|---|---|
| Reverse-query list APIs | Zanzibar family: SpiceDB `LookupResources`, OpenFGA `ListObjects` | weak at attribute/condition grants; not needed as a separate API — R18 makes an ordinary query return exactly the accessible set |
| Partial evaluation / filter push-down | OPA Compile API, Postgres RLS, Oracle VPD, Hasura row+column permissions (role-scoped schema introspection) | **the chosen core** |
| Resource-attached advertisement | Solid `WAC-Allow` header (clients MUST discover modes via GET/HEAD) | precedent for `NGSILD-Results-Restricted` + a per-entity access-modes header |
| Ticket negotiation | UMA 2.0 permission tickets | inverted (learn by failing); its "you lack X, request it here" loop = the R21 access-request flow |
| Capability tokens | Biscuit (offline attenuation, Datalog in the token), macaroons | attractive for cross-FDT agent delegation later; fights revocation — add-on, never core |

The credible systems derive "what can I access" from the **same compiled
policy that enforces** (Hasura, OPA) — never from a second bookkeeping
system, which drifts. Hence:

- `GET /access/permissions` — effective-permissions document per entity
  type: operations, id patterns, readable attrs, writable attrs, scopes,
  conditions. Generated by **OPA partial evaluation**; the residual
  constraint set IS both this answer and the injected query filter, so
  discovery and enforcement cannot diverge.
- `POST /access/check` — dry-run: permit/deny + matched policy + on
  deny, the missing grant → seeds the R21 access-request flow. Plain
  Data API call, no unknowns, never touches the broker.
- `GET /access/policies` — the caller's raw `Policy` entities (ordinary
  gateway-filtered query).
- `ScopeDefinition` tree (R19) renders the permission hierarchy for UIs.
- `NGSILD-Results-Restricted: true` (R22) whenever injected constraints
  removed anything the raw query would have matched.

## 5. Enforcement wiring (OPA Compile API)

Policies flow into OPA **as data, not as Rego**: the policy-change
notification consumer pushes `PUT /v1/data/policies/<assignee-did>` /
`DELETE` on every change (push over bundles — bundles are eventually
consistent; revocation must land in one hop). One generic Rego module
interprets the data:

```rego
package ngsild.filters

# METADATA
# scope: document
# compile:
#   unknowns: [input.entity]
include if {
    some p in data.policies[input.assignee]
    input.operation in p.operations
    entity_matches(p, input.entity)      # id/idPattern/type per 5.2.8
    scope_within(p.scopeQ, input.entity.scope)
    q_holds(p.q, input.entity)
}
```

Per request, the gateway calls the data-filter Compile endpoint with the
**UCAST** target (`Accept: application/vnd.opa.ucast.minimal+json`):

```http
POST /v1/compile/ngsild/filters/include
{ "input": { "assignee": "did:web:users.bb.sk:bob",
             "operation": "queryEntity", "type": "Streetlight" } }
```

Result semantics map exactly onto the gateway's duties:

| OPA result | Meaning | Gateway action |
|---|---|---|
| `{"result": {}}` | never satisfiable | deny — empty set for queries, 404 for retrieve (R20); fail closed (R5) |
| `"query": ""` | unconditionally true | forward untouched |
| `"query": <UCAST>` | residual conditions | translate UCAST → NGSI-LD QL, AND into the user's AST, forward via `POST /entityOperations/query` (R13) |

- **Translate UCAST → NGSI-LD, not SQL.** The data plane is a broker;
  the built-in SQL dialects and `targetSQLTableMappings` are irrelevant.
  The translation belongs in the antares-ql Wasm plugin (see §6).
- **`maskRule` = the R9 attribute projection**: the same compile call
  yields the row filter and the column mask from one policy evaluation,
  so they cannot disagree. The gateway strips unpermitted attributes
  from responses, keeping them valid NGSI-LD.
- `decision_id` + decision logging = the R42 audit trail; `metrics=true`
  (`timer_rego_query_eval_ns`) verifies the R40 single-digit-ms budget.
- Keep `nondeterministicBuiltins` off; never `http.send` inside the
  filter rule — all policy data is already pushed, or the latency
  budget dies.

Per-request flow: APISIX plugin parses → one Compile call (UCAST) →
merge into AST → forward to Antares → mask response attributes → set
`NGSILD-Results-Restricted` if the residual was non-trivial.

## 6. Future-proofing principles

Each component sits on a standards seam and holds no neighbor's logic —
any piece is swappable without touching the rest:

1. **One parser, everywhere.** The riskiest code is R10–R13 AST
   rewriting: a gateway parsing NGSI-LD `q` slightly differently from
   the broker is a privilege-bleed factory (ADR 006 class). Compile the
   broker's own query-language crate (antares-ql) to Wasm and run it
   inside the APISIX plugin — gateway and broker share one grammar,
   tested by the broker's suite.
2. **Standards on every trust edge**: OID4VCI/OID4VP (the eIDAS 2.0 /
   EUDI-wallet trajectory), did:web + trusted-issuers registry, ODRL for
   policy exchange, DPV for MIM4 consent, IDSA Data Space Protocol for
   MIM3 contracts.
3. **Federation stays inside NGSI-LD**: CSR prefix routing, EntityMap
   pagination, scopes (R32–R39) — no proprietary routing side-channel.
4. **R41 escape hatch is real**: Antares natively implements `scopeQ`,
   so if regex-folded scope filtering measures too slow, a broker-native
   scope path exists without vendor risk. Measure first.

Anti-patterns to avoid: Keycloak Authorization Services (§1), APISIX
Lua for the rewriter (Wasm is the direction and how the Rust parser is
reused), permissions embedded in JWT claims (staleness + bloat).

## 7. Unsolved

### Structural — no settled design

1. **The notification path bypasses the PEP.** Subscriptions deliver
   broker→endpoint directly; nothing applies attribute projection or
   re-checks `q`-grants at delivery time (MQTT streams likewise — ADR
   011 ACLs are topic-coarse). Options: a notification egress proxy, or
   broker-side hooks (collides with broker-stays-policy-free). ADR 009
   authenticates cross-boundary notifications; nothing *filters* them.
   **Biggest hole.**
2. **`idPattern` grants can't be folded into `q`.** NGSI-LD QL has no
   regex-on-id operator, and two idPatterns (user's + policy's) can't be
   AND-ed into one request parameter — CIM 009's own 5.12 matching gives
   up on the both-patterns case. Likely resolution: gateway-side id
   post-filtering or a broker extension; neither designed.
3. **Conditional writes are a TOCTOU race.** A write policy with `q`
   needs current entity state; gateway check-then-forward races
   concurrent updates. Airtight enforcement needs an atomic conditional
   write (If-Match-style predicate) the API doesn't offer.
4. **Revocation doesn't reach derived state.** Subscriptions created
   under a revoked policy keep firing; EntityMaps built under old grants
   keep serving pages. A reaper must re-evaluate/narrow existing subs
   and invalidate the user's entity maps on policy change. Failure mode
   is silent continued data flow.
5. **The meta-level is ungoverned.** Who may write `Policy` entities,
   assigner delegation depth, escalation prevention — undesigned. R21
   approval is "manual ticket" for iteration one.

### Unproven at scale

6. **R41 unmeasured** — regex-folded scope filtering vs lost native
   indexes at realistic entity counts.
7. **Residual explosion** — 10k+ per-id policies for one assignee OR-ed
   together → giant injected filters, OPA partial-eval latency, request
   body limits. R40 unproven at the 10k-tenant targets.
8. **Response-masking throughput** — APISIX body rewriting on large
   paginated responses, untested.

### Unbuilt (designed only)

9. UCAST → NGSI-LD-QL translator (the antares-ql Wasm plugin — the
   security-critical piece).
10. ODRL round-trip mapper (ADR 003 on paper only).
11. VC revocation: status lists + trusted-issuers registry operations
    (a revoked credential currently works until expiry).
12. DPV consent integration (MIM4).
13. `datasetId` / multi-instance granularity — absent from the policy
    model entirely.

### Verification

14. **Nothing tests the rewriter itself.** The ETSI suite validates the
    broker, not the PEP — and the rewriter is where the ADR 006 bleed
    class lives. Missing: a property-based suite (for arbitrary user
    query × policy set, no returned entity/attribute falls outside the
    union of grants) plus an adversarial bleed corpus. Until it exists,
    every rewriter change is a potential silent security regression.

**Suggested order of attack**: the property-test harness first (makes
everything else safe to build), the notification egress question second
(biggest hole), the R41/residual benchmarks third (they decide whether
the architecture holds at target scale before more is built on it).
