# ADR-0021 — A stored @context belongs to the Tenant that stored it, and a Cached copy belongs to none

Date: 2026-09-03. Status: accepted; the column and its RLS policy are the
implementation this decision calls for. Extends ADR-0001 (shared schema,
`tenant_id` per row, RLS with `FORCE`) to the one tenant-bearing table it
left out, and ADR-0012 (durable internal state as `Kind`-tagged rows).

## Context

5.13 gives the broker three kinds of stored `@context`
(Table 5.2.31-1 `kind`):

- **Hosted** — a document a client POSTed to `/jsonldContexts`. The broker
  mints a URL for it and serves it (5.13.2, 5.13.3).
- **ImplicitlyCreated** — the same, created for a client that supplied an
  `@context` inline where the API needs a URL (5.13.4).
- **Cached** — a copy of a document the broker downloaded from a public URL
  a request referenced, kept so the next expansion needs no round trip, and
  periodically invalidated (5.13.1).

4.14 says any information related to one Tenant is visible only to users of
the same Tenant. A Hosted `@context` is such information: it is term
mappings one Tenant's payloads are written against, authored through that
Tenant's requests. A Cached copy is not: it is a public document, its URL
was chosen by whoever wrote the payload, and two Tenants referencing the
same URL are referencing the same document. Making a Cached copy per Tenant
would fetch the same bytes once per Tenant and store them once per Tenant
for no isolation anyone can observe — the document is public.

The code already draws that line. A Hosted or ImplicitlyCreated row carries
an `owner` member naming its creating Tenant; `contexts::row_visible` gates
every 5.13 operation on it, so another Tenant's row is as absent as one that
never existed, and `Loader::put_local_for` binds the parsed copy to the same
Tenant so `FetchedDoc::serves` refuses it in a resolution for anyone else.
A Cached row has no `owner` and the loader holds it with `owner: None`,
which serves every Tenant. Rows written before the member existed are read
as the default Tenant's.

What the schema does not do is say any of that. `jsonld_contexts` is
`(id, body, kind, created_at, last_usage, hits)` with `id text PRIMARY KEY`
and the comment "deliberately cross-tenant". It is the one table holding
Tenant-bound documents that has no `tenant_id` column, and therefore the
one tenant-bearing table with no Row-Level Security: the isolation rests
entirely on `row_visible` being called, and nothing fails closed if a
future read path forgets it. ADR-0001's whole posture is that the belt
exists because braces are forgettable.

## Decision

**1. Ownership follows `kind`, and the schema says so.** `Hosted` and
`ImplicitlyCreated` belong to the Tenant whose request created them.
`Cached` belongs to no Tenant and is shared by all of them. That is the
existing behaviour; this ADR makes it the schema's statement rather than a
convention held up by one function.

**2. `tenant_id` becomes a column, NULL meaning "no Tenant".** It is
derived from what the row already carries, so no write path changes and no
backfill can disagree with the document:

```sql
tenant_id text GENERATED ALWAYS AS (
  CASE WHEN kind = 'Cached' THEN NULL
       ELSE COALESCE(body ->> 'owner', 'default') END
) STORED
```

`COALESCE` to the default Tenant reproduces `row_visible`'s reading of a
row written before the `owner` member existed, so the column and the code
answer the same question for every row in an existing database.

**3. The primary key stays `id`.** A Hosted URL is minted by the broker and
is unique whatever Tenant owns it; a Cached URL is the external URL, and
one row per URL is the point of a cache. A `(tenant_id, id)` key would
duplicate public documents per Tenant, which decision 1 rejects. `id` is
therefore cross-tenant BY the key and per-Tenant BY the policy — the two
are not in tension.

**4. RLS with `FORCE`, and the Tenant reaches the four store calls that
need it.** The policy is the ADR-0001 one plus the shared-row arm:

```sql
USING (tenant_id IS NULL
       OR tenant_id = current_setting('antares.tenant', true))
WITH CHECK (tenant_id IS NULL
            OR tenant_id = current_setting('antares.tenant', true))
```

No `antares.service` escape. The escape exists for the outbox drain and the
4.22 reaps, which have work to do across every Tenant; nothing about a
stored `@context` does. A policy whose escape is armed on a request path
would be a policy in name only.

Every statement keeps its explicit `tenant_id IS NULL OR tenant_id = $n`
predicate beside the policy, the way every other statement in this store
carries `tenant_id = $1`. The predicate is what holds under a role that
bypasses RLS — a superuser, a `BYPASSRLS` role — and the policy is what
holds when a future statement forgets the predicate. On the write side the
predicate has two halves: a `WHERE` on the `ON CONFLICT DO UPDATE` so a row
another Tenant owns is not replaced, and a check on the document that
ARRIVES, because no clause about an existing row can stop a Tenant from
storing mappings under another Tenant's name.

That requires the Tenant at the four driver methods — `context_put`,
`context_get`, `context_delete`, `context_list_meta` — which today take
none, and at `Loader`'s three store hooks, whose local lookup is
`Fn(&str) -> Option<(Option<TenantId>, Value)>` and becomes
`Fn(Option<&TenantId>, &str) -> …`. Every caller already holds the Tenant:
the 5.13 handlers take it from the request, the resolution path has it in
`resolve_for`, and the boot warm reads `Cached` rows alone and passes none.

**5. One statement of the rule, and a tenant-less resolution is not a
skeleton key.** `contexts::row_visible`, the loader's `FetchedDoc::serves`
and both stores read the same `context_row_owner` / `context_row_visible`
in `antares-store`. `serves` used to answer `true` for any owner when the
resolution named no Tenant, on the reasoning that such a resolution is
broker-internal. Every production caller is `resolve_for` or
`resolve_quiet_for`, so that arm was unreachable — and it disagreed with
what the store now answers, which is the kind of disagreement that becomes
a leak the day an internal path goes tenant-less. `None` means the
documents that belong to no Tenant, in both layers.

The store's own `purge_tenant` gains the table too, as one indexed
`DELETE ... WHERE tenant_id = $1`. `contexts::purge_tenant` stays for what
only it can do: releasing the loader's warm copy and usage entry for each
URL, which live in this process and would otherwise keep serving a document
whose row is gone.

Two independent statements of one rule is what defence in depth means here;
the day they disagree, the database is the one that refuses.

## Consequences

- The one tenant-bearing table without RLS gets it, and ADR-0001's
  statement ("`tenant_id` on every row under Row-Level Security") stops
  having an exception a reader has to discover from a comment.
- A Cached row is readable by every Tenant by policy, which is a decision
  and not an accident: `tenant_id IS NULL` is in the `USING` clause on
  purpose, and a reviewer sees the sharing rather than inferring it from a
  missing column.
- The four driver methods gain a Tenant, so the change reaches
  `CurrentStateDriver`, both built-in backends, `AnyStore`, the driver
  contract and `examples/plugin-example`. That is the price of the seam
  being real: `run_current_state_contract` now probes the rule, so a
  backend that ignored the Tenant fails the contract from outside.
- The memory and file backends have no policy engine under them, so there
  the store applies the rule itself. It is the same rule and the same
  function; only the enforcement point differs.
- The generated column costs one `CASE` per write and an index's worth of
  space; it is never the query's own predicate, since every request path
  still names its Tenant explicitly.
- A deployment on a role that bypasses RLS gets nothing from this, as
  everywhere else. `ANTARES_REQUIRE_RLS=1` remains the production gate.

## Alternatives rejected

**A real column written by the code rather than a generated one.** Two
sources for one fact, and an insert that set one and not the other would
make the row invisible to its own Tenant. The document is the source of
truth (ADR-0012); the column is its projection.

**Per-Tenant Cached rows.** Isolation nobody can observe, paid for with one
fetch and one copy of a public document per Tenant. It would also make the
5.13.1 invalidation cap (`MAX_CACHED_CONTEXTS`) per Tenant, so one Tenant
referencing many URLs would evict its own cache while the table grew
without bound.

**Leaving it as it is, with `row_visible` as the whole enforcement.** It is
correct today. It is also the only thing standing between one Tenant's term
mappings and another's, on the one table where the database would otherwise
have an opinion. Every other tenant-bearing table already carries the
second opinion.
