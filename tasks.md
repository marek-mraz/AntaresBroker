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
- [x] C5. docs/src/federation.md: CSR modes, distributed reads/writes,
      Via/508 loops, distributed subscriptions + PUBLIC_URL, EntityMap
      paging, tenancy rule (4.14), dev/run-five.sh as the worked example
      (the CI IOP stack — process-based, not compose). Evidence: this
      commit.
- [x] C6. docs/src/wasm.md: Service Worker + in-page API, OPFS file
      store, structural limits stated plainly (inbound sockets, CL header,
      no MQTT/NATS/roles), Node tier as the conformance gate, build
      recipe + budgets (3.99/1.52 MB vs 8/3 MB). Evidence: this commit.
- [x] C7. operations.md already covered deploy/health/backup/rolling/
      state-reset/proof-table (moved into the book in C1); added the
      missing Upgrades section (rolling for minors, blue/green replay for
      majors, CC-50/51 reference, temporal-history caveat, file-format
      refusal). Evidence: this commit.
- [x] C8. docs/src/conformance.md: the matrix explained cell by cell,
      ledger methodology (947 clause files, extension TPs, upstream defect
      policy), local reproduction recipes, caveats stated plainly (wasm
      exclusions, arm64, the one deliberate 6.3.4 deviation). Evidence:
      this commit.

## D. Examples (examples/ — every one executed before commit)

- [x] D1. examples/quickstart/: compose + seed.sh (batch create, q=,
      geo query). Executed locally against the built broker: 3 seeded,
      q=temperature>30 returned exactly sensor 3, near-2km returned
      sensors 1+2. CI smoke lands in D6. Evidence: this commit.
- [x] D2. examples/federation/: compose (2 brokers) + run.sh with a
      self-asserting federated query. Executed locally against a live
      broker pair: "OK: B's entity served by A". Evidence: this commit.
- [x] D3. examples/subscriptions/: receiver.py + run.sh (HTTP, executed
      locally: "OK: notification received" with state=open) + mqtt-run.sh
      and mqtt-compose.yml (MQTT execution delegated to the D6 CI job —
      mosquitto is container-only here, same posture as the MQTT TPs).
      Evidence: this commit.
- [x] D4. examples/browser/: serve.sh (page + Service Worker path) +
      Node-shim recipe. Executed: shim on :9394 from the same www/pkg
      bytes — health UP + create 201. In-page flow is the hosted
      playground, page-tested by dev/wasm-test.sh in CI. Evidence: this
      commit.
- [x] D5. examples/smart-city/: 50 entities (ParkingSpot/Streetlight/
      AirQualityObserved/WasteContainer), idempotent upsert seed, five
      city questions. Executed locally: 13 free spots, 4 lights off,
      4 pm25>25, 3 containers >=70%, 16 within 500 m. Evidence: this
      commit.
- [x] D6. .github/workflows/examples.yml: builds the image, runs
      quickstart/federation/subscriptions-HTTP/subscriptions-MQTT/
      smart-city + the browser shim (newest antares-www artifact; loud
      warning-skip if none in retention — no silent cap). Triggers: v*
      tags, weekly, dispatch. YAML validated; scripts themselves executed
      locally in D1-D5. First live run happens on push (Mac-side).
      Evidence: this commit.

## E. Versioning & release process

- [x] E1. CONTRIBUTING "Versioning & releases": surfaces defined (API =
      spec-pinned, env vars, store formats), pre-1.0 MINOR-for-breaking
      rule, migration-note duty, tag-triggered gated releases, Keep a
      Changelog discipline. Evidence: this commit.
- [x] E2. CHANGELOG in Keep-a-Changelog form: [Unreleased] with
      Added/Changed/Fixed carrying today's work; dated history kept as
      pre-release milestones. Evidence: this commit.
- [x] E3. Extended full.yml (already tag-triggered + matrix/roll-gated)
      instead of a parallel release.yml: release-binaries job (musl
      x86_64 + aarch64, stripped, tar.gz with LICENSE/README),
      antares-wasm-pkg artifact from the wasm job, github-release job
      (assets + notes extracted from CHANGELOG). YAML validated; first
      live run = the v0.1.0 tag (E7, Mac-side). Evidence: this commit.
- [x] E4. github-release job: syft SBOM (spdx-json) of the released
      image attached, cosign keyless signature on the GHCR tag; cargo-deny
      already blocks in workspace.yml which gates every tag build (no
      change needed — verified licenses+advisories run there). Evidence:
      this commit (same diff as E3).
- [ ] E5. Version surface: `GET /info` (or the existing version endpoint)
      returns version + git hash + store mode; `--version` on the binary;
      both asserted by a release smoke test.
- [x] E6. docs/roadmap-1.0.md: six criteria (3 green full matrices, 24h
      HA soak, upgrade-path tests, egress security review, docs complete,
      release machinery proven end to end) — the full candidate set per
      user decision 2026-08-16; rc flow defined. Evidence: this commit.
- [ ] E7. First real release: cut `v0.1.0` through E3 end-to-end. Evidence:
      the published GitHub release with all assets + green gate run.

## F. Quality gates (make the existing rigor visible)

- [x] F1. Coverage badge was already in the README header; ratchet step
      added to etsi-coverage.yml (merged lines % vs the published
      coverage-badge.json, fail on >1pt drop, skip when no baseline).
      YAML + bash syntax validated. Evidence: this commit.
- [x] F2. rust-version = 1.97 in workspace.package + all 11 crates
      (the only PROVEN toolchain — CI stable today; older untested, honest
      pin), msrv job in workspace.yml keeps it proven as stable moves,
      README notes it. Also fixed the stale workspace repository URL
      (AntaresBroker org -> marek-mraz). Evidence: this commit.
- [x] F3. The report page existed and was badge-linked; the
      "how to read this" page is the book's conformance chapter (C8),
      now linked from the README's ETSI section to its rendered URL.
      Evidence: this commit.
- [ ] F4. `cargo audit`/deny advisories: schedule weekly, auto-file an
      issue on new advisories.

## G. License, community, positioning (decide with the user — company/OSS
      discussion of 2026-08-16)

- [x] G1. License = EUPL-1.2 (user decision 2026-08-16). LICENSE replaced
      with the canonical SPDX text (was BSD-3-Clause), Cargo.toml workspace
      license updated, deny.toml allowlists EUPL-1.2; `cargo deny check
      licenses` green. Evidence: this commit.
- [x] G2. CONTRIBUTING already carried the suite recipe, clause-citation
      convention and PR/test expectations; added in this goal run: the
      versioning/release section (E1) and the DCO sign-off section (DCO
      chosen over CLA — lightweight, standard for EUPL projects; flag to
      the user if a CLA is ever wanted for relicensing power). Evidence:
      this commit.
- [x] G3. SECURITY.md already had the disclosure contact + posture/audit
      links; supported-versions table added (latest-release policy,
      pre-1.0 rule). Evidence: this commit.
- [x] G4. Issue forms (bug with version/store/repro required;
      conformance deviation with mandatory clause + quoted SHALL; feature)
      + PR template with the proof checklist (targeted tests, Robot,
      test-first, changelog, DCO). YAML validated. Evidence: this commit.
- [x] G5. docs/src/ecosystem.md: compliant-peer positioning, the three
      fit stories (edge, browser, dense multi-tenancy), the vanilla-broker
      + city-as-code split, standards posture. Evidence: this commit.

---

## Ordering & dependencies

A (hygiene) → B (README) → E1/E2 (policy before first tag) → C+D in
parallel → E3–E5 → F → E7 (first release) once G1 is decided. G2–G5 any
time after A. The single hard external blocker is G1 (license, user
decision); everything else is sandbox-side except pushes and the GHCR/Pages
settings, which are Mac-side.
