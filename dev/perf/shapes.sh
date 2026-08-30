#!/usr/bin/env bash
# Throughput table for the published read shapes: query at c50 and c200,
# single-entity retrieve at c50, median of three 5 s runs, p99 from the
# same runs. One broker, started here per store. The write shapes are
# writes.sh.
#
#   dev/perf/shapes.sh                          # memory
#   DATABASE_URL=postgres://… dev/perf/shapes.sh   # + postgres
#
# Env: BIN (target/release/antares), OUT (results/perf), PORT (9471),
#      DATABASE_URL, RUNS (3), DURATION (5s).
set -euo pipefail
cd "$(dirname "$0")/../.."

BIN="${BIN:-target/release/antares}"
OUT="${OUT:-results/perf}"
PORT="${PORT:-9471}"
RUNS="${RUNS:-3}"
DURATION="${DURATION:-5s}"
mkdir -p "$OUT"
command -v k6 >/dev/null || { echo "k6 missing"; exit 1; }
[ -x "$BIN" ] || { echo "missing $BIN"; exit 1; }
DATA=$(mktemp -d); trap 'rm -rf "$DATA"; [ -n "${PID:-}" ] && kill "$PID" 2>/dev/null' EXIT

run() {  # run <shape> <vus>: prints "<req/s> <p99 ms>" (median over RUNS by req/s)
  local rows=()
  for _ in $(seq "$RUNS"); do
    k6 run --quiet --summary-export "$OUT/shape.json" -e BROKER_URL="http://127.0.0.1:$PORT" \
      -e SHAPE="$1" -e VUS="$2" -e DURATION="$DURATION" dev/perf/k6-shapes.js >/dev/null
    rows+=("$(python3 -c "
import json; s=json.load(open('$OUT/shape.json'))['metrics']
print(f\"{s['http_reqs']['rate']:.0f} {s['http_req_duration']['p(99)']:.2f}\")")")
  done
  printf '%s\n' "${rows[@]}" | sort -n | sed -n "$(( (RUNS + 1) / 2 ))p"
}

{
  echo "| store | shape | concurrency | req/s | p99 |"
  echo "|---|---|---|---|---|"
  for store in memory ${DATABASE_URL:+postgres}; do
    ANTARES_STORE="$store" ANTARES_DATA_DIR="$DATA" ANTARES_HTTP_PORT="$PORT" \
      ANTARES_DATABASE_URL="${DATABASE_URL:-}" ANTARES_ALLOW_SHARED_LOCAL=1 \
      "$BIN" > "$OUT/shapes-$store.log" 2>&1 & PID=$!
    until curl -sf -o /dev/null "http://127.0.0.1:$PORT/q/health"; do sleep 0.05; done
    for spec in "query 50" "query 200" "retrieve 50"; do
      echo "shapes $store ${spec// /-c}" > "$OUT/phase"
      read -r shape vus <<<"$spec"
      read -r rps p99 < <(run "$shape" "$vus")
      echo "| $store | $shape | c$vus | $rps | $p99 ms |"
    done
    kill "$PID"; wait "$PID" 2>/dev/null || true; PID=
  done
} | tee "$OUT/shapes.md"
