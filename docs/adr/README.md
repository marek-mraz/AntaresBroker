# Architecture Decision Records

One file per irreversible decision, numbered, never rewritten.

## Index

| ADR | decision | status |
|---|---|---|
| [ADR-0001](https://github.com/marek-mraz/AntaresBroker/blob/master/docs/adr/ADR-0001-shared-schema-tenancy.md) | Shared-schema multi-tenancy (tenant_id + RLS) | accepted, amended (tenant inventory) |
| [ADR-0002](https://github.com/marek-mraz/AntaresBroker/blob/master/docs/adr/ADR-0002-nats-jetstream-bus.md) | NATS JetStream as the change bus (with a local mode) | accepted |
| [ADR-0003](https://github.com/marek-mraz/AntaresBroker/blob/master/docs/adr/ADR-0003-websocket-deferred.md) | WebSocket binding deferred out of v1 | accepted |
| [ADR-0004](https://github.com/marek-mraz/AntaresBroker/blob/master/docs/adr/ADR-0004-store-ladder-redb-file-mode.md) | Store ladder, redb as the `file`-mode durability shadow | accepted |
| [ADR-0005](https://github.com/marek-mraz/AntaresBroker/blob/master/docs/adr/ADR-0005-anystore-sync-facade.md) | AnyStore enum + synchronous Pg facade | accepted · enum seam superseded by ADR-0013 |
| [ADR-0006](https://github.com/marek-mraz/AntaresBroker/blob/master/docs/adr/ADR-0006-rls-vs-timescale-compression.md) | RLS and Timescale compression collide on attr_instances | accepted |
| [ADR-0007](https://github.com/marek-mraz/AntaresBroker/blob/master/docs/adr/ADR-0007-synchronous-temporal-recording.md) | temporal auto-recording stays in the write path | accepted (reverses the earlier bus-consumer design) |
| [ADR-0008](https://github.com/marek-mraz/AntaresBroker/blob/master/docs/adr/ADR-0008-browser-wasm-build.md) | The browser build: one crate, the same router, no fourth backend | accepted |
| [ADR-0009](https://github.com/marek-mraz/AntaresBroker/blob/master/docs/adr/ADR-0009-temporal-read-cutover.md) | Temporal read cutover: attr_instances becomes the read path | accepted, implemented |
| [ADR-0010](https://github.com/marek-mraz/AntaresBroker/blob/master/docs/adr/ADR-0010-egress-allow-private-by-default.md) | Private-range egress allowed by default | accepted, implemented |
| [ADR-0011](https://github.com/marek-mraz/AntaresBroker/blob/master/docs/adr/ADR-0011-tenant-specific-context-source-alias.md) | The Via pseudonym identifies a (Context Source, Tenant) pair | accepted, implemented |
| [ADR-0012](https://github.com/marek-mraz/AntaresBroker/blob/master/docs/adr/ADR-0012-durable-internal-state-doc-kinds.md) | Internal broker state lives in the store as doc kinds, keyed under reserved tenants | accepted, implemented |
| [ADR-0013](https://github.com/marek-mraz/AntaresBroker/blob/master/docs/adr/ADR-0013-storage-drivers.md) | storage drivers — current-state and temporal as separate traits | accepted; supersedes the enum half of ADR-0005; the driver-identity consequence superseded by ADR-0017 |
| [ADR-0014](https://github.com/marek-mraz/AntaresBroker/blob/master/docs/adr/ADR-0014-extension-hooks-model.md) | extension hooks — fixed phases, batch granularity | accepted; sink paragraph superseded by ADR-0016; two phases given a named user, and rule 1 an exception, by ADR-0020 |
| [ADR-0015](https://github.com/marek-mraz/AntaresBroker/blob/master/docs/adr/ADR-0015-notification-delivery-policy.md) | Notification delivery policy: one attempt by default, retries as transport, dead letters in the store | accepted, implemented |
| [ADR-0016](https://github.com/marek-mraz/AntaresBroker/blob/master/docs/adr/ADR-0016-notification-bindings-behind-the-sink-registry.md) | Notification bindings behind the sink registry | accepted, implemented |
| [ADR-0017](https://github.com/marek-mraz/AntaresBroker/blob/master/docs/adr/ADR-0017-drivers-are-named-not-enumerated.md) | A driver is identified by its name, not by an enum value | accepted, implemented |
| [ADR-0018](https://github.com/marek-mraz/AntaresBroker/blob/master/docs/adr/ADR-0018-ci-actions-pinned-by-tag.md) | CI actions are pinned by tag, third-party binaries by version | accepted, implemented |
| [ADR-0019](https://github.com/marek-mraz/AntaresBroker/blob/master/docs/adr/ADR-0019-remote-notification-endpoint.md) | The distributed-subscription notification receiver lives outside the ETSI namespace | accepted, implemented |
| [ADR-0020](https://github.com/marek-mraz/AntaresBroker/blob/master/docs/adr/ADR-0020-policy-seam.md) | The policy seam: one trait, one built-in engine, every engine an addon | accepted |

## Format

Nygard's fields — **Title, Status, Context, Decision, Consequences** —
plus one borrowed from MADR:

- **Confirmation**: how compliance with the decision can be checked — a
  named test, a CI job, a grep, or an explicit "manual review only".
  Every new ADR names its own fitness check; a decision nobody can verify
  drifts silently.

Shorter ADRs fold Context/Decision/Consequences into prose sections, as
the existing files do; the five concerns must all be answerable from the
text either way.

## Immutability policy

Append-with-status. An accepted ADR's body is frozen; when a decision
changes, a NEW ADR supersedes it and the old one's Status line gains
`superseded by ADR-00XX` (see ADR-0005/ADR-0013 and the reversal recorded
in ADR-0007). Never edit an old ADR's Decision to match new reality —
the record of what was believed, and when it stopped being true, is the
point of keeping them.
