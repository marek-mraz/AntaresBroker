# Book structure — maintainer note

The four documentation modes (tutorial / how-to / reference / explanation)
are a CHECKLIST for gaps, not a mandate for scaffolding: a chapter exists
because it has content, never to fill a quadrant.

Current mapping of `docs/src`:

| chapter | mode |
|---|---|
| introduction.md | explanation |
| getting-started.md | tutorial |
| configuration.md | reference |
| deployment.md, operations.md, federation.md, wasm.md | how-to |
| conformance.md, conformance-ics.md | reference |
| ecosystem.md | explanation |
| api.html (ReDoc, generated at deploy) + /api rustdoc | reference |

Gap check when adding a feature: does it need a how-to (an operator can
run it), and did the reference (configuration table, ICS, rustdoc) pick
it up? A tutorial or explanation chapter is added only when a reader has
actually asked for one.
