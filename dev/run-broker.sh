#!/usr/bin/env bash
# Build + run the broker locally (release for realistic memory numbers).
set -euo pipefail
cd "$(dirname "$0")/.."
cargo build --release -p antares-broker
# a shared build.target-dir puts the binary outside the repository
TARGET=$(cargo metadata --format-version 1 --no-deps | python3 -c 'import json,sys;print(json.load(sys.stdin)["target_directory"])')
exec "$TARGET/release/antares"
