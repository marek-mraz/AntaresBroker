#!/usr/bin/env bash
# The write shapes together, with reads, over a loaded tenant that carries
# its subscriptions: upsert20, delete20, replace, update and get drawn from
# one weighted wheel against the broker that owns the dataset. One row per
# client count; the sink is watched until it goes quiet, so the delivery
# columns cover the notifications the mix produced.
#
#   TENANT=t7 ENTITIES=1000000 dev/perf/mix.sh
#
# Env: BROKER_URL (http://127.0.0.1:9090), SINK (http://127.0.0.1:9800),
#      OUT (results/perf), TENANT, ENTITIES, VUS ("50 200"), DURATION (60s),
#      MIX (k6-mix.js's default wheel).
set -euo pipefail
cd "$(dirname "$0")/../.."
BROKER_URL="${BROKER_URL:-http://127.0.0.1:9090}"
SINK="${SINK:-http://127.0.0.1:9800}"
OUT="${OUT:-results/perf}"; mkdir -p "$OUT"
VUS="${VUS:-50 200}"
DURATION="${DURATION:-60s}"
: "${TENANT:?}" "${ENTITIES:?}"
command -v k6 >/dev/null || { echo "k6 missing"; exit 1; }

read -r _ SUBS_IN_TENANT _ < <(python3 dev/perf/tenant-counts.py "$BROKER_URL" "$TENANT")

{
  echo "Mixed workload in tenant $TENANT ($SUBS_IN_TENANT subscriptions, wheel ${MIX:-update:4,replace:2,get:2,upsert20:1,delete20:1})."
  echo
  echo "| clients | ops/s | update/s | replace/s | get/s | upsert20/s | entities/s via upsert | delete20/s | failed ops | p99 update | p99 get | notification POSTs | POSTs/s | quiet after (s) | dropped by broker | dead letters | broker cores | host busy cores |"
  echo "|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|"
} > "$OUT/mix.md"

for vus in $VUS; do
  echo "mix c$vus" > "$OUT/phase"
  curl -s -X DELETE "$SINK/stats" >/dev/null || true
  curl -sf "$BROKER_URL/q/health" > "$OUT/mix-$vus-health-before.json"
  t_start=$(date +%s)
  k6 run --quiet --summary-export "$OUT/mix-$vus.json" -e BROKER_URL="$BROKER_URL" -e TENANT="$TENANT" \
    -e ENTITIES="$ENTITIES" -e VUS="$vus" -e DURATION="$DURATION" ${MIX:+-e MIX="$MIX"} \
    dev/perf/k6-mix.js >/dev/null || true
  t_end=$(date +%s)
  # quiet = the sink's count unchanged for 5 s (cap 180 s)
  prev=-1; quiet=0
  while (( quiet < 5 && $(date +%s) - t_end < 180 )); do
    cur=$(curl -s "$SINK/stats" | python3 -c 'import sys,json; print(json.load(sys.stdin).get("posts",0))' 2>/dev/null || echo 0)
    if [ "$cur" = "$prev" ]; then quiet=$((quiet+1)); else quiet=0; fi
    prev=$cur; sleep 1
  done
  curl -sf "$BROKER_URL/q/health" > "$OUT/mix-$vus-health-after.json"
  SINK="$SINK" RSS_CSV="$OUT/rss.csv" T_START="$t_start" python3 - "$OUT/mix-$vus.json" "$vus" "$t_end" <<'PY' >> "$OUT/mix.md"
import sys, json, os, csv, urllib.request
m = json.load(open(sys.argv[1]))["metrics"]
rate = lambda k: m.get(k, {}).get("rate", 0.0)
p99 = lambda k: m.get(k, {}).get("p(99)", 0.0)
count = lambda k: int(m.get(k, {}).get("count", 0))
try:
    s = json.load(urllib.request.urlopen(os.environ["SINK"] + "/stats"))
except Exception:
    s = {}
t0, t1 = int(os.environ["T_START"]), int(sys.argv[3])
rows = []
if os.path.exists(os.environ["RSS_CSV"]):
    rows = [r for r in csv.DictReader(open(os.environ["RSS_CSV"])) if t0 <= int(r["t"]) <= t1]
mean = lambda k: (sum(float(r.get(k) or 0) for r in rows) / len(rows)) if rows else 0.0
base = sys.argv[1][:-5]
h0, h1 = (json.load(open(f"{base}-health-{w}.json")) for w in ("before", "after"))
delta = lambda k: int(h1.get(k, 0)) - int(h0.get(k, 0))
ups = rate("op_upsert20")
print(f'| c{sys.argv[2]} | {rate("http_reqs"):.0f} | {rate("op_update"):.0f} | {rate("op_replace"):.0f} | '
      f'{rate("op_get"):.0f} | {ups:.0f} | {ups * 20:.0f} | {rate("op_delete20"):.0f} | {count("op_errors")} | '
      f'{p99("dur_update"):.0f} ms | {p99("dur_get"):.0f} ms | {s.get("posts", 0)} | {s.get("posts_per_second") or 0} | '
      f'{max(0, int((s.get("last") or t1) - t1))} | {delta("changesDropped")} | {delta("deadLetters")} | '
      f'{mean("broker_cpu_pct")/100:.1f} | {mean("host_busy_cores"):.1f} |')
PY
done
cat "$OUT/mix.md"
