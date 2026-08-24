# Architecture Decision Records

One file per irreversible decision, numbered, never rewritten.

## Format

Nygard's fields — **Title, Status, Context, Decision, Consequences** —
plus one borrowed from MADR:

- **Confirmation**: how compliance with the decision can be checked — a
  named test, a CI job, a grep, or an explicit "manual review only".
  Every new ADR names its own fitness check; a decision nobody can verify
  drifts silently.

Shorter ADRs fold Context/Decision/Consequences into prose sections, as
the existing files do; the five concerns must all be answerable from the
text either way.

## Immutability policy

Append-with-status. An accepted ADR's body is frozen; when a decision
changes, a NEW ADR supersedes it and the old one's Status line gains
`superseded by ADR-00XX` (see ADR-0005/ADR-0013 and the reversal recorded
in ADR-0007). Never edit an old ADR's Decision to match new reality —
the record of what was believed, and when it stopped being true, is the
point of keeping them.
