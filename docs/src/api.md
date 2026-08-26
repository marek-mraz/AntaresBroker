# API reference

Two generated references are published next to this book:

- **NGSI-LD HTTP API** — ETSI's own OpenAPI document (`docs/openapi/ngsi-ld-api.yaml`, vendored at a pinned tag), rendered with ReDoc:
  <https://antares-ngsi-ld-demo.marek-mraz.com/docs/api.html>
- **Rust crates** — rustdoc for every workspace crate, the entry point for the [shared crates](shared-crates.md):
  <https://antares-ngsi-ld-demo.marek-mraz.com/api/>

The broker serves the NGSI-LD API under `/ngsi-ld/v1`; the admin routes under `/q/` are in the [Admin API](admin-api.md) chapter.
