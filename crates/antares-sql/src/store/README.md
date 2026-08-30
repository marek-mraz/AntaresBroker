# Storage backends

One folder per backend: `mem/` (in-RAM maps, plus `mem/redb.rs` for the
`file` mode write-through shadow) and `pg/` (Postgres and TimescaleDB:
pool, migrations, tenancy, and one file per resource family). `any.rs` is
`AnyStore`, the dispatcher the broker holds behind the driver traits.

This folder holds the backends that ship IN the binary. A backend does not
have to live here: the two driver traits are the whole contract, and
`examples/plugin-example` implements them from outside the workspace, with
`docs/src/extending.md` as the procedure.

To add one here: copy `mem/`, implement `CurrentStateDriver` and
`TemporalDriver` (`crates/antares-store/src/lib.rs`), add an `AnyStore` arm
(`any.rs`), a `StoreMode` value (`crates/antares-store/src/lib.rs`) and the
`build_builtin` arm in `crates/antares-broker/src/main.rs`; if the backend
needs background jobs, an arm next to the sweep or the maintenance job in
the same file. Hold it to `antares_store::contract` (the `test-kit`
feature) and finish with a cell in the ETSI matrix
(`.github/workflows/etsi-matrix.yml`).
