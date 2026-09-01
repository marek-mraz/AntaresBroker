// SPDX-License-Identifier: EUPL-1.2
//! The `antares.service` escape of `0001_init.sql` is the one place row-level
//! security is set aside: with it armed, a transaction reads `entities`,
//! `outbox` and `attr_instances` across every tenant. `pg_rls_pentest`
//! proves what the database grants; this proves who in the broker is allowed
//! to ask for it.
//!
//! Two internal jobs are cross-tenant by nature — the outbox drain and the
//! temporal retention sweep — and nothing else may arm it. A request path
//! that did would serve one tenant another tenant's entities, silently and
//! with a correct-looking `tenant_id` predicate nowhere in sight. The rule
//! lives in a doc comment on `set_service`; a doc comment is not a gate, so
//! this is the gate.

use std::path::{Path, PathBuf};

/// Files allowed to call `set_service`, relative to the crate source root.
const CALLERS: &[&str] = &["store/pg/maintenance.rs", "store/pg/outbox.rs"];

/// Where the function itself is defined — its own body and doc comment
/// name it without calling it.
const DEFINITION: &str = "store/pg/mod.rs";

fn src_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("src")
}

fn rust_files(dir: &Path, out: &mut Vec<PathBuf>) {
    for entry in std::fs::read_dir(dir).expect("readable source directory") {
        let path = entry.expect("directory entry").path();
        if path.is_dir() {
            rust_files(&path, out);
        } else if path.extension().is_some_and(|e| e == "rs") {
            out.push(path);
        }
    }
}

#[test]
fn only_the_two_cross_tenant_jobs_arm_the_service_escape() {
    let root = src_root();
    let mut files = Vec::new();
    rust_files(&root, &mut files);
    assert!(
        files.len() > 10,
        "the walk found {} source files, so it is not walking the crate",
        files.len()
    );

    let mut callers: Vec<String> = Vec::new();
    for path in &files {
        let rel = path
            .strip_prefix(&root)
            .expect("under the source root")
            .to_string_lossy()
            .replace('\\', "/");
        if rel == DEFINITION {
            continue;
        }
        let src = std::fs::read_to_string(path).expect("readable source file");
        if src.contains("set_service(") {
            callers.push(rel);
        }
    }
    callers.sort();

    let mut allowed: Vec<String> = CALLERS.iter().map(|s| (*s).to_owned()).collect();
    allowed.sort();
    assert_eq!(
        callers, allowed,
        "a file outside the outbox drain and the retention sweep arms the \
         cross-tenant RLS escape"
    );
}

/// The other half of the same rule: the escape is transaction-scoped, so it
/// has to be `set_config(…, true)`. A session-scoped arming (`false`) would
/// outlive the job on a pooled connection and hand the escape to whatever
/// request picks that connection up next.
#[test]
fn the_escape_is_transaction_scoped() {
    let def = std::fs::read_to_string(src_root().join(DEFINITION)).expect("readable");
    let line = def
        .lines()
        .find(|l| l.contains("set_config('antares.service'"))
        .expect("set_service arms the GUC through set_config");
    assert!(
        line.contains(", true)"),
        "the escape must be transaction-scoped (set_config local = true): {line}"
    );
    // the tenant GUC every request path rides on is armed the same way, for
    // the same reason: residue on a recycled connection is the whole risk
    assert!(
        antares_sql::SET_TENANT_SQL.contains(", true)"),
        "the tenant GUC must be transaction-scoped: {}",
        antares_sql::SET_TENANT_SQL
    );
}
