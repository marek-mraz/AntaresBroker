# Contributing

## Build

```bash
cargo build -p antares-broker        # the `antares` binary
cargo test --workspace -j 2          # NOTE: -j 2 — default parallelism
                                     # OOM-kills the linker on small boxes
```

Integration tests that need services are env-gated and skip loudly:
`ANTARES_TEST_DATABASE_URL` (PostGIS), `ANTARES_TEST_NATS_URL` (JetStream),
`ANTARES_TEST_MQTT_URL` (mosquitto).

## The rules that are enforced

- **Spec-first.** Every normative behaviour is implemented from its ETSI
  CIM 009 V1.9.1 clause and the function carries a doc comment citing the
  clause number. The conformance ledger lives in `docs/spec/` (one file per
  clause; `python3 dev/spec.py check` gates format in CI).
- **Test-first.** Write the clause's tests before the implementation and
  watch them fail on the missing behaviour. Every test carries at least
  one negative assertion (what must NOT be in the response).
- **One clause = one commit**, message prefixed with the clause number
  (`5.6.6: …`), committed on a green targeted run (`cargo test -p
  <touched-crate> <filter> -j 2`) plus the clause's Robot TPs green against
  one local memory-store broker (`resources/variables.py` ships upstream's
  compose addresses, so a bare `robot` run overrides them the way
  `dev/etsi-run.sh` does):

  ```bash
  cargo build -q -p antares-broker -j 2
  ANTARES_HTTP_PORT=9377 ./target/debug/antares &
  cd ngsi-ld-test-suite && robot --variable url:http://localhost:9377/ngsi-ld/v1 \
    --variable temporal_api_url:http://localhost:9377/ngsi-ld/v1 \
    --variable notification_server_host:127.0.0.1 \
    --variable context_source_host:127.0.0.1 \
    --variable context_server_host:127.0.0.1 \
    TP/path/to/<tp>.robot
  ```

- **ETSI validation.** `STORE=<mode> dev/etsi-local.sh` runs the suite for
  ONE store mode locally (the one you touched); the CI cell matrix is the
  authority — the quick preset (file, postgres, timescale) runs all ten
  suites per cell on every dispatch, the full seven-cell preset twice a
  week and on `v*` tags.
- `cargo fmt` on touched crates; clippy is a CI wall
  (`unwrap_used`/`expect_used` denied outside tests, `unsafe_code` forbidden).
- Naming comes from the spec: types verbatim from CIM 009 §5.2, one public
  fn per spec operation; `Manager`/`Service`/`Util`/`Helper` are banned
  suffixes.

## Where things live

See the README's repository-layout table and [docs/README.md](docs/README.md).

## Versioning & releases

Semantic versioning, with the version's meaning defined by these surfaces:
the NGSI-LD API (pinned to ETSI CIM 009 V1.9.1 — spec-versioned, not
ours to break), the `ANTARES_*` environment variables
(docs/src/configuration.md), and the on-disk store formats (redb file
format version, Postgres migrations).

- **Pre-1.0**: `0.MINOR.PATCH` — breaking changes to env vars or store
  formats bump MINOR and are called out in the changelog; PATCH is
  fixes/additions.
- **1.0.0** is declared by the criteria in
  [docs/roadmap-1.0.md](docs/roadmap-1.0.md).
- **Store-format changes** always ship with a migration note (and for the
  file store, a format-version bump — the broker refuses mismatched files
  rather than guessing).
- **Releases** are `v*` tags. The tag triggers the full seven-cell ETSI
  matrix as the release gate plus the examples job; artifacts (multi-arch
  images, binaries, wasm bundle, SBOM) publish only on a green gate.
  CHANGELOG.md follows Keep a Changelog: maintain `[Unreleased]` per
  merge; the release moves it under the version heading.

## Sign-off (DCO)

Contributions use the [Developer Certificate of Origin](https://developercertificate.org/):
add `Signed-off-by: Your Name <email>` to each commit (`git commit -s`).
No CLA — the DCO plus the EUPL-1.2 inbound=outbound rule is the whole
agreement.
