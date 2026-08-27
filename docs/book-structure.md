# Book structure — maintainer note

The four documentation modes (tutorial / how-to / reference / explanation)
are a CHECKLIST for gaps, not a mandate for scaffolding: a chapter exists
because it has content, never to fill a quadrant.

Current mapping of `docs/src` (`SUMMARY.md` groups the chapters under the
same four part titles):

| part | chapter |
|---|---|
| tutorial | getting-started.md |
| how-to | deployment.md, subscriptions.md, temporal.md, federation.md, operations.md, wasm.md |
| reference | configuration.md, admin-api.md, storage.md, conformance.md, performance.md, coverage.md, shared-crates.md, api.md (links to the ReDoc render and rustdoc) |
| explanation | introduction.md, extending.md, ecosystem.md, decisions.md (the ADR index) |

Gap check, last run against this mapping: every feature an operator can
run (subscriptions, temporal, federation, bulk load, backup, tenants,
dead letters, retries, OTLP logs) has a how-to section; every environment
variable is in the configuration table (`dev/check-env-docs.sh`); every
`/q/` route is in the admin API chapter; the storage drivers and the
extension model each have a reference chapter. No tutorial beyond getting
started and no further explanation chapter has been asked for.

Gap check when adding a feature: does it need a how-to (an operator can
run it), and did the reference (configuration table, rustdoc) pick
it up? A tutorial or explanation chapter is added only when a reader has
actually asked for one.
