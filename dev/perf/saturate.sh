#!/usr/bin/env bash
# Saturation curve and knee per shape: the arrival rate steps up until
# p99 passes P99_MS or the error rate passes ERR; the knee is the last
# stage that held both. Curve to CSV, knee to the table.
#
#   dev/perf/saturate.sh                       # query + write, memory store
#   DATABASE_URL=… STORE=postgres dev/perf/saturate.sh
#
# Env: BIN, OUT (results/perf), PORT (9473), STORE (memory), DATABASE_URL,
#      STEP (500 rps), STAGES (20), STAGE (30s), P99_MS (50), ERR (0.001).
set -euo pipefail
cd "$(dirname "$0")/../.."

BIN="${BIN:-target/release/antares}"
OUT="${OUT:-results/perf}"
PORT="${PORT:-9473}"
STORE="${STORE:-memory}"
STEP="${STEP:-500}"; STAGES="${STAGES:-20}"; STAGE="${STAGE:-30s}"
P99_MS="${P99_MS:-50}"; ERR="${ERR:-0.001}"
mkdir -p "$OUT"
command -v k6 >/dev/null || { echo "k6 missing"; exit 1; }
[ -x "$BIN" ] || { echo "missing $BIN"; exit 1; }
DATA=$(mktemp -d); trap 'rm -rf "$DATA"; [ -n "${PID:-}" ] && kill "$PID" 2>/dev/null' EXIT

ANTARES_STORE="$STORE" ANTARES_DATA_DIR="$DATA" ANTARES_HTTP_PORT="$PORT" \
  ANTARES_DATABASE_URL="${DATABASE_URL:-}" ANTARES_ALLOW_SHARED_LOCAL=1 \
  "$BIN" > "$OUT/saturate.log" 2>&1 & PID=$!
until curl -sf -o /dev/null "http://127.0.0.1:$PORT/q/health"; do sleep 0.05; done

{
  echo "| store | shape | knee (rps held) | p99 at knee | first failing stage |"
  echo "|---|---|---|---|---|"
  for shape in query write; do
    k6 run --quiet --out "json=$OUT/saturate-$shape.jsonl" -e BROKER_URL="http://127.0.0.1:$PORT" \
      -e SHAPE="$shape" -e STEP="$STEP" -e STAGES="$STAGES" -e STAGE="$STAGE" \
      dev/perf/k6-saturate.js >/dev/null || true
    python3 - "$OUT/saturate-$shape.jsonl" "$OUT/saturate-$shape.csv" "$STORE" "$shape" "$P99_MS" "$ERR" <<'EOF'
import json, sys
src, dst, store, shape, p99_max, err_max = sys.argv[1:]
p99_max, err_max = float(p99_max), float(err_max)
lat, err = {}, {}
for line in open(src):
    d = json.loads(line)
    if d.get("type") != "Point": continue
    m, tags = d["metric"], d["data"].get("tags", {})
    if "rate" not in tags: continue
    r = int(tags["rate"])
    if m == "stage_latency": lat.setdefault(r, []).append(d["data"]["value"])
    elif m == "stage_errors": err.setdefault(r, []).append(d["data"]["value"])
knee, knee_p99, first_bad = None, None, None
with open(dst, "w") as f:
    f.write("rate,requests,p99_ms,error_rate\n")
    for r in sorted(lat):
        xs = sorted(lat[r]); p99 = xs[min(len(xs) - 1, int(len(xs) * 0.99))]
        e = sum(err.get(r, [0])) / max(1, len(err.get(r, [1])))
        f.write(f"{r},{len(xs)},{p99:.2f},{e:.4f}\n")
        if p99 <= p99_max and e <= err_max and first_bad is None: knee, knee_p99 = r, p99
        elif first_bad is None: first_bad = r
print(f"| {store} | {shape} | {knee or '—'} | {f'{knee_p99:.1f} ms' if knee_p99 is not None else '—'} | {first_bad or 'none reached'} |")
EOF
  done
} | tee "$OUT/saturate.md"
