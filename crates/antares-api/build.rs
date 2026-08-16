// E5 version surface: bake the short git hash in at build time so
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
    println!("cargo:rerun-if-changed=../../.git/HEAD");
}
