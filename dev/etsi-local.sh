#!/usr/bin/env bash
# The LOCAL gate: workspace tests + the SAME suite cells CI runs
# (.github/workflows/etsi.yml etsi-cell job), IN ORDER, one cell at a time —
# each cell gets its own fresh stack and results dir, then the per-store
# table via dev/etsi-matrix-summary.py.
#
# ONE store mode per local run — the one you are touching. A local box runs
# the cells serially, so all four modes cost ~4× wall-clock for a signal CI
# already produces: CI fans the full 4 × 8 matrix out in PARALLEL on every
# push and is the authority. Locally you want the fast loop, not the matrix.
#
#   STORE=memory (default)     the mode under test; also file|postgres|timescale
#   STORE=all                  the full 32-cell matrix (rarely worth it locally)
#   STOP_ON_ERROR=1 (default)  halt at the FIRST failing TP, loop stops there
#   STOP_ON_ERROR=0            run every cell, gate at the summary
set -euo pipefail
cd "$(dirname "$0")/.."

echo "=== workspace tests ==="
cargo test --workspace

case "${STORE:-memory}" in
  all) STORES=(memory file postgres timescale) ;;
  *)   STORES=("${STORE:-memory}") ;;
esac
SUITES=(CommonBehaviours Consumption Provision Subscription
        ContextSource jsonldContext DistributedOperations IOP)
CELLS=results/cells
rm -rf "$CELLS"

echo "=== build image under test ==="
docker build -t antares-local:latest .

for store in "${STORES[@]}"; do
  for suite in "${SUITES[@]}"; do
    echo "=== ETSI $store × $suite ==="
    STORE=$store SUITES=$suite SKIP_BUILD=1 \
    RESULTS_DIR="$CELLS/ETSI-cell-$store-$suite" \
    STOP_ON_ERROR="${STOP_ON_ERROR:-1}" \
      dev/etsi-pipeline.sh
  done
done

echo "=== matrix summary ==="
dev/etsi-matrix-summary.py "$CELLS"
echo "LOCAL GATE GREEN (${STORES[*]} × ${#SUITES[@]} suites) — CI gates all four modes in parallel"
