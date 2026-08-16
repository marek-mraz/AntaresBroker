# Road to 1.0.0

1.0 is declared by the criteria below (see CONTRIBUTING "Versioning &
releases"). Each criterion is checked off only with evidence: run links or
commit hashes recorded next to it.

- [ ] **R1 — Three consecutive green full matrices.** Three back-to-back
      `etsi-full` runs on master with every cell green (all seven,
      including `wasm-file`), no suite-fork or broker changes between
      them other than unrelated docs.
- [ ] **R2 — HA soak test.** The role-split fleet (10 pods, NATS,
      Postgres) under continuous load for ≥ 24 h with rolls every hour:
      zero lost notifications (pair semantics counters), zero 5xx bursts
      beyond the drain windows, RSS flat (no leak slope). Rig + numbers
      recorded in docs/.
- [ ] **R3 — Upgrade-path tests.** Automated: file→file and pg→pg data
      created on version N-1, served correctly by version N (entities,
      temporal history, subscriptions fire). Runs on every release tag
      from 1.0-rc1 on.
- [ ] **R4 — Security review of the egress surface.** External or
      structured self-review of the egress wall (SSRF guards, breakers,
      TLS trust, notification/forward/@context paths); findings fixed or
      accepted in writing.
- [ ] **R5 — User docs complete.** The book covers getting started,
      configuration, deployment, federation, wasm, operations and
      conformance (done 2026-08-16) and tracks any surface added
      before 1.0.
- [ ] **R6 — Release machinery proven.** At least one 0.x release has
      shipped end to end through the gated pipeline (image + binaries +
      wasm bundle + SBOM + signature + notes) and the examples job is
      green on that tag.

When all six hold, tag `v1.0.0-rc1`; 1.0.0 follows after one more green
full matrix on the rc with no code changes.
