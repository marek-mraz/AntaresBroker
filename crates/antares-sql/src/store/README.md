# Storage backends

One folder per backend: `mem/` (in-RAM maps, plus `mem/redb.rs` for the
`file` mode write-through shadow) and `pg/` (Postgres and TimescaleDB:
pool, migrations, tenancy, and one file per resource family). `any.rs` is
`AnyStore`, the dispatcher the broker holds behind the driver traits.

To add a backend: copy `mem/`, implement `CurrentStateDriver` and
`TemporalDriver` (`crates/antares-store/src/lib.rs:139` and `:384`), add an
`AnyStore` arm (`any.rs`), a `StoreMode` value (`crates/antares-store/src/lib.rs`), the
`build_store` arm in `crates/antares-broker/src/main.rs:380` and, if the
backend needs background jobs, an arm next to the sweep (`main.rs:617`) or
the maintenance job (`main.rs:638`). Finish with a cell in the ETSI matrix
(`.github/workflows/etsi-matrix.yml`).
