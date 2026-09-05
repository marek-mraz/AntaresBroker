# Vendored NGSI-LD OpenAPI

`ngsi-ld-api.yaml` is ETSI's own OpenAPI document, vendored VERBATIM —
never edited, never regenerated from our code. For a conformance product
the standard defines the contract; code-first generation (utoipa/aide)
would let the implementation define a contract the spec already fixes.

- Source: https://forge.etsi.org/rep/cim/ngsi-ld-openapi,
  `openapi-3.1.0/ngsi-ld-api.yaml`, tag **v1.8.1** (the newest tag; the
  repo's `main` still declares 1.7.1). License: BSD-3-Clause (`LICENSE`
  alongside).
- Version drift, known and accepted: the document tracks CIM 009
  **V1.8.1** while this broker implements **V1.9.1**. The pin moves when
  ETSI tags a newer version, in its own commit.
- Known upstream defect at this tag: `GET /temporal/entities` and
  `GET /temporal/entities/{entityId}` each reference two parameter
  components both named `options`, so strict OAS validators reject the
  file (raised upstream — see `docs/upstream/etsi-raises.md`). Gates on
  this file therefore use lenient tooling (oasdiff), not strict
  validation.

The playground ships its own copy at `www/public/openapi/ngsi-ld-api.yaml`
— `www/public` is what vite serves to the browser, and the API console
loads the document over HTTP. That copy is this one with a single line
changed: it declares OAS `3.0.3`, the version Swagger UI is exercised
against in the browser tier, where ReDoc renders `3.1.0` here.
`python3 dev/spec.py check` refuses any other difference between the two,
so moving the pin has to move both.

CI: a PR that touches this file runs `oasdiff breaking` against the
version on the base branch. Rendering: ReDoc (the CIM 047 Annex B
recommendation) builds `api.html` beside the book in the Pages deploy.

## The broker's own routes

`antares-admin.yaml` describes every route outside the NGSI-LD API root:
the `/q/` operator surface and the `/ex/v1/` peer wire. This one IS ours,
hand-written from the handlers, because no standard fixes that contract.
It is held to the code by a unit test in `antares-api` (`Admin` in
`lib.rs`): every path and method the document lists must be mounted, and
every mounted one must be listed, so a route added to the router without
its documentation fails the build. The same `oasdiff breaking` gate and
the same ReDoc render (`admin-api.html`) apply to it.
