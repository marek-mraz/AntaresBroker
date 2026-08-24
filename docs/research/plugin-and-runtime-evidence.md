# Plugin architecture and runtime design — what the evidence actually supports

Adversarially-verified research pass. Six search angles, 29
sources fetched, 145 candidate claims extracted, top 25 verified by a 3-vote
panel (a claim dies on 2 of 3 refutes): **21 confirmed, 4 refuted**.

Read this before designing the plugin tiers or reaching
for a runtime rewrite. Its purpose is to separate what is *evidenced* from
what is *plausible* — several attractive ideas did not survive.

## 1. What the plugin evidence supports

### The gateway model to copy is APISIX's — confirmed 3-0 on every element

- A **fixed set of named request-lifecycle phases** (`rewrite`, `access`,
  `before_proxy`, `header_filter`, `body_filter`, `log`). Plugins implement
  methods for one or more phases; they never register arbitrary hooks or
  define new phases.
- **Numeric priority within a phase** (higher first), overridable per plugin
  instance via `_meta.priority`.
- **Per-route/per-instance config validated against a declared JSON Schema**
  before acceptance.
- **Hot loading**: add/delete/modify/code-update with no restart, triggered
  by an admin call (`PUT /apisix/admin/plugins/reload`).

Sources: apisix.apache.org plugin terminology, plugin-develop, admin-api.

### Production gateways ship a TIERED extension model, not one mechanism

Confirmed 3-0. APISIX: native in-process Lua + out-of-process runners for
Java/Go/Python (subprocess + unix-socket RPC) + a Proxy-Wasm tier. Envoy:
WASM and `ext_proc` tiers **plus** native dlopen dynamic modules built
against a **pure-C ABI header**, with an official SDK for C++, Go and Rust.

The native tier's stated justification is zero-copy access to headers and
body. Its stated price, in Envoy's own docs: **total loss of the security
boundary** (a native module cannot be sandboxed) and a **one-minor-version
ABI compatibility window**. That is why WASM and ext_proc exist alongside it
— untrusted code goes there, not into the native tier.

### No measured WASM overhead number survived

Confirmed as an absence, not a finding. Tetrate's widely-cited ranking of
Envoy tiers (dynamic modules "high, near-native"; Wasm "moderate — cross-VM
serialization"; ext_proc "lower — cross-process cost") is labelled by its own
author as architectural observation, **not benchmarked measurement**, and no
credible published figure was found for APISIX ext-plugin vs native Lua.

**Consequence for us:** any claim that "WASM plugins cost ~X%" in a design
doc is unsupported. If the WASM tier is ever built, its overhead must be
measured on our own hot path before it is promised to anyone.

### Rust has no stable ABI — and that is the whole story for a dlopen tier

Confirmed 3-0 from primary rustc documentation. Unless types carry an
explicit `#[repr(_)]` and functions an explicit `extern "_"`, layout is
compiler-, version- and optimization-level-dependent. The mismatch is
**undetectable by the linker** and manifests as **silent memory corruption at
runtime** — canonically, host and plugin disagreeing on which 8 bytes of a
`Vec` are the pointer versus the length. A practical dlopen seam therefore
requires converting the boundary interface to the C ABI.

`stabby` does **not** make Rust's ABI stable. It pins the ABI of a chosen
subset and adds **opt-in canary symbols** (`#[stabby::export(canaries)]`)
recording rustc version, optimization level and target triple, so an
incompatible plugin **fails to link instead of corrupting memory**. Standing
costs, confirmed: `#[repr(stabby)]` enums lose pattern matching and hurt
compile times, and Rust 1.78's move of trait-object v-tables into a global
lock-free set causes performance degradation plus a deliberate memory leak
for stabby's `dyn` support (rust-lang/rust#121675).

### The one real end-to-end number: Tremor, and what it actually measured

Medium confidence (2-1; single un-reproduced 2022 prototype, and the original
citation was misattributed — chase the primary source, not the blog).

Tremor converted its internal interfaces to an FFI/`abi_stable`
dynamically-loaded plugin system: **~36% throughput loss initially, reduced
to ~30%**, and the project was declared functional but **not production-ready
because it did not meet its performance objectives**.

The decisive detail: **the cost is dominated by marshalling a hot,
allocation-heavy `Value` across the seam per event — not by dynamic-call
overhead.** Per-call cost measured **3.2 ns (abi_stable) vs 817 ps (native
static)**.

**This is the most actionable finding in the whole pass.** It says the
granularity of the seam, not the existence of the seam, decides the cost. A
storage driver called once or a few times per request pays ~nanoseconds. A
plugin seam crossed once per attribute, per event, or per matched
subscription pays Tremor's 30%. Design accordingly: coarse seams, and never
put a plugin boundary inside a per-item loop.

## 2. What the runtime evidence supports — and what it refuted

### Thread-per-core: real pattern, NOT a verified win for us

Confirmed 3-0 as a *description*: Seastar runs one application thread per
core with explicit message passing instead of shared memory, "avoid[ing]
slow, unscalable lock primitives and cache bounces", and forces an explicit
ownership model — a core that receives a request for state it does not own
must forward it (`smp::submit_to(cpu, lambda)`) rather than lock.

What was **refuted (0-3)**: the general argument against shared-memory /
work-stealing runtimes. Verifiers surfaced the qualifications: withoutboats'
critique frames TPC as a tradeoff, not a win; Enberg's ANCS'19 gains came
from idealized uniform-work benchmarks and message-passing steering "suffers
from high overheads because of thread wake-ups"; the Shadowfax paper
(arXiv 2006.03206) reports 85 Mops/s on 64 threads against Seastar's ~10
Mops/s on 28, arguing Seastar "partitions work at the wrong layer".

**Antares-specific caveat recorded by the verifiers:** Seastar owns its state
in-process; **Antares' state lives in Postgres**, so the sharding benefit is
much weaker while hot-key/skew hotspotting risk remains. Do not adopt
thread-per-core as a performance strategy on this evidence. It remains only
one of the possible answers to the arena/thread-migration problem below — and
the cheaper answer (arena-per-task rather than thread-local) is untested but
unrefuted.

Discard the search artifact claiming `submit_to` latency of "several
milliseconds" (AI-generated blog); real cross-shard hop cost is
sub-microsecond.

### Arenas: confirmed suitable for per-request lifetimes and nothing else

Confirmed 3-0 (merged from three claims). Bump allocation is a capacity check
plus a pointer bump — amortized O(1), no free list or size classes; bulk
teardown is a pointer reset, **O(number of chunks), not O(number of
allocations)**. But individual objects can never be freed and dead memory is
not reclaimed until reset, which **rules arenas out for long-lived caches or
a resident entity representation**.

**Consequence:** the arena idea and the resident-representation idea are
strictly separate work items. Arena = request scope only (Orion-LD/coraine
model). The resident store still needs its own compact representation —
which is what the binary-jsonb resident-representation work is for. Do not conflate.

### simd-json: payload-dependent, not a uniform win

Confirmed 3-0, but medium confidence — the benchmark is archived, with stale
pins and 2015-era hardware. DOM parse: **380 vs 320 MB/s** on float-heavy
`canada.json` (~1.2×) but **720 vs 420 MB/s** on key/string-heavy
`citm_catalog.json` (~1.7×). For **typed struct parsing** of `canada.json`
both hit **580 MB/s — zero SIMD advantage**.

Also refuted (1-2): the specific twitter.json throughput figures, and the
claim that deserializing directly into typed structs is categorically faster.

**Consequence:** a JSON-parser swap must be benchmarked on *our* entity
shapes (expanded JSON-LD is key/IRI-heavy, which is the shape where SIMD
looks best — but that must be shown, not assumed). A parser or render-path
swap lands only with a measured win; keep that gate.

### Nothing was verified about deferral or resident representation

No claim survived on: trait-object vs closed-enum driver seams (in either
direction), binary jsonb vs `serde_json::Value` as the resident
representation, or deferring subscription matching / temporal writes past
response queueing.

Those remain grounded in our **own measurements** (the storage
study: 37.6 KB/entity for `Value` vs 4.6 KB binary jsonb, with 1.7× faster
attribute lookup) and in **read code** (Orion-LD's `rest.cpp:387-432`
post-response hook). That is adequate evidence for our own decisions — it is
simply not externally corroborated, and derived work must not cite
outside authority they do not have.

## 3. What this changes in the plan

1. **Phase P stands, with its cost model now evidenced.** Coarse-grained
   trait seams (storage, temporal) are cheap — 3.2 ns per dynamic call is
   noise against a database round-trip. The Tremor result is not an argument
   against Phase P; it is an argument against fine-grained seams, which
   Phase P does not create.
2. **The hook chain (Layer 2) must stay coarse.** Hooks fire per request or
   per drained batch, never per attribute or per matched subscription. If a
   hook ever needs per-item granularity, it takes the whole batch and
   iterates inside the plugin.
3. **P7 (dynamic loading) stays deferred, and its cost is now written down**:
   C ABI at the boundary, canary symbols so mismatches fail to link, one
   compatibility window to document, no security boundary for native modules.
   WASM is the tier for untrusted code — with overhead to be measured, since
   no published figure survived.
4. **No thread-per-core rewrite on this evidence.** The arena/thread-migration
   problem is real, but arena-per-task is the cheaper hypothesis and TPC's
   advantage is unproven for a Postgres-backed broker.
5. **B12 keeps its measured-win gate**; B1 keeps its own measurement as its
   justification.

## 4. Open, unresearched

Questions 3 (benchmark CI on dedicated hardware) and 4 (documentation
practice) were searched in this pass but never reached verification — the
verify budget was consumed by the architecture and plugin claims. A second
scoped pass covers them; until it lands, treat both as unresearched here.
