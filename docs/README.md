# Antares documentation index

| Document | What it holds |
|---|---|
| [src/](src/) | The user book (mdBook, rendered to the Pages site under `/docs/`): getting started, configuration, deployment, federation, wasm, [operations runbook](src/operations.md) |
| [spec/](spec/) + [spec/README.md](spec/README.md) | The conformance ledger — ETSI CIM 009 V1.9.1 full text, ONE file per clause with `status`/`evidence`/`notes` frontmatter; tooling: `python3 dev/spec.py status\|gaps\|check` |
| [adr/](adr/) | Irreversible decisions, one file each (shared-schema tenancy, JetStream bus, store ladder, wasm build, …) |
| [upstream/etsi-raises.md](upstream/etsi-raises.md) | Ready-to-file upstream issues against the ETSI suite/spec (defects proven from clause text) |
| [roadmap-1.0.md](roadmap-1.0.md) | The criteria that declare 1.0.0 |
| [openapi/](openapi/) | The ETSI NGSI-LD OpenAPI description served by the playground's API console |
| [book-structure.md](book-structure.md) | Maintainer note on the four documentation modes of the user book |

Working notes (architecture analysis, audits, research) stay out of
version control.

Live artifacts: [the ETSI conformance report page](https://antares-ngsi-ld-demo.marek-mraz.com/reports/latest/)
· [the browser playground](https://antares-ngsi-ld-demo.marek-mraz.com/).
