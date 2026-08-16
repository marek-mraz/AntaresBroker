# Antares documentation index

| Document | What it holds |
|---|---|
| [src/](src/) | The user book (mdBook, rendered to the Pages site under `/docs/`): getting started, configuration, deployment, federation, wasm, [operations runbook](src/operations.md), conformance |
| [spec/](spec/) + [spec/README.md](spec/README.md) | The conformance ledger — ETSI CIM 009 V1.9.1 full text, ONE file per clause with `status`/`evidence`/`notes` frontmatter; tooling: `python3 dev/spec.py status\|gaps\|check` |
| [adr/](adr/) | Irreversible decisions, one file each (shared-schema tenancy, JetStream bus, store ladder, wasm build, …) |
| [upstream/etsi-raises.md](upstream/etsi-raises.md) | Ready-to-file upstream issues against the ETSI suite/spec (defects proven from clause text) |
| [policies.md](policies.md) | Access-policy design for federated deployments (gateway/PDP layer; the broker stays policy-free) |
| [roadmap-1.0.md](roadmap-1.0.md) | The criteria that declare 1.0.0 |

Live artifacts: [the ETSI conformance report page](https://antares-ngsi-ld-demo.marek-mraz.com/reports/latest/)
· [the browser playground](https://antares-ngsi-ld-demo.marek-mraz.com/).
