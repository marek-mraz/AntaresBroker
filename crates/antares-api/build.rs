// SPDX-License-Identifier: EUPL-1.2
// Bake the short git hash in at build time so
// /q/health and --version can report it. "unknown" outside a git checkout
// (e.g. a source tarball) — never a build failure.
fn main() {
    let hash = std::process::Command::new("git")
        .args(["rev-parse", "--short", "HEAD"])
        .output()
        .ok()
        .filter(|o| o.status.success())
        .and_then(|o| String::from_utf8(o.stdout).ok())
        .map(|s| s.trim().to_owned())
        .unwrap_or_else(|| "unknown".into());
    println!("cargo:rustc-env=ANTARES_GIT_HASH={hash}");
    // A commit on the current branch rewrites the ref HEAD names, not HEAD
    // itself, so watching HEAD alone bakes in whatever hash was current the
    // last time this script ran. Watch the ref too. When it does not exist
    // as a file — packed refs, or a worktree where ../../.git is a file —
    // cargo treats it as always changed and reruns this script every build,
    // which keeps the reported commit honest for one `git rev-parse`.
    println!("cargo:rerun-if-changed=../../.git/HEAD");
    if let Ok(head) = std::fs::read_to_string("../../.git/HEAD") {
        if let Some(git_ref) = head.strip_prefix("ref: ") {
            println!("cargo:rerun-if-changed=../../.git/{}", git_ref.trim());
        }
    }
}
