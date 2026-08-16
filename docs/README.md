# Antares documentation index

| Document | What it holds |
|---|---|
| [deep-analysis.md](deep-analysis.md) | The architecture & design analysis: targets, capacity budgets, store ladder, bus design, federation, the §16 security wall, conventions, Scorpio reference mapping |
| [src/operations.md](src/operations.md) | The operations runbook: deploy, backup per store mode, rolling updates, health/readiness/metrics, state-reset discipline |
| [spec/](spec/) + [spec/README.md](spec/README.md) | The conformance ledger — ETSI CIM 009 V1.9.1 full text, ONE file per clause with `status`/`evidence`/`notes` frontmatter; tooling: `python3 dev/spec.py status\|gaps\|check` |
| [adr/](adr/) | Irreversible decisions, one file each (shared-schema tenancy, JetStream bus, store ladder, wasm build, …) |
| [production-readiness-audit-2026-08-09.md](production-readiness-audit-2026-08-09.md) | The 8-agent production audit + the 2026-08-15 re-audit status table (every P0/P1 closed-with-evidence) |
| [security-audit-2026-08-04.md](security-audit-2026-08-04.md) | The security-wall audit behind SECURITY.md |
| [upstream/etsi-raises.md](upstream/etsi-raises.md) | Ready-to-file upstream issues against the ETSI suite/spec (defects proven from clause text) |
| [iop-tp-checklist.md](iop-tp-checklist.md) | The 118-TP interoperability campaign record (2026-08-14) |
| [perf-observability-goal.md](perf-observability-goal.md) | The perf/observability goal document |
| [policies.md](policies.md) | Deployment policy notes |
| [playground-ui-analysis.md](playground-ui-analysis.md) | The browser playground (www/) UI analysis |

Suite/spec defect logs live at the repo root (`error.md`) and in the suite
fork (`ngsi-ld-test-suite/testsuite-doubts.md`); the task list is `tasks.md`;
the agent working contract is `claude.md`.

Live artifacts: [the ETSI conformance report page](https://antares-ngsi-ld-demo.marek-mraz.com/reports/latest/)
· [the browser playground](https://antares-ngsi-ld-demo.marek-mraz.com/).
