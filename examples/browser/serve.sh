#!/usr/bin/env bash
# Serve the playground (the broker runs INSIDE the page via a Service
# Worker). Build the wasm artifact first if www/pkg is missing.
set -euo pipefail
cd "$(dirname "$0")/../.."
[ -f www/pkg/antares_wasm_bg.wasm ] || { ./dev/install-wasm-tools.sh; ./dev/wasm-build.sh; }
echo "open http://localhost:${1:-8000}/ — entities live in your tab only"
exec python3 -m http.server "${1:-8000}" -d www
