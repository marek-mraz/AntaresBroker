# Tasks — production-ready release

## Goal (2026-08-16): make Antares look and behave like a production product

Everything a stranger touches in the first hour — README, docs, examples,
releases — must look deliberate. The discipline of claude.md §0.3 applies to
every item: one item = one commit, checked off `[x]` only with evidence
(commit hash + the artifact: a rendered page, a green run, a published tag)
recorded next to it. Working notes NEVER return to the repo root — they live
in `/workspace` (workspace-level, outside the repo); the repo carries only
what a user of the product needs.

### The /goal prompt

```
/goal Work tasks.md top-to-bottom until every checkbox is [x] with evidence.
One item = one commit. Nothing speculative: every doc page describes behaviour
that exists and is CI-proven; every example is executed before it is committed
(rule 8 discipline — run it, paste the proof). Ask only where a task
explicitly says "decide with the user".
```

---

## A. Repo hygiene

- [x] A1. Move working notes out of the repo root (error.md,
      taskImplementation.md, old tasks.md → /workspace; firewall-docs corpus
      indexes → /workspace). Evidence: this commit.
- [x] A2. Slim `claude.md`: §6 moved to
      /workspace/antares-state-handoff-archive.md (32 KB of session log),
      replaced by a ≤10-line current-position block; all `error.md`
      references repointed to /workspace/error.md. claude.md 51 KB → 19 KB.
      Evidence: this commit.
- [x] A3. `.gitignore`: ETSI-matrix-results*, results*/, .DS_Store, *.zip
      added. Evidence: this commit.
- [x] A4. demo/hfp-bento/ and demo/hfp/map/ moved to /workspace/hfp-demos/
      (user decision 2026-08-16: keep the repo clean; examples/ is built
      fresh per D1-D5). Evidence: this commit + clean git status.
- [x] A5. Root inventory verified 2026-08-16: only product files, tooling
      config (.github, claude.md, mempalace.yaml) and gitignored noise
      remain. OPEN NOTE for the public release: etsi-cim-specs/ PDFs are
      tracked — ETSI documents carry redistribution restrictions; before
      the repo goes public, replace with a download script (fetch from
      etsi.org by URL) and gitignore the PDFs. Recorded here, not acted on
      (the audit loop and MemPalace mining depend on the local copies).
      Evidence: this commit message carries the ls output.

## B. README (the front door)

- [x] B1. "Why Antares" section added with the three CI-backed numbers:
      35 MiB RSS avg / 9 MiB idle, 1713/1713 across six native cells
      (report linked), 3.99 MB / 1.52 MB gzip wasm artifact. Numbers taken
      from full #4 run summaries. Evidence: this commit.
- [x] B2. 60-second quickstart added after "Why Antares": docker run +
      create + query. Curls executed verbatim against the locally built
      binary (same bytes CI images ship): 201 Created + entity returned
      (host-docker pulls are out of sandbox scope per claude.md rule 1).
      Evidence: this commit; proof pasted in the commit message.
- [x] B3. Store table prefaced with a which-mode-when guide + the two
      orthogonal switches (ANTARES_BUS local/nats, MQTT per subscription);
      links docs/operations.md. Evidence: this commit.
- [x] B4. Badges audit: ci/strict/roll-weekly + ETSI endpoint + per-cell +
      coverage were already live; EUPL-1.2 license badge added. Deliberately
      NOT added (would be aspirational/unsupported): release badge (no
      release exists until E7 — add it in E7's commit) and Docker-pulls
      (shields has no GHCR pull counter). Evidence: this commit.
- [x] B5. "How Antares compares" table added (language/runtime, storage,
      bus, minimum footprint, measured RSS, wasm, conformance evidence);
      other brokers' conformance left to their own reporting; License
      section fixed BSD -> EUPL-1.2. Evidence: this commit.

## C. Docs (docs/ becomes user-facing)

- [x] C1. mdBook (user decision): book.toml + docs/src/ is the rendered
      user set (introduction + operations moved in); internal analysis
      (deep-analysis, audits, spec/ ledger, adr/) stays unrendered in
      docs/. pages.yml builds the book into site/docs on every deploy.
      `mdbook build` green locally (v0.4.44). Evidence: this commit.
- [x] C2. Getting started page: install, first entity (201+query), first
      subscription (201, PATCH 204 -> Notification value 42.0 received on a
      local listener), federation pair (entity on B, CSR on A 201,
      federated GET via A returns B's entity). Every snippet executed
      against the locally built broker before commit; the 6.3.5
      json-vs-ld+json @context rule documented from a live 400.
      Evidence: this commit.
- [x] C3. docs/src/configuration.md: 23 vars, defaults verified at their
      parse sites (main.rs/state.rs/bounds.rs/shutdown.rs/loader.rs/
      telemetry.rs/wiring.rs/nats.rs). dev/check-env-docs.sh green locally,
      wired into workspace.yml next to spec.py check. Evidence: this commit.
- [x] C4. docs/src/deployment.md: sizing table from full #4 measurements,
      single-node/pg/HA/role-split shapes (each one a CI-exercised compose
      file), k8s manifests with their store-dictated strategies, upgrade
      contract. Evidence: this commit.
- [ ] C5. Federation guide: CSRs, distributed operations, Via/loop
      protection, EntityMap paging, the 5-broker compose stack as the worked
      example.
- [ ] C6. Browser/wasm guide: what runs in a page (memory+file/OPFS, no
      MQTT/NATS — the N8 structural limits stated plainly), Service Worker
      mode, the Node shim tier, size budgets.
- [ ] C7. Operations: monitoring endpoints, state reset discipline
      (API-delete + truncate pairing), backup/restore (store files, pg
      dumps), upgrade procedure (blue/green replay — references the
      city-as-code plane, /workspace/docs/city-as-code-requirements.md
      CC-50/51).
- [ ] C8. Conformance page: what 1713/1713 means, the seven-cell matrix,
      the ledger methodology (`docs/spec/`, 947 clause files), how to re-run
      the suite locally — this page is the procurement evidence artifact.

## D. Examples (examples/ — every one executed before commit)

- [ ] D1. `examples/quickstart/` — compose file (broker + nothing), seed
      script with 3 entities, query walkthrough. Smoke-tested in CI
      (a 2-minute job, not the full matrix).
- [ ] D2. `examples/federation/` — two brokers + CSR registration script;
      shows a federated query resolving across both.
- [ ] D3. `examples/subscriptions/` — HTTP callback + MQTT notification
      variants with a tiny receiver.
- [ ] D4. `examples/browser/` — the www/ demo promoted to a documented
      example: serve, create entities in-page, watch notifications.
- [ ] D5. `examples/smart-city/` — small realistic dataset (Smart Data
      Models types, ~50 entities), the demo that sells the digital-twin
      story; reused by README screenshots.
- [ ] D6. CI job `examples.yml`: every example's run script executes on
      every release tag (gate) and weekly (drift catch).

## E. Versioning & release process

- [ ] E1. Adopt semver with a written policy in CONTRIBUTING: pre-1.0 =
      0.MINOR.PATCH, breaking changes bump MINOR; 1.0.0 criteria defined
      (E6). API surface = the NGSI-LD API (spec-versioned) + env-var config
      + store file formats; store-format changes require a migration note.
- [ ] E2. CHANGELOG.md → Keep-a-Changelog format, `[Unreleased]` section
      maintained per merge; release tag moves it under the version heading.
- [ ] E3. Release workflow (`release.yml`, triggered by `v*` tag): full
      seven-cell ETSI matrix MUST be green as the gate (etsi-full on the
      tag already runs — wire it as the release gate, not just a report);
      then: multi-arch (amd64+arm64) image → GHCR `:X.Y.Z` + `:latest`,
      static binaries (musl) as release assets, `www/pkg` wasm bundle as a
      release asset, auto-generated release notes from CHANGELOG.
- [ ] E4. Supply-chain minimum: SBOM (cargo auditable or syft) attached to
      the release, image signed (cosign keyless), `cargo deny` green as a
      release gate (already in CI — make it blocking on tags).
- [ ] E5. Version surface: `GET /info` (or the existing version endpoint)
      returns version + git hash + store mode; `--version` on the binary;
      both asserted by a release smoke test.
- [ ] E6. Write `docs/roadmap-1.0.md`: the explicit 1.0 criteria list
      (candidate set, decide with the user: N consecutive green full
      matrices, HA soak test, upgrade-path test file→file and pg→pg across
      one minor version, security review of the egress surface, docs C1–C8
      complete). 1.0 is declared by criteria, not by feeling.
- [ ] E7. First real release: cut `v0.1.0` through E3 end-to-end. Evidence:
      the published GitHub release with all assets + green gate run.

## F. Quality gates (make the existing rigor visible)

- [ ] F1. Coverage: the weekly etsi-coverage report gets a README badge and
      a one-line ratchet (fail if merged line coverage drops >1pt below the
      last main run). Report-only history stays.
- [ ] F2. MSRV: pin and test a minimum supported Rust version in CI;
      document in README.
- [ ] F3. Public CI dashboard: the Pages conformance report linked from
      README (B4) renders per-suite pass/fail + RSS trend — this exists;
      task is linking + a short "how to read this" page.
- [ ] F4. `cargo audit`/deny advisories: schedule weekly, auto-file an
      issue on new advisories.

## G. License, community, positioning (decide with the user — company/OSS
      discussion of 2026-08-16)

- [x] G1. License = EUPL-1.2 (user decision 2026-08-16). LICENSE replaced
      with the canonical SPDX text (was BSD-3-Clause), Cargo.toml workspace
      license updated, deny.toml allowlists EUPL-1.2; `cargo deny check
      licenses` green. Evidence: this commit.
- [ ] G2. CONTRIBUTING.md rewrite: how to run the suite locally (the
      etsi-local recipe), clause-citation convention for normative code,
      PR expectations (targeted tests + robot proof), DCO or CLA decision.
- [ ] G3. SECURITY.md: real disclosure contact + supported-versions table
      (tracks E1 policy).
- [ ] G4. Issue/PR templates: bug (broker version, store mode, repro
      payload), conformance deviation (clause number required), feature.
- [ ] G5. Positioning page (docs or website): Antares within the FIWARE /
      NGSI-LD ecosystem — compliant peer, not a fork; the wasm/edge story;
      the city-as-code configuration plane as the companion project.

---

## Ordering & dependencies

A (hygiene) → B (README) → E1/E2 (policy before first tag) → C+D in
parallel → E3–E5 → F → E7 (first release) once G1 is decided. G2–G5 any
time after A. The single hard external blocker is G1 (license, user
decision); everything else is sandbox-side except pushes and the GHCR/Pages
settings, which are Mac-side.
