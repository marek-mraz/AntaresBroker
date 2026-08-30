#!/usr/bin/env bash
# Write throughput, one shape at a time. Each cell is its own k6 run
# against a quiet broker, so a row is that shape alone: median of three
# 5 s runs by req/s, p99 from the same runs, at 50 and 200 concurrent
# clients. The set the writes hit carries its own entity type, so no
# subscription of a loaded tenant matches it.
#
#   dev/perf/writes.sh                              # memory
#   DATABASE_URL=postgres://… dev/perf/writes.sh    # + postgres
#   TENANT=t7 DATABASE_URL=… dev/perf/writes.sh     # inside a loaded tenant
#
# Env: BIN (target/release/antares), OUT (results/perf), PORT (9475),
#      DATABASE_URL, TENANT, RUNS (3), DURATION (5s), VUS ("50 200"),
#      STORES (memory, plus postgres when DATABASE_URL is set).
set -euo pipefail
cd "$(dirname "$0")/../.."

BIN="${BIN:-target/release/antares}"
OUT="${OUT:-results/perf}"
PORT="${PORT:-9475}"
RUNS="${RUNS:-3}"
DURATION="${DURATION:-5s}"
VUS="${VUS:-50 200}"
SHAPES="${SHAPES:-update partial merge replace append create upsert20}"
STORES="${STORES:-memory ${DATABASE_URL:+postgres}}"
mkdir -p "$OUT"
command -v k6 >/dev/null || { echo "k6 missing"; exit 1; }
[ -x "$BIN" ] || { echo "missing $BIN"; exit 1; }
DATA=$(mktemp -d); trap 'rm -rf "$DATA"; [ -n "${PID:-}" ] && kill "$PID" 2>/dev/null' EXIT

run() {  # run <shape> <vus>: prints "<req/s> <p99 ms> <failed checks>" (median by req/s)
  local rows=()
  for _ in $(seq "$RUNS"); do
    k6 run --quiet --summary-export "$OUT/write.json" -e BROKER_URL="http://127.0.0.1:$PORT" \
      -e SHAPE="$1" -e VUS="$2" -e DURATION="$DURATION" -e TENANT="${TENANT:-}" \
      dev/perf/k6-writes.js >/dev/null 2>&1 || true
    rows+=("$(python3 -c "
import json; s=json.load(open('$OUT/write.json'))['metrics']
print(f\"{s['http_reqs']['rate']:.0f} {s['http_req_duration']['p(99)']:.2f} {int(s.get('checks',{}).get('fails',0))}\")")")
  done
  printf '%s\n' "${rows[@]}" | sort -n | sed -n "$(( (RUNS + 1) / 2 ))p"
}

{
  echo "| store | shape | request | concurrency | req/s | entities/s | p99 | rejected |"
  echo "|---|---|---|---|---|---|---|---|"
  for store in $STORES; do
    ANTARES_STORE="$store" ANTARES_DATA_DIR="$DATA" ANTARES_HTTP_PORT="$PORT" \
      ANTARES_DATABASE_URL="${DATABASE_URL:-}" ANTARES_ALLOW_SHARED_LOCAL=1 \
      "$BIN" > "$OUT/writes-$store.log" 2>&1 & PID=$!
    until curl -sf -o /dev/null "http://127.0.0.1:$PORT/q/health"; do sleep 0.05; done
    for shape in $SHAPES; do
      case "$shape" in
        update)   req='`PATCH /entities/{id}/attrs`'; per=1 ;;
        partial)  req='`PATCH /entities/{id}/attrs/{attr}`'; per=1 ;;
        merge)    req='`PATCH /entities/{id}`'; per=1 ;;
        replace)  req='`PUT /entities/{id}`'; per=1 ;;
        append)   req='`POST /entities/{id}/attrs`'; per=1 ;;
        create)   req='`POST /entities`'; per=1 ;;
        upsert20) req='`POST /entityOperations/upsert`, 20 per request'; per=20 ;;
        *)        req="$shape"; per=1 ;;
      esac
      for vus in $VUS; do
        echo "writes $store $shape c$vus" > "$OUT/phase"
        read -r rps p99 fails < <(run "$shape" "$vus")
        echo "| $store | $shape | $req | c$vus | $rps | $((rps * per)) | $p99 ms | $fails |"
      done
    done
    kill "$PID"; wait "$PID" 2>/dev/null || true; PID=
  done
} | tee "$OUT/writes.md"
