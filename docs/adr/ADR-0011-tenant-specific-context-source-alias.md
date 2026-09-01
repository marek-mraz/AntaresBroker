# ADR-0011 — The Via pseudonym identifies a (Context Source, Tenant) pair

Date: 2026-08-10. Status: accepted, implemented.

## Context

Antares carried ONE `host_alias` per process (`ANTARES_HOST_ALIAS`, default
`antares`). It went out as the `Via: 1.1 <alias>` pseudonym on every forward,
it was the value `via_loop` compared against, and it was what
`/info/sourceIdentity` served as `contextSourceAlias`.

Table 5.2.40-1 says otherwise:

> `contextSourceAlias` — A unique id for a `Context Source` which can be used
> to identify loops. **In the multi-tenancy use case (see clause 4.14), this
> id shall be identifying a specific `Tenant` within a registered
> `Context Source`.**

6.3.17 words the 508 the same way — "if the single registered source **and
tenant** is registered to redirect back on to the Context Broker".

A tenant-blind alias makes every tenant of one broker look like one Context
Source, and cross-tenant federation *inside* a single broker is a first-class
deployment shape here: a reader tenant reaches other tenants' data only
through CSRs whose `tenant` member points back at this same broker
(`csr_tenant_federates_across_tenants_in_one_broker`, the wasm playground's
context-spaces UI, and the Urbivita federated-twin model generally). Under a
single alias, the forwarded request arrives with this broker already named in
the chain, so the inner tenant reads it as a loop. Single-hop reads survived
that by accident — the inner tenant held the data locally, and suppressing a
further forward changed nothing. Anything else broke: a second hop was
dropped, and an inner `exclusive`/`redirect` registration answered 508.

A second half of the same clause was missing entirely. Table 5.2.9-1 gives a
registration its peer's `contextSourceAlias` — "a previously retrieved unique
id for a registered Context Source which is used to identify loops" — and
Table 6.3.18-2 says the inbound Via listing "is used when determining matching
registrations". Antares only ever compared its own alias; it persisted the
peer's under the non-spec member name `hostAlias` and never read it back.

## Decision

1. **`federation::alias_for(host_alias, tenant)`** is the only source of this
   broker's pseudonym: the bare configured alias for the default tenant,
   `{alias}~{tenant}` otherwise. It is used for the outbound `Via`, the loop
   comparison, the `NGSILD-Warning` warn-agent, and `/info/sourceIdentity`
   (which therefore answers per `NGSILD-Tenant`).

2. **Separator `~`.** It is an RFC 7230 token character, so the pseudonym
   stays legal; it cannot occur in a `TenantId` (`[A-Za-z0-9_-]{1,64}`); and
   it is free in the Urbivita URN conventions, which already spend `_` on the
   reverse-DNS razidlo and `-` on kebab-case local names. `ANTARES_HOST_ALIAS`
   is validated as an RFC 7230 token minus `~` at startup, so `a~b` in the
   default tenant can never collide with `a` in tenant `b`.

3. **The default tenant keeps the bare alias**, mirroring 6.3.14 (the tenant
   header is omitted, not sent as `default`). Single-tenant deployments and
   every peer that already registered a bare alias are unaffected.

4. **A registration's `contextSourceAlias` is read** (the spec member name;
   the `csource_index.host_alias` column keeps its name) into `FedReg.alias`,
   and `matching_regs` drops any registration whose alias is already in the
   inbound chain — at the one place every read and write path resolves
   candidates.

5. **508 stays scoped** to 6.3.17's case: a single `exclusive`/`redirect`
   source looping back. Unchanged by this ADR, restated because the two rules
   are read together.

## Deployment convention (Urbivita ADR 001)

Set `ANTARES_HOST_ALIAS` to the deployment's **razidlo** — the reverse-DNS
stamp already used in entity URNs (`sk_banskabystrica`). Then one identity
runs through all three layers:
`urn:ngsi-ld:WasteContainer:sk_banskabystrica:…` for the data,
`did:web:banskabystrica.sk` for the credential, and
`Via: 1.1 sk_banskabystrica~odpady` for the hop. DNS delegation supplies the
federation-wide uniqueness the pseudonym needs with no central registry, which
is the same argument ADR 001 makes for the URN prefix, and the admission rule
that binds a CSR's `idPattern` prefix to a verified domain binds the alias too.

The alias is predictable from (razidlo, tenant), so a peer can anticipate what
it will see in a chain, and readable, so a human triaging a five-broker loop
sees which twin and which register produced the hop. Hashing it would buy
pseudonymity the deployment does not want: the peer being contacted already
learns the tenant from the registration's `tenant` member and the
`NGSILD-Tenant` header.

## Consequences

- Chained and proxied cross-tenant federation inside one broker works; only
  a genuine same-tenant cycle is a loop.
- The alias is a **published identifier**: peers retrieve it from
  `/info/sourceIdentity` and store it in `contextSourceAlias`. Renaming a
  tenant or `ANTARES_HOST_ALIAS` invalidates peers' loop detection until they
  re-register — the same churn class as changing an endpoint.
- Deployments whose tenant names are internal artifacts leak them into Via
  chains. If that ever matters, the fix is a per-tenant alias override in
  config, not a change of format; nothing in the wire contract has to move.
- ETSI `D018_01` still fails: it registers `mode=inclusive` and asserts 508.
  That is a suite defect (docs/upstream/etsi-raises.md, issue 1), not a
  consequence of this ADR.

## Confirmation

In `crates/antares-api/src/federation.rs`:
`alias_identifies_the_tenant_not_just_the_broker`,
`registered_alias_in_the_via_chain_is_not_a_matching_registration`,
`via_loop_compares_tokens_not_suffixes`; end to end in
`crates/antares-api/tests/federation_loop.rs`.
