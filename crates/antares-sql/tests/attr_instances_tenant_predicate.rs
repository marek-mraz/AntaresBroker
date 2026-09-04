// SPDX-License-Identifier: EUPL-1.2
//! `attr_instances` is the one tenant-bearing table that runs without a
//! row-security policy in timescale mode: TimescaleDB refuses columnstore
//! compression on a table with RLS, and ADR-0006 decided compression wins
//! there. What is left holding tenants apart on that table is the explicit
//! `tenant_id` predicate every request-path statement carries — a discipline
//! nothing enforces once the belt is off, and one statement written without
//! it reads another tenant's history with no error anywhere.
//!
//! So the discipline is read out of the source: every SQL literal in the
//! Postgres store that selects, inserts into, updates or deletes from
//! `attr_instances` either compares `tenant_id`, or is named below with the
//! reason it does not. A new statement is neither until someone says which.
//!
//! ponytail: plain string scanning over `#[cfg(test)]`-free source, not a
//! Rust parser — it reads ordinary `"…"` literals and would miss a raw
//! string or a table name assembled at runtime. Both are absent here, and
//! `EXEMPT` is what a future one would have to be added to.

use std::path::{Path, PathBuf};

/// Statements that touch the table across tenants on purpose, and why. Each
/// entry is a fragment that appears in exactly one literal.
const EXEMPT: &[(&str, &str)] = &[
    (
        "WHERE try_timestamptz(data->>'expiresAt') < now()",
        "the 4.22 expired-instance reap: cross-tenant work under the service \
         role, which is what lets it collect every tenant's expired rows in \
         one statement",
    ),
    (
        "DELETE FROM attr_instances_default\n         WHERE observed_at < now()",
        "the ANTARES_TEMPORAL_RETENTION_DAYS purge of the DEFAULT partition: \
         a horizon is a property of the deployment, not of a tenant",
    ),
    (
        "WITH moved AS (DELETE FROM attr_instances_default",
        "partition adoption moves a whole week out of DEFAULT so the ATTACH \
         revalidates; a tenant predicate would strand the other tenants' rows \
         there and the ATTACH would fail",
    ),
];

fn store_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("src/store")
}

fn sources(dir: &Path, out: &mut Vec<PathBuf>) {
    for entry in std::fs::read_dir(dir).expect("read store dir") {
        let path = entry.expect("dir entry").path();
        if path.is_dir() {
            sources(&path, out);
        } else if path.extension().is_some_and(|e| e == "rs") {
            out.push(path);
        }
    }
}

/// Every `"…"` literal in `src`, with `\"` respected. Runs over the source
/// ahead of the first `#[cfg(test)]` only: an assertion about a statement is
/// not a statement.
fn literals(src: &str) -> Vec<String> {
    let src = match src.find("#[cfg(test)]") {
        Some(at) => &src[..at],
        None => src,
    };
    let mut out = Vec::new();
    let bytes: Vec<char> = src.chars().collect();
    let mut i = 0;
    while i < bytes.len() {
        // a `//` comment can hold an unbalanced quote; skip the line
        if bytes[i] == '/' && bytes.get(i + 1) == Some(&'/') {
            while i < bytes.len() && bytes[i] != '\n' {
                i += 1;
            }
            continue;
        }
        if bytes[i] != '"' {
            i += 1;
            continue;
        }
        let mut lit = String::new();
        i += 1;
        while i < bytes.len() && bytes[i] != '"' {
            if bytes[i] == '\\' {
                i += 1;
                if i >= bytes.len() {
                    break;
                }
            }
            lit.push(bytes[i]);
            i += 1;
        }
        i += 1;
        out.push(lit);
    }
    out
}

/// Does this literal read or write the table, as opposed to naming it? The
/// catalog lookups (`relname = 'attr_instances'`), the diagnostics and the
/// partition DDL name it without addressing its rows.
fn touches_rows(lit: &str) -> bool {
    [
        "FROM attr_instances",
        "INTO attr_instances",
        "UPDATE attr_instances",
    ]
    .iter()
    .any(|k| lit.contains(k))
}

/// Where the tenant has to appear. A statement that narrows rows says so in
/// a predicate, so `tenant_id` has to be at or after the first `WHERE` — a
/// mention in a SELECT list or a GROUP BY is not isolation. A statement with
/// no `WHERE` narrows nothing and only writes: there the tenant is a column
/// it fills, which is where the INSERT names it.
///
/// ponytail: this reads WHERE the name appears, not what it is compared to,
/// so a statement that mentions `tenant_id` in a predicate about some OTHER
/// table would pass. The failure it is built to catch is the one that
/// actually happens — a statement written with no tenant in it at all.
fn names_its_tenant(lit: &str) -> bool {
    match lit.find("WHERE") {
        Some(at) => lit[at..].contains("tenant_id"),
        None => lit.contains("tenant_id"),
    }
}

#[test]
fn every_statement_on_attr_instances_carries_its_tenant() {
    let mut files = Vec::new();
    sources(&store_dir(), &mut files);
    assert!(!files.is_empty(), "no store sources found");
    let mut naked = Vec::new();
    let mut used = vec![false; EXEMPT.len()];
    for file in &files {
        let src = std::fs::read_to_string(file).expect("read source");
        for lit in literals(&src) {
            if !touches_rows(&lit) {
                continue;
            }
            if let Some(i) = EXEMPT.iter().position(|(frag, _)| lit.contains(frag)) {
                used[i] = true;
                continue;
            }
            if !names_its_tenant(&lit) {
                naked.push(format!("{}: {lit}", file.display()));
            }
        }
    }
    assert!(
        naked.is_empty(),
        "attr_instances statements with no tenant predicate and no written \
         exemption — in timescale mode nothing else keeps tenants apart \
         (ADR-0006):\n{}",
        naked.join("\n\n")
    );
    for (i, (frag, _)) in EXEMPT.iter().enumerate() {
        assert!(
            used[i],
            "stale exemption: no statement contains {frag:?} any more"
        );
    }
}
