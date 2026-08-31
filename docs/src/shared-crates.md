# Shared crates

The broker is built from crates a gateway, a PEP or a proxy can depend on
without pulling in the broker: same parsing, same validation, same query
semantics. Scorpio and Stellio each keep one query AST with two evaluation
backends so the query path and the notification path cannot drift; here a
gateway is the third consumer of that same AST.

| crate | what it gives an embedder | example |
|---|---|---|
| `antares-model` | `NgsiError` (Table 6.3.2-1 problem details), `TenantId`/`EntityId` parse-don't-validate identifiers, `dt_key` DateTime ordering | — |
| `antares-ql` | `parse_q` → `QNode` (`Serialize`, `Clone`, `Display` renders back to `q=`), `eval::eval_q` in-memory evaluation, `sql::compile_q` bind-parameter jsonpath lowering, the bounded regex cache | `cargo run -p antares-ql --example gateway_filter` |
| `antares-jsonld` | `Loader` (per-instance @context cache over the caller's own HTTP client via `Loader::with_client`), `expand_entity` — the broker's request validation, usable at the edge | `cargo run -p antares-jsonld --example gateway_expand` |
| `antares-matcher` | subscription matching against an in-memory entity: entity selector (5.2.33), `q`/`scopeQ`/`geoQ` conditions, activity and throttling | `cargo run -p antares-matcher --example would_notify` |
| `antares-store` | the two storage driver traits, `TemporalEvent`, `NoTemporal` | — |

The backends behind those traits live one folder each under
`crates/antares-sql/src/store/`; its `README.md` is the procedure for adding
one.

CI builds and tests each of them standalone and fails on any dependency
path back into the broker, the API crate or a storage backend
(`shared-crates` job in `workspace.yml`).

## Stability

All five are **workspace-shared, not published**: `publish = false`, path
dependencies, one version for the whole workspace. Publishing (crates.io or
a git tag consumers pin) is deferred until a consumer outside this
repository exists — cutting a public API before that would freeze surfaces
that are still moving with the conformance work. When that happens the
public surface of each crate is reviewed like an API change and the crate
gets its own semver line; until then breaking changes inside the workspace
are ordinary commits.

## The PEP boundary

Authentication, authorization, rate limiting and request transforms stay in
the gateway in front of the broker: the broker grows no authorization code
(SECURITY.md states the same boundary). These crates are how a gateway does that job with
broker-identical semantics — rewrite the incoming `q=` with an authorization
predicate (`gateway_filter`), refuse a payload the broker would refuse
before it costs a hop (`gateway_expand`), or answer "would this change
notify subscription X" without a broker (`would_notify`). Stellio's
entity-level authorization (a permission CTE injected into the main query)
is the reference for doing the same on the SQL side with `antares-ql::sql`.
