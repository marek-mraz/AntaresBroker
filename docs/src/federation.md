# Federation

Antares implements the full CIM 009 distributed-operations model. The
short version: **Context Source Registrations (CSRs) are the routing
table.** A broker holding CSRs forwards matching requests to the
registered sources, merges the answers, and reports per-source problems
without failing the whole request. Everything below is validated by the
DistributedOperations and IOP suites in every CI cell (130 + 278 TPs).

## Registrations route everything

A CSR declares *what* a source holds (entity types, ids, `idPattern`,
attribute names) and *how* to treat it:

- **inclusive** (default) — the source is one of possibly many holders;
  results merge (4.5.5).
- **exclusive** — the source is the only holder of the registered scope;
  matching writes forward there.
- **redirect** — the broker proxies without keeping local data.
- **auxiliary** — consulted only when nobody else answers.

The `operations` member bounds what may be forwarded (default
`federationOps`); registrations carrying `contextSourceInfo` pass
per-source headers (auth material) on every forward. The
[getting-started page](getting-started.md#first-federation-pair) shows the
minimal two-broker pair.

## Distributed reads

A query that matches CSRs fans out concurrently (bounded by
`ANTARES_FED_FANOUT`, default 8), each forward with a per-request timeout
and a response-size ceiling (`ANTARES_MAX_FED_RESPONSE_BYTES`). Per-source
failures become `NGSILD-Warning` headers with the spec's warning codes
(Table 6.3.17-1) — 404 from a peer is a miss, not a warning; a dead peer
degrades the answer instead of failing it. Entity halves split across
sources merge per 4.5.5 before pagination.

## Distributed writes

Writes forward according to registration mode and the registered
`operations`: exclusive/redirect scopes forward, inclusive-unsupported
sources are skipped, and a fully unsupported write answers 409 Conflict.
Batch operations return per-entity success/error arrays with remote
results folded in.

## Loop protection

Every forward carries a `Via` hop (6.3.18) with the broker's
`ANTARES_HOST_ALIAS` as the pseudonym. A broker seeing its own alias in
the chain answers 508 (loop detected) instead of forwarding forever.
Replicas of one logical broker behind a load balancer share one alias on
purpose — they are one hop.

## Distributed subscriptions

A subscription whose scope matches CSRs is reduced per source and created
remotely (5.8); the remote broker notifies back to `ANTARES_PUBLIC_URL`
— set it whenever the default `http://{host_alias}:{port}` is not
routable from peers. Reduced copies follow the local subscription's
lifecycle (update, delete) and the registration's `csf` filter gates
inbound notifications.

## Pagination without amplification

The first distributed query can build an **EntityMap** (5.14): entity id →
contributing registrations. Subsequent pages contact only the sources that
actually hold the page's entities instead of re-broadcasting the query.
Maps expire (`expiresAt`) and are honoured on retrieve and temporal paths.

## Tenancy across the federation

The client's `NGSILD-Tenant` never propagates to forwards (4.14); a CSR
addresses a specific tenant of a remote source via its own registration
(`tenant` member / `contextSourceAlias`). Cross-broker isolation is
therefore explicit in registrations, never ambient.

## The five-broker stack

The IOP worked example — five brokers, no Docker:

```bash
dev/run-five.sh    # ports 9090..9094, aliases antares1..antares5
```

Each broker gets `ANTARES_PUBLIC_URL=http://localhost:PORT` (distributed
subscriptions hand that URL to peers as the notification endpoint — the
alias default is not resolvable between local processes). This is the
stack the 278-TP IOP tree runs against in CI.
