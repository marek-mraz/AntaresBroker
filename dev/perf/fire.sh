#!/usr/bin/env bash
# Do the subscriptions fire, and up to what rate? For each arrival rate:
# an update+delete stream over the loaded dataset (k6-fire.js), then the
# sink is watched until it goes quiet, and delivered is set against the
# count the stream had to produce. The limit is the last rate that
# delivered 99% with no failed operation.
#
#   TENANTS=… SUBS=… ENTITIES=… dev/perf/fire.sh
#
# Env: BROKER_URL (http://127.0.0.1:9090), SINK (http://127.0.0.1:9800),
#      OUT (results/perf), RATES ("100 200 500 1000 2000 4000"), DURATION (60s),
#      TENANTS, SUBS, ENTITIES (the loader's counts), MQTT (0|1).
set -euo pipefail
cd "$(dirname "$0")/../.."
BROKER_URL="${BROKER_URL:-http://127.0.0.1:9090}"
SINK="${SINK:-http://127.0.0.1:9800}"
OUT="${OUT:-results/perf}"; mkdir -p "$OUT"
RATES="${RATES:-100 200 500 1000 2000 4000}"
DURATION="${DURATION:-60s}"
: "${TENANTS:?}" "${SUBS:?}" "${ENTITIES:?}"
command -v k6 >/dev/null || { echo "k6 missing"; exit 1; }

# what the broker holds, from its own tenant counts
read -r ENT SUB REG TENANT_COUNT < <(python3 dev/perf/tenant_totals.py "$BROKER_URL")
{
  echo "In the broker: $ENT entities, $SUB subscriptions, $REG registrations over $TENANT_COUNT tenants."
  echo
  echo "| rate (rps) | updates | deletes | reads | failed ops (conn/4xx/5xx) | entity notifications due | delivered | delivered % | subscriptions that fired | notification POSTs | POSTs/s | quiet after (s) | dropped by broker | dead letters | PATCH p99 (ms) | GET p99 (ms) | broker cores | host busy cores |"
  echo "|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|"
} > "$OUT/fire.md"
LIMIT=none
echo "Per class (api-load.py SUB_CLASSES; k6-fire.js evaluates the same rule for the count due):" > "$OUT/fire-classes.md"
echo "" >> "$OUT/fire-classes.md"
echo "| rate (rps) | class | due | delivered | delivered % |" >> "$OUT/fire-classes.md"
echo "|---|---|---|---|---|" >> "$OUT/fire-classes.md"
for rate in $RATES; do
  echo "fire $rate" > "$OUT/phase"
  curl -s -X DELETE "$SINK/stats" >/dev/null
  curl -sf "$BROKER_URL/q/health" > "$OUT/fire-$rate-health-before.json"
  t_start=$(date +%s)
  k6 run --quiet --summary-export "$OUT/fire-$rate.json" -e BROKER_URL="$BROKER_URL" -e RATE="$rate" \
    -e DURATION="$DURATION" -e TENANTS="$TENANTS" -e SUBS="$SUBS" -e ENTITIES="$ENTITIES" \
    -e MQTT="${MQTT:-0}" -e READ_PCT="${READ_PCT:-20}" dev/perf/k6-fire.js >/dev/null || true
  t_end=$(date +%s)
  # quiet = the sink's count unchanged for 5 s (cap 180 s)
  prev=-1; quiet=0
  while (( quiet < 5 && $(date +%s) - t_end < 180 )); do
    cur=$(curl -s "$SINK/stats" | python3 -c 'import sys,json; print(json.load(sys.stdin)["posts"])')
    if [ "$cur" = "$prev" ]; then quiet=$((quiet+1)); else quiet=0; fi
    prev=$cur; sleep 1
  done
  curl -sf "$BROKER_URL/q/health" > "$OUT/fire-$rate-health-after.json"
  row=$(SINK="$SINK" TENANTS="$TENANTS" RSS_CSV="$OUT/rss.csv" T_START="$t_start" python3 - "$OUT/fire-$rate.json" "$rate" "$t_end" 2>"$OUT/fire-verdict.txt" <<'PY'
import sys,json,os,csv,urllib.request
m=json.load(open(sys.argv[1]))["metrics"]
c=lambda k: int(m.get(k,{}).get("count",0))
s=json.load(urllib.request.urlopen(os.environ["SINK"]+"/stats"))
# per class: subscription k -> class (k // TENANTS) % 8, delivered from the sink's per-id counts
tenants=int(os.environ["TENANTS"]); NCLASS=8
got_cls=[0]*NCLASS
for sid,n in (s.get("by_sub") or {}).items():
    try: k=int(sid.rsplit(":",1)[1])
    except ValueError: continue
    got_cls[(k//tenants)%NCLASS]+=n
names=["vehicle-any","vehicle-cold-attr","vehicle-high-speed","vehicle-id-tail","building-any","sensor-any","vehicle-geo-west","any-scope"]
with open(os.environ["RSS_CSV"].replace("rss.csv","fire-classes.md"),"a") as f:
    for i,nm in enumerate(names):
        d=c(f"notifications_expected_class{i}")
        f.write(f"| {sys.argv[2]} | {nm} | {d} | {got_cls[i]} | {(100.0*got_cls[i]/d) if d else 0:.1f} |\n")
# resource window: rss.csv rows between the stream's start and the quiet point
t0=int(os.environ["T_START"]); t1=int(sys.argv[3])
rows=[r for r in csv.DictReader(open(os.environ["RSS_CSV"])) if t0<=int(r["t"])<=t1] if os.path.exists(os.environ["RSS_CSV"]) else []
mean=lambda k: (sum(float(r.get(k) or 0) for r in rows)/len(rows)) if rows else 0.0
bcores=mean("broker_cpu_pct")/100; hcores=mean("host_busy_cores")
p99=m.get("http_req_duration{scenario:fire}",m.get("http_req_duration",{})).get("p(99)",0)
rp99=m.get("http_req_duration{scenario:reads}",{}).get("p(99)",0)
# broker-side counters over this rate: where the missing notifications went
base=sys.argv[1][:-5]
h0,h1=(json.load(open(f"{base}-health-{w}.json")) for w in ("before","after"))
delta=lambda k: int(h1.get(k,0))-int(h0.get(k,0))
failed=f'{c("op_errors")} ({c("op_errors_conn")}/{c("op_errors_4xx")}/{c("op_errors_5xx")})'
due=c("notifications_expected_http"); got=s["entities"]
pct=(100.0*got/due) if due else 0.0
quiet=max(0,int((s["last"] or int(sys.argv[3]))-int(sys.argv[3])))
print(f'| {sys.argv[2]} | {c("updates_ok")} | {c("deletes_ok")} | {c("reads_ok")} | {failed} | {due} | {got} | {pct:.1f} | {s.get("subscriptions",0)} | {s["posts"]} | {s.get("posts_per_second") or 0} | {quiet} | {delta("changesDropped")} | {delta("deadLetters")} | {p99:.1f} | {rp99:.1f} | {bcores:.1f} | {hcores:.1f} |')
print("OK" if c("op_errors")==0 and due and pct>=99.0 else "FAIL", file=sys.stderr)
PY
  )
  echo "$row" | tee -a "$OUT/fire.md"
  [ "$(cat "$OUT/fire-verdict.txt")" = OK ] && LIMIT=$rate || break
done
echo -e "\nLimit: $LIMIT rps (the last rate that delivered 99% with no failed operation)." | tee -a "$OUT/fire.md"
