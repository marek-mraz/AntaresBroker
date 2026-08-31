# ADR-0018 — CI actions are pinned by tag, third-party binaries by version

Date: 2026-08-31. Status: accepted, implemented.

## Context

Every workflow in this repository runs third-party code: `uses:` steps
that GitHub resolves to a commit, and `run:` steps that fetch a release
tarball and execute it. Both are code this project does not review,
running with whatever the job's `permissions:` block grants. The jobs
that publish an image, cut a release or file an issue grant more than
`contents: read`, and two workflows carry the Hetzner and runner-
registration secrets.

A `uses: owner/action@v5` reference resolves through a tag the action's
owner controls and can move. Pinning the 40-character commit SHA instead
removes that: the reference names an immutable object, and an upstream
account takeover cannot change what a past workflow runs. It also
removes automatic patch updates, so every security fix upstream ships
becomes a commit here, and a stale pin is itself a vulnerability.

The release downloads had drifted apart. `workspace.yml` pinned
cargo-deny to an exact version; `advisories.yml` resolved
`releases/latest` at run time, in a job holding `issues: write`, so it
ran whatever that answer named on the day.

## Decision

**`uses:` references are pinned to a tag**, not to a commit SHA.

The actions used here are `actions/*`, `docker/*`, `sigstore/*`,
`helm/*`, `anchore/*` and five well-known community actions
(`Swatinem/rust-cache`, `dtolnay/rust-toolchain`, `taiki-e/install-action`,
`lycheeverse/lychee-action`, `errata-ai/vale-action`). Against the cost
of a pin that nothing renews, a moving major tag on this set is the
smaller risk for a project with no dependency-update automation.

Four references name a channel or a tool rather than a version, and stay
that way because the name is the argument: `dtolnay/rust-toolchain@stable`,
`@nightly` and `@master` select a Rust channel, and
`taiki-e/install-action@nextest` selects the tool. Pinning those to a commit would freeze the channel
resolution, not just the action.

**Binaries fetched in a `run:` step are pinned to an exact version**, in
the URL, the package specifier or the image tag, with no lookup against a
`latest` endpoint. This binds cargo-deny, kubeconform, k6, mdBook, the
Actions runner, cargo-fuzz, the ReDoc renderer that builds the published
API page and the oasdiff image that gates the vendored OpenAPI. The
version is a literal in the workflow, so changing it is a reviewable diff.

The service containers a job starts to test against — PostGIS, TimescaleDB,
NATS, mosquitto — are pinned as far as the upstream tag is meaningful and no
further, because moving them is what keeps the broker proven against the
databases and brokers people actually run. The exception is the rented
perf runner, where the containers share a machine with the Hetzner and
runner-registration secrets: mosquitto there names a patch version.

**Every job declares `permissions:`**, so a compromised action holds the
smallest token the job can do its work with.

## Consequences

An upstream tag that moves reaches this repository without review. The
mitigation is the permission floor rather than the pin: a job with
`contents: read` and no secrets can read a public repository, which is
what a public repository already offers.

Adopting SHA pins later means renewal has to come with it — an automated
bump that raises pull requests. Until that exists, SHA pins would rot,
and a rotting pin is worse than a tag: it looks like control while
holding a version nobody has looked at.

## Confirmation

Every `uses:` in `.github/` either names a path inside this repository
or carries a `@ref`; no `run:` step asks a release API which version is
newest; and parsing every workflow finds no job without a `permissions:`
block, at the job or at the file. The three branch references above are
the only refs that are not tags.
