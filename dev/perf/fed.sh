#!/usr/bin/env bash
# Federated query stream over the loaded registrations: k6-fed.js sends
# entity queries (five shapes: type, q, geoQ, scopeQ, idPattern) on random
# tenants; the broker forwards each to the registrations its index says
# can match; the sink answers every /csr/<k> empty and counts the calls.
# One row per rate: queries, failures by class, queries that came back
# with an NGSILD-Warning (a source failed), p99, source calls and calls per
# query (the fan-out the registry narrowing left), CPU over the window.
#
#   TENANTS=… dev/perf/fed.sh
#
# Env: BROKER_URL (http://127.0.0.1:9090), SINK (http://127.0.0.1:9800),
#      OUT (results/perf), RATES ("50 100 200 500"), DURATION (30s), TENANTS.
set -euo pipefail
cd "$(dirname "$0")/../.."
OUT="${OUT:-results/perf}"; mkdir -p "$OUT"
SINK="${SINK:-http://127.0.0.1:9800}"
BROKER_URL="${BROKER_URL:-http://127.0.0.1:9090}"
RATES="${RATES:-50 100 200 500}"
DURATION="${DURATION:-30s}"
TENANTS="${TENANTS:-10}"

REGS=$(curl -sf "$BROKER_URL/q/tenants" | python3 -c '
import sys,json
r=json.load(sys.stdin); rows=r if isinstance(r,list) else r.get("tenants",[])
print(sum(int(x["counts"]["registrations"]) for x in rows))')
echo "Federated queries over $REGS registrations (every source is the sink, answering empty)." | tee "$OUT/fed.md"
echo "" | tee -a "$OUT/fed.md"
echo "| rate (rps) | queries | failed (conn/4xx/5xx) | with a source warning | GET p99 (ms) | source calls | calls per query | broker cores | host busy cores |" | tee -a "$OUT/fed.md"
echo "|---|---|---|---|---|---|---|---|---|" | tee -a "$OUT/fed.md"

for rate in $RATES; do
  curl -s -X DELETE "$SINK/stats" >/dev/null
  t_start=$(date +%s)
  k6 run --quiet --summary-export "$OUT/fed-$rate.json" -e BROKER_URL="$BROKER_URL" -e RATE="$rate" \
    -e DURATION="$DURATION" -e TENANTS="$TENANTS" dev/perf/k6-fed.js >/dev/null || true
  t_end=$(date +%s)
  SINK="$SINK" RSS_CSV="$OUT/rss.csv" python3 - "$OUT/fed-$rate.json" "$rate" "$t_start" "$t_end" <<'PY' | tee -a "$OUT/fed.md"
import sys,json,os,csv,urllib.request
m=json.load(open(sys.argv[1]))["metrics"]
c=lambda k: int(m.get(k,{}).get("count",0))
s=json.load(urllib.request.urlopen(os.environ["SINK"]+"/stats"))
calls=sum((s.get("csr_gets") or {}).values())
q=c("queries_ok")
t0,t1=int(sys.argv[3]),int(sys.argv[4])
rows=[r for r in csv.DictReader(open(os.environ["RSS_CSV"])) if t0<=int(r["t"])<=t1] if os.path.exists(os.environ["RSS_CSV"]) else []
mean=lambda k: (sum(float(r.get(k) or 0) for r in rows)/len(rows)) if rows else 0.0
p99=m.get("http_req_duration",{}).get("p(99)",0)
print(f'| {sys.argv[2]} | {q} | {c("op_errors")} ({c("op_errors_conn")}/{c("op_errors_4xx")}/{c("op_errors_5xx")}) | {c("queries_with_warning")} | {p99:.1f} | {calls} | {(calls/q) if q else 0:.2f} | {mean("broker_cpu_pct")/100:.1f} | {mean("host_busy_cores"):.1f} |')
PY
done
