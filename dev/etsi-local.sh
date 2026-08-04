#!/usr/bin/env bash
# The LOCAL gate: workspace tests + the SAME store × suite matrix CI runs
# (.github/workflows/etsi.yml etsi-cell job), but IN ORDER, one cell at a
# time — each cell gets its own fresh stack and results dir, then the same
# 4 per-store tables via dev/etsi-matrix-summary.py. Green only when ALL
# 32 cells are green.
#
#   STOP_ON_ERROR=1 (default)  halt at the FIRST failing TP, loop stops there
#   STOP_ON_ERROR=0            run the whole matrix, gate at the summary
set -euo pipefail
cd "$(dirname "$0")/.."

echo "=== workspace tests ==="
cargo test --workspace

STORES=(memory file postgres timescale)
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
echo "LOCAL GATE GREEN (all ${#STORES[@]}×${#SUITES[@]} cells)"
