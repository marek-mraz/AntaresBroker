# ADR-0004: Store ladder, redb as the `file`-mode durability shadow

Date: 2026-08-04 · Status: accepted

Store ladder `memory → file → postgres → timescale`: same binary, same
compose, same ETSI pipeline, one config value (`ANTARES_STORE`). `file` is
not a second store implementation — it is the in-memory store plus a redb
write-through shadow, so `memory` costs no duplicate code and stays the
unit-test default, the control mode, and the read-only-rootfs mode.

Mechanics: one redb table per resource family (spec names), key
`tenant\0id` (TenantId is token-safe, cannot contain `\0`), value = expanded
JSON bytes. Commits use `Durability::Immediate` and run INSIDE the store's
write-critical section — redb apply order equals memory apply order, and the
fsync completes before the store call returns, i.e. before the HTTP ack
(**commit-before-ack**; proven by the `kill -9`-after-201 e2e). Commits run
under `block_in_place` so the fsync never stalls a tokio worker. A
failed commit aborts the process: acking a write the file does not hold is
the one lie a durable store must never tell. Boot rebuilds the maps from the
file and refuses to start on a format-version mismatch or corruption.
Backup is stop-copy only — redb holds an exclusive file lock and a live copy
can tear mid-commit (tested).

Measured (dev box, 1.5 KB entities): ~3,127 fsynced writes/s, commit p50
0.21 ms / p99 0.85 ms vs 407k/s memory-only — the cost IS the fsync barrier
(raw 4 KB write+fsync p50 0.35 ms on the same fs). Single-writer redb makes
this a per-process ceiling; group-commit is the documented lever, not the
default.

SQLite rejected: redb is pure Rust (no C build, `unsafe_code=forbid`
workspace posture), typed tables, copy-on-write MVCC with no WAL to manage,
and this use is a KV shadow — SQL adds surface without adding value here.
Rolling updates in `file` mode are Recreate-only (exclusive lock, one broker
per volume); HA needs `postgres` mode.

## Confirmation

`crates/antares-broker/tests/file_mode.rs` (redb durability across `kill -9`, single-process lock) and `store_combos.rs` (every store × temporal pairing); the file cell of the ETSI matrix.
