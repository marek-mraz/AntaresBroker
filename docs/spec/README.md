# docs/spec — the conformance ledger, at full-text granularity

CIM 009 V1.9.1 (`etsi-cim-specs/gs_cim009v010901p.pdf`) split into **one file
per outline clause** — 947 sections, `<chapter>/<clause>.md`, each holding the
clause's own text so implementing never starts from a page number. This
replaces `docs/ics.yaml` (deleted 2026-08-10; its 122 audited rows live in git
history) with a deliberate **reset to zero**: every clause starts
`not-implemented` and earns its status back through re-audit.

## One file = one clause

```markdown
---
clause: 4.5.16.3
title: GeoJSON Representation of Multiple Entities
pages: '68'
status: not-implemented   # HAND: not-implemented | partial | implemented | staged-v1x | informative
evidence: ''              # HAND: code/test anchors, like the old ics.yaml evidence
notes: ''                 # HAND: named gaps, spec doubts, dates
robot: []                 # GENERATED from the suite fork's [Tags] — do not edit
---

<the clause's full text, extracted from the PDF>
```

`status`/`evidence`/`notes` are hand-maintained and survive re-extraction.
`robot` is generated; to mark a clause tested, tag the TP in the suite fork
(`[Tags] 4_5_16_2` style) and run `python3 dev/spec.py robot`.

## Commands (`dev/spec.py`)

| command | does |
|---|---|
| `split` | re-extract all bodies from the PDF; hand fields preserved |
| `robot` | refresh every `robot:` list from the suite's `[Tags]` |
| `status` | counts per status + robot-tagged total |
| `gaps` | leaf clauses that are `not-implemented` with no TPs |

## Rules

- The **PDF stays the authority** — the extracted body is a working copy for
  reading and grep; when wording matters down to a comma, read the page range
  in the PDF (or `mempalace_get_pdf_pages`).
- One clause per commit rule unchanged (tasks.md step 5): the code, its
  tests, and this file's `status`/`evidence` move together.
- Never hand-edit `robot:` or the body; both are regenerated.
