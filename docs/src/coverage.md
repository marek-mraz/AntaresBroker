# Coverage

Two jobs measure which broker code no test executes; both publish
rather than gate, except for the unit floor.

| job | what runs under instrumentation | where the numbers land |
|---|---|---|
| `strict` → Coverage floor (daily) | the workspace unit and integration tests, with live PostGIS, MQTT and NATS so the integration tests count instead of skipping | Two floors, never one blended number: the unit surface (`--lib --bins`, less two things no in-crate test can execute — the `test-kit`-gated store contract kit, and the PostgreSQL driver, which needs a live database and a multi-thread runtime where an in-crate `#[tokio::test]` is current-thread; both are measured by the API surface instead) at 82 % lines / 79 % functions and the API surface (the integration binaries of `antares-api`, `antares-broker`, `antares-bus`, `antares-sql`) at 78 % lines / 77 % functions. Gating each test source separately is what stops one rising while the other falls. Floors only ratchet up after a green run — never lowered to make a red one pass. The run also publishes a `coverage-map` artifact — the two summaries and an `lcov` file per surface — so the uncovered lines can be read by name; the artifact's totals count every object left in the coverage map and are not the gated figures. |
| `etsi-coverage` (weekly, per store) | the workspace tests **and** the whole Robot suite against an instrumented broker, once per store mode (memory, file, postgres, timescale), then merged | [`/reports/coverage/`](https://antares-ngsi-ld-demo.marek-mraz.com/reports/coverage/) with the per-store and merged HTML, the badge on the README, and the step summary's `lcov --list` table |

A zero-count line in the merged view means no Rust test and no ETSI test
procedure in any store mode ever ran it. `dev/coverage-attribution.py`
splits the merged profile into lines only the unit tests reach, lines only
the Robot suite reaches, and lines both reach, so a clause whose only
witness is a unit test shows up as such (the [spec-statement
table](conformance.md#spec-statement-coverage) is the clause-level view
of the same question).

The merged table's function columns count *source* functions, not compiled
ones. An lcov tracefile names functions mangled, so one generic appears once
per instantiation and one closure once per test binary that linked it — and a
binary that never linked an instantiation records it as a miss. Counted that
way the workspace has about twice as many functions as it has, half of them
permanently uncovered, and the merged figure cannot be compared with the
floors above. The table therefore keys a function by the start line of its
`FN` record, which reproduces what `cargo llvm-cov --summary-only` reports to
within a point. `dev/coverage-attribution.py --selftest` pins that, and
`workspace.yml` runs it.

## Reading the uncovered lines

Uncovered code falls into two kinds, and they call for different work:

- **Reachable but untested**: a request shape no test sends yet (an
  optional URL parameter, a rarely combined pair of options, a tenant
  header on an admin route). The fix is a test, usually a Robot TP with
  the clause tag so the ledger picks it up.
- **Needs fault injection**: the error arms behind a store that fails
  mid-transaction, a peer that answers with a truncated body, a
  notification endpoint that hangs. No request from the outside reaches
  them on a healthy stack; they need a failing dependency (the
  `nats_e2e` and `mqtt_notify` tests do this for their subsystems) or a
  mock that misbehaves (`federation` tests use one).

The weekly ratchet fails the job on a drop of more than one point against
the published run, so a change that removes a test's reach is visible
before the badge moves.

## Reproducing locally

```bash
dev/etsi-coverage.sh memory        # one store mode, same script CI runs
cargo llvm-cov --workspace --html   # the unit half only, target/llvm-cov/html
```

Both need `cargo-llvm-cov` and the `llvm-tools-preview` component.
