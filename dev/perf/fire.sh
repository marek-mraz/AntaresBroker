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
read -r ENT SUB REG < <(curl -sf "$BROKER_URL/q/tenants" | python3 -c '
import sys,json
rows=json.load(sys.stdin)
rows=rows if isinstance(rows,list) else rows.get("tenants",[])
c=[r["counts"] for r in rows]
print(sum(x["entities"] for x in c), sum(x["subscriptions"] for x in c), sum(x["registrations"] for x in c))')
{
  echo "In the broker: $ENT entities, $SUB subscriptions, $REG registrations over $(curl -sf "$BROKER_URL/q/tenants" | python3 -c 'import sys,json; r=json.load(sys.stdin); print(len(r if isinstance(r,list) else r.get("tenants",[])))') tenants."
  echo
  echo "| rate (rps) | updates | deletes | failed ops (conn/4xx/5xx) | entity notifications due | delivered | delivered % | subscriptions that fired | notification POSTs | POSTs/s | quiet after (s) | dropped by broker | dead letters |"
  echo "|---|---|---|---|---|---|---|---|---|---|---|---|---|"
} > "$OUT/fire.md"
LIMIT=none
for rate in $RATES; do
  curl -s -X DELETE "$SINK/stats" >/dev/null
  curl -sf "$BROKER_URL/q/health" > "$OUT/fire-$rate-health-before.json"
  k6 run --quiet --summary-export "$OUT/fire-$rate.json" -e BROKER_URL="$BROKER_URL" -e RATE="$rate" \
    -e DURATION="$DURATION" -e TENANTS="$TENANTS" -e SUBS="$SUBS" -e ENTITIES="$ENTITIES" \
    -e MQTT="${MQTT:-0}" dev/perf/k6-fire.js >/dev/null || true
  t_end=$(date +%s)
  # quiet = the sink's count unchanged for 5 s (cap 180 s)
  prev=-1; quiet=0
  while (( quiet < 5 && $(date +%s) - t_end < 180 )); do
    cur=$(curl -s "$SINK/stats" | python3 -c 'import sys,json; print(json.load(sys.stdin)["posts"])')
    if [ "$cur" = "$prev" ]; then quiet=$((quiet+1)); else quiet=0; fi
    prev=$cur; sleep 1
  done
  curl -sf "$BROKER_URL/q/health" > "$OUT/fire-$rate-health-after.json"
  row=$(SINK="$SINK" python3 - "$OUT/fire-$rate.json" "$rate" "$t_end" 2>"$OUT/fire-verdict.txt" <<'PY'
import sys,json,os,urllib.request
m=json.load(open(sys.argv[1]))["metrics"]
c=lambda k: int(m.get(k,{}).get("count",0))
s=json.load(urllib.request.urlopen(os.environ["SINK"]+"/stats"))
# broker-side counters over this rate: where the missing notifications went
base=sys.argv[1][:-5]
h0,h1=(json.load(open(f"{base}-health-{w}.json")) for w in ("before","after"))
delta=lambda k: int(h1.get(k,0))-int(h0.get(k,0))
failed=f'{c("op_errors")} ({c("op_errors_conn")}/{c("op_errors_4xx")}/{c("op_errors_5xx")})'
due=c("notifications_expected_http"); got=s["entities"]
pct=(100.0*got/due) if due else 0.0
quiet=max(0,int((s["last"] or int(sys.argv[3]))-int(sys.argv[3])))
print(f'| {sys.argv[2]} | {c("updates_ok")} | {c("deletes_ok")} | {failed} | {due} | {got} | {pct:.1f} | {s.get("subscriptions",0)} | {s["posts"]} | {s.get("posts_per_second") or 0} | {quiet} | {delta("changesDropped")} | {delta("deadLetters")} |')
print("OK" if c("op_errors")==0 and due and pct>=99.0 else "FAIL", file=sys.stderr)
PY
  )
  echo "$row" | tee -a "$OUT/fire.md"
  [ "$(cat "$OUT/fire-verdict.txt")" = OK ] && LIMIT=$rate || break
done
echo -e "\nLimit: $LIMIT rps (the last rate that delivered 99% with no failed operation)." | tee -a "$OUT/fire.md"
