#!/usr/bin/env bash
# Build the browser artifact — cargo (wasm-release profile) → wasm-bindgen
# (--target web: one ES module usable from a page, a module Service Worker,
# and Node ≥18) → wasm-opt -Oz → size gate.
#
# Budgets: raw .wasm ≤ 8 MB, gzip -9 ≤ 3 MB. The script FAILS
# when a budget is blown, and prints sizes for the CI summary either way.
#
# Tools: wasm-bindgen must match Cargo.lock's wasm-bindgen version; wasm-opt
# from binaryen. dev/install-tools.sh fetches both prebuilt.
set -euo pipefail
cd "$(dirname "$0")/.."

OUT="${OUT:-www/pkg}"
RAW_BUDGET=$((8 * 1024 * 1024))
GZ_BUDGET=$((3 * 1024 * 1024))

cargo build --profile wasm-release --target wasm32-unknown-unknown -p antares-wasm

wasm-bindgen --target web --no-typescript \
  --out-dir "$OUT" \
  target/wasm32-unknown-unknown/wasm-release/antares_wasm.wasm

wasm-opt -Oz --enable-bulk-memory --enable-nontrapping-float-to-int \
  -o "$OUT/antares_wasm_bg.wasm" "$OUT/antares_wasm_bg.wasm"

RAW=$(stat -c%s "$OUT/antares_wasm_bg.wasm")
GZ=$(gzip -9 -c "$OUT/antares_wasm_bg.wasm" | wc -c)
printf 'antares_wasm_bg.wasm  raw %s bytes (budget %s)  gzip-9 %s bytes (budget %s)\n' \
  "$RAW" "$RAW_BUDGET" "$GZ" "$GZ_BUDGET"

fail=0
[ "$RAW" -le "$RAW_BUDGET" ] || { echo "FAIL: raw size over the 8 MB budget" >&2; fail=1; }
[ "$GZ" -le "$GZ_BUDGET" ] || { echo "FAIL: gzip size over the 3 MB budget" >&2; fail=1; }
exit "$fail"
