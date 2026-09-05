# API reference

Three generated references are published next to this book:

- **NGSI-LD HTTP API** — ETSI's own OpenAPI document (`docs/openapi/ngsi-ld-api.yaml`, vendored at a pinned tag), rendered with ReDoc:
  <https://antares-ngsi-ld-demo.marek-mraz.com/docs/api.html>
- **Operational API** — the broker's own OpenAPI document for every route outside `/ngsi-ld/v1` (`docs/openapi/antares-admin.yaml`: `/q/` and the peer wire `/ex/v1/`), rendered the same way:
  <https://antares-ngsi-ld-demo.marek-mraz.com/docs/admin-api.html>
- **Rust crates** — rustdoc for every workspace crate, the entry point for the [shared crates](shared-crates.md):
  <https://antares-ngsi-ld-demo.marek-mraz.com/api/>

The broker serves the NGSI-LD API under `/ngsi-ld/v1`; the prose for the routes under `/q/` is the [Admin API](admin-api.md) chapter. A unit test in `antares-api` holds the operational document to the router: every path and method it lists is mounted, and every mounted one is listed.
