#!/usr/bin/env bash
# Does the broker use the cores it is given? Pin the broker to 1, 2, 4, 8
# physical cores (SMT siblings excluded), keep the load generator on the
# remaining cores, and record req/s at c50.
#
#   dev/perf/core-scale.sh
#   SHAPE=update dev/perf/core-scale.sh
#
# Env: BIN (target/release/antares), OUT (results/perf), PORT (9472),
#      DURATION (5s), STORE (memory — the point is the broker, not the
#      database, so the store stays in-process), SHAPE (query; `update` is
#      the write shape, which is what takes the store's writer lock — the
#      two answer different questions and the tables are kept apart).
# Refuses a step where the load generator would share a core with the
# broker; the table stops at the largest step the box can isolate.
set -euo pipefail
cd "$(dirname "$0")/../.."
. dev/perf/probe.sh

BIN="${BIN:-target/release/antares}"
OUT="${OUT:-results/perf}"
PORT="${PORT:-9472}"
DURATION="${DURATION:-5s}"
STORE="${STORE:-memory}"
SHAPE="${SHAPE:-query}"
# one table per shape, so a second run never overwrites the first
SUFFIX=""; [ "$SHAPE" = query ] || SUFFIX="-$SHAPE"
mkdir -p "$OUT"
command -v k6 >/dev/null || { echo "k6 missing"; exit 1; }
[ -x "$BIN" ] || { echo "missing $BIN"; exit 1; }

# one CPU id per physical core: the first thread of each core
mapfile -t CORES < <(lscpu -p=CPU,CORE | grep -v '^#' | sort -t, -k2,2n -u | cut -d, -f1)
TOTAL=${#CORES[@]}
DATA=$(mktemp -d); trap 'rm -rf "$DATA"; [ -n "${PID:-}" ] && kill "$PID" 2>/dev/null' EXIT

{
  echo "| cores allotted | req/s | vs 1 core | efficiency | cores used | peak threads |"
  echo "|---|---|---|---|---|---|"
  base=
  for n in 1 2 4 8 16; do
    [ "$n" -le $(( TOTAL / 2 )) ] || { echo "| $n | (needs $((n * 2)) physical cores, box has $TOTAL) | | | | |"; break; }
    bset=$(IFS=,; echo "${CORES[*]:0:$n}")
    lset=$(IFS=,; echo "${CORES[*]:$n}")
    ANTARES_STORE="$STORE" ANTARES_DATA_DIR="$DATA" ANTARES_HTTP_PORT="$PORT" \
      taskset -c "$bset" "$BIN" > "$OUT/core-scale$SUFFIX-$n.log" 2>&1 & PID=$!
    until curl -sf -o /dev/null "http://127.0.0.1:$PORT/q/health"; do sleep 0.05; done
    probe_start "$PID"
    taskset -c "$lset" k6 run --quiet --summary-export "$OUT/core-scale$SUFFIX.json" \
      -e BROKER_URL="http://127.0.0.1:$PORT" -e SHAPE="$SHAPE" -e VUS=50 -e DURATION="$DURATION" \
      dev/perf/k6-shapes.js >/dev/null
    rps=$(python3 -c "import json; print(round(json.load(open('$OUT/core-scale$SUFFIX.json'))['metrics']['http_reqs']['rate']))")
    read -r used threads < <(probe_stop)
    kill "$PID"; wait "$PID" 2>/dev/null || true; PID=
    if [ -z "$base" ]; then base=$rps; echo "| $n | $rps | — | — | $used | $threads |"
    else python3 -c "r=$rps/$base; print(f'| $n | $rps | {r:.2f}x | {100*r/$n:.0f}% | $used | $threads |')"; fi
  done
} | tee "$OUT/core-scale$SUFFIX.md"
