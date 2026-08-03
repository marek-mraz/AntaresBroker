#!/usr/bin/env bash
# Build + run the broker locally (release for realistic memory numbers).
set -euo pipefail
cd "$(dirname "$0")/.."
cargo build --release -p antares-broker
exec ./target/release/antares
