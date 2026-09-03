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
The same job checks that `antares-api` itself names no storage backend:
a consumer composes its two drivers and hands them to
`AppState::with_drivers`; the built-in memory and file store behind
`AppState::new` is a dev-dependency the crate's own tests enable.

## `antares-api` — what a host binary compiles against

`antares-api` is not one of the five: it is the broker's HTTP surface, and
the only crates that depend on it are hosts that run that surface —
`antares-broker`, `antares-wasm` and `examples/plugin-example`. Its API is
small on purpose, because everything a host needs it reaches through the
router or through `AppState`.

`AppState::call` is how a surface reaches the NGSI-LD API without a socket:
it serves one request through this broker's own router, carrying the
caller's `NGSILD-Tenant`, `NGSILD-Snapshot`, `Link` and policy subject
headers into it. There is no second data path — a façade for another
standard is a translation in front of the same handlers, so negotiation,
the bounds wall, tenancy, the policy seam, history and notifications apply
to its callers exactly as they do to an NGSI-LD client.

At the crate root: `router(state)` builds the NGSI-LD router,
`ops_router(state)` the operational one behind `Admin::PATHS`, and
`wire(&mut state)` installs the notification pipeline that a read-only host
never has to pay for. `spawn` and `background_tasks()` are the crate's
task bookkeeping, `Admin` names the operational paths, `ApiSurface` is the
trait another standard's surface implements, and `GIT_HASH` is what
`/q/health` reports. `AppState` and `TemporalRecord` come from `state`;
`DeliveryPolicy`, `page_sink` and `scope_matches` are re-exported from the
crates that own them so a host names one dependency instead of three.

Eleven modules stay public because a host or a surface reaches into them:

| module | what a host uses it for |
|---|---|
| `bounds` | every request limit in one place — the `MAX_*` constants and the env-read statics behind them, re-exporting the ones `antares-ql` and `antares-jsonld` own — plus `LimitStats::snapshot` for `/q/health` |
| `conformance` | `prefer_version_layer`, the middleware that answers the version `Prefer` |
| `egress` | `Egress`, the notification egress policy and its per-registration record |
| `geo` | `antares_ql::geo` re-exported, so a surface parses a geo query (`GeoQuery::from_params`) the way the broker does |
| `history` | `drain_errors`, the count of post-response history writes a driver failed |
| `mirror` | `Mirror`, `DocMirror`, `SubMirror` and the `Change` tuple: the change pipeline a bus driver feeds |
| `negotiate` | `ApiError`/`ApiResult`, `tenant_from`, `CleanParams`, `QUERY_PARAMS` — what an `ApiSurface` needs to answer like the broker |
| `policy` | the policy seam (ADR-0020): `PolicyEngine`, `Subject`, `Operation`, `Decision`, `Filter`, `NotifyDecision`, the built-in `AllowAll`, and the fail-closed `decide`/`pre_notify` an engine is called through |
| `notify` | `seed_mirror`, `process_change`, `record_temporal_change`, `interval_tick`: the pipeline steps a host drives |
| `qeval` | `antares_ql::eval` re-exported: `eval_q`, in-memory `q` evaluation over a stored document |
| `state` | `AppState`, its builders (`with_drivers`, `with_store`, `with_sink`, `with_surface`, `with_surfaces`, `with_policy`) and `call`, the in-process handle a façade surface drives the NGSI-LD router through |

Everything else is `pub(crate)`. There is no ratchet on this: once an item
is crate-visible the compiler's dead-code lint is the gate, and it fires
the moment an item loses its last caller. A helper that only a test
reaches is `pub` behind the `test-kit` feature, so a release build does not
carry it and the lint keeps seeing the crate as the shipped binary sees it.

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

Authentication, rate limiting and request transforms stay in the gateway in
front of the broker, and the broker ships no policy engine (SECURITY.md
states the same boundary). It does carry a policy seam — one trait, one
built-in allow-all engine, every other engine an addon crate outside the
workspace (ADR-0020) — for the three decisions a gateway cannot make from
outside: narrowing the query the store runs, filtering one subscription's
notification, and filtering a federated result before it is rendered. These crates are how a gateway does that job with
broker-identical semantics — rewrite the incoming `q=` with an authorization
predicate (`gateway_filter`), refuse a payload the broker would refuse
before it costs a hop (`gateway_expand`), or answer "would this change
notify subscription X" without a broker (`would_notify`). Stellio's
entity-level authorization (a permission CTE injected into the main query)
is the reference for doing the same on the SQL side with `antares-ql::sql`.
