#!/usr/bin/env python3
"""Target-specific dependency versions must equal the workspace ones.

`[workspace.dependencies]` is the single place a version is written. A
`[target.'cfg(target_arch = "wasm32")'.dependencies]` section cannot use
`workspace = true`: cargo features are additive, so a member can neither drop
a workspace feature (`tokio`'s `net`/`signal`, `reqwest`'s `rustls-tls`) nor
turn defaults off when the workspace entry left them on (`axum`). Those
sections therefore spell a version out, and nothing keeps the two in step —
a workspace bump leaves the browser build on the old major, and the mismatch
surfaces as a wasm-only build failure long after the bump.

This asserts the pair equal. A dependency the workspace does not name at all
is a violation too: it would be a version this file cannot govern.
"""

import pathlib
import sys
import tomllib


def version(spec):
    """The version string of a dependency entry, or None when it names none."""
    return spec if isinstance(spec, str) else spec.get("version")


def main() -> int:
    root = pathlib.Path(__file__).resolve().parent.parent
    workspace = tomllib.loads((root / "Cargo.toml").read_text())["workspace"]["dependencies"]
    violations = []
    checked = 0
    for manifest in sorted((root / "crates").glob("*/Cargo.toml")):
        member = tomllib.loads(manifest.read_text())
        for target, table in member.get("target", {}).items():
            for name, spec in table.get("dependencies", {}).items():
                pinned = version(spec)
                if pinned is None:  # `workspace = true`, or a path dependency
                    continue
                checked += 1
                where = f"{manifest.relative_to(root)} [target.'{target}'] {name}"
                if name not in workspace:
                    violations.append(f"{where} = {pinned!r}, but the workspace names no {name}")
                elif version(workspace[name]) != pinned:
                    violations.append(
                        f"{where} = {pinned!r}, workspace has {version(workspace[name])!r}"
                    )
    for v in violations:
        print(f"wasm pin check: {v}")
    if violations:
        print(f"wasm pin check: {len(violations)} violations")
        return 1
    print(f"wasm pin check: OK ({checked} target-specific pins equal the workspace)")
    return 0


if __name__ == "__main__":
    sys.exit(main())
