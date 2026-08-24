# Documentation practice — what the sources actually say

Research pass sourced from primary documents (project repos,
ETSI PDFs, official docs) rather than blog summaries. Companion documents:
`plugin-and-runtime-evidence.md`, `performance-ci-evidence.md`.

Read this before adding documentation tooling. Two widely-repeated beliefs
turned out to be false, and one ETSI deliverable turned out to be a
differentiator nobody in this ecosystem is using.

## 1. The headline: NGSI-LD has an official ICS, and nobody publishes one

**ETSI GS CIM 029 V2.1.1 (2025-07), "NGSI-LD Implementation Conformance
Statement".** (The web research pass found V1.1.1 (2024-04); we hold the
newer V2.1.1 at `etsi-cim-specs/gs_cim029v020101p.pdf`, and the structure
below is read from THAT text, not from the older edition.) It is a checklist
"for a client-owner and developers of implementations so they know what
parts of the specification will be tested and if any is optional", and
**Annex A is normative and explicitly grants reproduction rights**: users
"may freely reproduce the ICS pro forma in this annex so that it can be used
for its intended purposes and may further publish the completed ICS pro
forma."

**Clause 4 constrains the generator**: a conforming ICS pro forma "shall be
technically equivalent to annex A, and shall preserve the numbering and
ordering of the items in annex A", and a conforming ICS shall describe an
implementation claiming conformance to CIM 009, be completed per the A.1
instructions, and "include the information necessary to uniquely identify
both the supplier and the implementation".

Structure (ISO/IEC 9646-7 conventions; status `m` mandatory / `o` optional /
`n/a` / `c.<n>` conditional / `o.<n>` qualified-optional; support Y/N):

- **A.2** — identification of the IUT and SUT, supplier, client, contact
  person. Free-text, but mandated by clause 4(c).
- **A.4** — global statement of conformance: "Are all mandatory capabilities
  implemented? (Yes/No)". Answering "No" indicates non-conformance, and
  every unsupported mandatory capability must be identified with an
  explanation on attached pages.
- **A.5.1** — feature tables: Architecture (**11** items: `CENTRALIZED`,
  `DISTRIBUTED`, `FEDERATED`, `INCLUSIVECSR`, `AUXILIARYCSR`,
  `EXCLUSIVECSR`, `REDIRECTCSR`, `LOCAL`, `EXTRACS`, `PROCESSCS`,
  `ENTITYMAP_QUERY`); Core NGSI-LD @context (1: `USERCONTEXT`); Data
  Representation (**37**: `NGSILDNULL`, `E_*`, `P_*`, `R_*`, `GEOJSON_REP`,
  `LANG`, `AGGRTEMPORAL`, `VOCAB`, `LISTP`, `LISTR`, `LINKED_RETRIEVAL`,
  `JSONP`, `ENTITYMAP_REP`); Data Representation Restrictions (9:
  `TEXTENCODING`, `NAMES`, `JSONNATIVE`, `GEOJSONGEOM`, `DATETIME`, `DATE`,
  `TIME`, `URI`, `CONTENT`); Other CIM transversal features (6:
  `MULTITYPING`, `TYPESELECTLANG`, `SCOPES`, `SCOPESELECTLANG`,
  `LANGFILTER`, `PROJECTIONLANG`); API Operation Definition; API HTTP
  Binding.
- **A.5.2** — **35** tables, one per resource (Entity List, Entity by id, …,
  Snapshots, Clone Snapshot), each row a path + method with its CIM 009
  clause reference and a mnemonic.
- **A.6 and A.7** — two "Mnemonics for PICS" cross-reference clauses, whose
  stated purpose is "to avoid an update of all TP tables when the PICS
  document is changed". An explicit anti-drift indirection layer, and the
  thing that makes a generated ICS cheap to maintain.

**What the competition publishes: nothing.** Orion-LD's
`implementationState.md` is prose Done/Missing sections referencing spec
v1.6 (2022), stale, and its companion `progress.md` was abandoned in 2019.
Scorpio's conformance evidence is a single green test-suite Actions badge,
pass/fail, not a clause matrix. Neither publishes a completed CIM 029.

**Caveat, and it is real.** CIM 029 V2.1.1's only normative reference is CIM
009 **V1.6.1**, while its own tables already cover later features
(`ENTITYMAP_REP`, `ENTITYMAP_QUERY`, `LINKED_RETRIEVAL`, `JSONP`, snapshot
resources, `deletedAt`), and CIM 009 is now V1.9.1. So the ICS trails the
API spec it points at, and its feature list is ahead of its own normative
reference. It is a good scaffold, not a current-version checklist — say so
in anything we publish from it.

**IXIT does not exist for NGSI-LD.** Searching the text of all 14 published
CIM PDFs for "IXIT" returns zero hits. Conformance here is ICS-only: no
grey-box proforma, no Test Lab role, no INCONCLUSIVE verdict scheme. (For
reference, ICS is defined in ETSI EG 201 058 as "a statement made by the
supplier of an implementation … stating which capabilities have been
implemented", and IXIT in ETSI TS 103 701 for other series.)

**The gap we would have to close:** the ETSI Robot suite tags tests by CIM
009 **clause** (`5_6_7`, `6_5_3_4`) and by `since_vX.Y.Z`, across 63 distinct
tags in 131 `.robot` files. It does **not** tag by ICS mnemonic. So a
generated ICS needs a mnemonic-to-clause mapping, which is precisely what
A.6 exists to make cheap.

## 2. Toolchain: mdBook is the right pick for a Rust project, for one reason

We already use mdBook, and the evidence supports keeping it. The decisive
differentiator is not styling:

- **`mdbook test` drives rustdoc over the book**, so Rust code in prose is
  compiled and run.
- **`{{#include}}` / `{{#rustdoc_include}}` pull snippets straight out of
  real `.rs` files**, prefixing out-of-range lines with `#` so they compile
  but stay hidden.

Neither MkDocs-Material nor Docusaurus has an equivalent. What we give up is
**versioned docs**: mdBook has none natively (issue #2245 open). MkDocs gets
it via `mike`; Docusaurus has it built in, though its own docs warn "most of
the time, you don't need versioning" and to keep versions under ten.

**Publishing book plus rustdoc together is a de-facto `mv` in CI**, not a
plugin or a convention. The clearest working example is wasmtime: `mdbook
build`, then `cargo doc --no-deps --workspace`, then move the book to
`gh-pages` and `target/doc` to `gh-pages/api` (live at docs.wasmtime.dev and
/api/wasmtime/). Our `pages.yml` already builds the book; adding the rustdoc
half is the same two lines.

Free from rustdoc: intra-doc links by item path with namespace
disambiguators, resolved even across re-exports, with `broken_intra_doc_links`
warning by default. Note the sharp edge: links containing `/` or `[]` are
**silently ignored**. `doc_cfg` (feature badges) is still nightly-only,
tracking issue #43781 open.

## 3. Doctests: powerful, and library-only

Attributes: `ignore` (not compiled), `no_run` (compiled, not run),
`compile_fail`, `should_panic`, `edition20xx`, and since 1.88
`ignore-<target>`. Hidden lines use `#`, escaped as `##`. Rustdoc wraps
snippets in `fn main`, injects `extern crate <mycrate>`, and applies a set
of allow-lints.

**The constraint that matters for a broker: doctests are library-only.**
Cargo is explicit that `doctest` "is only relevant for libraries, it has no
effect on other sections", and `cargo test --doc` tests only the library's
documentation. **Doc comments in `src/main.rs` are never doctested.** For us
that is mostly fine, since the substance lives in `crates/antares-*`, but it
means anything in `antares-broker`'s binary root cannot be executable
documentation. It also reinforces Phase S: the shared crates are exactly the
place where doctests pay, because they are the public surface a gateway
author would read.

Doctests link only against **public** items, `cfg(test)` is not set (so
`#[cfg(test)]` helpers are invisible), `cfg(doctest)` is.

`#[doc = include_str!("../README.md")]` is stable since 1.54. Two traps,
both real: every unmarked ` ``` ` fence in the README becomes a compiled Rust
doctest, so NGSI-LD payloads must be fenced as ` ```json ` or ` ```text `;
and relative links like `[design](docs/design.md)` render into rustdoc HTML
and 404 there.

## 4. Doc coverage: do not gate on it

`cargo doc --show-coverage` **does not exist**. The real form is
`cargo +nightly rustdoc -- -Z unstable-options --show-coverage`, whose
tracking issue (#58154) has been open since 2019 with a body that still reads
"FIXME: Write a proper synopsis". The numbers are known-buggy for
re-exported items (#110330), associated items on re-exports (#129007) and
ignored items (#145087), and there is a cargo bug where
`RUSTDOCFLAGS=--show-coverage cargo doc` deletes existing docs (cargo#9447).

**The stable equivalent is `#![deny(missing_docs)]` plus
`RUSTDOCFLAGS="-D warnings"`**: all-or-nothing on public items, no nightly,
no buggy percentages. That is what Phase S already asks for on the shared
crates.

Worth enabling regardless, all warn-by-default:
`invalid_codeblock_attributes` (catches a typo'd ` ```compile-fail ` fence
that would otherwise silently do nothing), `invalid_html_tags`, `bare_urls`.

## 5. CI hygiene

**lychee** for links: caching, `--offline`, fragment checking, JSON/JUnit
output. Its action defaults to `fail: true`, with a documented alternative of
a scheduled run at `fail: false` piped into `create-issue-from-file`.

For a conformance repo the split matters: **external ETSI and registry URLs
go down for reasons that have nothing to do with the PR**, so gate PRs with
`--offline` (internal links only) and run the full external check on a
schedule that files an issue. That is a judgement call, not a finding.

**Vale** for prose, syntax-aware so it can scope rules to headings and skip
code blocks, with official Microsoft/Google/write-good/proselint packages.
It cannot judge whether prose is true or whether an example still compiles,
which it says itself. Its action wraps reviewdog and defaults to
`filter_mode: added`, `fail_on_error: false`.

**Stale-doc detection has no mature tooling.** CODEOWNERS enforces review,
not freshness. Google's freshness metadata (`freshness: { owner, reviewed }`
with automated reminders) is the only documented large-scale practice, and
the tool that enforces it is internal and unreleased. `fiberplane/drift`
(hashes the tree-sitter AST of a symbol behind a markdown anchor, Rust
supported, `drift check` exits 1 on change) is the closest real thing at ~133
stars, which is adoption risk. No standard "docs required" PR check was
verifiable.

## 6. OpenAPI: vendor ETSI's spec, do not generate one

ETSI publishes the NGSI-LD OpenAPI spec, **BSD-3-Clause**, at
forge.etsi.org (`openapi-3.1.0/ngsi-ld-api.yaml`, ~5.4k lines, 28 paths,
~73 schemas), hand-written and deliberately single-file.

**It has visibly drifted**: `main` HEAD still declares `info.version: 1.7.1`
(last commit 2025-06-20) and a `v1.8.1` tag exists, while CIM 009 is now
V1.9.1 (2025-07). Orion-LD's README still links the 1.7.1 pin.

The normative companion, **ETSI GS CIM 047**, recommends **ReDoc** in its
Annex B as the tool for viewing the NGSI-LD OAS, saying SwaggerUI is
3.0.3-only. That last claim is now out of date: Swagger UI has supported
3.1.x since v5.32.0.

**Judgement, stated as such:** for a conformance product, code-first
generation (utoipa, aide, okapi) is backwards, because it lets the
implementation define a contract the standard already fixes. Vendor the ETSI
YAML, pin it, and parse it in CI with `oas3` (3.1-capable) rather than
`openapiv3` (3.0 only).

Tooling status, all verified: **oasdiff** is the diff tool to use (active,
Apache-2.0, first-party action, `fail-on` severity gates).
**Schemathesis** is the contract fuzzer (property-based from the schema, four
phases on by default) with one cost that lands squarely on us — it generates
extremes across the declared range, so validity that is **not expressible in
the schema produces false positives**, and JSON-LD term expansion and
`datasetId` semantics are exactly that. **Prism's validation proxy** can sit
in front of a live broker and validate both directions, which means the ETSI
Robot suite could be run through it for near-zero setup.

**Trap: Dredd is dead.** The repo was archived read-only in 2024-11 with the
last release in 2021, but there is no deprecation notice and dredd.org still
reads as a live project. **RapiDoc is stale** (last commit 2024-11), which
matches what we already found in the playground work. Pact is a different
thing entirely: consumer-driven contracts, not spec conformance.

## 7. Diátaxis: adopt the shape, drop two myths

The framework prescribes four modes (tutorials, how-to guides, reference,
explanation) generated by two axes: action versus cognition, and acquisition
versus application. The site claims completeness: "there could not be three,
or five".

**Two corrections to the common story.** **Django does not use Diátaxis** —
a full-repo grep finds only Procida's name in `AUTHORS`; Django's four-way
split is presented as its own and uses "Topic guides", not "Explanation".
**Cloudflare does not either** (13,253 files, zero hits). Confirmed adopters
are Gatsby (explicit), NumPy (NEP 44 cites it), and Canonical — though
Canonical's adoption post was written by Procida, who is its Director of
Engineering, so that is self-adoption rather than independent uptake.
Kubernetes cites it only as "helpful as a reference".

**No empirical evaluation of Diátaxis exists.** Asked directly what research
the model was based on, the author's answer was analytical, not empirical.
The site itself concedes that using it "does not guarantee deep quality",
has a whole section on boundaries blurring in practice, and warns against
empty scaffolding: "It certainly does not mean that you should create empty
structures… **It's horrible.**" Recurring practitioner critique: dogmatic
application is the standard failure mode, and quadrant siloing breaks
cross-linking between guides and reference.

Use it as a checklist for what is missing, not as a mandatory directory
layout.

## 8. ADRs

**Nygard's five fields**: Title (short noun phrase), Context (the forces at
play, in value-neutral language), Decision (full sentences, active voice,
"We will…"), Status (proposed/accepted/deprecated/superseded), Consequences
("All consequences should be listed here, not just the 'positive' ones").
His scope criterion: decisions "that affect the structure, non-functional
characteristics, dependencies, interfaces, or construction techniques".

**MADR 4.0.0 (2024-09)** adds front matter (`status`, `date`,
`decision-makers`, `consulted`, `informed`) and a **Confirmation** section:
"how the implementation / compliance of the ADR can/will be confirmed… any
automated or manual fitness function". That field is worth stealing for us
even without adopting MADR wholesale, because it forces an ADR to name its
own test.

**The sources genuinely disagree on immutability.** Nygard and Fowler say an
accepted ADR is never changed, only superseded. AWS says "becomes immutable"
and adds a Rejected state. But joelparkerhenderson says to amend with new
information, and MADR's `date: {when the decision was last updated}`
presupposes edits. **Pick one policy and write it down.** Ours currently
behaves as append-with-status (ADR-0006 was later revisited by research), so
say so explicitly.

**Failure modes are practitioner assertion, not evidence.** No study,
survey or postmortem quantifying ADR abandonment or retroactive
rubber-stamping was found. Note the structural argument in their favour:
because an ADR records what was decided *then*, it cannot go stale the way a
description of current state can — index sprawl is the real risk.

## 9. Doc rot: what is actually evidenced

**Evidence exists, and it is narrow — almost all about code comments.**

- Inconsistent comment changes are **~1.5× more likely** to accompany a
  bug-introducing commit, and the impact "is highest immediately after the
  inconsistency is introduced and diminishes over time" (arXiv:2409.10781;
  caveat: inconsistency labelled by an LLM, correlational).
- Linux kernel v6.18-rc1: an automated detector found **869 stale
  references** to functions that no longer exist; 50 of 75 repair patches
  were accepted upstream (arXiv:2608.03734). Cleanest direct measurement.
- The benchmark datasets in this line are themselves noisy, with "a
  substantial portion of sampled data mislabeled" (arXiv:2506.20558).

**The honest finding: no controlled study shows that docs-as-code, PR review
gates, CODEOWNERS or doctests reduce staleness.** That causal claim is
practitioner consensus. What the literature establishes is that staleness is
real, measurable, and correlates with bugs.

Practitioner prescriptions from sources operating at scale: Google's
"change your documentation in the same CL as the code change", "dead docs
are bad", "docs thrive when they're treated like tests", and "documents
without owners become stale". Write the Docs: "you can block merging of new
features if they don't include documentation" and "consider incorrect
documentation to be worse than missing documentation".

**The only mechanism with hard enforcement rather than a social norm is
executable examples**, and Rust gives it free. Combined with the finding that
rot's harm is **front-loaded**, that is the strongest available argument for
gating docs in the same PR rather than sweeping periodically.

**The pattern worth copying comes from the ETSI suite itself:** its
`doc/` Python generates ETSI deliverable CIM 013 *from* the Robot files, and
CONTRIBUTING.md makes it a required check — the documentation "is considered
healthy when it reports `0` failures… to confirm the documentation still
generates successfully and that **no drift was introduced**". Generated
artefacts verified against source on every change is exactly what
`dev/spec.py check` already does for the ledger.

## 10. FIWARE Generic Enabler requirements

Three stages (New, Incubated, Mature). Against the things we would need:

- **Read the Docs — MUST**, with prescribed structure: Getting Started,
  User/Programmer's Guide (mandatory), Installation/Admin Guide (mandatory),
  Deprecated Functionality (optional). Markdown "MUST NOT use HTML rendering
  such as `<a>` tags". Note this conflicts with our mdBook-on-Pages setup;
  it is a packaging requirement, not a quality one.
- **API spec — MUST**, "preferred format is OpenAPI, a.k.a. Swagger". (The
  New-GE checklist still says "Apiary file where necessary" — stale residue.)
- **Roadmap — MUST**, using the prescribed `GE_roadmap_template.md`, linked
  from README.
- **Docker — MUST**: Dockerfile, image on FIWARE Docker Hub, per-container
  README of all ENV vars, Docker Secrets support.
- **Tests and a CI badge — MUST. Coverage — only MAY.**
- **Security/privacy statement — NOT required.** No `SECURITY.md` or
  disclosure-process requirement appears in the rulebook; vulnerability
  obligations reach GEs only indirectly through the mandatory OpenSSF badge
  at Mature stage.
- QA teeth: a **yellow card** at overall QA label B or lower, a **red card**
  at C or lower.

We already exceed several of these (tests, CI, coverage, SECURITY.md).

## 11. What to act on

1. **Generate a filled CIM 029 ICS from the ledger.** Normative, freely
   reproducible, ~90 feature mnemonics plus 30 operation tables, and nobody
   in this ecosystem publishes one. `dev/spec.py` already holds clause
   status, evidence and Robot-tag mapping; the missing piece is a
   mnemonic-to-clause table, which A.6 is designed to make cheap. This is a
   machine-checked conformance artefact as a differentiator.
2. **Vendor and pin the ETSI OAS; diff it, do not generate it.** Render with
   ReDoc per CIM 047 Annex B, gate PRs with `oasdiff breaking` against the
   pin, point Schemathesis at it while accounting for the JSON-LD
   false-positive class.
3. **Copy the drift gate, not just the tests.** Generated documentation
   verified against source as a required check, the same principle as
   `spec.py check`, and the same principle that makes doctests the
   highest-confidence anti-rot measure available.
