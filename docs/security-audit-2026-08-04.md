# Antares NGSI-LD Broker — Security Audit

**Date:** 2026-08-04
**Scope:** `/workspace` — all 11 crates, `crates/antares-sql/migrations/*.sql`, `Dockerfile`, `deny.toml`, `Cargo.toml`, `.github/workflows/`, `compose-files/`
**Contract audited against:** `/workspace/claude.md` §16 (Security requirements), §2.1 (capacity budgets / bounded buffers), §4.1 (Scorpio lessons ledger), plus ETSI CIM 009 V1.9.1 clauses where behaviour is normative.
**Method:** dimension-parallel review (tenant isolation, SQL injection, SSRF/egress, DoS & input bounds, auth/header trust, JSON-LD @context, federation, supply chain & config, concurrency & state, dependency CVEs), each finding then re-verified by an independent skeptic who read the code and attempted refutation. Four findings were refuted and are excluded from this report. Severities below are the **post-verification** severities.

---

## 0. Amendment — 2026-08-04, after the audit

**Findings H1 and S11 (missing authn tower layer) are WITHDRAWN, not fixed.**
Scope decision by the project owner on the day of this audit: authentication,
authorization and rate limiting are **not NGSI-LD** — generic HTTP middleware
with no clause behind them — and belong to the PEP / reverse proxy Antares
sits behind. `claude.md` §16 and §16.3 were amended to match, and tasks.md I1
and I3 were deleted rather than deferred. This report's verdict below was
written against the *previous* §16 text, which promised a
`none | oidc-bearer | mtls` layer; read it with that in mind.

What this changes about the verdict: "safe when exposed directly" is now
scoped to mean an unauthenticated request cannot cross a tenant boundary,
inject SQL, exhaust the process, or make the broker attack another network.
It *can* read and write — deciding who may do that is delegated by design.
Every other finding in this report stands, and the two process-kill findings
(C1, C2) were fixed in commit d4e79e6.

---

## 1. Executive summary

### Verdict

> **The code does not yet meet the §16 contract.** The declarative controls §16 promises — typed `TenantId` threading, parameterised SQL, an `EgressPolicy`, bounded bodies — are genuinely implemented and hold up under attack (see §4, Verified clean). What is missing is the *enforcement layer around them*: the authn tower layer §16 specifies does not exist at all, three of the four §16.3 complexity caps are checked after the expensive work has already happened rather than before, the §16.7 federation fan-out bounds (semaphore + aggregate deadline) are absent, and two attacker inputs reach code that aborts the whole process. Antares today is safe **only** behind a policy-enforcement point on a trusted network, which is the deployment posture §16 names but not the "safe when exposed directly" property it also claims.

### Top three risks, in plain language

1. **One HTTP request kills the broker.** A query filter of the form `q=((((((…` recurses once per opening parenthesis with no depth counter; the size cap that was meant to stop it only runs *after* the tree is built. A Rust stack overflow aborts the process — it cannot be caught. A percent-encoded variant can be stored in a subscription, which turns it into a permanent crash loop that survives restart. → **C1**

2. **One registration document exhausts all memory.** `POST /csourceRegistrations` expands `entities × (propertyNames + relationshipNames)` into an in-memory `Vec` before any SQL runs, with no cardinality cap on any of the three arrays. A 4 MB body (the allowed maximum) produces on the order of 10¹⁰ objects. → **C2**

3. **Anyone on the network is every tenant, and can make the broker fetch anything.** There is no authentication code in the tree at all — the tenant is a self-asserted header — so every finding here is unauthenticated. On top of that, the SSRF guard misses IPv4-mapped IPv6 literals (`http://[::ffff:169.254.169.254]/`) and is never re-applied across HTTP redirects, and on the federation path the fetched response is handed back to the caller. → **H1, H2, H3**

### Counts

| Severity | Count |
|---|---|
| Critical | 2 |
| High | 8 |
| Medium | 20 |
| Low | 25 |
| Info (verified-clean records) | 6 |

---

## 2. Findings table

| # | Sev | Title | Anchor | Dimension(s) |
|---|---|---|---|---|
| C1 | **Critical** | Unbounded recursion in the `q=` parser aborts the process | `crates/antares-ql/src/lib.rs:44,102` | sql-injection · dos-input-bounds |
| C2 | **Critical** | `csource_index` expansion is quadratic in attacker-supplied arrays | `crates/antares-sql/src/store/pg_doc.rs:285` | federation |
| H1 | High | No authentication layer exists; tenant is a self-asserted header | `crates/antares-broker/src/main.rs:236,241` | auth-headers-trust · supply-chain-config |
| H2 | High | IPv4-mapped IPv6 literals bypass the private-range deny | `crates/antares-jsonld/src/loader.rs:77` | ssrf-egress · supply-chain-config |
| H3 | High | HTTP redirects are followed without re-applying the `EgressPolicy` | `crates/antares-jsonld/src/loader.rs:163` | ssrf-egress |
| H4 | High | Federation forward buffers/parses the peer response with no size cap | `crates/antares-api/src/federation.rs:290` | ssrf-egress · auth-headers-trust · federation · supply-chain-config |
| H5 | High | `@context` response-size cap is unenforceable against a chunked response | `crates/antares-jsonld/src/loader.rs:498` | ssrf-egress |
| H6 | High | `@context` fetch-count cap is checked after the whole crawl has run | `crates/antares-jsonld/src/loader.rs:392` | dos-input-bounds · jsonld-context |
| H7 | High | Cached-`@context` write-through persists unbounded rows and reloads all at boot | `crates/antares-broker/src/main.rs:184` | jsonld-context |
| H8 | High | Unbounded change channel drained by one serial, network-blocking consumer | `crates/antares-api/src/notify.rs:29` | dos-input-bounds · ssrf-egress · concurrency-state |
| M1 | Medium | MQTT connection pool keyed without password or tenant | `crates/antares-notifier/src/mqtt.rs:202` | tenant-isolation |
| M2 | Medium | Egress circuit breaker is process-global, keyed only by host:port | `crates/antares-api/src/egress.rs:58` | tenant-isolation |
| M3 | Medium | Attacker regexes recompiled inside hot loops, no cache, no size cap | `crates/antares-api/src/qeval.rs:117`; `notify.rs:160` | sql-injection · dos-input-bounds · concurrency-state |
| M4 | Medium | Federation fan-out: no semaphore, no source cap, no aggregate deadline | `crates/antares-api/src/federation.rs:392,497,770` | ssrf-egress · dos-input-bounds · auth-headers-trust · federation |
| M5 | Medium | MQTT egress has no DNS pinning (rebinding window) | `crates/antares-notifier/src/mqtt.rs:296,326` | ssrf-egress |
| M6 | Medium | Query Entities materialises every matching entity; no LIMIT pushed down | `crates/antares-sql/src/store/pg_entity.rs:211` | dos-input-bounds |
| M7 | Medium | `aggrPeriodDuration` reaches panicking chrono constructors | `crates/antares-api/src/temporal.rs:819,826` | dos-input-bounds |
| M8 | Medium | Unbounded `@context` usage map keyed by client-supplied URLs | `crates/antares-jsonld/src/loader.rs:221` | dos-input-bounds · jsonld-context · supply-chain-config |
| M9 | Medium | Temporal responses have no instance ceiling; `lastN` unbounded | `crates/antares-api/src/temporal.rs:718` | dos-input-bounds |
| M10 | Medium | `geoQ` geometry has no vertex cap; DE-9IM relate runs per entity | `crates/antares-api/src/geo.rs:165` | dos-input-bounds |
| M11 | Medium | `Host` header trusted to mint and persist `@context` URLs | `crates/antares-api/src/contexts.rs:20` | auth-headers-trust · jsonld-context |
| M12 | Medium | Hostile `Cache-Control: max-age` panics the request task | `crates/antares-jsonld/src/loader.rs:528` | jsonld-context |
| M13 | Medium | Parsed-context caches capped by entry count, not bytes | `crates/antares-jsonld/src/loader.rs:260` | jsonld-context |
| M14 | Medium | The 32-permit resolve semaphore is held across the whole network crawl | `crates/antares-jsonld/src/loader.rs:381` | jsonld-context |
| M15 | Medium | Batch write forwarding sends the entire batch to every matching registration | `crates/antares-api/src/batch.rs:488,602` | federation |
| M16 | Medium | `fed_query` imports any entity a peer returns (no identity check) | `crates/antares-api/src/federation.rs:549` | federation |
| M17 | Medium | Every federated request full-scans the tenant's registrations; `csource_index` never read | `crates/antares-api/src/federation.rs:161` | federation |
| M18 | Medium | Egress pre-check does an untimed DNS resolution on the request path | `crates/antares-api/src/egress.rs:55` | federation |
| M19 | Medium | Notifications still delivered for a subscription deleted after matching | `crates/antares-api/src/notify.rs:1119` | concurrency-state |
| M20 | Medium | No connection cap, header-read timeout or request timeout on the accept loop | `crates/antares-broker/src/main.rs:245` | concurrency-state |
| L1 | Low | `/jsonldContexts` is mutable cross-tenant (delete/reload ignore the tenant) | `crates/antares-api/src/contexts.rs:330,365` | tenant-isolation · auth-headers-trust |
| L2 | Low | RLS backstop is inert: the broker connects as a Postgres superuser | `crates/antares-sql/src/pg.rs:14` | tenant-isolation |
| L3 | Low | The CI SQL-injection grep guard matches zero lines and cannot fail | `.github/workflows/ci.yml:76` | sql-injection |
| L4 | Low | `POST /jsonldContexts` flushes the entire parsed-context LRU | `crates/antares-jsonld/src/loader.rs:550` | dos-input-bounds |
| L5 | Low | Raw database error text returned in the 500 `ProblemDetails` body | `crates/antares-sql/src/store/any.rs:20` | auth-headers-trust |
| L6 | Low | `Prefer` header forces unbounded buffering + full JSON re-parse | `crates/antares-api/src/conformance.rs:256` | auth-headers-trust |
| L7 | Low | `EntityId::new` accepts control characters (log/header injection) | `crates/antares-model/src/id.rs:70` | auth-headers-trust |
| L8 | Low | Cached `@contexts` are served on demand, contradicting 5.13.4.4 | `crates/antares-api/src/contexts.rs:89` | jsonld-context |
| L9 | Low | No hop limit on forwarded operations; loop check is suffix-matched self-alias only | `crates/antares-api/src/federation.rs:130` | federation |
| L10 | Low | Registration `tenant` / `contextSourceInfo` stored but never applied on forwards | `crates/antares-api/src/federation.rs:272` | federation |
| L11 | Low | GHCR `:latest` publishes on a gate that excludes clippy, tests and cargo-deny | `.github/workflows/etsi.yml:211` | supply-chain-config |
| L12 | Low | Container bases, build tool and toolchain are unpinned; release build not `--locked` | `Dockerfile:3`; `rust-toolchain.toml:2` | supply-chain-config · dependency-cves |
| L13 | Low | Subscriber MQTT credentials written to the log in cleartext | `crates/antares-api/src/notify.rs:1144` | supply-chain-config |
| L14 | Low | `cargo-deny` gate omits `[bans]` and `[sources]` | `deny.toml:15`; `.github/workflows/ci.yml:70` | supply-chain-config · dependency-cves |
| L15 | Low | Per-destination circuit-breaker map grows without bound | `crates/antares-api/src/egress.rs:28` | supply-chain-config |
| L16 | Low | Fatal unknown-key check rejects Kubernetes-injected `ANTARES_*` vars | `crates/antares-broker/src/main.rs:41` | supply-chain-config |
| L17 | Low | Deleted entity's temporal history resurrected by a concurrent write | `crates/antares-api/src/entities.rs:110` | concurrency-state |
| L18 | Low | Change events emitted after the lock and carry no version/incarnation | `crates/antares-sql/src/store.rs:440` | concurrency-state |
| L19 | Low | `expiresAt` enforced by raw string comparison against a `Z` timestamp | `crates/antares-api/src/notify.rs:132` | concurrency-state |
| L20 | Low | Attacker `observedAt` permanently poisons the plain-mode maintenance transaction | `crates/antares-sql/src/maintenance.rs:73` | concurrency-state |
| L21 | Low | Doc-kind create on Postgres is a check-then-act race (two 201s, one clobber) | `crates/antares-sql/src/store/any.rs:106` | concurrency-state |
| L22 | Low | The transactional outbox is implemented but wired to nothing | `crates/antares-sql/src/store/outbox.rs:7` | concurrency-state |
| L23 | Low | `rumqttc 0.24.0` pins an out-of-support rustls 0.22.4 into the shipped binary | `Cargo.toml:51` | dependency-cves |
| L24 | Low | Every `mqtts://` notification re-loads and re-parses the system trust store | `crates/antares-notifier/src/mqtt.rs:302,332` | dependency-cves |
| L25 | Low | Advisory scanning is push/PR-only; the embedded SBOM is never audited | `.github/workflows/ci.yml:3` | dependency-cves |

---

## 3. Findings in detail

### C1 — Unbounded recursion in the `q=` parser aborts the whole broker process

**Anchor:** `crates/antares-ql/src/lib.rs:44` (call site), `:52` (the cap that runs too late), `:77` `or_expr`, `:89` `and_expr`, `:101-108` `term`
**Dimensions:** sql-injection, dos-input-bounds (found independently by both; merged)
**Violates:** claude.md §16.3 ("query AST depth/size → `TooComplexQuery` 403"; "All limits observable via metrics before users hit them"), §2.1 rule 1.

**What it is.** `parse_q` runs the recursive descent first and checks the size cap afterwards:

```rust
pub fn parse_q(input: &str) -> Result<QNode, NgsiError> {
    let mut p = Parser { rest: input.trim() };
    let node = p.or_expr()?;                      // recursion happens here, unguarded
    ...
    const MAX_Q_NODES: usize = 512;               // line 52: checked AFTER the parse
    if q_nodes(&node) > MAX_Q_NODES { ... TooComplexQuery ... }
```

The cycle `or_expr(:77) → and_expr(:89) → term(:101-108, which calls self.or_expr() on '(')` carries no depth counter, so each `(` costs three stack frames. The cap can never fire on this input shape anyway: `q_nodes()` (`lib.rs:61-66`) counts only `And`/`Or` nodes, and a chain of single-child parentheses collapses to one node.

**Repro (three vectors, all unauthenticated — see H1).**
1. `GET /ngsi-ld/v1/entities?type=X&q=` + ~4000 `(` — roughly 6 KB, under `MAX_URI_BYTES` = 8 KiB (`bounds.rs:18`), over the overflow threshold. Reached unconditionally at `crates/antares-api/src/entities.rs:890` (`Some(q) => Some(parse_q(q)?)`).
2. `POST /ngsi-ld/v1/entityOperations/query` with `{"type":"Query","entities":[{"type":"T"}],"q":"((((…"}`. `crates/antares-api/src/batch.rs:683-687` copies the body's `q` into the virtual param map, so the ceiling is `MAX_BODY_BYTES` = 4 MiB (`bounds.rs:17`) — about 10⁶ nesting levels. The `json_depth` pre-scan (`bounds.rs:52-76`) counts only `{`/`[` and is string-aware, so parentheses inside a JSON string are invisible to it.
3. **Persistent crash loop.** Create a subscription whose `q` is percent-*encoded* (`"q": "%28%28%28…"`). Create-time validation at `crates/antares-api/src/subscriptions.rs:121` calls `parse_q` on the **raw** string — `%28` contains none of the delimiters `path()` scans for (`lib.rs:125`), so it parses as a harmless `Exists` node and is persisted. At notification time `crates/antares-api/src/notify.rs:169-170` percent-decodes **first**, then parses, now with thousands of real `(`, inside a `tokio::spawn`ed task (`notify.rs:37-41`). Any subsequent entity write by anyone aborts the process, and the poisoned row survives restart.

**Why the verifier believed it.** Confirmed by reading *and* by execution. A test calling `antares_ql::parse_q` on 50k `(` aborted with `thread has overflowed its stack / fatal runtime error: stack overflow` (SIGABRT); a release-mode replica of the exact recursion survived depth 2700 and aborted at 4000 on a 2 MiB stack — tokio's default worker stack size. A Rust stack overflow is a guard-page trap and an `abort`, **not** an unwindable panic: `tower`'s `CatchPanic` cannot contain it, and no such layer exists anyway (`api/lib.rs:215-239` merges only `prefer_version_layer`, `bounds_layer`, `options_204`).

**Fix.** Thread a `depth: usize` through `Parser`, increment it in `term()`'s paren branch, and return `NgsiError::TooComplexQuery` above a small limit (32–64) **before** recursing. Cap the raw `q` string length at parse entry (`bounds.rs` already owns the sibling caps). Separately, resolve the validate-one-string / evaluate-another split: either percent-decode `q` before the create-time check in `subscriptions.rs:121`, or stop decoding it in `notify.rs:169`.

---

### C2 — Registration `csource_index` expansion is quadratic in attacker-supplied array sizes

**Anchor:** `crates/antares-sql/src/store/pg_doc.rs:285` (`index_rows`, loop at `:279-295`), bound whole at `:397`
**Dimension:** federation
**Violates:** claude.md §16.3 (every request-shaped resource has a configured cap), §2.1 (every buffer bounded).

**What it is.** `index_rows()` nests

```rust
for ent in entities {
    for p in &props { rows.push(common(ent, Some(p), None)); }
    for r in &rels  { rows.push(common(ent, None, Some(r))); }
}
```

so the row count is `|entities| × (|propertyNames| + |relationshipNames|)` per `RegistrationInfo`, materialised as a `Vec<Value>` in process memory and then bound whole as one jsonb parameter at `pg_doc.rs:397` (`.bind(Value::Array(index_rows(doc)))`). `normalize_registration` (`crates/antares-api/src/csource.rs:52-160`) validates element *shapes* and expands terms but imposes **no cardinality limit** on `information[].entities`, `propertyNames` or `relationshipNames`. The only bound on the path is the generic `MAX_BODY_BYTES` = 4 MiB (`bounds.rs:17`).

**Repro.** `POST /ngsi-ld/v1/csourceRegistrations` with a ~4 MiB body:
```json
{"type":"ContextSourceRegistration","endpoint":"http://x.example",
 "information":[{"entities":[{"id":"a:b"} × ~150k],"propertyNames":["a" × ~400k]}]}
```
against a broker running `ANTARES_STORE=postgres` (the CI matrix and every compose deployment). ~10¹⁰ `serde_json` objects are constructed before any SQL executes; the process is OOM-killed far below the 500 MB budget / 350 MiB CI gate. Repeatable, and the stored registration re-poisons any pod that later re-upserts it.

**Why the verifier believed it.** The nested loop and the whole-`Vec` bind were read directly; `normalize_registration` was read line by line and confirmed to contain no cardinality check. **Scope caveat, recorded not disqualifying:** this is the `pg_doc` path, i.e. `ANTARES_STORE=postgres|timescale`; the `memory`/`file` default store is unaffected.

**Fix.** Cap at the validation boundary in `normalize_registration`: reject `information` longer than N, and each element with more than N `entities` / `propertyNames` / `relationshipNames` (128 each is generous). Additionally guard `index_rows` with a hard row ceiling that errors instead of pushing past ~10 000, so the store cannot be driven quadratically by a document written through some other path.

---

### H1 — No authentication layer exists; the tenant is a self-asserted client header

**Anchor:** `crates/antares-broker/src/main.rs:236` (only middleware), `:241` (binds `0.0.0.0`), `:11-25` (`KNOWN_KEYS`); `crates/antares-api/src/lib.rs:215-239`; `crates/antares-api/src/negotiate.rs:98-108`
**Dimensions:** auth-headers-trust, supply-chain-config (merged)
**Violates:** claude.md §16 preamble ("Per-request authn is a tower layer (`none | oidc-bearer | mtls`, config-selected)"; "it must be safe when exposed directly"), §16.1.

**What it is.** `grep -rniE 'authorization|bearer|oidc|jwt|mtls|authn|api_key' crates/ --include=*.rs` returns **zero hits**. `main.rs:236-239` serves `NormalizePathLayer::trim_trailing_slash()` over `antares_api::router(state)`, and that router merges exactly three layers — `conformance::prefer_version_layer`, `bounds::bounds_layer`, `options_204`. Identity comes solely from `tenant_from()` reading `NGSILD-Tenant`. `KNOWN_KEYS` contains no auth key and unknown `ANTARES_*` keys are fatal (`main.rs:41-45`), so authn cannot even be switched on out of band. `main.rs:241` binds `0.0.0.0` with no warning.

**Repro.** `curl -H 'NGSILD-Tenant: victim-city' http://broker:9090/ngsi-ld/v1/entities?type=Building` — any unauthenticated network peer reads, writes, purges or subscribes inside any tenant by naming it. `DELETE /ngsi-ld/v1/entities` (Purge) and `POST /csourceRegistrations` are equally anonymous.

**Why the verifier believed it (and why high, not critical).** The greps and layer stack were confirmed. Severity was set at high rather than critical because §16 explicitly designs Antares to sit behind a data-space PEP and defines `none` as a valid authn mode — this is an *unimplemented planned control*, not the bypass of an existing one. It is nonetheless the precondition that makes every other finding in this report unauthenticated-reachable, and the seven §16.1 isolation seams are correctness controls, not security controls, until a principal exists to bind the header to.

**Fix.** Implement the config-selected tower layer in `antares-broker::wiring` (`none|oidc-bearer|mtls`), add its key to `KNOWN_KEYS`, and validate `NGSILD-Tenant` against the authenticated principal's allowed-tenant claim inside `tenant_from` (403 on mismatch). Make `authn=none` an explicit opt-in that logs a loud startup warning.

---

### H2 — IPv4-mapped IPv6 literals bypass the private-range deny (SSRF to metadata / loopback / RFC1918)

**Anchor:** `crates/antares-jsonld/src/loader.rs:77-93` (`ip_is_private`), `:105-110` (literal short-circuit)
**Dimensions:** ssrf-egress, supply-chain-config (merged — same defect, found twice)
**Violates:** claude.md §16.4 (deny-by-default for loopback/link-local/RFC1918/metadata ranges).

**What it is.** The V6 arm tests only `is_loopback()`, `is_unspecified()`, `fc00::/7` and `fe80::/10` — it never calls `to_ipv4_mapped()`. `::ffff:169.254.169.254` has `segments()[0] == 0`, so none of the four predicates fire. `check_host` takes the literal-IP fast path at `:105-110` (`host.trim_matches(['[',']']).parse::<IpAddr>()` then `return Ok(())`), and hyper-util skips the DNS `PolicyResolver` for literals, so nothing re-checks it. The kernel routes a v4-mapped destination to the real IPv4 host.

**Repro.** Any of the three egress classes:
- `POST /ngsi-ld/v1/csourceRegistrations` with `endpoint: "http://[::ffff:169.254.169.254]"`, then `GET /ngsi-ld/v1/entities?type=X` — `federation.rs:290` returns the internal response body into the client-visible merged result (a full internal-network **read** primitive, plus a port scanner via the status code).
- `Link: <http://[::ffff:127.0.0.1:9090]/x.jsonld>; rel="http://www.w3.org/ns/json-ld#context"` (`loader.rs:477`).
- A subscription `notification.endpoint.uri` with the same host form (`notify.rs:1143`).

**Why the verifier believed it.** Confirmed by **execution** against the real `Egress::check_url` with `allow_private=false`: `http://[::ffff:169.254.169.254]/latest/meta-data` → `Ok(())`, `http://[::ffff:127.0.0.1]:9090/x` → `Ok(())`, `http://[::ffff:10.0.0.5]:8080/` → `Ok(())`, while plain `http://127.0.0.1:9090/x` → `Err`. The existing unit test (`egress.rs:110-120`) covers only the plain-IPv4 form, so CI is green. Not rated critical because it does not give cross-tenant data access, RCE or trivial total DoS — but it is a genuine unauthenticated SSRF with read-back.

**Fix.** Canonicalise first:
```rust
let ip = match ip { IpAddr::V6(v6) => v6.to_ipv4_mapped().map(IpAddr::V4).unwrap_or(IpAddr::V6(v6)), v => v };
```
then apply the V4 rules. Additionally deny `100.64.0.0/10` (CGNAT), `0.0.0.0/8`, `192.0.0.0/24`, `198.18.0.0/15`, `240.0.0.0/4`, and `64:ff9b::/96` (NAT64) — all currently allowed. Add the `[::ffff:…]` forms to the `egress.rs` test.

---

### H3 — HTTP redirects are followed without re-applying the `EgressPolicy`

**Anchor:** `crates/antares-jsonld/src/loader.rs:163` (`client_builder`), `:477` and `crates/antares-api/src/egress.rs:55` (the single, pre-redirect check)
**Dimension:** ssrf-egress
**Violates:** claude.md §16.4 (redirect cap; deny-by-default private ranges).

**What it is.** `client_builder()` installs `.redirect(reqwest::redirect::Policy::limited(MAX_REDIRECTS))` plus `.dns_resolver(Arc::new(PolicyResolver(policy)))`. The DNS resolver is the *only* per-hop guard — and hyper-util's connector short-circuits it for IP literals (`hyper-util-0.1.20/src/client/legacy/connect/http.rs`: "If the host is already an IP addr … skip resolving the dns"; `dns::SocketAddrs::try_parse(host, port)` returns before `resolve(&mut self.resolver, …)`). The explicit `check_host` runs exactly once, on the pre-redirect URL. `grep -rn redirect --include=*.rs crates/` finds only `loader.rs:163` — no `redirect::Policy::custom` anywhere.

**Repro.** Register any of the three egress classes at a public attacker host (`notification.endpoint.uri`, a CSR `endpoint`, or a `Link` `@context` URL). The attacker's server answers `302 Location: http://169.254.169.254/latest/meta-data/iam/security-credentials/` (or `http://127.0.0.1:9090/ngsi-ld/v1/entities`). reqwest follows up to 3 hops; the literal IP never touches `PolicyResolver`. On the federation path the body is read and merged into the client-visible query result (`federation.rs:290-291`), turning blind SSRF into exfiltration.

**Why the verifier believed it.** Both halves confirmed by reading — the single-check placement in `loader.rs`/`egress.rs`, and the IP-literal short circuit in the vendored `hyper-util` source. Downgraded from critical to high on the same reasoning as H2.

**Fix.** Replace `Policy::limited(N)` with `Policy::custom(|attempt| { … })` that policy-checks each hop's host (including IP literals) and `attempt.stop()`s on a private address, while retaining the hop count and keeping `PolicyResolver` for named hosts.

---

### H4 — Federation forward buffers and parses the peer's response with no size cap

**Anchor:** `crates/antares-api/src/federation.rs:290`; client at `crates/antares-api/src/state.rs:73-78`
**Dimensions:** ssrf-egress, auth-headers-trust, federation, supply-chain-config (found four times; merged)
**Violates:** claude.md §2.1 rule 1 ("Every buffer is bounded and configured"), §16.3, §16.4.

**What it is.**
```rust
let body = resp.json::<Value>().await.unwrap_or(Value::Null);
```
No `content_length()` pre-check, no streaming cap. Contrast the `@context` path, which *does* enforce one (`loader.rs:198` + `:497-510`, `MAX_CONTEXT_BYTES = 5 MiB`). `fed_http` sets only `connect_timeout(2s)` / `timeout(8s)` — a time bound, not a byte bound — and reqwest imposes no default limit. `bounds::MAX_BODY_BYTES` (4 MiB, applied at `bounds.rs:92`) governs only *inbound client* bodies. Worse, `fed_query` never sends a `limit` parameter to the peer (`federation.rs:519-533`), so the peer alone chooses the page size, and every returned element then goes through full JSON-LD expansion in `import_entity` (`federation.rs:305-316`).

**Repro.** `POST /ngsi-ld/v1/csourceRegistrations` with `information=[{entities:[{type:"Device"}]}]` and an attacker-controlled `endpoint` (only shape-validated at `csource.rs:149-155`; any public host passes the egress policy), then `GET /ngsi-ld/v1/entities?type=Device`. The peer streams a large JSON array for the full 8 s window; it is buffered into `Bytes` and re-materialised as a `serde_json::Value` (5–10× expansion), then expanded again per element.

**Why the verifier believed it.** The call site, the absence of any ceiling, and the contrasting capped `@context` path were all read directly; the missing `limit` on forwarded queries was confirmed at `federation.rs:519-533`.

**Fix.** Read forwarded responses through the same bounded pattern as `@context` fetches: check `resp.content_length()` against a `MAX_FEDERATED_BODY_BYTES`, then accumulate `resp.chunk()` with a running total and abort past the cap, reporting the source as a 502 part in the 207. Also push an explicit `limit` onto every forwarded read so the page size is the caller's, not the peer's.

---

### H5 — `@context` response-size cap is unenforceable against a chunked response

**Anchor:** `crates/antares-jsonld/src/loader.rs:497-511`
**Dimension:** ssrf-egress
**Violates:** claude.md §16.4 (response-size caps on `@context` fetches), §2.1.

**What it is.**
```rust
if resp.content_length().is_some_and(|l| l as usize > MAX_CONTEXT_BYTES) { return Err(...) }  // absent under chunked
let bytes = resp.bytes().await ...;   // buffers the ENTIRE body first
if bytes.len() > MAX_CONTEXT_BYTES { return Err(...) }   // too late
```
`is_some_and` means the pre-check is skipped entirely when the peer omits `Content-Length`.

**Repro.** Any request carrying `Link: <https://evil.example/ctx.jsonld>; rel="http://www.w3.org/ns/json-ld#context"` (or an `@context` array of 32 such URLs). The server replies `200 Transfer-Encoding: chunked` and streams indefinitely; `resp.bytes()` accumulates in the broker's heap until the 10 s client timeout (`loader.rs:256`). `resolve_permits` (`loader.rs:265`) allows 32 concurrent cold resolutions, so 32 unbounded buffers grow at once against a 350–500 MiB budget.

**Why the verifier believed it.** Code read directly; reachability traced from `merge_entry` (`loader.rs:432`) for any client-supplied `@context` URL.

**Fix.** Stream with a running total instead of `bytes()`:
```rust
let mut buf = Vec::new();
while let Some(c) = resp.chunk().await? {
    if buf.len() + c.len() > MAX_CONTEXT_BYTES { return Err(err("@context document too large")); }
    buf.extend_from_slice(&c);
}
```

---

### H6 — `@context` fetch-count cap is checked after the whole crawl has already run

**Anchor:** `crates/antares-jsonld/src/loader.rs:388` (crawl), `:392` (cap, too late), `:422` (depth-only guard in `merge_entry`); dead constant at `crates/antares-api/src/bounds.rs:22`
**Dimensions:** dos-input-bounds, jsonld-context (merged; the two verifiers disagreed on severity — high vs medium — see below)
**Violates:** claude.md §16.3 ("@context chain length and fetch count per request"), §16.4, §16.7.

**What it is.**
```rust
self.merge_entry(&mut ctx, user, 0, &urls).await?;      // line 388: the ENTIRE crawl runs here
let urls = urls.into_inner().unwrap_or_default();
if urls.len() > 32 { return Err(NgsiError::LdContextNotAvailable(...)); }   // line 392: too late, hardcoded
```
`merge_entry` bounds **depth** only (`if depth > 8 { return Err(...) }`, `:422`) and iterates arrays with no breadth limit, awaiting `self.fetch(url)` per `Value::String` element. `MAX_CONTEXT_FETCHES` in `bounds.rs:22` is never referenced by enforcement code — `grep` finds only the definition and the `/q/health` JSON.

**Repro.** `POST /ngsi-ld/v1/entities` with `Content-Type: application/ld+json` and `"@context": ["https://attacker/1", … N entries]`, N limited only by the 4 MiB body cap (tens to hundreds of thousands). All N sequential GETs happen, each with a 10 s timeout, before `urls.len() > 32` is ever evaluated. Nested arrays returned by the fetched documents multiply this further within the depth-8 budget.

**Why the verifier believed it, and the severity dispute.** Both verifiers confirmed the code. The dos-input-bounds verifier rated it **high** (one request → an unbounded outbound crawl; 32 concurrent such requests hold every resolve permit and stall all cold `@context` resolution broker-wide). The jsonld-context verifier rated it **medium**, reasoning that the crawl is *serial* so the effect is permit starvation rather than an instantaneous request flood. Recorded at **high** because the permit-starvation consequence is itself cross-tenant and the fan-out is unbounded; the counter-argument is preserved here rather than discarded.

**Fix.** Move the cap **inside** `merge_entry`: pass the shared `urls` list down and return `LdContextNotAvailable` the moment `urls.len()` reaches the cap, before issuing the next fetch. Reject an `@context` array longer than the cap before fetching anything. Reference `antares_api::bounds::MAX_CONTEXT_FETCHES` (or move the constant into `antares-jsonld`) instead of a second hardcoded `32`. See also **M14** — the resolve permit is held across this crawl.

---

### H7 — Cached-`@context` write-through persists unbounded attacker-chosen rows and re-loads all of them at boot

**Anchor:** `crates/antares-broker/src/main.rs:181-196` (cache writer), `:197-210` (boot re-seed); sink `crates/antares-sql/src/store/pg_doc.rs:562-575`, listing `pg_doc.rs:598`; hook fired at `crates/antares-jsonld/src/loader.rs:531-536`
**Dimension:** jsonld-context
**Violates:** claude.md §2.1 (every buffer bounded), §16.3.

**What it is.** The cache-writer hook fires on **every** fresh remote fetch and does a bare `INSERT INTO jsonld_contexts … ON CONFLICT (id) DO UPDATE` with no quota; `jsonld_contexts` has no tenant column (the sanctioned cross-tenant table, §8.3). Each stored `body` may be up to `MAX_CONTEXT_BYTES` = 5 MiB (`loader.rs:198`). At boot, `main.rs:197-210` iterates `store.context_list()` — `SELECT body FROM jsonld_contexts ORDER BY id`, no LIMIT — into memory and calls `seed_cached` for each, which also repopulates the uncapped `usage` map (see **M8**).

**Repro.** N requests each carrying `Link: <https://attacker/ctx?n=N>` with the attacker serving a ~5 MiB valid `@context` document. ~1000 requests durably write ~5 GB of attacker-chosen content. Then restart the broker: the entire table is loaded into memory, so a broker that survived the attack OOMs at boot and cannot be restarted back into service.

**Why the verifier believed it.** Every link in the chain was read: hook registration, the unconditional fire site, the unquota'd INSERT, the LIMIT-less `context_list`, and the boot loop.

**Fix.** Bound the write-through (cap total `Cached` rows with LRU eviction on `last_usage`; cap the persisted body well below 5 MiB) and page/limit the boot preload instead of loading the whole table.

---

### H8 — Unbounded change channel drained by a single serial consumer that awaits network delivery inline

**Anchor:** `crates/antares-api/src/notify.rs:29-41`, delivery await at `:574`/`:642`, timeouts at `crates/antares-api/src/state.rs:65-72`
**Dimensions:** dos-input-bounds, ssrf-egress, concurrency-state (found three times; merged — the dos-input-bounds verifier's **high** is recorded)
**Violates:** claude.md §2.1 ("Every buffer is bounded"; "Backpressure over buffering"; "Any unbounded queue is a 3am page"), §4.1 U2, §16.1 (noisy-neighbour isolation).

**What it is.**
```rust
let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel::<(String, Option<Value>, Option<Value>)>();
state.store.set_change_hook(Box::new(move |tenant, before, after| {
    let _ = tx.send((tenant.as_str().to_owned(), before, after));   // never blocks, never drops
}));
tokio::spawn(async move {
    while let Some((tenant, before, after)) = rx.recv().await {
        process_change(&st, &tenant, before, after).await;          // one at a time
    }
});
```
The queued tuple carries the **full before- and after-images** of the entity (up to 4 MiB each). `process_change` iterates subscriptions and awaits each `deliver()` inline (HTTP: 5 s timeout, `state.rs:71`; MQTT: 5 s × 2 attempts). `grep unbounded_channel crates/` shows this is the only one; there is no capacity bound, no `try_send`, no drop policy and no second consumer.

**Repro.** Create one subscription whose endpoint accepts the connection and answers after ~4.9 s (just under the 5 s timeout, so the circuit breaker — which counts *failures* — never trips). Then `POST /ngsi-ld/v1/entities` in a loop with large bodies. The producer enqueues one tuple per write with no backpressure while the consumer drains one every ~5 s: the queue and its multi-MiB payloads grow without bound until OOM. Second impact on the same defect: because **one** task serves every tenant, a single tenant's slow endpoint stalls every other tenant's notifications.

**Why the verifier believed it.** Read verbatim; the singular consumer and the inline `.await` on the network send were both confirmed at the cited lines.

**Fix.** Replace `unbounded_channel` with a bounded `mpsc::channel(N)` plus an explicit overflow policy (coalesce-latest per entity id, or drop-oldest with a counter exported on `/q/health`), and dispatch deliveries concurrently under a semaphore so one dead endpoint cannot serialise the whole broker's notification path. Export queue depth as a health signal.

---

### M1 — MQTT notification connection pool is keyed without the password or the tenant

**Anchor:** `crates/antares-notifier/src/mqtt.rs:202-209` (key), `:263` (`checkout`), `:296`/`:326` (`set_credentials`, first connect only), `:45-55` (`MqttEndpoint::parse`); single process-wide sink at `crates/antares-api/src/state.rs:79-80`
**Dimension:** tenant-isolation
**Violates:** claude.md §16.1.4 (tenant-keyed everything in memory), §4.1 L5.

**What it is.**
```rust
let key = format!("{}:{}@{}:{}/v{}",
    ep.username.as_deref().unwrap_or(""), ep.secure, ep.host, ep.port,
    if params.v5 { 5 } else { 3 });
```
No password, no tenant, no topic. `checkout(&key)` removes and returns a live, already-authenticated `rumqttc` client; credentials are applied only inside `connect()`, i.e. on the first connect. `MqttEndpoint::parse` yields `username=Some(u), password=None` for a userinfo with no `:`, producing a byte-identical key. `MqttSink` is one process-wide `Arc` shared by all tenants.

**Repro.** Tenant B's subscription endpoint `mqtt://ingest:S3cret@mqtt.example/plantB/telemetry` fires once, checking an authenticated session into the pool under `ingest:false@mqtt.example:1883/v3`. Attacker then `POST /ngsi-ld/v1/subscriptions` with `NGSILD-Tenant: A` and endpoint `mqtt://ingest@mqtt.example/plantA` — same key, `checkout()` returns B's session, and A publishes on B's authenticated session to any topic A names. The MQTT broker's ACLs see B's identity. The same collision also lets A evict/steal B's connection under the 32-entry LRU.

**Why the verifier believed it.** Key construction, `checkout` semantics, credential timing, the parse behaviour for password-less userinfo, and the single shared `Arc` were each read at the cited lines.

**Fix.** Include the full credential material and the owning tenant in the key — e.g. `hash(username, password, secure, host, port, v5)` prefixed with `tenant.as_str()` — and pass `&TenantId` into `MqttSink::deliver` so the key can never be shared across tenants even for identical URIs.

---

### M2 — Egress circuit breaker is process-global, keyed only by `scheme://host:port`

**Anchor:** `crates/antares-api/src/egress.rs:58-69` (`key`), `:96` (`record_failure`), `:15` (`TRIP_AFTER = 5`, `COOLDOWN = 30s`); single instance at `crates/antares-api/src/state.rs:83`; consumed at `crates/antares-api/src/notify.rs:1147`, victim status write at `:1175-1201`
**Dimension:** tenant-isolation
**Violates:** claude.md §16.1.4, §16.7 (per-endpoint circuit breakers).

**What it is.** `key()` discards the path, and `Egress` is constructed once per process with no tenant dimension. `record_failure` fires on any non-2xx.

**Repro.** Attacker knows tenant B notifies `http://receiver.victim.example:8080/ngsi/notify`. In their own tenant A they create 5 subscriptions with endpoint `http://receiver.victim.example:8080/does-not-exist` and trigger each with a matching write. Each 404 records a failure on key `http://receiver.victim.example:8080`; the breaker trips. For 30 s every notification tenant B sends to that host is short-circuited without a network attempt, and B's subscription rows are rewritten with `status: "failed"` + `lastFailure`. Repeating every 30 s makes the suppression permanent.

**Why the verifier believed it.** Key shape, single process-wide instance, the short-circuit site and the victim-side status writeback were all read; `TRIP_AFTER`/`COOLDOWN` confirmed. Any client may pick any tenant (see H1).

**Fix.** Key the breaker map by `(tenant, scheme, host, port)` — `Egress::key` should take `&TenantId` — or, if a shared breaker is intentional for peer health, keep failure accounting per tenant and only short-circuit the tenant whose own attempts failed. (See also **L15**: the same map is unbounded.)

---

### M3 — Attacker-supplied regexes are recompiled inside hot loops, with no cache and no pattern-size cap

**Anchor:** `crates/antares-api/src/qeval.rs:117`; also `crates/antares-api/src/notify.rs:157-160` (per subscription per change), `crates/antares-api/src/csource.rs:564,573`, `crates/antares-api/src/entities.rs:868`; loop at `entities.rs:940-946`; SQL refusal at `crates/antares-sql/src/compile/q.rs:176`; the only validation at `crates/antares-api/src/subscriptions.rs:88`
**Dimensions:** sql-injection, dos-input-bounds, concurrency-state (found three times; merged)
**Violates:** claude.md §16.3 (complexity bounds → `TooComplexQuery` 403), §9.3 (`CompiledSubscription`).

**What it is.** `CmpOp::Pattern => regex::Regex::new(s).is_ok_and(|re| re.is_match(t))` compiles a fresh `Regex` **inside** `compare()`, the innermost function of the evaluator. `eval_q` calls it per resolved target and it recurses per array element; `filter_entities_fed` runs `eval_q` for every candidate document. The store cannot narrow the set: `compile/q.rs:176` returns `None` for `CmpOp::Pattern`, so the predicate is dropped from the WHERE clause; memory/file modes return the whole snapshot regardless (`any.rs:294`). No compiled-regex cache exists anywhere in the workspace, `RegexBuilder::size_limit`/`dfa_size_limit` are never configured, and nothing caps pattern length — `MAX_Q_NODES` counts AST nodes, and one `~=` is one node with an 8 KB (URI) or multi-MB (POST body) operand.

**Repro.**
- `GET /ngsi-ld/v1/entities?type=Vehicle&q=name~="<~8 KB alternation>"` against a tenant with many entities → one compile per candidate value.
- `POST /ngsi-ld/v1/entityOperations/query` with a megabyte pattern (`batch.rs:683-687` copies body `q` through; the 8 KiB URI cap does not apply).
- Persistent: store the pattern as a subscription `idPattern` or `q`; every entity write then recompiles it per candidate via `notify.rs:160`, on the already-serial change consumer (**H8**).

**Why the verifier believed it.** The compile-inside-compare, the per-document loop, the SQL refusal and the absence of any cache or size limit were all read at the cited lines. Severity is medium, not high: the `regex` crate is linear-time in matching and caps its own compiled program at 10 MB, so the cost is repeated **compilation** — CPU amplification, not exponential blow-up.

**Fix.** Compile once per request/subscription before the loop (hoist into the parsed AST node or the compiled-filter struct, as `id_pattern` already is at `entities.rs:866-871`), build with `RegexBuilder::size_limit()`/`dfa_size_limit()`, and cap pattern source length at parse/creation time with `TooComplexQuery` 403.

---

### M4 — Federation fan-out has no semaphore, no source cap and no aggregate deadline

**Anchor:** `crates/antares-api/src/federation.rs:392` (`fed_retrieve`), `:497` (`fed_query`), `:770` (`fed_attr_parts`), `:154-236` (`matching_regs`, no `.take(n)`); client bounds at `crates/antares-api/src/state.rs:73-78`; stated-but-unimplemented contract at `crates/antares-registry/src/lib.rs:7`
**Dimensions:** ssrf-egress, dos-input-bounds, auth-headers-trust, federation (found four times; merged)
**Violates:** claude.md §16.7 ("per-request forward semaphore (default ~16), per-source timeout, and an aggregate request deadline").

**What it is.** `for reg in matching_regs(...) { ... forward(...).await }` — strictly serial, unbounded iteration over every matching registration. The only bound is the per-request `connect_timeout(2s)`/`timeout(8s)`. `grep TimeoutLayer|ConcurrencyLimit crates/` returns nothing, and `main.rs:236` applies only `NormalizePathLayer`, so there is no global request deadline either.

**Repro.** Create N registrations (claude.md §1 targets 1000+ per tenant; nothing caps the count) with **distinct** blackholing endpoint hosts, then one `GET /ngsi-ld/v1/entities?type=X`. Each forward burns the full connect/read timeout; N=1000 occupies a task and a connection for ~33 minutes (2 s each) to ~2.2 hours (8 s each). Per-destination circuit breakers do not help: distinct destinations each need 5 consecutive failures before tripping, and `egress.rs:58-70` keys them per destination.

**Why the verifier believed it.** The loops, the uncapped `matching_regs`, the absence of any semaphore/deadline in the crate, and the missing tower layers were all confirmed. Downgraded from high to medium: because the loop is **sequential**, one inbound request produces one outbound at a time — the impact is long-lived task/connection occupancy and unbounded per-request latency, not instantaneous request amplification.

**Fix.** Take at most N (config, ~64) matching registrations per request, run them through a `tokio::sync::Semaphore` (~16) with `futures::stream::buffer_unordered`, and wrap the whole fan-out in a single `tokio::time::timeout`, reporting unfinished sources as failure parts in the 207 rather than waiting.

---

### M5 — MQTT notification egress has no DNS pinning (rebinding window)

**Anchor:** `crates/antares-notifier/src/mqtt.rs:296` (v5), `:326` (v3); one-shot check at `crates/antares-jsonld/src/loader.rs:110-120` via `crates/antares-api/src/egress.rs:47-56` from `notify.rs:1141-1145`; the pinning mechanism at `loader.rs:131-165`
**Dimension:** ssrf-egress
**Violates:** claude.md §16.4 ("DNS-pinned re-resolution: resolve once, connect to the resolved IP").

**What it is.** `PolicyResolver` is a **reqwest** resolver and therefore covers only reqwest clients. `MqttOptions::new(id, &ep.host, ep.port)` hands the bare hostname to `rumqttc`, which resolves it itself at connect and exposes no resolver hook. The only guard is the one-shot `tokio::net::lookup_host` inside `check_host` — a genuine TOCTOU window that the reqwest paths deliberately close.

**Repro.** `POST /ngsi-ld/v1/subscriptions` with `notification.endpoint.uri = mqtt://rebind.attacker.example/topic`, served with a 1 s TTL A record alternating between a public IP and `10.0.0.5`. `check_url` passes on the public answer; `rumqttc` re-resolves milliseconds later and dials the private one, publishing the notification body (and any credentials in the URI) to an internal broker. Notifications fire on every matching change, so the window is retried until it lands.

**Why the verifier believed it.** The reqwest-only scope of `PolicyResolver` and `rumqttc`'s own resolution were both confirmed at the cited lines.

**Fix.** Resolve once inside the egress check, keep the passed `SocketAddr`, and hand `rumqttc` the literal IP while pinning the TLS SNI/hostname to the original name for `mqtts`; alternatively re-verify the peer address after connect and drop if private.

---

### M6 — Query Entities materialises every matching entity in RAM; no LIMIT is pushed into the store

**Anchor:** `crates/antares-sql/src/store/pg_entity.rs:211` (SQL build), `:246-249`, `fetch_all` at `~:262`; callers `crates/antares-api/src/entities.rs:900-907` and `:1009`; memory/file path `crates/antares-sql/src/store/any.rs:294`; compile refusal `crates/antares-sql/src/compile/q.rs:148`
**Dimension:** dos-input-bounds
**Violates:** claude.md §2.1 (the Scorpio J3/J11c "nothing streams" lesson), §16.3 (result ceilings).

**What it is.** `SELECT entity FROM entities WHERE {…} ORDER BY id` — no LIMIT, no `fetch()` stream — collected into `Vec<Value>`; pagination is applied afterwards in Rust (`.skip(offset).take(limit)`).

**Repro.** `GET /ngsi-ld/v1/entities?type=Building&limit=1&q=address.city=="x"` against a tenant with a large entity set. `compile/q.rs:148` refuses every dotted path (and every `~=` and string ordering), so the predicate is dropped from the WHERE clause and Postgres streams back every row of the type, all materialised before one page is sliced. The same shape applies to `GET /temporal/entities` (`temporal.rs:987`).

**Why the verifier believed it.** SQL construction, `fetch_all`, the post-hoc slice and the memory-mode full snapshot were all read. Downgraded from high because the attack needs a tenant already holding a very large entity set — self-seeded via ~1000 batch requests, or an existing large tenant.

**Fix.** Push `LIMIT $n OFFSET $m` (offset+limit plus a probe row) into `PgEntityStore::query` and stream with sqlx `fetch()`; bound the in-memory/file snapshot the same way. At minimum, abort the collection loop in `filter_entities_fed` once `offset+limit+1` documents have passed the filter.

---

### M7 — `aggrPeriodDuration` reaches chrono constructors that panic on out-of-range input

**Anchor:** `crates/antares-api/src/temporal.rs:819` (`Duration::seconds`), `:826` (`checked_add_months(...).expect("date range")`); unvalidated parse at `:617`, `:631`, `:745`
**Dimension:** dos-input-bounds
**Violates:** claude.md §16.3 ("every request-shaped resource has a configured cap"), §14.5.

**What it is.** `parse_iso_duration` accumulates `n as i64` seconds and months from the query string with no range check; the aggregation window builder then calls `chrono::Duration::seconds` (which panics above ~i64::MAX/1000) and `checked_add_months(...).expect(...)`.

**Repro.** `GET /ngsi-ld/v1/temporal/entities/{id}?timerel=after&timeAt=1970-01-01T00:00:00Z&aggrMethods=sum&aggrPeriodDuration=PT10000000000000000S`, or `...=P400000Y` for the `Months` arm. The handler future panics; the connection is torn down with no response, and a backtrace is logged. Repeatable at will.

**Why the verifier believed it.** Parse path and both panicking call sites read directly. Downgraded from high: there is no `panic=abort` in `Cargo.toml` and no `catch_unwind`, so the panic unwinds inside the per-connection tokio task (`broker/main.rs:253-266`) — it kills that connection, not the process — and no `std::sync::Mutex` is held across the aggregation, so nothing is poisoned.

**Fix.** Validate the parsed `AggrPeriod` in `parse_iso_duration` (e.g. ≤ 10 years of seconds, ≤ 1200 months) and return `BadRequestData`; replace `Duration::seconds`/`checked_add_months(...).expect()` with `try_seconds`/`checked_*` mapped to `NgsiError`. Also bound the `Months` loop iteration count instead of walking month by month from an attacker-chosen anchor.

---

### M8 — Unbounded, never-evicted `@context` usage map keyed by client-supplied URLs

**Anchor:** `crates/antares-jsonld/src/loader.rs:216-221` (the field, under a doc comment claiming it is bounded), `:303-323` (`bump_url`), `:325-327` (`usage_list` clones the map), `:337` (the only removal); capped siblings at `:260-264`
**Dimensions:** dos-input-bounds, jsonld-context, supply-chain-config (found three times; merged)
**Violates:** claude.md §4.1 (R4/L7 — every cache has a max size), §2.1, §16.3.

**What it is.** `usage: RwLock<HashMap<String, CtxUsage>>` — a plain `HashMap` with no cap and no eviction, while every neighbouring cache is a `moka::sync::Cache::new(256)`. `bump_url` inserts one entry per distinct URL after every successful resolution; the only delete path is `usage_remove`, reachable solely from `DELETE /jsonldContexts/{id}`. `usage_list()` clones the whole map on every `GET /jsonldContexts`.

**Repro.** Repeated requests carrying `Link: <https://attacker/ctx/<n>.jsonld>; rel="http://www.w3.org/ns/json-ld#context"` with increasing `n`, each served a valid tiny `{"@context":{}}`. Every distinct URL adds a permanent `CtxUsage`. The `Link` header itself is never length-checked (`bounds_layer` measures only `req.uri()`), so keys can be tens of KB.

**Why the verifier believed it.** The type, the insert path, the absence of eviction and the contrast with the capped siblings were all read. Kept at medium rather than higher: `bump_url` runs only *after* a successful fetch, so each entry costs the attacker an outbound round trip — real unbounded growth, poor amplification.

**Fix.** Back `usage` with a size-capped `moka` cache (same 256-entry ceiling as `fetched`), cap the accepted `@context` URL length, and count evictions on `/q/health`.

---

### M9 — Temporal responses have no instance-count ceiling and `lastN` is parsed without an upper bound

**Anchor:** `crates/antares-api/src/temporal.rs:718-724` (`lastN` parse), `:270-311` (`window()`), `:234` (`TEMPORAL_INSTANCE_LIMIT`), `:386` (`content_range()`), `:459-567` (`present_temporal`); listing path at `:987`
**Dimension:** dos-input-bounds
**Violates:** claude.md §14.5 / audit U3 ("unbounded temporal aggregation with `lastN` applied after aggregating everything"), §16.3; CIM 009 6.3.10.

**What it is.** `lastN` has no maximum. `window()` clones every instance that passes the filters into a `Vec`, sorts it, and only then applies `last_n`. `TEMPORAL_INSTANCE_LIMIT` is used **only** by `content_range()` to decide whether to *set* a `Content-Range` header — `present_temporal` still renders every instance into the body, so the 206 advertises a truncation it does not perform.

**Repro.** Accumulate a large attribute history (`POST /temporal/entities/{id}/attrs`, many instances per 4 MiB body, or via auto-recording), then `GET /ngsi-ld/v1/temporal/entities/{id}?timerel=after&timeAt=1970-01-01T00:00:00Z`. The full history is cloned into `instances`, sorted, cloned again in `present_temporal`, and serialised in one body. Combined with `GET /temporal/entities` (which lists all temporal entities of the tenant), one request can allocate the entire tenant history twice.

**Why the verifier believed it.** All four code facts (unbounded parse, clone-then-truncate, header-only limit, full render) were read at the cited lines.

**Fix.** Clamp `lastN` to a server maximum (403 `TooManyResults` above it) and truncate `instances` to a hard per-attribute ceiling **inside** `window()`, emitting 206/`Content-Range` for the window actually returned — which is what 6.3.10 intends.

---

### M10 — `geoQ` query geometry has no vertex-count cap and DE-9IM relate runs per candidate entity

**Anchor:** `crates/antares-api/src/geo.rs:165-173` (parse), `:233-239` (`relate`); POST entry point at `crates/antares-api/src/batch.rs:699-701`; subscription path at `crates/antares-api/src/subscriptions.rs:143`
**Dimension:** dos-input-bounds
**Violates:** claude.md §16.3, §2.1.

**What it is.** `coordinates` is parsed with `serde_json` and only checked for `is_array()` before `parse_geometry`; no vertex or size limit exists. `matches()`/`matches_geometry()` then run `target.relate(q)` per candidate entity.

**Repro.** `POST /ngsi-ld/v1/entityOperations/query` with `{"type":"Query","entities":[{"type":"T"}],"geoQ":{"georel":"intersects","geometry":"Polygon","coordinates":[[ ~150k vertices ]]}}`. `batch.rs:699-701` serialises the body value straight into the `coordinates` virtual param, so the 8 KiB URI cap does not apply and `json_depth` does not bound a flat 150k-element array — the only ceiling is `MAX_BODY_BYTES` = 4 MiB. One request pins a CPU core for minutes; combined with **M6** the candidate set is unpaginated.

**Why the verifier believed it.** Parse site, relate site and the POST-body bypass of the URI cap were confirmed.

**Fix.** Cap the total coordinate count of a query geometry (e.g. 10 000 positions) in `GeoQuery::from_params` and reject with `TooComplexQuery` 403; apply the same cap to subscription `geoQ` at creation.

---

### M11 — `Host` header is trusted to mint and persist `@context` URLs (cross-tenant poisoning + stored SSRF)

**Anchor:** `crates/antares-api/src/contexts.rs:20-26` (`base_url`), used at `:138-147` and `crates/antares-api/src/subscriptions.rs:469-484`; listed cross-tenant at `contexts.rs:191-231`; boot re-seed only for `kind == "Cached"` at `crates/antares-broker/src/main.rs:197-200`; network resolve at `crates/antares-api/src/notify.rs:231-243`
**Dimensions:** auth-headers-trust, jsonld-context (merged)
**Violates:** claude.md §16.1 (identity is never read from a client-controlled field), §16.4; CIM 009 5.13.1.

**What it is.**
```rust
pub(crate) fn base_url(headers: &HeaderMap) -> String {
    let host = headers.get(header::HOST).and_then(|h| h.to_str().ok()).unwrap_or("localhost:9090");
    format!("http://{host}/ngsi-ld/v1/jsonldContexts")
}
```
Unvalidated `Host`, scheme hardcoded to `http`. The result becomes the stored identity of a Hosted context and — more importantly — the `jsonldContext` member written into subscriptions with an array `@context`. Because the boot re-seed covers only `Cached` rows, Hosted/ImplicitlyCreated URLs are **not** local after a restart and `sub_context` resolves them over the network.

**Repro.**
```
curl -H 'Host: attacker.example' -X POST http://broker:9090/ngsi-ld/v1/jsonldContexts -d '{"@context":{}}'
```
stores `http://attacker.example/ngsi-ld/v1/jsonldContexts/<uuid>`, which `GET /jsonldContexts` then serves to **every** tenant as a legitimate broker context URL. The same header on a subscription create writes that URL into `jsonldContext`; per CIM 009 5.8.6 it ships in that subscription's notifications, so any receiver dereferencing it fetches term definitions from the attacker (semantic confusion + dereference observation) — and after the next broker restart the notifier itself fetches it (`resolve_quiet`), a persistent restart-triggered SSRF.

**Why the verifier believed it.** `base_url`, both use sites, the tenant-blind listing, the `kind == "Cached"`-only re-seed and the network resolve were each read.

**Fix.** Derive the public base URL from configuration (`ANTARES_PUBLIC_BASE_URL`, falling back to the bind address) rather than the `Host` header; if `Host` must be used, validate against an allowlist and preserve the request scheme.

---

### M12 — Hostile `Cache-Control: max-age` panics the request task (`Instant + Duration` overflow)

**Anchor:** `crates/antares-jsonld/src/loader.rs:524-531`; unclamped parse at `:178-186`
**Dimension:** jsonld-context
**Violates:** CIM 009 6.3.16 (@context cache expiration); claude.md §16.3.

**What it is.** `stale_at: ttl.map(|d| std::time::Instant::now() + d)` — `impl Add<Duration> for Instant` is `checked_add(...).expect("overflow when adding duration to instant")` — and the TTL comes straight from the header via `Duration::from_secs(secs)` on a parsed `u64` with no clamp.

**Repro.** Host a context server answering `Cache-Control: max-age=18446744073709551615` with a valid `{"@context":{}}` body, then send any request referencing it (`Link: <https://attacker/ctx>; rel="http://www.w3.org/ns/json-ld#context"`). `fetch()` panics **after** the successful HTTP response; the connection is dropped with no response, and every retry re-panics because the entry is never cached. Served instead to a URL a *victim* tenant's subscription references, it kills that tenant's notification task (`notify.rs:239` `resolve_quiet`).

**Why the verifier believed it.** Both lines read; no `CatchPanicLayer` exists (router layers confirmed). Impact is a killed task/connection, not process death (no `panic=abort`).

**Fix.** `Duration::from_secs(secs.min(MAX_CONTEXT_TTL_SECS))`, and use `Instant::now().checked_add(d)` with a `None` fallback instead of `+`.

---

### M13 — Parsed-context caches are capped by entry count only, not by bytes

**Anchor:** `crates/antares-jsonld/src/loader.rs:260-264`; per-entry ceiling at `:198` and `:497-510`; the `merged` key at `:369`
**Dimension:** jsonld-context
**Violates:** claude.md §2.1 (@context cache ~50 MB budget; every buffer bounded).

**What it is.** `moka::sync::Cache::new(256)` caps **entry count**; no `weigher`/`max_capacity`-by-size is configured anywhere in the crate. Each `fetched` entry may hold a parsed `Value` of up to `MAX_CONTEXT_BYTES` = 5 MiB, and the `merged` cache's **key** is `user.to_string()` — the full serialised `@context`, up to the 4 MiB body cap.

**Repro.** Serve 256 distinct ~5 MiB `@context` documents from one host and reference each once (Link header or `ld+json` body). The `fetched` cache alone then holds ~1.3 GB against a 500 MB budget / 350 MiB CI gate, with no eviction pressure because moka counts 256 entries as "within capacity".

**Why the verifier believed it.** Construction sites, the per-entry ceiling and the key shape were all read.

**Fix.** Give each moka cache a `weigher` (serialised byte length) plus `max_capacity` in bytes, and lower `MAX_CONTEXT_BYTES` to something a broker actually needs (hundreds of KB).

---

### M14 — The 32-permit resolve semaphore is held across an unbounded network crawl with no aggregate deadline

**Anchor:** `crates/antares-jsonld/src/loader.rs:380-388` (permit held across `merge_entry`), `:265` (32 permits), `:255-256` (per-HTTP timeouts only), `:172-177` (`no-cache` → `Duration::ZERO`); single shared loader at `crates/antares-api/src/state.rs:58`
**Dimension:** jsonld-context
**Violates:** claude.md §16.1 seam 7, §2.1 (backpressure over buffering).

**What it is.** The permit is acquired and held for the *entire* recursive crawl. Critically, the fetch-count cap is post-hoc (**H6**), so a 400-URL `@context` array performs 400 sequential fetches while holding the permit, each bounded only by the per-request 10 s timeout. There is no aggregate deadline on `resolve()`, no timeout on `acquire()`, and no request-timeout layer anywhere.

**Repro.** 32 concurrent `POST /entities`, each with an `@context` array of ~400 attacker URLs whose server stalls ~9 s per response — every permit held for roughly an hour. Because the `Loader` is a single process-wide `Arc` shared by all tenants, every other tenant's cold `@context` resolution (entity create, query with a Link header, and the notification path) blocks on `acquire()`. A hostile server answering `Cache-Control: no-cache` makes `ttl_from_headers` return `Duration::ZERO`, so the re-fetch happens on every single request and the starvation is self-sustaining.

**Why the verifier believed it.** Permit scope, the post-hoc cap, the absent deadlines and the `no-cache` behaviour were all read; the shared-`Arc` cross-tenant scope confirmed at `state.rs:58`.

**Fix.** Wrap the whole resolution in `tokio::time::timeout` (aggregate deadline), acquire the permit per individual fetch rather than for the entire crawl, and floor the honoured TTL so a `no-cache` context cannot force a network round trip per request.

---

### M15 — Batch write forwarding sends the entire batch payload to every matching registration

**Anchor:** `crates/antares-api/src/batch.rs:217-249` (union matching), `:487-491` (forward loop), `:602` (`batch_delete` forwards the full id array); `crates/antares-api/src/federation.rs:665-687` (`reduce_to_scope`)
**Dimension:** federation
**Violates:** CIM 009 4.3.6.1 (registration-scope narrowing); claude.md §14.8, §16.7.

**What it is.** Matching is computed **once** from the union of all batch items (`spec_ids`/`spec_types`/`spec_attrs` accumulate over every item), and the forward loop then sends every item to each matching registration. `reduce_to_scope` filters only *attribute members* — when the registration has no `propertyNames`/`relationshipNames` it returns the whole object verbatim — and it never checks the entity id/type against `reg.ent_ids`/`reg.ent_types`. `batch_delete` is worse: `Some(Value::Array(ids.clone()))` forwards the complete, un-narrowed id list.

**Repro.** Register `information=[{entities:[{id:"urn:ngsi-ld:Device:harmless",type:"Device"}]}]` with `endpoint=https://attacker.example`. Any later `POST /ngsi-ld/v1/entityOperations/upsert` (or create/update/merge/delete) whose batch contains that id — or merely a matching type — causes the broker to POST **all** up-to-1000 entities of that batch, full attribute values included, to `attacker.example`.

**Why the verifier believed it.** Union accumulation, the per-reg forward loop, and `reduce_to_scope`'s attribute-only filtering were read directly. Downgraded from high: the attacker already needs write access to the tenant to create the registration, and with no per-resource authz that same principal can already read the tenant — so this is an over-broad-forwarding / 4.3.6.1 narrowing violation with a leak flavour, not a privilege escalation.

**Fix.** Filter `fwd_items` per registration to the items whose id/type actually matches that registration's `RegistrationInfo` (reuse `csource::entity_info_matches` against a per-item `CsrSpec`), narrow the `batch_delete` id array the same way, and make `reduce_to_scope` reject entities outside `reg.ent_ids`/`reg.ent_types` instead of only projecting attributes.

---

### M16 — `fed_query` imports any entity a peer returns

**Anchor:** `crates/antares-api/src/federation.rs:549-555`; the check that *does* exist at `:465-467` (`fed_retrieve`); `import_entity` `:305-329`; `recency`/merge `:330-336`, `:341-372`, `:562-586`
**Dimension:** federation
**Violates:** CIM 009 5.12 / 4.3.6.1; claude.md §16.7.

**What it is.** `if let Value::Array(a) = &body { for c in a { if let Some(doc) = import_entity(c, &reg, ctx) { out.push(...) } } }` — no check that the returned entity's id/type is one the registration covers. `fed_retrieve` performs exactly that check, so the omission is inconsistent, not deliberate. `import_entity` filters attribute members but keeps id/type unconditionally, and `merge_docs` then replaces a local instance whenever `recency(ai) > recency(ci)` — where `recency()` reads the **peer-supplied** `observedAt`/`modifiedAt`.

**Repro.** Register a CSR scoped to one entity you own, pointing at your server. On any `GET /ngsi-ld/v1/entities?type=Device` the broker forwards; return `urn:ngsi-ld:Device:victim` with attribute values carrying `observedAt: "9999-01-01T00:00:00Z"`. Those values overwrite the genuine local values in the response served to every client of that tenant, and attacker-invented entity ids are appended outright.

**Why the verifier believed it.** Both branches (checked vs unchecked) and the peer-controlled recency comparison were read at the cited lines.

**Fix.** Apply the same identity check `fed_retrieve` uses: drop any returned entity whose id is not in `reg.ent_ids` / does not match the registration's `idPattern` and whose type is not in `reg.ent_types`. Additionally clamp remote `observedAt`/`modifiedAt` used for recency, or resolve conflicts by registration precedence rather than peer-supplied timestamps.

---

### M17 — Every federated request full-scans and deserialises the tenant's entire registration table

**Anchor:** `crates/antares-api/src/federation.rs:160-165` (`matching_regs`); `crates/antares-sql/src/store/pg_doc.rs:519-535` (`SELECT registration FROM csource_registrations WHERE tenant_id = $1 ORDER BY id`, no LIMIT); the unused index built at `pg_doc.rs:374-399`
**Dimension:** federation
**Violates:** claude.md §16.7 ("Matching is SQL, not iteration … never a scan over all of a tenant's registrations").

**What it is.** `st.store.list(tenant, Kind::Registration).unwrap_or_default().into_iter().filter_map(...)` — every registration document of the tenant is read and parsed into a `serde_json::Value` in the request path. `grep -rn csource_index crates/ --include=*.rs` shows the writer and tests only: **the indexed match structure §16.7 mandates is never queried.** No per-tenant registration cap exists.

**Repro.** Create a few hundred registrations near the 4 MiB body cap, then issue any `GET /ngsi-ld/v1/entities?type=X` on that tenant — every read and every write becomes tens of MB of transient allocation plus a full table read, degrading the whole broker (shared pool, shared process).

**Why the verifier believed it.** The listing call, the LIMIT-less SQL and the grep result were all confirmed.

**Fix.** Select candidates with an indexed query against `csource_index` (`(tenant_id, entity_type)` / `(tenant_id, entity_id)` + `ops` bitmask + `expires_at`) instead of listing whole documents, and cap the candidate count per request.

---

### M18 — Egress pre-check performs an untimed DNS resolution on the request path for every forward

**Anchor:** `crates/antares-api/src/egress.rs:47-56`; `crates/antares-jsonld/src/loader.rs:110-120` (`tokio::net::lookup_host`, no timeout); callers `crates/antares-api/src/federation.rs:252` and `crates/antares-api/src/notify.rs:1143`; second resolution at `loader.rs:139-155`
**Dimension:** federation
**Violates:** claude.md §16.4, §2.1 (every outbound wait bounded).

**What it is.** `check_host` calls `tokio::net::lookup_host((host, port)).await` with no timeout wrapper, on every forward, uncached — and reqwest's client then resolves the same name a second time through `PolicyResolver`. tokio implements `lookup_host` via `spawn_blocking`/`getaddrinfo`, so a stalled resolver pins a blocking-pool thread. reqwest's connect/request timeouts do not cover this pre-flight.

**Repro.** Register CSRs (or subscriptions) whose endpoint hostnames are delegated to a nameserver you control that black-holes queries. Each forwarded operation stalls in `getaddrinfo` for the system resolver timeout (typically 5–40 s) *before* the 8 s request timeout can apply; a few hundred such registrations exhaust the blocking pool and stall unrelated work broker-wide.

**Why the verifier believed it.** The untimed lookup, the per-forward invocation and the double resolution were confirmed at the cited lines.

**Fix.** Wrap `check_host` in `tokio::time::timeout` (~1–2 s, expiry = denied) and memoise resolution results for a short TTL keyed by `host:port` so a single request does not resolve the same destination twice.

---

### M19 — Notifications are still delivered for a subscription that was deleted after matching

**Anchor:** `crates/antares-api/src/notify.rs:1119-1140` (bookkeeping whose `None` is discarded), `:1141-1166` (unconditional send), `:559-562`/`:574` (match-time snapshot and `is_active` evaluation), `:128-135` (`is_active`); the guard that **does** exist, on the CSource path, at `:864-876`
**Dimension:** concurrency-state
**Violates:** CIM 009 5.8.5 / 5.8.6; claude.md §4.1 (L3/L4a), §14.1.

**What it is.** `deliver_as` writes bookkeeping via `st.store.mutate(...)` whose `None` result (row gone) is swallowed by `.unwrap_or_else(|e| { warn!; None })`, then sends unconditionally. The CSource path does exactly the missing check:
```rust
// §4.1 L4 / 5.11.7: re-check the subscription still exists right before the send
if !matches!(st.store.get(tenant, Kind::CSourceSubscription, sub_id), Ok(Some(_))) { continue; }
```
No equivalent guard, and no re-evaluation of `is_active`, exists on the entity-subscription path.

**Repro.** Create a subscription pointing at a victim URL, drive a write burst so the (unbounded, serial — **H8**) change queue backs up by minutes, then `DELETE /ngsi-ld/v1/subscriptions/{id}`. The API answers 204 and the resource is gone, yet every backlogged notification is still POSTed. The same window applies to a subscription that expires between match and send — the named Scorpio L3/L4a defect class, and expiry/deletion is the only lifetime control the resource has.

**Why the verifier believed it.** Both paths read side by side; the swallowed `None` and the unbounded backlog that widens the window were confirmed.

**Fix.** Re-read the subscription immediately before the send and require `Some(doc)` + `is_active(&doc)` — make the store read the single yield point for the entity path exactly as `send_csource_jobs` already does, or gate on the `mutate` result being `Some`.

---

### M20 — No connection cap, header-read timeout or request timeout on the accept loop

**Anchor:** `crates/antares-broker/src/main.rs:245-268`; router layers at `crates/antares-api/src/lib.rs:215-238`; `bounds_layer` at `crates/antares-api/src/bounds.rs:83-98`
**Dimension:** concurrency-state
**Violates:** claude.md §2.1 rule 1, §16.3 (resource bounds).

**What it is.** The accept loop spawns one task per connection with a bare hyper builder configured only with `http1().title_case_headers(true)` — no `header_read_timeout`, no keep-alive/idle timeout, no `Semaphore`, no max-connections. `grep -rn 'TimeoutLayer|ConcurrencyLimit' crates/` returns nothing. `bounds_layer` can only act once headers are parsed and the body is being read, so a never-completing request is never bounded by it.

**Repro.** Open many TCP connections and send `GET /ngsi-ld/v1/entities HTTP/1.1\r\n` followed by one header byte every 30 s (or `Content-Length: 4194304` with a trickled body). Each connection holds a tokio task, hyper connection state and a partially filled read buffer forever. FDs and RSS grow until the budget is exhausted or `accept()` fails.

**Why the verifier believed it.** The bare builder, the missing layers and the ordering of `bounds_layer` relative to header parsing were all confirmed.

**Fix.** Add `builder.http1().header_read_timeout(...)`, a global `TimeoutLayer` on the router, and gate `accept()` on a `Semaphore` sized to a configured max-connections (permit released when the connection task ends).

---

### L1 — `/jsonldContexts` is mutable cross-tenant: delete and reload parse the tenant but never use it

**Anchor:** `crates/antares-api/src/contexts.rs:330` (tenant bound but unused), `:88-114` (`find_entry`), `:191-231` (`list_contexts`), `:273` (serve), `:339`/`:343` (`refetch`), `:367` (`context_delete`), `:369` (`usage_remove`); store methods without a tenant at `crates/antares-sql/src/store/any.rs:377-389`; SQL at `crates/antares-sql/src/store/pg_doc.rs:589`
**Dimensions:** tenant-isolation, auth-headers-trust (merged — the same defect reported twice)
**Violates:** claude.md §16.1.2 (`&TenantId` is the first parameter of every public store method), read against §8.3 / §16.1.4.

**What it is.** `context_get`/`context_list`/`context_delete` are the only store methods in the crate with no `&TenantId` parameter, and `DELETE FROM jsonld_contexts WHERE id = $1` carries no tenant predicate. The **disclosure** half is design-sanctioned: §8.3 declares `jsonld_contexts` "shared across tenants BY DESIGN (only cross-tenant table)" and §16.1.4 names it "the single sanctioned cross-tenant structure". What the sanction does **not** cover is cross-tenant **mutation**.

**Repro.** `GET /ngsi-ld/v1/jsonldContexts?details=true` with any tenant header to read another tenant's `localId`s, then `DELETE /ngsi-ld/v1/jsonldContexts/{localId}` → 204, row gone. Every subsequent request of the victim tenant that resolves that Link header fails with `LdContextNotAvailable`; subscriptions silently fall back to the core context, changing the term mapping of their notifications. `DELETE …?reload=true` additionally forces an on-demand re-fetch of an arbitrary cached URL.

**Why the verifier believed it.** All call sites and the tenant-less SQL were read. Downgraded to low precisely because the read side is sanctioned — the residual defect is availability/integrity, not confidentiality.

**Fix.** Store an owner tenant on `Hosted`/`ImplicitlyCreated` context rows and require it to match on delete/reload; thread `&TenantId` through `AnyStore::context_get/list/delete` and filter the listing for those kinds. The read-side cache sharing sanctioned by §8.3 does not extend to destructive operations.

---

### L2 — The RLS backstop is inert in every shipped configuration: the broker connects as a Postgres superuser

**Anchor:** `crates/antares-sql/src/pg.rs:14-20`; `crates/antares-sql/migrations/0001_init.sql:6` (needs `CREATE EXTENSION postgis`), `:126-128` (the migration's own note), `:131-143` (the policies); `compose-files/docker-compose.yml:9`, `compose-files/docker-compose-etsi.yml:35`; `crates/antares-broker/src/main.rs:95`
**Dimension:** tenant-isolation
**Violates:** claude.md §16.1.3 ("RLS backstop always on"), §3.

**What it is.** `connect()` runs `MIGRATOR` on the **serving** pool; migration 0001 requires superuser-only `CREATE EXTENSION postgis`, and the migration itself states "the broker must connect as a non-superuser role for the backstop to bite". Both compose files set `POSTGRES_USER: antares`, which the `postgres` image makes SUPERUSER, and startup performs no `rolsuper`/`rolbypassrls` assertion. A superuser session ignores `ENABLE`/`FORCE ROW LEVEL SECURITY`, so the policies never evaluate.

**Repro.** Bring up `compose-files/docker-compose.yml` and run `SELECT rolsuper FROM pg_roles WHERE rolname='antares'` → `true`.

**Why the verifier believed it — and why only low.** Every code and config fact was confirmed. Downgraded because this removes **defense in depth only**: an independent re-check confirmed every doc/entity store path carries an explicit `tenant_id = $1` predicate (`pg_doc.rs:519-535`, `pg_entity.rs:177-233`) plus `set_tenant`, so no live cross-tenant leak follows. The originally cited "proof" — `outbox.rs:35`'s `SELECT` without a tenant predicate — is a drain-loop query that legitimately spans tenants and is currently only called from tests, so it is not evidence of a leaking pattern.

**Fix.** Split the migrator role from the serving role (run `MIGRATOR` with an admin URL; serve with an app role that has neither `rolsuper` nor `rolbypassrls`), ship that split in compose/CI, and fail startup hard when `SELECT rolsuper OR rolbypassrls FROM pg_roles WHERE rolname = current_user` is true.

---

### L3 — The CI SQL-injection grep guard matches zero lines and cannot fail

**Anchor:** `.github/workflows/ci.yml:75-78`; the actual dynamic-SQL sites at `crates/antares-sql/src/store/pg_doc.rs:356,420,460,476,508,528,551`, `crates/antares-sql/src/store/pg_entity.rs:226`, `crates/antares-sql/src/maintenance.rs:73,116`
**Dimension:** sql-injection
**Violates:** claude.md §16.2 ("Enforcement, not intention: CI greps deny `format!`/string-concat feeding `sqlx::query`").

**What it is.**
```yaml
! grep -rn 'sqlx::query(&format!' crates/ --include='*.rs' | grep -v 'crates/antares-sql/'
```
Two independent reasons it is inert: (a) the literal `sqlx::query(&format!` occurs nowhere — every dynamic statement uses the sqlx 0.9 spelling `sqlx::query(sqlx::AssertSqlSafe(...))`; (b) the `grep -v 'crates/antares-sql/'` exclusion removes the **only** crate that contains SQL (the sole `sqlx` reference outside it is a comment in `broker/main.rs:10`). The search space after exclusion is empty by construction. `AssertSqlSafe` makes this worse, not better: it is an explicit opt-out of sqlx's own const-string safety check, so the compiler no longer objects either.

**Repro.** `grep -rn 'sqlx::query(&format!' crates/` returns nothing; the step exits 0 unconditionally.

**Why the verifier believed it — and why low.** Both greps were run. No present exploit: the paths this should guard are, today, verifiably safe (see the verified-clean record in §4). This is a missing control.

**Fix.** Grep for the actual opt-out and scope it *to* `antares-sql` rather than excluding it: fail when `AssertSqlSafe(` appears on a line that also contains `format!` unless the file is on a reviewed allowlist (today: `pg_doc.rs` enum-derived names, `pg_entity.rs::query`, `maintenance.rs` DDL). Useful second rule: `! grep -rn 'AssertSqlSafe' crates/ --include='*.rs' | grep -v 'crates/antares-sql/src/\(store/pg_doc\|store/pg_entity\|maintenance\).rs'`.

---

### L4 — `POST /jsonldContexts` flushes the entire parsed-context LRU on every call

**Anchor:** `crates/antares-jsonld/src/loader.rs:545-556` (`put_local`); called unconditionally at `crates/antares-api/src/contexts.rs:146-147`
**Dimension:** dos-input-bounds
**Violates:** claude.md §6.3, §2.1.

**What it is.** `put_local` ends with `self.merged.invalidate_all(); self.merged_urls.invalidate_all();` — the whole 256-entry parsed-context cache that §6.3 calls "the centerpiece". Each call also inserts a permanent row into the cross-tenant contexts map with no count limit.

**Repro.** Loop `POST /ngsi-ld/v1/jsonldContexts` with `{"@context":{}}`. Every call empties the LRU shared by all tenants, so concurrent requests using a non-core `@context` fall back to the cold path, queue on the 32-permit resolve semaphore and re-issue their remote fetches. A trickle of cheap POSTs converts the hot path into a network-bound one.

**Fix.** Invalidate selectively — drop only merged entries whose recorded URL list (`merged_urls`) contains the new URL — and cap/rate-limit the number of Hosted contexts a tenant may create.

---

### L5 — Raw database error text is returned to the client in the 500 `ProblemDetails` body

**Anchor:** `crates/antares-sql/src/store/any.rs:19-21`; `crates/antares-model/src/error.rs:69-76`; `crates/antares-api/src/negotiate.rs:74-92`
**Dimension:** auth-headers-trust
**Violates:** claude.md §16.1.7 ("No side-channel leaks"), §5.2 error mapping.

**What it is.** `fn db(e: sqlx::Error) -> NgsiError { NgsiError::InternalError(format!("database error: {e}")) }` → `ProblemDetails.detail` → the client-visible JSON error body. Handlers propagate store errors with `?`.

**Repro.** Any request Postgres rejects — e.g. an oversized entity id producing a btree index-row-size error naming the index, or an embedded NUL in a value (`unsupported Unicode escape sequence …`), or any request while the pool cannot connect (which surfaces the DSN host:port). This hands an unauthenticated attacker the backend product, table/constraint/index names and internal hostnames.

**Why the verifier believed it.** The full chain was traced end to end. Low: verbose-error information disclosure, not data access, and only in Pg mode.

**Fix.** Log the `sqlx::Error` at error level and return a fixed detail (`"internal error"`) plus a correlation id; never let driver `Display` output reach `ProblemDetails`.

---

### L6 — The `Prefer` header switches any response into unbounded buffering plus a full JSON re-parse

**Anchor:** `crates/antares-api/src/conformance.rs:242` (early return), `:255-267` (`to_bytes(body, usize::MAX)` → `from_slice` → `to_vec`), `:42` (the `ver < NATIVE` test, which happens later)
**Dimension:** auth-headers-trust
**Violates:** claude.md §2.1 rule 1, §16.3.

**What it is.** The early return only skips requests with no parsable `Prefer`; a version at or above `NATIVE` (e.g. `Prefer: ngsi-ld=9.9`) still buffers, parses and re-serialises, because the amendment test happens inside `amend_entity`.

**Repro.** `GET /ngsi-ld/v1/entities?type=Building&limit=1000` with `Prefer: ngsi-ld=9.9`, issued concurrently.

**Why the verifier believed it — and why low.** Code confirmed, but the buffered body is one the handler already produced fully in memory (`axum::Json`), not an attacker-streamed body, so `usize::MAX` is not an attacker-controlled ceiling; the real cost is a ~2–3× transient amplification bounded by `max_limit = 1000` (`state.rs:63-64`).

**Fix.** Return early when the requested version is ≥ `NATIVE`, and bound the buffer with `bounds::MAX_BODY_BYTES` (or a dedicated response cap), passing the body through untouched above it.

---

### L7 — `EntityId::new` accepts control characters (log-line forgery, invalid header)

**Anchor:** `crates/antares-model/src/id.rs:70-86`; sole check for subscription ids (`crates/antares-api/src/subscriptions.rs:38-44`), `notification.endpoint.uri` (`subscriptions.rs:177-180`) and registration endpoints (`crates/antares-api/src/csource.rs:149-155`); sinks at `crates/antares-api/src/notify.rs:1085`, `:1144`, and `negotiate.rs` `created()`
**Dimension:** auth-headers-trust
**Violates:** claude.md §16.1.7, §7 (token-safe identifiers).

**What it is.** `EntityId::new` validates only that a scheme precedes the first `:` and that the remainder is non-empty; no character class is applied to the remainder, so CR, LF and NUL pass.

**Repro.** `POST /ngsi-ld/v1/subscriptions` with `"id": "urn:x:1\n2026-08-04T00:00:00Z  INFO antares: forged audit line"` and an endpoint that fails delivery — the tracing fmt layer writes the id verbatim, injecting attacker-authored lines into the operator's log stream. With `\r\n` in the id, `created()` produces an invalid `HeaderValue` and axum answers 500 **after** the resource was persisted, leaving the API and the store disagreeing.

**Fix.** Reject C0 control characters (and DEL) in `EntityId::new`, and sanitise client-derived strings before they reach a `tracing` message.

---

### L8 — Cached `@contexts` written through to the store are served on demand

**Anchor:** `crates/antares-api/src/contexts.rs:89-92` (`find_entry` checks the store first), `:272-274` (Stored branch returns `doc["body"]`), `:276-283` (the `Cached` guard that is never reached); deterministic id minted at `crates/antares-broker/src/main.rs:184-190`
**Dimension:** jsonld-context
**Violates:** CIM 009 5.13.4.4.

**What it is.** `Stored` wins in `find_entry`, and `serve_context`'s Stored branch returns the raw body without inspecting `doc["kind"]`, skipping the guard whose own comment reads "Cached @contexts are never served on demand (5.13.4.4)". The write-through hook stores externally fetched documents as `kind="Cached"` under the client-computable id `uuid5(NAMESPACE_URL, url)`.

**Repro.** Cause the broker to fetch `https://attacker/ctx` (any request with that Link `@context`), then `GET /ngsi-ld/v1/jsonldContexts/<uuid5(NAMESPACE_URL,'https://attacker/ctx')>` — the attacker-supplied document is returned verbatim from the broker's own origin as `application/json` instead of 422.

**Why the verifier believed it — and why low.** Both branches read. Impact is bounded: only the `@context` member of a document the broker was induced to fetch is echoed — a spec violation with a mild content-mirroring side effect.

**Fix.** In `find_entry`, check the stored row's `kind` and return `CtxEntry::Cached` for `kind == "Cached"` so the existing `OperationNotSupported` guard applies.

---

### L9 — No hop limit on forwarded operations; loop detection is self-alias suffix matching only

**Anchor:** `crates/antares-api/src/federation.rs:126-128` (`via_loop`), `:130-135` (`outbound_via`), `:691` (508 raised only on self-alias recurrence)
**Dimension:** federation
**Violates:** claude.md §16.7 ("Via/hostAlias chains with hop limit (default 5)"); CIM 009 6.3.17/6.3.18.

**What it is.** `outbound_via` copies the inbound chain verbatim and appends, with no count of the tokens already present; `via_loop` only tests whether any token `ends_with(alias)`. No hop constant exists in the tree.

**Repro.** Requires a multi-broker ring: with N mutually-registering brokers of distinct aliases, one client request propagates N hops before any broker recognises its own alias; with K registrations per broker the tree is Kᴺ outbound requests, none cut short (and no aggregate deadline — **M4**).

**Why the verifier believed it — and why low.** The code facts hold, but exploitation needs an operator-configured topology, not something an attacker creates against a single deployment: self-alias detection already terminates any cycle through this broker.

**Fix.** Count the comma-separated tokens of the inbound `Via` and reject with 508 above a configured hop limit (default 5) **before** forwarding; match Via tokens by exact pseudonym equality after parsing `protocol SP received-by`, not `ends_with`.

---

### L10 — Registration `tenant` and `contextSourceInfo` are stored but ignored on forwards

**Anchor:** `crates/antares-api/src/federation.rs:272-274`; persisted-but-never-read at `crates/antares-sql/src/store/pg_doc.rs:256-257` (`"tenant_at_peer"`, `"headers"`)
**Dimension:** federation
**Violates:** CIM 009 5.2.9 / 6.3.19; claude.md §16.1.1 ("federation peer tenants come from the registration's own `tenant` member").

**What it is.** `if tenant.as_str() != "default" { req = req.header("NGSILD-Tenant", tenant.as_str()); }` sends the **local** tenant name. `grep -rn 'contextSourceInfo|tenant_at_peer' crates/ --include=*.rs` returns only the write sites — never a read.

**Repro.** Register a CSR pointing at a listener you control, then issue any matching `GET /entities` as tenant X — the forwarded request arrives with `NGSILD-Tenant: X`.

**Why the verifier believed it — and why low.** Confirmed; impact is limited to disclosing an internal tenant label to a peer the tenant itself registered. The "tenant confusion at the far end" consequence is plausible but not demonstrated.

**Fix.** Send the registration's own `tenant` member (`tenant_at_peer`) as `NGSILD-Tenant` when present, omitting the local tenant name otherwise. If `contextSourceInfo` is later applied as outbound headers, allowlist header names and reject control characters at registration time (see **L7**).

---

### L11 — GHCR `:latest` is published on a gate that excludes clippy, unit tests and cargo-deny advisories

**Anchor:** `.github/workflows/etsi.yml:211` (`publish`, `needs: etsi-aggregate`, `if: … success()`); the excluded checks live in `.github/workflows/ci.yml:55-73`
**Dimension:** supply-chain-config
**Violates:** claude.md §16.5 (cargo-deny advisories + license gate in CI).

**What it is.** `success()` is scoped to jobs in the *same* workflow. `cargo test --workspace`, `cargo clippy -- -D warnings` and `cargo-deny check licenses advisories` live in `ci.yml`, which is not a dependency of `publish`. A push to master introducing an active RUSTSEC advisory, a non-allowlisted license, or a failing test still publishes the tag operators pull.

**Repro.** Push a commit that fails `ci.yml` but passes the 32-cell ETSI matrix; `ghcr.io/<owner>/antares-broker:latest` is published anyway.

**Fix.** Either move the cargo-deny/clippy/test steps into a job inside `etsi.yml` and add it to `publish`'s `needs:`, or make `publish` a `workflow_run` job requiring the `ci` workflow conclusion to be `success` for the same SHA.

---

### L12 — Container bases, the build tool and the toolchain are unpinned; the release build is not `--locked`

**Anchor:** `Dockerfile:3` (`FROM rust:1-slim`), `:15` (`cargo install cargo-auditable --locked`), `:17` (`cargo auditable build --release` — no `--locked`), `:20` (`FROM gcr.io/distroless/cc-debian12:nonroot`); `rust-toolchain.toml:2` (`channel = "stable"`)
**Dimensions:** supply-chain-config, dependency-cves (merged)
**Violates:** claude.md §16.5 (SBOM in release builds), §9 ("rust-toolchain.toml # pinned toolchain").

**What it is.** Three unpinned inputs feed every published image: mutable base tags; a `cargo install` that resolves the newest `cargo-auditable` at build time and then *rewrites the shipped executable's embedded dependency manifest* with full build-time privileges (`--locked` pins that crate's own deps, not its version); and a release build that omits `--locked`. `rust-toolchain.toml` floats the channel despite §9 specifying a pin. The positives: the image is genuinely multi-stage, non-root (`:nonroot` = UID 65532) and carries no build secret.

**Why the verifier believed it — and why low.** All facts confirmed. The `--locked` argument is the weakest: cargo honours a committed, up-to-date `Cargo.lock` without it, so the audited and shipped graphs diverge only if `Cargo.toml`/`Cargo.lock` are out of sync. The residual concern is build reproducibility — the SBOM describes a build whose compiler and tooling cannot be re-derived.

**Fix.** Pin both `FROM` lines by digest, pin the tool (`cargo install cargo-auditable --locked --version X.Y.Z`), add `--locked` to the release build, and pin `channel` in `rust-toolchain.toml` to the version actually in use.

---

### L13 — Subscriber MQTT credentials are written to the log in cleartext

**Anchor:** `crates/antares-api/src/notify.rs:1144`; URI form documented at `crates/antares-notifier/src/mqtt.rs:31-49`; same shape at `crates/antares-api/src/federation.rs:253`
**Dimension:** supply-chain-config
**Violates:** claude.md §16.1.7, §16.5 (secrets never in anything that reaches logs).

**What it is.** `tracing::warn!("notification endpoint {uri} refused by egress policy: {e}")` logs the raw endpoint URI, which may carry `user:pass`. In the shipped distroless image stdout goes straight to the container log collector, so a tenant's MQTT credential lands in the operator's shared aggregator.

**Repro.** Subscribe with `notification.endpoint.uri = "mqtts://ingest:S3cret@10.0.0.5/topic"` while `ANTARES_EGRESS_ALLOW_PRIVATE` is unset — the refusal WARN prints the full URI including the password.

**Why the verifier believed it — and the narrowing.** Confirmed, but materially narrower than first claimed: this WARN fires **only** when the egress policy *refuses* the endpoint, so a public `mqtts` host never reaches it; the other URI-bearing log (`notify.rs:1149`) is at debug level and `mqtt.rs` emits no tracing at all.

**Fix.** Redact userinfo before logging: parse with `reqwest::Url`, `set_password(None)`/`set_username("")`, and log the sanitised form (or scheme+host+port only). Give `MqttEndpoint` (`mqtt.rs:20-28`) a manual `Debug` so its password cannot reach a log through `{:?}`.

---

### L14 — `cargo-deny` gate omits `[bans]` and `[sources]`

**Anchor:** `deny.toml:3-13` (`[licenses]`), `:15-16` (`[advisories] yanked = "deny"`); `.github/workflows/ci.yml:70-73` (`command: check licenses advisories`)
**Dimensions:** supply-chain-config, dependency-cves (merged)
**Violates:** claude.md §16.5, §9.5.

**What it is.** `deny.toml` is 16 lines with no `[sources]`, no `[bans]` and no `unmaintained` setting, and CI never invokes `check sources` or `check bans`. Consequences visible in this very lockfile: the duplicate TLS stack of **L23** (rustls 0.22.4 *and* 0.23.43, tokio-rustls 0.25.0 *and* 0.26.4, rustls-webpki 0.102.8 *and* 0.103.13) passes silently; `paste 1.0.15` (RUSTSEC-2024-0436, unmaintained; `Cargo.lock:1528`, via `tikv-jemalloc-ctl`) has no `ignore` entry, so the gate's verdict depends on which version the floating `EmbarkStudios/cargo-deny-action@v2` tag resolves to. With no `[sources]` allowlist, a future `foo = { git = "https://attacker/foo" }` passes CI unremarked — the exact substitution §16.5 claims to gate. Positive: there is **no** `ignore = [...]` list quietly suppressing RUSTSEC ids.

**Fix.** `command: check advisories bans licenses sources`; add `[bans] multiple-versions = "warn"` with explicit `deny` entries for known-EOL crates; `[sources] unknown-registry = "deny"`, `unknown-git = "deny"`, `allow-registry = ["https://github.com/rust-lang/crates.io-index"]`; `[advisories] unmaintained = "all"` with a comment-justified `ignore` so the outcome is deterministic. Pin the action to a commit SHA.

---

### L15 — Per-destination circuit-breaker map grows without bound

**Anchor:** `crates/antares-api/src/egress.rs:26-29` (the map), `:95-102` (`record_failure` inserts), `:88-93` (`record_success` is the only removal)
**Dimension:** supply-chain-config
**Violates:** claude.md §2.1 rule 1, §4.1 (L2/L3/L5 — maps that only ever grow), §16.7.

**What it is.** `breakers: Mutex<HashMap<String, Breaker>>` with no cap, TTL or sweep; entries for destinations that never recover stay forever.

**Repro.** Create subscriptions with endpoints at many distinct public-resolving hostnames under a wildcard domain that refuse connections; each failing delivery adds a permanent entry.

**Why the verifier believed it — and the caveat.** Confirmed; growth is ~tens of bytes per distinct destination and each entry costs a real failed delivery, so it is a slow leak. The originally attached `.expect("breaker lock")` concern is **not** a separate defect: the critical sections cannot panic, so the mutex cannot be poisoned by this code.

**Fix.** Replace the `HashMap` with a bounded `moka::sync::Cache` (TTL ≈ a few multiples of `COOLDOWN`, max entries in the low thousands). See also **M2** — the same map is not tenant-keyed.

---

### L16 — The fatal unknown-key check rejects Kubernetes-injected `ANTARES_*` service vars

**Anchor:** `crates/antares-broker/src/main.rs:40-45`; `KNOWN_KEYS` at `:11-24`
**Dimension:** supply-chain-config
**Violates:** claude.md §14.3 / §9.1 (unknown keys are a startup error) **vs** §10 (stateless broker pods).

**What it is.** Every env var starting with `ANTARES_` that is not one of the eight known keys aborts startup. Kubernetes injects Docker-link-style vars for every Service in the namespace: a Service named `antares` — the natural name for §10's stateless broker pods — produces `ANTARES_SERVICE_HOST`, `ANTARES_SERVICE_PORT`, `ANTARES_PORT`, `ANTARES_PORT_9090_TCP*`. Every pod then exits with `unknown config key ANTARES_PORT`. CI never catches it because `ci.yml` sets `ANTARES_TEST_*` only for cargo, never for the binary.

**Repro.** Run the binary with `ANTARES_PORT=tcp://10.0.0.1:9090` in the environment.

**Fix.** Scope the fatal check to a config-only prefix Kubernetes cannot collide with (`ANTARES_CFG_`), or keep `ANTARES_` but explicitly skip the injected shapes (`*_SERVICE_HOST`, `*_SERVICE_PORT`, `ANTARES_PORT*`) and log them at debug instead of dying.

---

### L17 — A deleted entity's temporal history is resurrected by a concurrent write

**Anchor:** `crates/antares-api/src/entities.rs:102-149` (`mirror_record`, check-then-act that unconditionally re-creates), `:153-157` (`mirror_delete_entity`); call sites after the store commit at `:1118-1121` (delete) and `:1395-1403` (merge)
**Dimension:** concurrency-state
**Violates:** claude.md §3.1 (the `entityDeleted` fence; ordering key `(incarnation, version)`), §3 (per-tenant erasure).

**What it is.** The temporal mirror runs **after** the entity store call has committed and released its lock; nothing serialises the pair, and there is no deletion tombstone on `Kind::Temporal` to reject a write for a deleted entity.

**Repro.** Issue `PATCH /ngsi-ld/v1/entities/{id}` and `DELETE /ngsi-ld/v1/entities/{id}` concurrently in a loop. The interleaving A.mutate-commit → B.delete-commit → B.mirror_delete → A.mirror_record re-creates `temporal_entities`/`attr_instances` rows for the just-deleted entity; `GET /ngsi-ld/v1/temporal/entities/{id}` then returns 200 for an entity that no longer exists, and nothing later cleans it up.

**Why the verifier believed it — and why low.** Code shape confirmed. Downgraded: exploitation requires winning a narrow interleaving, the result is orphaned history for an entity the attacker could already write, and no tenant or confidentiality boundary is crossed. It is nonetheless a silent retention/erasure failure reachable by ordinary concurrent client traffic.

**Fix.** Perform the temporal mirror inside the same critical section as the entity write (extend the store `mutate` closure / same transaction), or make the temporal store refuse `create` for an id with a recorded `deleted_at` fence — the incarnation fence §3.1 specifies but which is not implemented anywhere in the tree.

---

### L18 — Change events are emitted after the write lock and carry no version or incarnation

**Anchor:** `crates/antares-sql/src/store.rs:138` (`ChangeHook` signature), `:370-373`, `:387-390`, `:413-418`, `:440-463`; Pg backend `crates/antares-sql/src/store/any.rs:116-118,210,255-257,323-327`; the bumped-but-unused version at `crates/antares-sql/src/store/pg_entity.rs:279-280`; dead bus at `crates/antares-broker/src/main.rs:160`
**Dimension:** concurrency-state
**Violates:** claude.md §3.1.3 (version bumped under the row lock and carried by `ChangeEvent`; incarnation fence), §7.

**What it is.** `emit` runs after the guard is dropped / after `tx.commit()`, and `ChangeHook` carries only `(&TenantId, Option<Value>, Option<Value>)` — no ordering token. The `version` column `PgEntityStore::mutate` does bump never reaches a consumer, and `antares-bus`'s `ChangeEvent { …, version }` is dead code (`let _bus = LocalBus::new(1024);`).

**Repro.** Two concurrent `PATCH /ngsi-ld/v1/entities/{id}/attrs`: T1 commits v1 and is descheduled before `emit`; T2 commits v2 and emits first. The subscriber receives v2 then v1, presenting stale values as current — and with delete/recreate it receives `entityCreated` before `entityDeleted`, so a state-projecting consumer believes a live entity is deleted. Nothing in the payload lets the consumer discard the stale one.

**Why the verifier believed it — and why low.** Confirmed. The consequence is stale/reordered notifications under concurrent single-entity writes — a correctness/integrity defect with no confidentiality or availability impact; the design itself states the matcher is deliberately ordering-tolerant.

**Fix.** Capture the bumped `version` (and the row `created_at` as incarnation) inside the locked section and pass them through `ChangeHook`/`ChangeEvent`; either emit while still holding the write critical section, or order the queue by `(incarnation, version)` so consumers can apply the §3.1 last-writer-wins rule.

---

### L19 — `expiresAt` is enforced by raw string comparison against a `Z` timestamp

**Anchor:** `crates/antares-api/src/notify.rs:128-135` (`is_active`); `crates/antares-api/src/state.rs:87-90` (`now_iso` always emits `Z`); stored verbatim after the same lexicographic check at `crates/antares-api/src/subscriptions.rs:247-256`; same pattern at `crates/antares-api/src/csource.rs:157-165`, `crates/antares-api/src/federation.rs:165-172`, `subscriptions.rs:392-395`; offsets explicitly accepted by `crates/antares-jsonld/src/expand.rs:604-608`
**Dimension:** concurrency-state
**Violates:** CIM 009 5.8.1 / 5.8.6 / 4.22; claude.md §4.1 (L4a/L4b), §5.4.6.

**What it is.** `!sub.get("expiresAt").and_then(Value::as_str).is_some_and(|e| e < now_iso().as_str())` — a lexicographic comparison of an offset-bearing RFC-3339 string against a `Z` string.

**Repro.** `POST /ngsi-ld/v1/subscriptions` at 10:00Z with `"expiresAt": "2026-08-04T23:00:00+14:00"` (real instant 09:00Z). (a) The create-time guard compares `"2026-08-04T23…" < "2026-08-04T10…"` → false, so a subscription already one hour expired is accepted — the named Scorpio 5.8.1 violation. (b) `is_active` uses the same comparison, so it keeps notifying for ~14 hours past its true expiry; the same trick on a `csourceRegistration` keeps an expired registration in `matching_regs`, so the broker keeps forwarding requests and tenant data to a lapsed context source.

**Why the verifier believed it — and why low.** Every site read. Downgraded: the blast radius is confined to resources the actor already owns within their own tenant, with no cross-tenant or cross-principal consequence.

**Fix.** Parse `expiresAt` once with `chrono::DateTime::parse_from_rfc3339`, normalise to UTC on store, and compare `DateTime<Utc>` values at the yield point, the create-time past check, the registration path and the presented `status`. Never compare timestamps as strings.

---

### L20 — Attacker-chosen `observedAt` permanently poisons the plain-mode maintenance transaction

**Anchor:** `crates/antares-sql/src/maintenance.rs:58-127`, failure swallowed at `:73-80`, retention loop `:85-122`, `last_run` update `:124`; client-supplied value at `crates/antares-sql/src/store/pg_temporal.rs:77-79`; DEFAULT partition from migration `0003`; scheduler at `crates/antares-broker/src/main.rs:225-234`
**Dimension:** concurrency-state
**Violates:** claude.md §8.2 (broker-scheduled partition/retention job), §2.1.

**What it is.** The whole pass runs in **one** transaction and treats a failed partition DDL as recoverable (`tracing::debug!` then continue). In PostgreSQL any statement error puts the transaction in the aborted state (25P02), so every subsequent statement — the remaining DDL, the retention `DROP TABLE` loop and the `last_run` update — fails, and `commit()` errors. There is no SAVEPOINT.

**Repro.** `POST /ngsi-ld/v1/temporal/entities` with an attribute instance whose `observedAt` is ~6 weeks in the future; the row lands in `attr_instances_default`. Once wall-clock advances so that week enters the job's `[now-1w, now+4w)` window, `CREATE TABLE … PARTITION OF … FOR VALUES FROM/TO` fails ("updated partition constraint for default partition would be violated"), poisoning the transaction on every 15-minute run thereafter. Retention then never executes again — temporal history is retained indefinitely, visible only as a `warn!` line.

**Why the verifier believed it — and why low.** Confirmed. The consequence is a stalled maintenance/retention job in plain-postgres mode with retention configured: a data-retention control failure, no exposure or availability loss.

**Fix.** Wrap each partition-creation attempt in its own SAVEPOINT (or its own transaction/connection) so one failure cannot abort the pass, and run the retention step and the `last_run` update independently of partition creation. Reject or clamp `observedAt` values outside a configured window at ingest.

---

### L21 — Create of subscriptions/registrations on Postgres is a check-then-act race

**Anchor:** `crates/antares-sql/src/store/any.rs:103-119`; `crates/antares-sql/src/store/pg_doc.rs:413-427` (`get`) and `:334-403` (`upsert`, `ON CONFLICT … DO UPDATE`); the atomic entity path for contrast at `crates/antares-sql/src/store/pg_entity.rs:127-136`
**Dimension:** concurrency-state
**Violates:** CIM 009 5.8.1 / 5.9.2 (`AlreadyExists` 409); claude.md §9.3.

**What it is.** The existence check and the write are two separate transactions, and the write is an unconditional overwrite. The entity path deliberately uses `INSERT … ON CONFLICT DO NOTHING`; the doc kinds do not.

**Repro.** Two concurrent `POST /ngsi-ld/v1/csourceRegistrations` (or `/subscriptions`) carrying the same client-chosen `"id"`. Both `get` return `None`, both `upsert` run, both handlers answer **201 Created** instead of one 409, and the first document — including its `endpoint`, `mode` and `operations`, which decide where forwarded requests and tenant data go, plus the rebuilt `csource_index` rows — is silently replaced.

**Fix.** Make it one statement: `INSERT … ON CONFLICT (tenant_id, id) DO NOTHING RETURNING id`, treating zero rows as `AlreadyExists` — exactly as `PgEntityStore::create` already does.

---

### L22 — The transactional outbox is implemented but wired to nothing

**Anchor:** `crates/antares-sql/src/store/outbox.rs:7-9` (the module's own note); production emit path at `crates/antares-api/src/notify.rs:29-41` and `crates/antares-sql/src/store/any.rs:116-118,210,255-257,323-327`; dead bus at `crates/antares-broker/src/main.rs:160`
**Dimension:** concurrency-state
**Violates:** claude.md §10 (transactional outbox), §9.4 (write lifecycle).

**What it is.** `grep -rn outbox crates --include=*.rs` outside `outbox.rs` returns only `crates/antares-sql/tests/pg_outbox_maps.rs` and a table-name string in `tests/pg.rs` — no production enqueue/peek/ack. The live path emits in-process **after** commit into the unbounded mpsc, with the send result discarded (`let _ = tx.send(...)`). The `outbox` table and its RLS policy exist in migration 0001 and are never written.

**Repro (as an observation, not an attack).** Kill the broker between an entity COMMIT and the in-memory `emit` — including the OOM kill reachable via **H8** — and those notifications are lost permanently with no replay path. A subscriber can never tell that a change it was subscribed to was silently dropped. The §10 guarantee ("a broker crash between commit and publish can never lose an event") is not in force.

**Fix.** Wire `outbox::enqueue` into the entity write transactions (it already takes `&mut PgConnection` for exactly that) and run the drain loop that peeks/publishes/acks — or explicitly downgrade the documented guarantee until the drain lands, so operators are not relying on durability the code does not provide.

---

### L23 — `rumqttc 0.24.0` pins an out-of-support rustls 0.22.4 stack into the shipped binary

**Anchor:** `Cargo.toml:51`; `Cargo.lock:2000` (rustls 0.22.4) alongside `:2014` (0.23.43), `:2749` (tokio-rustls 0.25.0) alongside `:2760` (0.26.4); default feature at `crates/antares-api/Cargo.toml:38`; built at `Dockerfile:17`
**Dimension:** dependency-cves
**Violates:** claude.md §16.5 (rustls only, advisories gated), §6.1 ("production-maturity" stack picks).

**What it is.** `cargo tree -i rustls@0.22.4 -e normal` resolves rustls 0.22.4 → tokio-rustls 0.25.0 → rumqttc 0.24.0 → antares-notifier → antares-api → antares-broker, with the matching EOL siblings (rustls-webpki 0.102.8, rustls-native-certs 0.7.3). Two independent TLS implementations are linked. rustls' published policy backports security fixes only for two years after the semver-compatible release; 0.22.0 shipped 2023-12-02, so that window closed 2025-12-02 — about eight months ago. `rumqttc 0.25.1` (2025-11-21) already moved to tokio-rustls ^0.26 / rustls 0.23.

**Repro.** `POST /ngsi-ld/v1/subscriptions` with `notification.endpoint.uri = "mqtts://attacker.host:8883/t"` drives the broker's client handshake through rustls 0.22.4 (`notify.rs:1158` → `MqttSink::deliver` → `mqtt.rs:302/332` `Transport::Tls`), against a peer that controls every byte of the server side.

**Why the verifier believed it — and why low.** Lockfile facts confirmed. Downgraded from medium: there is **no unpatched advisory against rustls 0.22.4 today** (RUSTSEC-2024-0336 is fixed in 0.22.4), so this is exposure to *future* unbackported fixes on an attacker-facing handshake path — real maintenance debt, not a present vulnerability.

**Fix.** `rumqttc = { version = "0.25.1", default-features = false, features = ["use-rustls"] }` — collapses the tree to a single supported TLS stack and drops rustls-native-certs 0.7.3 and rustls-webpki 0.102.8.

---

### L24 — Every `mqtts://` notification re-loads and re-parses the entire system trust store on a tokio worker

**Anchor:** `crates/antares-notifier/src/mqtt.rs:301-303` (v5), `:331-333` (v3); dependency body at `rumqttc-0.24.0/src/lib.rs:365-377`; pooling behaviour at `mqtt.rs:213-226`
**Dimension:** dependency-cves
**Violates:** claude.md §2.1 rule 1, §4.1 U1 ("one client, timeouts and bounded pool set at construction").

**What it is.** `TlsConfiguration::default()` builds a fresh `RootCertStore` from `load_native_certs()` — whose own docs warn "This function can be expensive: on some platforms it involves loading and parsing a ~300KB disk file" — with `.expect()`/`.unwrap()`, once per connection, inside `async fn connect` on a tokio worker rather than `spawn_blocking`. Failed connects are never pooled, so a non-responding endpoint repeats the full load on every notification; the 32-entry pool cap re-triggers it for live endpoints too. Note both `.expect(...)` and `.unwrap()` live in dependency code, invisible to the workspace `unwrap_used`/`expect_used` clippy gate.

**Repro.** K subscriptions with distinct `mqtts://` hosts (distinct URIs each get their own breaker budget) that accept TCP but never CONNACK, then one matching entity write — each delivery re-reads and re-parses the platform trust store before the 5 s connect timeout even starts. Secondary: a root the container trust store contains but rustls 0.22 rejects turns `.unwrap()` into a panic on that worker task.

**Why the verifier believed it — and why low.** Both call sites and the vendored dependency body were read. Downgraded: the cost is a ~200 KB read plus a few ms of X.509 parsing per connect — a blocking-IO smell, not a DoS primitive — the panic arm depends on an operator-misconfigured trust store, and the whole path is behind the optional `mqtt` feature.

**Fix.** Build the rustls `ClientConfig` once — `static TLS: OnceLock<Arc<rustls::ClientConfig>>` from `webpki_roots` (or `load_native_certs()` with per-cert `add()` errors logged and skipped, not unwrapped) — and pass `TlsConfiguration::Rustls(Arc::clone(...))` at both sites.

---

### L25 — Advisory scanning is push/PR-triggered only; the embedded SBOM is never audited

**Anchor:** `.github/workflows/ci.yml:2-5` (no `schedule:`), `:70-73` (the cargo-deny step); the SBOM produced at `Dockerfile:11-17` and never consumed; contrast `.github/workflows/fuzz.yml:7-9` which *does* have a cron
**Dimension:** dependency-cves
**Violates:** claude.md §16.5 ("cargo-deny advisories + license gate in CI …; SBOM (cargo-auditable) in release builds").

**What it is.** The advisories gate runs only when someone pushes, so a RUSTSEC advisory published against an already-pinned crate stays undetected for as long as the repository is quiet — and the already-published container image is never re-checked at all. The Dockerfile goes to the trouble of embedding a dependency manifest specifically so that "`cargo audit bin /antares` can then verify a shipped broker against advisories", and no workflow ever runs it.

**Fix.** Add `schedule: - cron: "0 5 * * *"` to `ci.yml` (or split a small `audit` job with that trigger running `cargo deny check advisories`), and add a step running `cargo audit bin` against the built `target/release/antares`.

---

## 4. Verified clean — §16 controls checked and found actually implemented

These are negative results, recorded as evidence rather than assumed. They matter as much as the defects: they mark the parts of the contract that hold under attack, and they are what makes several findings above "low" rather than "high".

### 4.1 Tenant isolation (§16.1, §3.1.5, ADR-0006)

- **`&TenantId` threading is real.** It is the first parameter of every method of `PgEntityStore`, `PgTemporalStore`, `PgDocStore`, `EntityMapStore` and `outbox::enqueue`, and of every `AnyStore` method **except** the four `context_*` methods (`crates/antares-sql/src/store/any.rs:377-389` — reported as **L1**).
- **Every statement touching a tenant-scoped table carries `tenant_id = $1`:** `pg_entity.rs:119/144/158/179/241/263/278/329/367/387`, `pg_temporal.rs:92/101/133/161/175/183/198/219/234`, `pg_doc.rs:334/347/375/382/413/445/448/455/502/521/545`, `entity_map.rs:47/53/83/90/123`.
- **`SET LOCAL` is correct and parameterised.** `SET_TENANT_SQL = "SELECT set_config('antares.tenant', $1, true)"` (`crates/antares-sql/src/lib.rs:98`) — `is_local = true`, bound not interpolated, issued inside `pool.begin()` in every store method (`pg.rs:29-33`). No session-level `SET` exists anywhere, so a recycled pooled connection carries no tenant residue.
- **RLS + FORCE with `USING`/`WITH CHECK` on `tenant_id`** is applied to `entities`, `subscriptions`, `csource_subscriptions`, `csource_registrations`, `csource_index`, `entity_maps`, `outbox` (`0001:131-143`), `temporal_entities` (`0002:22-25`) and `attr_instances` in plain mode (`0003:69-73`). The single policy per table is permissive but is the *only* policy, so it is deny-by-default when `antares.tenant` is unset (`current_setting(...,true)` → NULL). (It is nevertheless inert in shipped configs — **L2**.)
- **ADR-0006 traced.** `grep -rn attr_instances` finds exactly five statements in `src` (`pg_temporal.rs:92,101,183`; `maintenance.rs:50,70/116`); all three data statements are tenant-predicated, and the table currently has **no read path at all**, so the timescale RLS gap is latent, not exploitable today.
- **No existence oracle.** A cross-tenant id and a nonexistent id take the identical store path (`any.rs:234-246` → `None` → `ResourceNotFound` 404). `NgsiError::NonexistentTenant` is defined (`error.rs:24`) but never raised, so tenant existence is not probeable.
- **Tenant comes only from the header, through a validated newtype.** `negotiate.rs:98-108` is the sole source; `TenantId::new` (`crates/antares-model/src/id.rs:19-32`) enforces `[A-Za-z0-9_-]{1,64}` on bytes — NATS-subject-safe and path-safe, rejecting `.`, `*`, `>`, `/`, space, newline and non-ASCII, with tests at `id.rs:123-127`. All ~30 handler entry points call `tenant_from(&headers)`; no code path reads a tenant from a body, query parameter or forwarded payload. The two `tenant_from(...).unwrap_or_default()` sites (`lib.rs:310/321`) are error-response header echoes that touch no data. `pg_doc.rs:256` reads a body member named `"tenant"` — that is the registration's *peer* tenant, stored not applied (**L10**).
- **A self-directed registration cannot forge `NGSILD-Tenant`:** the federation forward sets a fixed header set (`federation.rs:260-279`) and registration-controlled `contextSourceInfo` is not injected.
- **No inbound EntityMap surface:** `grep -rni 'NGSILD-EntityMap' crates/` returns zero hits, and no `/entityMaps` route is registered.

*Suggested hardening (not a defect):* the RLS-denial test at `crates/antares-sql/tests/pg.rs:79` covers only `entities` — extend it to `temporal_entities`, `subscriptions`, `csource_registrations` and `entity_maps`, and add a CI grep that fails when a statement naming `attr_instances` lacks `tenant_id = $`, since ADR-0006 makes that predicate the only isolation on that table in timescale mode.

### 4.2 SQL injection (§16.2) — impossible by construction, verified

Attacker-controlled text (q= values and paths, expanded attribute IRIs from a client `@context`, entity/subscription/registration ids, type selectors, attrs lists, the tenant header, pagination params, batch bodies) was traced to a SQL string position in **every** store method. No path exists: each terminates in a bind or an enum-derived constant.

- **The live `q=` compiler binds the jsonpath.** `format!("jsonb_path_exists({col}, ${}::jsonpath)", first + binds.len())` (`crates/antares-sql/src/compile/q.rs:129-133`) emits only `$n`; `col` is the caller-supplied literal `"entity"` (`pg_entity.rs:204`). Expanded attribute IRIs go through `quoted()`/`jsonpath_string()` (`q.rs:201-221`), which escapes `"`, `\` and control chars (test at `q.rs:283-286`); string operands use the same escaper (`q.rs:182`). Unsupported shapes return `None` and fall back to in-memory evaluation rather than guessing (`q.rs:148,176,160-164`). Placeholder numbering uses the original offset plus `binds.len()` only (`q.rs:93-106`), so no two predicates alias one `$n`.
- **`pg_entity.rs::query` (177-233)** assembles `wheres` exclusively from its own literals plus `$n` (`id = ANY($n)`, `types @> $n`, `entity ?| $n`); ORDER BY is the constant `ORDER BY id`. There is no user-driven ORDER BY / collation / LIMIT / OFFSET string anywhere.
- **Batch paths are single statements over one bound jsonb array** (`jsonb_array_elements($2::jsonb)`): `pg_entity.rs:328-343`, `:366-369`, `pg_temporal.rs:100-116`, `pg_doc.rs:381-393`, `entity_map.rs:52-57`. No UNNEST built by concatenation. `entity_map.rs:92` binds OFFSET/LIMIT as `$3`/`$4`.
- **Dynamic identifiers are compiler constants:** `DocKind::table()`/`doc_column()` return `&'static str` from a closed match (`pg_doc.rs:26-38`); every `AssertSqlSafe(format!(...))` interpolates only those. Migration `0001_init.sql:137-142` uses `format('%I')`.
- **Other interpreters:** PostGIS SQL *is* built (`pg_entity.rs:118-136` `ST_SetSRID(ST_GeomFromGeoJSON($9), 4326)`) but the geometry travels as bind `$9`; NATS subject construction does not exist in-tree (`antares-bus` ships only the in-process `LocalBus`); there are no shell-outs.
- One cosmetic nit, **not** attacker-reachable: `maintenance.rs:69-72` and `:116` build DDL with unquoted identifiers, but `suffix`/`lo`/`hi` come from Postgres-computed `to_char(date_trunc(...))` and `name` from `pg_class.relname` of `attr_instances`' own partitions.

### 4.3 SSRF / egress (§16.4) and TLS (§16.5)

- **Exactly one outbound client constructor.** `grep -rn "danger_accept_invalid|Client::builder|client_builder("` finds `crates/antares-jsonld/src/loader.rs:161-165` and its three uses: `loader.rs:254` (connect 5 s / total 10 s), `state.rs:65` `http` (2 s / 5 s), `state.rs:73` `fed_http` (2 s / 8 s). Timeouts are set at construction, so a call site cannot forget them.
- **TLS verification is never disableable.** No `danger_accept_invalid_certs` / `danger_accept_invalid_hostnames` / `use_preconfigured_tls` anywhere; `Cargo.toml:39` pins reqwest `default-features = false` + `rustls-tls`; `Cargo.lock` contains no `openssl-sys` (only `openssl-probe`). `rumqttc`'s `TlsConfiguration::default()` builds a *verifying* rustls config from native roots. There is no per-registration `insecureSkipVerify`.
- **Scheme allowlist is enforced:** `Egress::check_url` (`egress.rs:49-52`) allows exactly `http/https/mqtt/mqtts` and rejects `file://` (test at `egress.rs:115`).
- **`ANTARES_EGRESS_ALLOW_PRIVATE` defaults to false** (`loader.rs:70-75`); the only place it is set true is `compose-files/docker-compose-etsi.yml:79-127`, the sanctioned ETSI/IOP mock stack, and no production manifest ships.
- **No egress class bypasses the policy entirely.** Notification delivery (`notify.rs:1143`), federation forwarding (`federation.rs:252`) and `@context` fetching (`loader.rs:477`) all pass through it — the defects above (**H2**, **H3**, **M5**) are gaps *inside* the policy, not missing call sites.
- **Subscription creation rejects unregistered endpoint schemes with 422 before storing** (`subscriptions.rs:181-191`) and validates the mqtt endpoint shape / `notifierInfo` at creation (`subscriptions.rs:196-208`).
- **Federation forward does not pass credentials through.** `forward()` constructs its header set from scratch — Accept, Link, Via, NGSILD-Tenant, Content-Type (`federation.rs:260-279`); a workspace grep for `Authorization` finds no propagation, and registration `contextSourceInfo` is never injected as outbound headers, so no CRLF header-injection path exists today.
- **Loop detection is checked at every forwarding site:** `entities.rs:297,1073,1232,1328,1536`; `attrs.rs:104,374,525,680,790`; `batch.rs:252,580`; read paths gate at `entities.rs:773-774` and `batch.rs:706-707`. A registration pointing at the broker itself terminates after one hop.
- **`idPattern` values are compiled with the `regex` crate** (`csource.rs:102,564,573`), which is linear-time — no catastrophic-backtracking ReDoS (the defect is repeated compilation, **M3**).

### 4.4 JSON-LD (§6.3, §14.4) and memory safety (§9.5)

- **No `unsafe`.** `grep -rn unsafe --include=*.rs crates/` returns only a test *name* (`antares-model/src/id.rs:123`); `Cargo.toml:70` sets `unsafe_code = "forbid"` and all 11 crates inherit `[lints] workspace = true`. The `sonic` feature that §9.5 reserves an unsafe exception for does not use one (`antares-api/src/batch.rs:24-27` is a two-line `sonic_rs::from_slice`).
- **Compaction cannot mutate its input** — the Scorpio J5 defect is unrepresentable: `pub fn compact_entity(internal: &Value, ctx: &Context) -> Value` (`crates/antares-jsonld/src/compact.rs:30`) and every helper (`:58`, `:85`, `:121`) take `&Value` and build a fresh `Map`. The borrow checker enforces it for free.
- **The core-context fast path uses exact URL equality, not a fuzzy resemblance check.** `pinned()` is `PINNED.iter().find(|(u, _)| *u == url)` (`loader.rs:561-567`) and `fetch()` short-circuits on it before any network access (`:457-459`), so a hostile server at a lookalike URL cannot supply core terms. `resolve_counted` always merges the pinned core **last** (`:403-405`), so a user context — including its `@vocab` — can never override a core term.
- **Cyclic/self-referential `@context` chains terminate:** `merge_entry` returns `LdContextNotAvailable` at `depth > 8` (`loader.rs:421-426`), and request bodies are depth-capped at 64 by a pre-parse byte scan (`bounds.rs:104`). (This is unrelated to the `q=` parser overflow, **C1**.)

### 4.5 Input bounds (§16.3) and supply chain (§16.5)

- **The inbound body cap is a true streaming cap, not a Content-Length check:** `crates/antares-api/src/bounds.rs:92` uses `axum::body::to_bytes(body, MAX_BODY_BYTES)`, with JSON depth scanned before parse at `:104` and the URI capped at 8 KiB at `:18`.
- **No suppressed advisories:** `deny.toml` has no `ignore = [...]` list, `yanked = "deny"` is explicit, and cargo-deny v2 denies vulnerability advisories by default — so the advisories gate itself is real (its *scope* is the defect, **L14**/**L25**).
- **No baked build secrets, non-root runtime:** the Dockerfile is multi-stage with a `gcr.io/distroless/cc-debian12:nonroot` final layer (UID 65532, no shell) copying only the binary; `.dockerignore` excludes `.git/` and `*.log`.
- **No `pull_request_target`** in any workflow (`ci.yml`, `etsi.yml`, `fuzz.yml`); PR code runs under `pull_request` with read-only fork tokens and no secrets referenced; the only secret used is the built-in `GITHUB_TOKEN` in the push-only publish job, scoped `packages: write`.
- **The only hardcoded credentials** are `POSTGRES_PASSWORD: antares` in `compose-files/docker-compose.yml:10`, `docker-compose-etsi.yml:36` and `ci.yml:21/33` — all dev/CI stacks, and `README.md` does not present either compose as a production recipe.
- **Dependency advisory sweep (as of the 2026-08-04 database).** All 331 pinned versions were extracted from `Cargo.lock`, the actually-compiled graph resolved with `cargo tree`, and each crate checked against rustsec.org and osv.dev. No applicable advisory for: tokio 1.53.1, hyper 1.11.0 (all 7 hyper advisories are 0.x-only), h2 0.4.15, sqlx 0.9.0, ring 0.17.14, tracing-subscriber 0.3.23, idna 1.1.0, rustls 0.23.43, tower-http 0.6.11 (only the `normalize-path` feature is enabled), rustls-webpki 0.102.8 (the CRL panic requires explicit `RevocationOptions`, which no call site passes), chrono 0.4.45, regex 1.13.1, crossbeam-channel 0.5.16, slab 0.4.12. No advisory page exists at all for serde_json, axum, reqwest, moka, redb, geo, geojson, uuid, url, rumqttc, sonic-rs. Lockfile-only and never compiled (`cargo tree -i` returns "nothing to print"): sqlx-mysql/sqlite, libsqlite3-sys, quinn, rkyv, generic-array 0.12/0.13, heapless — all unselected optional features. *This clean bill is valid only as of the advisory database on 2026-08-04; see **L25**.*

*Suggested hardening (not a defect):* the ETSI compose (`docker-compose-etsi.yml:29-31`) sets no `read_only: true`, `cap_drop: [ALL]` or `no-new-privileges` on the broker services, so the §16.5 "read-only rootfs" posture is asserted in a Dockerfile comment but never exercised anywhere in the repo.

---

## 5. Prioritized remediation checklist

Ordered by severity × cheapness of fix. Paste into `tasks.md`.

### Tier 0 — process-killing, fix first (hours)

- [ ] **S1** — Add a depth counter to `antares-ql::Parser`; reject `> 32` nested parens with `TooComplexQuery` **before** recursing; cap raw `q` string length at parse entry. `crates/antares-ql/src/lib.rs:44,101-108` (**C1**)
- [ ] **S2** — Percent-decode `q` before create-time validation in `subscriptions.rs:121` (or stop decoding in `notify.rs:169`) so the validated string is the evaluated string — this is what turns C1 into a persistent crash loop. (**C1**)
- [ ] **S3** — Cap `information[]`, `entities[]`, `propertyNames[]`, `relationshipNames[]` cardinality in `normalize_registration` (128 each) **and** add a hard row ceiling in `index_rows`. `crates/antares-api/src/csource.rs:52-160`, `crates/antares-sql/src/store/pg_doc.rs:279-295` (**C2**)

### Tier 1 — high severity, small diffs (a day each)

- [ ] **S4** — Unmap IPv4-mapped IPv6 in `ip_is_private` (`to_ipv4_mapped()` first), and add CGNAT / `0.0.0.0/8` / `192.0.0.0/24` / `198.18.0.0/15` / `240.0.0.0/4` / NAT64 to the deny list. Add `[::ffff:…]` cases to the `egress.rs` test. `crates/antares-jsonld/src/loader.rs:77-93` (**H2**)
- [ ] **S5** — Replace `redirect::Policy::limited` with `Policy::custom` that policy-checks every hop's host including IP literals. `crates/antares-jsonld/src/loader.rs:163` (**H3**)
- [ ] **S6** — Move the `@context` fetch-count cap **inside** `merge_entry`; reject an over-long `@context` array before fetching anything; reference `bounds::MAX_CONTEXT_FETCHES` instead of a second hardcoded 32. `crates/antares-jsonld/src/loader.rs:388-396,421-441` (**H6**)
- [ ] **S7** — Stream the `@context` response with a running byte total instead of `resp.bytes()`. `crates/antares-jsonld/src/loader.rs:497-511` (**H5**)
- [ ] **S8** — Cap the federation response body (mirror `MAX_CONTEXT_BYTES`) via `content_length` + bounded `chunk()` loop; report over-cap as a 502 part. Also push an explicit `limit` onto forwarded reads. `crates/antares-api/src/federation.rs:290`, `:519-533` (**H4**)
- [ ] **S9** — Make the change channel bounded with an explicit overflow policy, and dispatch deliveries under a semaphore instead of inline in the single consumer; export queue depth on `/q/health`. `crates/antares-api/src/notify.rs:29-41,574,642` (**H8**)
- [ ] **S10** — Bound the `Cached` write-through (row cap + LRU on `last_usage`, body cap well below 5 MiB) and page the boot preload. `crates/antares-broker/src/main.rs:181-210`, `crates/antares-sql/src/store/pg_doc.rs:562-598` (**H7**)
- [~] **S11** — WITHDRAWN (§0 amendment; authn is out of core, not NGSI-LD). Was: implement the `ANTARES_AUTHN = none|oidc-bearer|mtls` tower layer, add the key to `KNOWN_KEYS`, validate `NGSILD-Tenant` against the principal's claim, and log a loud warning under `none`. `crates/antares-broker/src/main.rs:11-24,236` (**H1**) *(larger than the rest of this tier; start with the warning + config key.)*

### Tier 2 — medium severity, cheap (half a day each)

- [ ] **S12** — Tenant-key the MQTT pool: hash `(username, password, secure, host, port, v5)` prefixed with the tenant; pass `&TenantId` into `MqttSink::deliver`. `crates/antares-notifier/src/mqtt.rs:202-209` (**M1**)
- [ ] **S13** — Tenant-key the egress breaker map (`Egress::key` takes `&TenantId`) **and** back it with a bounded, TTL'd `moka` cache. `crates/antares-api/src/egress.rs:26-29,58-69` (**M2** + **L15**)
- [ ] **S14** — Hoist `~=` regex compilation out of `compare()` into the parsed AST / compiled filter; add `RegexBuilder::size_limit`/`dfa_size_limit` and a pattern-length cap (403). `crates/antares-api/src/qeval.rs:117`, `notify.rs:160`, `csource.rs:564,573` (**M3**)
- [ ] **S15** — Clamp `aggrPeriodDuration` in `parse_iso_duration`; replace `Duration::seconds` / `checked_add_months().expect()` with fallible variants mapped to `BadRequestData`. `crates/antares-api/src/temporal.rs:617,631,819,826` (**M7**)
- [ ] **S16** — Clamp the `@context` TTL and use `Instant::now().checked_add(d)`. `crates/antares-jsonld/src/loader.rs:178-186,524-531` (**M12**)
- [ ] **S17** — Cap `geoQ` coordinate count (~10 000 positions) in `GeoQuery::from_params` and at subscription creation → 403. `crates/antares-api/src/geo.rs:165-173`, `subscriptions.rs:143` (**M10**)
- [ ] **S18** — Clamp `lastN` to a server maximum and truncate instances inside `window()`, emitting 206/`Content-Range` for the window actually returned. `crates/antares-api/src/temporal.rs:270-311,718-724` (**M9**)
- [ ] **S19** — Back the `@context` `usage` map with a size-capped `moka` cache and cap accepted `@context` URL length. `crates/antares-jsonld/src/loader.rs:216-221,303-323` (**M8**)
- [ ] **S20** — Give the `fetched`/`merged`/`merged_urls` caches a byte `weigher` + `max_capacity`; lower `MAX_CONTEXT_BYTES`. `crates/antares-jsonld/src/loader.rs:198,260-264` (**M13**)
- [ ] **S21** — Derive the public base URL from `ANTARES_PUBLIC_BASE_URL` instead of the `Host` header (allowlist + preserve scheme if `Host` must be used). `crates/antares-api/src/contexts.rs:20-26` (**M11**)
- [ ] **S22** — Re-read the subscription (exists + `is_active`) immediately before the send, matching the CSource path. `crates/antares-api/src/notify.rs:1119-1166` vs `:864-876` (**M19**)
- [ ] **S23** — Add the `fed_retrieve` identity check to `fed_query`; clamp or ignore peer-supplied `observedAt`/`modifiedAt` for recency resolution. `crates/antares-api/src/federation.rs:549-555,330-372` (**M16**)
- [ ] **S24** — Wrap `check_host`'s `lookup_host` in a 1–2 s `tokio::time::timeout` and memoise per `host:port` for a short TTL. `crates/antares-jsonld/src/loader.rs:110-120`, `crates/antares-api/src/egress.rs:47-56` (**M18**)
- [ ] **S25** — Add `header_read_timeout`, a global `TimeoutLayer`, and an accept-loop `Semaphore` for max-connections. `crates/antares-broker/src/main.rs:245-268` (**M20**)

### Tier 3 — medium severity, larger work (multi-day)

- [ ] **S26** — Bound the federation fan-out: per-request source cap, `Semaphore` (~16) with `buffer_unordered`, and an aggregate `tokio::time::timeout` reporting unfinished sources as 207 failures. `crates/antares-api/src/federation.rs:392,497,770` (**M4**)
- [ ] **S27** — Acquire the resolve permit per fetch rather than per crawl; add an aggregate deadline to `resolve()`; floor the honoured TTL. `crates/antares-jsonld/src/loader.rs:172-177,380-388` (**M14**)
- [ ] **S28** — Push `LIMIT`/`OFFSET` into `PgEntityStore::query` and stream with `fetch()`; bound the memory/file snapshot; abort `filter_entities_fed` at `offset+limit+1`. `crates/antares-sql/src/store/pg_entity.rs:211`, `crates/antares-api/src/entities.rs:900-1009` (**M6**)
- [ ] **S29** — Read federation candidates from `csource_index` with an indexed query (+ candidate cap) instead of listing every registration document. `crates/antares-api/src/federation.rs:160-165`, `crates/antares-sql/src/store/pg_doc.rs:519-535` (**M17**)
- [ ] **S30** — Narrow batch forwarding per registration (id/type match per item, narrowed delete id array) and make `reduce_to_scope` reject out-of-scope entities. `crates/antares-api/src/batch.rs:217-249,487-491,602`, `federation.rs:665-687` (**M15**)
- [ ] **S31** — Pin the DNS answer for MQTT: resolve once in the egress check and hand `rumqttc` the literal IP (SNI pinned to the name for `mqtts`), or re-verify the peer address after connect. `crates/antares-notifier/src/mqtt.rs:296,326` (**M5**)

### Tier 4 — low severity, near-free (minutes to an hour each)

- [ ] **S32** — Fix the CI SQLi grep: match `AssertSqlSafe` + `format!` and scope it *to* `antares-sql` with a reviewed allowlist. `.github/workflows/ci.yml:75-78` (**L3**)
- [ ] **S33** — `cargo-deny`: add `[bans]` + `[sources]`, run `check advisories bans licenses sources`, pin the action to a SHA. `deny.toml`, `.github/workflows/ci.yml:70-73` (**L14**)
- [ ] **S34** — Add a `schedule:` cron to `ci.yml` and a `cargo audit bin` step against the built binary. `.github/workflows/ci.yml:2-5` (**L25**)
- [ ] **S35** — Gate GHCR `:latest` on the `ci` workflow result (via `needs:` in one workflow or `workflow_run`). `.github/workflows/etsi.yml:211` (**L11**)
- [ ] **S36** — Redact userinfo before logging endpoint URIs; add a manual `Debug` for `MqttEndpoint`. `crates/antares-api/src/notify.rs:1144`, `federation.rs:253`, `crates/antares-notifier/src/mqtt.rs:20-28` (**L13**)
- [ ] **S37** — Return a fixed `detail` + correlation id for `InternalError`; log the `sqlx::Error` server-side only. `crates/antares-sql/src/store/any.rs:19-21` (**L5**)
- [ ] **S38** — Reject C0 control characters in `EntityId::new`. `crates/antares-model/src/id.rs:70-86` (**L7**)
- [ ] **S39** — Compare `expiresAt` as `DateTime<Utc>` everywhere (create check, `is_active`, registration path, presented `status`). `notify.rs:128-135`, `subscriptions.rs:247-256,392-395`, `csource.rs:157-165`, `federation.rs:165-172` (**L19**)
- [ ] **S40** — Return `CtxEntry::Cached` for stored rows with `kind == "Cached"` so the 5.13.4.4 guard applies. `crates/antares-api/src/contexts.rs:89-92` (**L8**)
- [ ] **S41** — Early-return the `Prefer` middleware when the requested version ≥ `NATIVE`; bound the buffer with `MAX_BODY_BYTES`. `crates/antares-api/src/conformance.rs:242,255-267` (**L6**)
- [ ] **S42** — Invalidate the merged-context LRU selectively (by `merged_urls` membership) and cap Hosted contexts per tenant. `crates/antares-jsonld/src/loader.rs:545-556` (**L4**)
- [ ] **S43** — Make doc-kind create atomic: `INSERT … ON CONFLICT DO NOTHING RETURNING id` → `AlreadyExists`. `crates/antares-sql/src/store/any.rs:103-119` (**L21**)
- [ ] **S44** — Add a hop counter to `outbound_via` (508 above 5) and match Via tokens by exact pseudonym equality, not `ends_with`. `crates/antares-api/src/federation.rs:126-135` (**L9**)
- [ ] **S45** — Send `tenant_at_peer` as `NGSILD-Tenant` on forwards when present. `crates/antares-api/src/federation.rs:272-274` (**L10**)
- [ ] **S46** — Wrap each partition-creation attempt in its own SAVEPOINT; run retention and `last_run` independently; clamp ingest `observedAt`. `crates/antares-sql/src/maintenance.rs:58-127` (**L20**)
- [ ] **S47** — Scope the fatal unknown-key check to `ANTARES_CFG_` (or skip the K8s-injected shapes). `crates/antares-broker/src/main.rs:40-45` (**L16**)
- [ ] **S48** — Bump `rumqttc` to 0.25.1 to collapse the duplicate TLS stack. `Cargo.toml:51` (**L23**)
- [ ] **S49** — Build the MQTT rustls `ClientConfig` once in a `OnceLock`; never `unwrap` per-cert `add()`. `crates/antares-notifier/src/mqtt.rs:301-303,331-333` (**L24**)
- [ ] **S50** — Pin the Dockerfile bases by digest, pin `cargo-auditable --version`, add `--locked` to the release build, pin `rust-toolchain.toml`. `Dockerfile:3,15,17,20`, `rust-toolchain.toml:2` (**L12**)

### Tier 5 — architectural debt (schedule deliberately)

- [ ] **S51** — Split the migrator role from the serving role; assert `NOT (rolsuper OR rolbypassrls)` at startup; ship the split in compose/CI. `crates/antares-sql/src/pg.rs:14-20`, `compose-files/*` (**L2**)
- [ ] **S52** — Add an owner tenant to `Hosted`/`ImplicitlyCreated` context rows; thread `&TenantId` through `context_get/list/delete`; filter listings and 404 non-owned ids on delete/reload. `crates/antares-api/src/contexts.rs:88-114,191-231,330-373`, `crates/antares-sql/src/store/any.rs:377-389` (**L1**)
- [ ] **S53** — Carry `version` + incarnation (`created_at`) through `ChangeHook`/`ChangeEvent`, captured inside the locked section. `crates/antares-sql/src/store.rs:138,370-463` (**L18**)
- [ ] **S54** — Move the temporal mirror into the entity write's critical section, or add a `deleted_at` fence the temporal store honours on create. `crates/antares-api/src/entities.rs:102-157` (**L17**)
- [ ] **S55** — Wire `outbox::enqueue` into the entity write transactions and run the drain loop — or explicitly downgrade the §10 durability claim until it lands. `crates/antares-sql/src/store/outbox.rs:7-9` (**L22**)

### Tier 6 — test/CI hardening surfaced by the clean-bill review

- [ ] **S56** — Extend the RLS-denial test beyond `entities` to `temporal_entities`, `subscriptions`, `csource_registrations`, `entity_maps`. `crates/antares-sql/tests/pg.rs:79`
- [ ] **S57** — Add a CI grep that fails when a statement naming `attr_instances` lacks `tenant_id = $` (ADR-0006 makes that predicate the only isolation in timescale mode).
- [ ] **S58** — Add `read_only: true`, `cap_drop: [ALL]` and `no-new-privileges` to the broker services in `compose-files/docker-compose-etsi.yml:29-31` so the §16.5 read-only-rootfs posture is actually exercised.

---

*End of report.*
