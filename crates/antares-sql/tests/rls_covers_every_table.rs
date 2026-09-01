// SPDX-License-Identifier: EUPL-1.2
//! Row-level security is the backstop under every tenant-scoped table: the
//! request path already carries an explicit `tenant_id = $1` on every
//! statement, and the policy is what holds when one of them is written
//! without it.
//!
//! A backstop only covers the tables it names. `pg_rls_pentest` proves what
//! the policies DO, against a list written by hand — so a migration that adds
//! a table and forgets the policy adds a table nobody tests and nothing
//! polices, and both the list and the schema stay quietly green. This reads
//! the migrations instead: every table they create is either policed or
//! exempt for a stated reason, and an exempt table that no longer exists is
//! a stale exemption.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

/// Tables that are deliberately not tenant-scoped, and why.
const EXEMPT: &[(&str, &str)] = &[
    (
        "tenants",
        "the tenant inventory itself — it holds one row PER tenant, not rows \
         belonging to one, and the broker reads it across tenants to seed \
         mirrors and answer /q/tenants",
    ),
    (
        "jsonld_contexts",
        "5.13.1: Cached rows are copies of documents the broker fetched from \
         public URLs and belong to no tenant. The tenant-authored kinds \
         (Hosted, ImplicitlyCreated) carry their owner inside the row and are \
         filtered where they are served, listed and deleted",
    ),
    (
        "maintenance_jobs",
        "one row per internal job, no tenant column — the sweep leases it \
         with SELECT … FOR UPDATE SKIP LOCKED",
    ),
];

fn migrations_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("migrations")
}

fn migration_sql() -> String {
    let mut files: Vec<PathBuf> = std::fs::read_dir(migrations_dir())
        .expect("readable migrations directory")
        .map(|e| e.expect("directory entry").path())
        .filter(|p| p.extension().is_some_and(|e| e == "sql"))
        .collect();
    files.sort();
    assert!(files.len() >= 2, "found {} migrations", files.len());
    files
        .iter()
        .map(|p| std::fs::read_to_string(p).expect("readable migration"))
        .collect::<Vec<_>>()
        .join("\n")
}

/// Every identifier following `kw` in `sql`, lowercased, with `IF NOT EXISTS`
/// / `IF EXISTS` skipped and quoting stripped.
fn named_after(sql: &str, kw: &str) -> Vec<String> {
    let lower = sql.to_lowercase();
    let mut out = Vec::new();
    let mut from = 0;
    while let Some(i) = lower[from..].find(kw) {
        let at = from + i + kw.len();
        from = at;
        let rest = lower[at..].trim_start();
        let rest = rest
            .strip_prefix("if not exists")
            .or_else(|| rest.strip_prefix("if exists"))
            .unwrap_or(rest)
            .trim_start();
        let name: String = rest
            .chars()
            .take_while(|c| c.is_ascii_alphanumeric() || *c == '_')
            .collect();
        if !name.is_empty() {
            out.push(name);
        }
    }
    out
}

/// The tables the migrations leave behind: created, minus dropped, minus the
/// partitions (a partition takes its policies from the parent when it is
/// reached through the parent, which is how the request path reaches it).
fn live_tables(sql: &str) -> BTreeSet<String> {
    let mut created: BTreeSet<String> = named_after(sql, "create table").into_iter().collect();
    for dropped in named_after(sql, "drop table") {
        created.remove(&dropped);
    }
    created.retain(|t| !t.ends_with("_default"));
    created
}

/// The tables a policy is put on: the literal
/// `ALTER TABLE x ENABLE ROW LEVEL SECURITY` statements, plus the
/// `FOREACH t IN ARRAY ARRAY[…]` loop that writes most of them through
/// `format('ALTER TABLE %I …')` — where the name is the loop variable and
/// only the array literal carries it.
fn policed(sql: &str) -> BTreeSet<String> {
    const ENABLE: &str = "enable row level security";
    let lower = sql.to_lowercase();
    let mut out = BTreeSet::new();
    for (i, kw) in lower.match_indices("alter table") {
        let rest = lower[i + kw.len()..].trim_start();
        let name: String = rest
            .chars()
            .take_while(|c| c.is_ascii_alphanumeric() || *c == '_')
            .collect();
        // the statement, up to its terminator — a later ALTER on the same
        // table must not lend this one a policy it does not have
        let stmt = &rest[..rest.find(';').unwrap_or(rest.len())];
        if !name.is_empty() && stmt.contains(ENABLE) {
            out.insert(name);
        }
    }
    for (i, kw) in lower.match_indices("array[") {
        let Some(end) = lower[i..].find(']') else {
            continue;
        };
        if !lower[i + end..].contains(ENABLE) {
            continue;
        }
        for part in lower[i + kw.len()..i + end].split(',') {
            let name = part.trim().trim_matches('\'').trim();
            if !name.is_empty() {
                out.insert(name.to_owned());
            }
        }
    }
    out
}

#[test]
fn every_table_the_migrations_create_is_policed_or_exempt() {
    let sql = migration_sql();
    let live = live_tables(&sql);
    assert!(
        live.contains("entities") && live.contains("dead_letters"),
        "the parse found {live:?}, which is not the schema"
    );
    let policed = policed(&sql);
    let exempt: BTreeSet<&str> = EXEMPT.iter().map(|(t, _)| *t).collect();

    let unpoliced: Vec<&String> = live
        .iter()
        .filter(|t| !policed.contains(*t) && !exempt.contains(t.as_str()))
        .collect();
    assert!(
        unpoliced.is_empty(),
        "these tables carry tenant rows with no row-level-security policy and \
         no stated exemption: {unpoliced:?}"
    );
}

/// An exemption for a table that is gone is an exemption nobody re-examined.
#[test]
fn no_exemption_outlives_its_table() {
    let live = live_tables(&migration_sql());
    for (table, reason) in EXEMPT {
        assert!(
            live.contains(*table),
            "{table} is exempt from row-level security ({reason}) but the \
             migrations no longer create it"
        );
    }
}

/// The pentest drives its checks from a hand-written list of policed tables.
/// A table gaining a policy without gaining a case there is a policy nobody
/// exercises, so the two lists have to agree.
#[test]
fn the_pentest_exercises_every_policed_table() {
    let sql = migration_sql();
    let policed = policed(&sql);
    let live = live_tables(&sql);
    let pentest = std::fs::read_to_string(
        Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/pg_rls_pentest.rs"),
    )
    .expect("readable pentest");
    let missing: Vec<&String> = policed
        .iter()
        .filter(|t| live.contains(*t))
        .filter(|t| !pentest.contains(&format!("\"{t}\"")))
        .collect();
    assert!(
        missing.is_empty(),
        "policed but never driven by pg_rls_pentest: {missing:?}"
    );
}
