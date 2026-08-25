#!/usr/bin/env bash
# Read-only code x-ray over the workspace: symbol index, module/crate graphs,
# per-function complexity, lint + unused-dependency sweep, and the radar that
# ranks functions by complexity x churn x (missing) coverage. Nothing here
# touches broker code; every output lands under results/x-ray/.
#
#   COVERAGE=path/to/coverage.json   optional llvm-cov export (etsi-coverage.sh
#                                    artifact) — without it the radar ranks on
#                                    complexity x churn only
set -uo pipefail
cd "$(dirname "$0")/.."
export PATH="$HOME/.cargo/bin:$PATH"
OUT=results/x-ray
mkdir -p "$OUT"

echo "== symbol index (SCIP) =="
rust-analyzer scip . --output "$OUT/index.scip" 2>"$OUT/scip.log" \
  && echo "  $OUT/index.scip" || echo "  rust-analyzer scip failed, see $OUT/scip.log"

echo "== structure + dependency graphs =="
cargo depgraph --workspace-only > "$OUT/crates.dot"
for dir in crates/*/; do
  crate=$(basename "$dir")
  # a lib crate, else the bin the manifest names (antares-broker builds `antares`)
  bin=$(sed -n '/^\[\[bin\]\]/,/^name/{s/^name = "\(.*\)"/\1/p}' "$dir/Cargo.toml" | head -1)
  cargo modules structure -p "$crate" --lib > "$OUT/structure-$crate.txt" 2>/dev/null \
    || cargo modules structure -p "$crate" --bin "${bin:-$crate}" > "$OUT/structure-$crate.txt" 2>/dev/null
  cargo modules dependencies -p "$crate" --lib > "$OUT/deps-$crate.dot" 2>/dev/null \
    || cargo modules dependencies -p "$crate" --bin "${bin:-$crate}" > "$OUT/deps-$crate.dot" 2>/dev/null
done

echo "== per-function complexity =="
# lizard: NLOC, CCN, tokens, params, length, location, file, function, signature, start, end
.venv/bin/lizard -l rust --csv crates > "$OUT/complexity.csv"

echo "== lint sweep (report, not a gate) =="
cargo clippy --workspace --all-targets --message-format short \
  -- -W clippy::pedantic -W clippy::nursery > "$OUT/clippy-pedantic.txt" 2>&1 || true
cargo machete > "$OUT/unused-deps.txt" 2>&1 || true
cargo build --workspace --message-format short 2>&1 | /usr/bin/grep -a "dead_code\|never used\|never read" \
  > "$OUT/dead-surface.txt" || true

echo "== radar =="
python3 dev/code-radar.py "$OUT/complexity.csv" "${COVERAGE:-}" > "$OUT/code-radar.txt"
head -40 "$OUT/code-radar.txt"
