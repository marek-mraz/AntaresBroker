#!/usr/bin/env bash
# Deployment scenario harness: verify edge behaviors and multi-broker topologies.
#
#   MODE=check STORE=memory   dev/perf/scenarios.sh [names...]
#   MODE=load  STORE=postgres dev/perf/scenarios.sh [names...]
#
# Env: MODE (check|load), STORE (memory|postgres), OUT (results/perf),
#      BIN (target/release/antares), NATS_URL, PG_URL_BASE, PG_ADMIN_URL.
set -euo pipefail
cd "$(dirname "$0")/../.."

MODE="${MODE:-check}"
STORE="${STORE:-memory}"
OUT="${OUT:-results/perf}"
BIN="${BIN:-target/release/antares}"
SCEN_DIR="$OUT/scenarios"
mkdir -p "$SCEN_DIR"

# The scenarios' own single-process sink. The load rig's sink on 9800 runs
# multi-process: its front door only folds /stats and answers a POST with
# 501, so a notification sent there is counted nowhere.
SINK_PORT=9810
SINK_URL="http://127.0.0.1:$SINK_PORT"
FLEET="bash dev/perf/fleet.sh"
CHECK="python3 dev/perf/scenario-check.py"
CTX="https://uri.etsi.org/ngsi-ld/v1/ngsi-ld-core-context-v1.9.jsonld"

SINK_PID=""
if ! curl -sf "$SINK_URL/stats" >/dev/null; then
  python3 dev/perf/sink.py "$SINK_PORT" > "$SCEN_DIR/sink.log" 2>&1 &
  SINK_PID=$!
  sleep 1
fi

trap '$FLEET stop-all; kill "${SINK_PID:-}" 2>/dev/null || true' EXIT

record_verdict() {
  local scen=$1 verd=$2 num=$3 note=$4
  printf "| %s | %s | %s | %s |\n" "$scen" "$verd" "$num" "$note" >> "$SCEN_DIR/verdicts.md"
}

run_k6() {
  local scen=$1 tag=$2 rate=$3 dur=${4:-20s}
  shift 4 || true
  k6 run --quiet --summary-export "$SCEN_DIR/$scen-$tag-$rate.json" \
    -e SCENARIO="$scen" -e RATE="$rate" -e DURATION="$dur" "$@" \
    dev/perf/k6-scenarios.js >/dev/null 2>&1 || true
}

k6_metrics() {
  python3 - "$1" <<'PY'
import sys, json
try:
    m = json.load(open(sys.argv[1]))["metrics"]
except Exception:
    m = {}
rate = m.get("http_reqs", {}).get("rate", 0) or 0
p99 = m.get("http_req_duration", {}).get("p(99)", 0) or 0
c = lambda k: int(m.get(k, {}).get("count", 0) or 0)
print(f"{rate:.1f} {p99:.1f} {c('ops_ok')} {c('ops_failed')} {c('ops_508')} {c('ops_warning')}")
PY
}

k6_trend_p99() {
  python3 - "$1" "$2" <<'PY'
import sys, json
try:
    m = json.load(open(sys.argv[1])).get("metrics", {})
    val = m.get(sys.argv[2], {}).get("p(99)", 0) or 0
except Exception:
    val = 0
print(f"{val:.1f}")
PY
}

sink_stats() {
  python3 - "$SINK_URL" "$@" <<'PY'
import sys, json, urllib.request
try:
    s = json.load(urllib.request.urlopen(sys.argv.pop(1) + "/stats", timeout=5))
except Exception:
    s = {}
by_sub = s.get("by_sub") or {}
by_entity = s.get("by_entity") or {}
out = []
for arg in sys.argv[1:]:
    if arg == "posts":
        out.append(str(s.get("posts", 0)))
    elif arg == "entities":
        out.append(str(s.get("entities", 0)))
    elif arg == "subscriptions":
        out.append(str(s.get("subscriptions", 0)))
    elif arg == "posts_per_second":
        out.append(str(s.get("posts_per_second") or 0))
    elif arg == "duplicates":
        dups = sum(1 for v in by_entity.values() if v > 1)
        out.append(str(dups))
    elif arg.startswith("by_sub_exclude:"):
        # subscription ids end in the class name; the tenant in the middle may contain it too
        ex = arg.split(":", 1)[1]
        tot = sum(v for k, v in by_sub.items() if not k.endswith(ex))
        out.append(str(tot))
    elif arg.startswith("by_sub_include:"):
        inc = arg.split(":", 1)[1]
        tot = sum(v for k, v in by_sub.items() if k.endswith(inc))
        out.append(str(tot))
    elif arg == "by_sub_sum":
        tot = sum(by_sub.values())
        out.append(str(tot))
    else:
        out.append(str(s.get(arg, 0)))
print(" ".join(out))
PY
}

sink_reset() {
  curl -s -X DELETE "$SINK_URL/stats" >/dev/null
}

wait_sink_quiet() {
  local max_s=${1:-30} prev=-1 quiet=0 t0
  t0=$(date +%s)
  while (( quiet < 2 && $(date +%s) - t0 < max_s )); do
    local cur
    cur=$(sink_stats posts)
    if [ "$cur" = "$prev" ]; then quiet=$((quiet+1)); else quiet=0; fi
    prev=$cur
    sleep 1
  done
}

wait_sink_fast_quiet() {
  local max_s=${1:-30} prev=-1 quiet=0 t0
  t0=$(date +%s)
  while (( quiet < 2 && $(date +%s) - t0 < max_s )); do
    local cur
    cur=$(sink_stats by_sub_exclude::slow)
    if [ "$cur" = "$prev" ]; then quiet=$((quiet+1)); else quiet=0; fi
    prev=$cur
    sleep 1
  done
}

# ================= S1: hot-entity =================
scenario_hot_entity() {
  local p=9101
  $FLEET start s1-broker $p ANTARES_HOST_ALIAS=s1_broker
  b="http://127.0.0.1:$p"

  # Seed 1000 entities, then the hot one on its own: a batch holds at most
  # ANTARES_MAX_BATCH_ITEMS (1000) entities
  curl -sf -X POST "$b/ngsi-ld/v1/entityOperations/create" \
    -H "NGSILD-Tenant: hot_entity" -H "Content-Type: application/json" \
    -d "$(python3 -c 'import json; print(json.dumps([{"id": f"urn:ngsi-ld:Vehicle:hot_entity:{i}", "type": "Vehicle", "speed": {"type": "Property", "value": 0}} for i in range(1000)]))')" >/dev/null
  curl -sf -X POST "$b/ngsi-ld/v1/entityOperations/create" \
    -H "NGSILD-Tenant: hot_entity" -H "Content-Type: application/json" \
    -d '[{"id":"urn:ngsi-ld:Vehicle:hot_entity:hot0","type":"Vehicle","speed":{"type":"Property","value":0}}]' >/dev/null

  if [ "$MODE" = "check" ]; then
    if $CHECK hot-entity --broker "$b"; then
      record_verdict "hot-entity" "PASS" "32 threads ok" "5.6.3 datasetId updates preserved under race"
    else
      record_verdict "hot-entity" "FAIL" "data loss" "concurrent datasetId update failed"
      return 1
    fi
  else
    if ! $CHECK hot-entity --broker "$b"; then
      record_verdict "hot-entity" "FAIL" "check failed" "concurrent datasetId update failed"
      $FLEET stop s1-broker
      return 1
    fi

    local tot_fail=0 first_fail_cnt=0 first_fail_rate=0 highest_under_2x="" max_hot_rps=0 max_hot_p99=0 max_spr_rps=0
    local rows=()
    for r in 100 200 500 1000; do
      run_k6 hot-entity hot0 $r 20s -e BROKER_URL="$b" -e SPREAD=0
      run_k6 hot-entity spread1 $r 20s -e BROKER_URL="$b" -e SPREAD=1
      local rps_hot p99_hot ok_hot fail_hot s508_hot warn_hot
      local rps_spr p99_spr ok_spr fail_spr s508_spr warn_spr
      read -r rps_hot p99_hot ok_hot fail_hot s508_hot warn_hot < <(k6_metrics "$SCEN_DIR/hot-entity-hot0-$r.json")
      read -r rps_spr p99_spr ok_spr fail_spr s508_spr warn_spr < <(k6_metrics "$SCEN_DIR/hot-entity-spread1-$r.json")
      rows+=("| $r rps | 1 hot entity | $rps_hot req/s | $p99_hot ms | $fail_hot |")
      rows+=("| $r rps | 1000 spread | $rps_spr req/s | $p99_spr ms | $fail_spr |")
      tot_fail=$((tot_fail + fail_hot + fail_spr))
      if [ "$first_fail_cnt" -eq 0 ] && [ "$((fail_hot + fail_spr))" -gt 0 ]; then
        first_fail_cnt=$((fail_hot + fail_spr))
        first_fail_rate=$r
      fi
      local under_2x
      under_2x=$(python3 -c "print(1 if float('$p99_hot') <= 2.0 * max(0.001, float('$p99_spr')) else 0)")
      if [ "$under_2x" -eq 1 ] && [ "$fail_hot" -eq 0 ]; then
        highest_under_2x=$r
      fi
      if python3 -c "import sys; sys.exit(0 if float('$rps_hot') > float('$max_hot_rps') else 1)"; then
        max_hot_rps=$rps_hot; max_hot_p99=$p99_hot
      fi
      if python3 -c "import sys; sys.exit(0 if float('$rps_spr') > float('$max_spr_rps') else 1)"; then
        max_spr_rps=$rps_spr
      fi
    done

    {
      echo "One broker with 1000 Vehicle entities, subjecting either a single hot entity or 1000 spread entities to concurrent attribute updates."
      echo
      echo "| rate | spread | req/s | p99 | failed |"
      echo "|---|---|---|---|---|"
      for row in "${rows[@]}"; do
        echo "$row"
      done
      echo
      echo "### What this shows"
      if [ "$tot_fail" -eq 0 ]; then
        echo "- Concurrent attribute updates succeeded across all rates with zero failures."
      else
        echo "- Contention caused $tot_fail failed writes under peak concurrency."
      fi
      if [ -n "$highest_under_2x" ]; then
        echo "- Contention latency on the hot entity stayed within 2x of spread latency up to $highest_under_2x rps."
      else
        echo "- Hot entity serialization elevated p99 latency above twice the spread baseline at every offered rate."
      fi
      echo "- One entity sustained at most $max_hot_rps updates/s (p99 $max_hot_p99 ms at that rate); the same load spread over 1000 entities reached $max_spr_rps updates/s."
    } > "$SCEN_DIR/hot-entity.md"

    if [ "$tot_fail" -eq 0 ]; then
      record_verdict "hot-entity" "PASS" "$max_hot_rps upd/s on one entity" "0 failed; one entity peaks at $max_hot_rps updates/s (p99 $max_hot_p99 ms), spread over 1000 at $max_spr_rps"
    else
      record_verdict "hot-entity" "FAIL" "data loss" "hot entity: $first_fail_cnt failed at $first_fail_rate rps"
      $FLEET stop s1-broker
      return 1
    fi
  fi
  $FLEET stop s1-broker
}

# ================= S2: noisy-tenant =================
scenario_noisy_tenant() {
  local p=9102
  $FLEET start s2-broker $p ANTARES_HOST_ALIAS=s2_broker
  b="http://127.0.0.1:$p"

  for t in quiet loud; do
    curl -sf -X POST "$b/ngsi-ld/v1/entityOperations/create" \
      -H "NGSILD-Tenant: $t" -H "Content-Type: application/json" \
      -d "$(python3 -c 'import json,sys; t=sys.argv[1]; print(json.dumps([{"id": f"urn:ngsi-ld:Vehicle:{t}:{i}", "type": "Vehicle", "speed": {"type": "Property", "value": 10}} for i in range(100)]))' "$t")" >/dev/null
  done

  if [ "$MODE" = "check" ]; then
    if $CHECK noisy-tenant --broker "$b" --sink "$SINK_URL"; then
      record_verdict "noisy-tenant" "PASS" "isolated" "quiet tenant GET intact during 16 loud writers"
    else
      record_verdict "noisy-tenant" "FAIL" "bleed" "quiet tenant degraded by loud flood"
      return 1
    fi
  else
    if ! $CHECK noisy-tenant --broker "$b" --sink "$SINK_URL"; then
      record_verdict "noisy-tenant" "FAIL" "check failed" "quiet tenant degraded by loud flood"
      $FLEET stop s2-broker
      return 1
    fi

    # Measure quiet alone at 50 rps
    run_k6 noisy-tenant alone 50 20s -e BROKER_URL="$b" -e LOUD=0
    local quiet_alone_p99
    quiet_alone_p99=$(k6_trend_p99 "$SCEN_DIR/noisy-tenant-alone-50.json" quiet_get_ms)

    local tot_fail=0 quiet_load_p99_200=0
    local rows=()
    for r in 100 200 500; do
      run_k6 noisy-tenant loud $r 20s -e BROKER_URL="$b"
      local rps p99 ok fail s508 warn quiet_load_p99
      read -r rps p99 ok fail s508 warn < <(k6_metrics "$SCEN_DIR/noisy-tenant-loud-$r.json")
      quiet_load_p99=$(k6_trend_p99 "$SCEN_DIR/noisy-tenant-loud-$r.json" quiet_get_ms)
      rows+=("| $r rps | $rps req/s | $fail | $quiet_alone_p99 ms | $quiet_load_p99 ms |")
      tot_fail=$((tot_fail + fail))
      if [ "$r" -eq 200 ]; then
        quiet_load_p99_200=$quiet_load_p99
      fi
    done

    local under_3x_200
    under_3x_200=$(python3 -c "print(1 if float('$quiet_load_p99_200') <= 3.0 * max(0.001, float('$quiet_alone_p99')) else 0)")

    {
      echo "One broker hosting two tenants (quiet and loud) with 100 Vehicle entities each, measuring quiet GET latency alone versus during a loud PATCH write flood."
      echo
      echo "| loud rate | loud req/s | loud failed | quiet GET p99 alone | quiet GET p99 under load |"
      echo "|---|---|---|---|---|"
      for row in "${rows[@]}"; do
        echo "$row"
      done
      echo
      echo "### What this shows"
      if [ "$under_3x_200" -eq 1 ]; then
        echo "- Quiet tenant read latency remained bounded (p99 $quiet_load_p99_200 ms under load vs $quiet_alone_p99 ms alone at 200 rps)."
      else
        echo "- Heavy write traffic degraded quiet tenant reads beyond 3x baseline at 200 rps."
      fi
      if [ "$tot_fail" -eq 0 ]; then
        echo "- Zero operations failed across quiet reads and loud writes up to 500 rps."
      else
        echo "- Load shed or errors observed: $tot_fail failed requests during write flood."
      fi
    } > "$SCEN_DIR/noisy-tenant.md"

    if [ "$tot_fail" -eq 0 ] && [ "$under_3x_200" -eq 1 ]; then
      record_verdict "noisy-tenant" "PASS" "500 rps" "quiet p99 ${quiet_load_p99_200} ms under load vs ${quiet_alone_p99} ms alone at 200 rps"
    else
      record_verdict "noisy-tenant" "FAIL" "bleed" "failed: $tot_fail, quiet p99 ${quiet_load_p99_200} ms (>3x alone ${quiet_alone_p99} ms)"
      $FLEET stop s2-broker
      return 1
    fi
  fi
  $FLEET stop s2-broker
}

# ================= S3: slow-subscriber =================
scenario_slow_subscriber() {
  local p=9103
  $FLEET start s3-broker $p ANTARES_HOST_ALIAS=s3_broker ANTARES_DELIVERY_WIDTH=64 ANTARES_DELIVERY_WIDTH_PER_TENANT=8
  b="http://127.0.0.1:$p"
  tenant="slow_sub"

  # Seed entity
  curl -sf -X POST "$b/ngsi-ld/v1/entities" -H "NGSILD-Tenant: $tenant" -H "Content-Type: application/ld+json" \
    -d '{"id":"urn:ngsi-ld:Vehicle:'$tenant':0","type":"Vehicle","speed":{"type":"Property","value":10},"@context":"'"$CTX"'"}' >/dev/null

  # 10 fast subs + 1 slow sub
  for i in $(seq 10); do
    curl -sf -X POST "$b/ngsi-ld/v1/subscriptions" -H "NGSILD-Tenant: $tenant" -H "Content-Type: application/ld+json" \
      -d '{"id":"urn:ngsi-ld:Subscription:'$tenant':fast'$i'","type":"Subscription","entities":[{"type":"Vehicle"}],"notification":{"endpoint":{"uri":"'$SINK_URL'/sub/fast'$i'","accept":"application/json"}},"@context":"'"$CTX"'"}' >/dev/null
  done
  curl -sf -X POST "$b/ngsi-ld/v1/subscriptions" -H "NGSILD-Tenant: $tenant" -H "Content-Type: application/ld+json" \
    -d '{"id":"urn:ngsi-ld:Subscription:'$tenant':slow","type":"Subscription","entities":[{"type":"Vehicle"}],"notification":{"endpoint":{"uri":"'$SINK_URL'/slow/500/sub/slow","accept":"application/json"}},"@context":"'"$CTX"'"}' >/dev/null

  if [ "$MODE" = "check" ]; then
    if $CHECK slow-subscriber --broker "$b" --sink "$SINK_URL"; then
      record_verdict "slow-subscriber" "PASS" "fast unblocked" "deliveryWidthPerTenant bounded slow drain"
    else
      record_verdict "slow-subscriber" "FAIL" "stall" "slow endpoint delayed fast subscribers"
      return 1
    fi
  else
    if ! $CHECK slow-subscriber --broker "$b" --sink "$SINK_URL"; then
      record_verdict "slow-subscriber" "FAIL" "check failed" "slow endpoint delayed fast subscribers"
      $FLEET stop s3-broker
      return 1
    fi

    local first_bad="" first_bad_pct=""
    local rows=()
    for r in 50 100 200; do
      sink_reset
      run_k6 slow-subscriber run $r 20s -e BROKER_URL="$b" -e TENANT="$tenant"
      local rps p99 ok fail s508 warn fast_del slow_del expected pct is_99
      read -r rps p99 ok fail s508 warn < <(k6_metrics "$SCEN_DIR/slow-subscriber-run-$r.json")
      wait_sink_fast_quiet 30
      read -r fast_del slow_del < <(sink_stats by_sub_exclude::slow by_sub_include::slow)
      expected=$((ok * 10))
      pct=$(python3 -c "print(f'{(100.0 * float($fast_del) / max(1, $expected)):.1f}')")
      rows+=("| $r rps | $ok | $fast_del / $expected | $slow_del | $pct% |")
      is_99=$(python3 -c "print(1 if float('$pct') >= 99.0 and $ok > 0 else 0)")
      if [ "$is_99" -ne 1 ] && [ -z "$first_bad" ]; then
        first_bad=$r
        first_bad_pct=$pct
      fi
    done

    {
      echo "One broker delivering notifications to 10 fast endpoints and 1 slow endpoint (500 ms delay) under increasing update rates."
      echo
      echo "| rate | updates ok | fast deliveries / expected | slow deliveries | delivered % fast |"
      echo "|---|---|---|---|---|"
      for row in "${rows[@]}"; do
        echo "$row"
      done
      echo
      echo "### What this shows"
      if [ -z "$first_bad" ]; then
        echo "- Fast subscribers maintained complete delivery (>= 99%) without head-of-line blocking."
      else
        echo "- Slow subscriber drain caused delivery backpressure, dropping fast delivery to $first_bad_pct% at $first_bad rps."
      fi
    } > "$SCEN_DIR/slow-subscriber.md"

    if [ -z "$first_bad" ]; then
      record_verdict "slow-subscriber" "PASS" "200 rps" "fast subscribers delivered >= 99% across all rates"
    else
      record_verdict "slow-subscriber" "FAIL" "stall" "fast delivered $first_bad_pct% at $first_bad rps"
      $FLEET stop s3-broker
      return 1
    fi
  fi
  $FLEET stop s3-broker
}

# ================= S4: fan-in =================
scenario_fan_in() {
  local p=9104
  $FLEET start s4-broker $p ANTARES_HOST_ALIAS=s4_broker
  b="http://127.0.0.1:$p"
  tenant="fan_in"

  curl -sf -X POST "$b/ngsi-ld/v1/entities" -H "NGSILD-Tenant: $tenant" -H "Content-Type: application/ld+json" \
    -d '{"id":"urn:ngsi-ld:Vehicle:'$tenant':0","type":"Vehicle","speed":{"type":"Property","value":10},"@context":"'"$CTX"'"}' >/dev/null

  cnt=50
  for i in $(seq $cnt); do
    curl -sf -X POST "$b/ngsi-ld/v1/subscriptions" -H "NGSILD-Tenant: $tenant" -H "Content-Type: application/ld+json" \
      -d '{"id":"urn:ngsi-ld:Subscription:'$tenant':w'$i'","type":"Subscription","entities":[{"type":"Vehicle"}],"notification":{"endpoint":{"uri":"'$SINK_URL'/sub/'$i'","accept":"application/json"}},"@context":"'"$CTX"'"}' >/dev/null
  done

  if [ "$MODE" = "check" ]; then
    if $CHECK fan-in --broker "$b" --sink "$SINK_URL"; then
      record_verdict "fan-in" "PASS" "fan-out 50x" "1 update fired all subscriptions"
    else
      record_verdict "fan-in" "FAIL" "delivery" "fan-in notification delivery lost"
      return 1
    fi
  else
    if ! $CHECK fan-in --broker "$b" --sink "$SINK_URL"; then
      record_verdict "fan-in" "FAIL" "check failed" "fan-in notification delivery lost"
      $FLEET stop s4-broker
      return 1
    fi

    local first_bad="" first_bad_pct="" peak_posts_s="0"
    local rows=()
    for r in 20 50 100; do
      sink_reset
      run_k6 fan-in run $r 20s -e BROKER_URL="$b"
      local rps p99 ok fail s508 warn posts posts_s expected pct is_99
      read -r rps p99 ok fail s508 warn < <(k6_metrics "$SCEN_DIR/fan-in-run-$r.json")
      wait_sink_quiet 30
      read -r posts entities posts_s < <(sink_stats posts entities posts_per_second)
      expected=$((ok * 50))
      pct=$(python3 -c "print(f'{(100.0 * float($entities) / max(1, $expected)):.1f}')")
      rows+=("| $r rps | $ok | $entities | $expected | $pct% | $posts_s |")
      peak_posts_s=$posts_s
      is_99=$(python3 -c "print(1 if float('$pct') >= 99.0 and $ok > 0 else 0)")
      if [ "$is_99" -ne 1 ] && [ -z "$first_bad" ]; then
        first_bad=$r
        first_bad_pct=$pct
      fi
    done

    {
      echo "One broker matching entity updates against 50 wildcard subscriptions with high notification fan-out."
      echo
      echo "| rate | updates ok | entities notified | expected | delivered % | sink posts/s |"
      echo "|---|---|---|---|---|---|"
      for row in "${rows[@]}"; do
        echo "$row"
      done
      echo
      echo "### What this shows"
      if [ -z "$first_bad" ]; then
        echo "- Fan-out notification pipeline delivered >= 99% of matching notifications across all rates."
      else
        echo "- High fan-out caused notification drops ($first_bad_pct% delivered at $first_bad rps)."
      fi
      echo "- Sustained notification delivery reached $peak_posts_s HTTP posts/s at the sink."
    } > "$SCEN_DIR/fan-in.md"

    if [ -z "$first_bad" ]; then
      record_verdict "fan-in" "PASS" "100 rps" "delivered >= 99% at all rates, up to $peak_posts_s posts/s"
    else
      record_verdict "fan-in" "FAIL" "delivery" "delivered $first_bad_pct% at $first_bad rps (< 99%)"
      $FLEET stop s4-broker
      return 1
    fi
  fi
  $FLEET stop s4-broker
}

# ================= S5: hub-sources =================
scenario_hub_sources() {
  local ph=9105 pa=9106 pb=9107
  $FLEET start s5-hub $ph ANTARES_HOST_ALIAS=s5_hub
  $FLEET start s5-srca $pa ANTARES_HOST_ALIAS=s5_srca
  $FLEET start s5-srcb $pb ANTARES_HOST_ALIAS=s5_srcb
  tenant="hub_src"

  # Seed sources
  for src in "srca:$pa" "srcb:$pb"; do
    IFS=: read -r name sport <<<"$src"
    curl -sf -X POST "http://127.0.0.1:$sport/ngsi-ld/v1/entityOperations/create" \
      -H "NGSILD-Tenant: hub_src" -H "Content-Type: application/json" \
      -d "$(python3 -c 'import json,sys; n=sys.argv[1]; print(json.dumps([{"id": f"urn:ngsi-ld:Vehicle:hub_src:{n}{i}", "type": "Vehicle", "speed": {"type": "Property", "value": i}} for i in range(20)]))' "$name")" >/dev/null
  done

  # Register at hub
  for src in "srca:$pa" "srcb:$pb"; do
    IFS=: read -r name sport <<<"$src"
    curl -sf -X POST "http://127.0.0.1:$ph/ngsi-ld/v1/csourceRegistrations" \
      -H "NGSILD-Tenant: hub_src" -H "Content-Type: application/ld+json" \
      -d '{"id":"urn:ngsi-ld:ContextSourceRegistration:hub_src:'$name'","type":"ContextSourceRegistration","tenant":"'$tenant'","information":[{"entities":[{"type":"Vehicle"}]}],"endpoint":"http://127.0.0.1:'$sport'","@context":"'"$CTX"'"}' >/dev/null
  done

  if [ "$MODE" = "check" ]; then
    if $CHECK hub-sources --broker "http://127.0.0.1:$ph" --source "http://127.0.0.1:$pa" --source-b "http://127.0.0.1:$pb"; then
      record_verdict "hub-sources" "PASS" "4.3.6.1 complete" "federated query and direct retrieve merged across sources"
    else
      record_verdict "hub-sources" "FAIL" "read" "federated read incomplete"
      return 1
    fi
  else
    if ! $CHECK hub-sources --broker "http://127.0.0.1:$ph" --source "http://127.0.0.1:$pa" --source-b "http://127.0.0.1:$pb"; then
      record_verdict "hub-sources" "FAIL" "check failed" "federated read incomplete"
      $FLEET stop s5-hub; $FLEET stop s5-srca; $FLEET stop s5-srcb
      return 1
    fi

    local tot_fail=0 tot_warn=0
    local rows=()
    for r in 50 100 200 500; do
      run_k6 hub-sources run $r 20s -e BROKER_URL="http://127.0.0.1:$ph" -e TENANT="$tenant"
      local rps p99 ok fail s508 warn
      read -r rps p99 ok fail s508 warn < <(k6_metrics "$SCEN_DIR/hub-sources-run-$r.json")
      rows+=("| $r rps | $rps req/s | $p99 ms | $fail | $warn |")
      tot_fail=$((tot_fail + fail))
      tot_warn=$((tot_warn + warn))
    done

    {
      echo "Three brokers: one hub federating queries across two context sources (src-a and src-b) registering Vehicle entities."
      echo
      echo "| rate | req/s | p99 | failed | warnings |"
      echo "|---|---|---|---|---|"
      for row in "${rows[@]}"; do
        echo "$row"
      done
      echo
      echo "### What this shows"
      if [ "$tot_fail" -eq 0 ] && [ "$tot_warn" -eq 0 ]; then
        echo "- Hub successfully merged distributed responses with zero 5xx errors and zero source warnings."
      else
        echo "- Distributed read anomalies detected: $tot_fail failed requests, $tot_warn warning headers."
      fi
    } > "$SCEN_DIR/hub-sources.md"

    if [ "$tot_fail" -eq 0 ] && [ "$tot_warn" -eq 0 ]; then
      record_verdict "hub-sources" "PASS" "500 rps" "0 failed, 0 warnings across all rates up to 500 rps"
    else
      record_verdict "hub-sources" "FAIL" "federation" "hub-sources: $tot_fail failed, $tot_warn warnings"
      $FLEET stop s5-hub; $FLEET stop s5-srca; $FLEET stop s5-srcb
      return 1
    fi
  fi
  $FLEET stop s5-hub; $FLEET stop s5-srca; $FLEET stop s5-srcb
}

# ================= S6: collision =================
scenario_collision() {
  local ph=9108 ps=9109
  $FLEET start s6-hub $ph ANTARES_HOST_ALIAS=s6_hub
  $FLEET start s6-src $ps ANTARES_HOST_ALIAS=s6_src
  tenant="collision"

  # Hub entity
  curl -sf -X POST "http://127.0.0.1:$ph/ngsi-ld/v1/entities" -H "NGSILD-Tenant: $tenant" -H "Content-Type: application/ld+json" \
    -d '{"id":"urn:ngsi-ld:Vehicle:'$tenant':both","type":"Vehicle","brand":{"type":"Property","value":"LocalBrand"},"@context":"'"$CTX"'"}' >/dev/null
  curl -sf -X POST "http://127.0.0.1:$ph/ngsi-ld/v1/entities" -H "NGSILD-Tenant: $tenant" -H "Content-Type: application/ld+json" \
    -d '{"id":"urn:ngsi-ld:Vehicle:'$tenant':aux","type":"Vehicle","speed":{"type":"Property","value":100},"@context":"'"$CTX"'"}' >/dev/null

  # Source entity
  curl -sf -X POST "http://127.0.0.1:$ps/ngsi-ld/v1/entities" -H "NGSILD-Tenant: $tenant" -H "Content-Type: application/ld+json" \
    -d '{"id":"urn:ngsi-ld:Vehicle:'$tenant':both","type":"Vehicle","speed":{"type":"Property","value":42},"@context":"'"$CTX"'"}' >/dev/null
  curl -sf -X POST "http://127.0.0.1:$ps/ngsi-ld/v1/entities" -H "NGSILD-Tenant: $tenant" -H "Content-Type: application/ld+json" \
    -d '{"id":"urn:ngsi-ld:Vehicle:'$tenant':aux","type":"Vehicle","speed":{"type":"Property","value":999},"@context":"'"$CTX"'"}' >/dev/null

  # Register inclusive and auxiliary
  curl -sf -X POST "http://127.0.0.1:$ph/ngsi-ld/v1/csourceRegistrations" -H "NGSILD-Tenant: $tenant" -H "Content-Type: application/ld+json" \
    -d '{"id":"urn:ngsi-ld:ContextSourceRegistration:'$tenant':inc","type":"ContextSourceRegistration","tenant":"'$tenant'","mode":"inclusive","information":[{"entities":[{"id":"urn:ngsi-ld:Vehicle:'$tenant':both","type":"Vehicle"}]}],"endpoint":"http://127.0.0.1:'$ps'","@context":"'"$CTX"'"}' >/dev/null
  curl -sf -X POST "http://127.0.0.1:$ph/ngsi-ld/v1/csourceRegistrations" -H "NGSILD-Tenant: $tenant" -H "Content-Type: application/ld+json" \
    -d '{"id":"urn:ngsi-ld:ContextSourceRegistration:'$tenant':aux","type":"ContextSourceRegistration","tenant":"'$tenant'","mode":"auxiliary","operations":["retrieveOps"],"information":[{"entities":[{"id":"urn:ngsi-ld:Vehicle:'$tenant':aux","type":"Vehicle"}]}],"endpoint":"http://127.0.0.1:'$ps'","@context":"'"$CTX"'"}' >/dev/null

  if [ "$MODE" = "check" ]; then
    if $CHECK collision --broker "http://127.0.0.1:$ph" --source "http://127.0.0.1:$ps"; then
      record_verdict "collision" "PASS" "spec conformant" "4.5.5 merge, 4.3.6.2 aux local-wins, 5.9.2.4 409 conflicts"
    else
      record_verdict "collision" "FAIL" "conflict" "collision behavior violated spec"
      return 1
    fi
  else
    if ! $CHECK collision --broker "http://127.0.0.1:$ph" --source "http://127.0.0.1:$ps"; then
      record_verdict "collision" "FAIL" "check failed" "collision behavior violated spec"
      $FLEET stop s6-hub; $FLEET stop s6-src
      return 1
    fi

    local tot_fail=0
    local rows=()
    for r in 100 200 500; do
      run_k6 collision run $r 20s -e BROKER_URL="http://127.0.0.1:$ph"
      local rps p99 ok fail s508 warn
      read -r rps p99 ok fail s508 warn < <(k6_metrics "$SCEN_DIR/collision-run-$r.json")
      rows+=("| $r rps | $rps req/s | $p99 ms | $fail |")
      tot_fail=$((tot_fail + fail))
    done

    {
      echo "Two brokers (hub and source) sharing entity IDs under inclusive and auxiliary registration modes."
      echo
      echo "Correctness assertions (4.5.5 merge, 4.3.6.2 auxiliary local priority, 5.9.2.4 409 conflict on registration overlap) verified by scenario-check."
      echo
      echo "| rate | req/s | p99 | failed |"
      echo "|---|---|---|---|"
      for row in "${rows[@]}"; do
        echo "$row"
      done
      echo
      echo "### What this shows"
      if [ "$tot_fail" -eq 0 ]; then
        echo "- Merged entity reads across colliding sources completed without errors up to 500 rps."
      else
        echo "- Merged entity reads encountered $tot_fail failures under concurrent retrieval."
      fi
    } > "$SCEN_DIR/collision.md"

    if [ "$tot_fail" -eq 0 ]; then
      record_verdict "collision" "PASS" "500 rps" "0 failed across 100-500 rps, merged entity served"
    else
      record_verdict "collision" "FAIL" "conflict" "collision: $tot_fail requests failed"
      $FLEET stop s6-hub; $FLEET stop s6-src
      return 1
    fi
  fi
  $FLEET stop s6-hub; $FLEET stop s6-src
}

# ================= S7: loop =================
scenario_loop() {
  local pa=9110 pb=9111
  $FLEET start s7-a $pa ANTARES_HOST_ALIAS=antares_a
  $FLEET start s7-b $pb ANTARES_HOST_ALIAS=antares_b
  tenant="loop"

  curl -sf -X POST "http://127.0.0.1:$pa/ngsi-ld/v1/entities" -H "NGSILD-Tenant: $tenant" -H "Content-Type: application/ld+json" \
    -d '{"id":"urn:ngsi-ld:Vehicle:'$tenant':onlyA","type":"Vehicle","speed":{"type":"Property","value":10},"@context":"'"$CTX"'"}' >/dev/null
  curl -sf -X POST "http://127.0.0.1:$pb/ngsi-ld/v1/entities" -H "NGSILD-Tenant: $tenant" -H "Content-Type: application/ld+json" \
    -d '{"id":"urn:ngsi-ld:Vehicle:'$tenant':onlyB","type":"Vehicle","speed":{"type":"Property","value":20},"@context":"'"$CTX"'"}' >/dev/null

  # Cross-register
  curl -sf -X POST "http://127.0.0.1:$pa/ngsi-ld/v1/csourceRegistrations" -H "NGSILD-Tenant: $tenant" -H "Content-Type: application/ld+json" \
    -d '{"id":"urn:ngsi-ld:ContextSourceRegistration:'$tenant':toB","type":"ContextSourceRegistration","tenant":"'$tenant'","information":[{"entities":[{"type":"Vehicle"}]}],"endpoint":"http://127.0.0.1:'$pb'","@context":"'"$CTX"'"}' >/dev/null
  curl -sf -X POST "http://127.0.0.1:$pb/ngsi-ld/v1/csourceRegistrations" -H "NGSILD-Tenant: $tenant" -H "Content-Type: application/ld+json" \
    -d '{"id":"urn:ngsi-ld:ContextSourceRegistration:'$tenant':toA","type":"ContextSourceRegistration","tenant":"'$tenant'","information":[{"entities":[{"type":"Vehicle"}]}],"endpoint":"http://127.0.0.1:'$pa'","@context":"'"$CTX"'"}' >/dev/null

  if [ "$MODE" = "check" ]; then
    if $CHECK loop --broker "http://127.0.0.1:$pa" --source "http://127.0.0.1:$pb"; then
      record_verdict "loop" "PASS" "508 detected" "6.3.17 Via loop cut cycle and write returned 508"
    else
      record_verdict "loop" "FAIL" "loop" "loop detection failed"
      return 1
    fi
  else
    if ! $CHECK loop --broker "http://127.0.0.1:$pa" --source "http://127.0.0.1:$pb"; then
      record_verdict "loop" "FAIL" "check failed" "loop detection failed"
      $FLEET stop s7-a; $FLEET stop s7-b
      return 1
    fi

    local tot_fail=0 tot_508=0
    local rows=()
    for r in 50 100 200; do
      run_k6 loop run $r 20s -e BROKER_URL="http://127.0.0.1:$pa"
      local rps p99 ok fail s508 warn
      read -r rps p99 ok fail s508 warn < <(k6_metrics "$SCEN_DIR/loop-run-$r.json")
      rows+=("| $r rps | $rps req/s | $p99 ms | $fail | $s508 |")
      tot_fail=$((tot_fail + fail))
      tot_508=$((tot_508 + s508))
    done

    {
      echo "Two brokers (A and B) mutually registered in a cycle to verify loop cut and 508 detection."
      echo
      echo "| rate | req/s | p99 | failed | 508s |"
      echo "|---|---|---|---|---|"
      for row in "${rows[@]}"; do
        echo "$row"
      done
      echo
      echo "### What this shows"
      if [ "$tot_fail" -eq 0 ] && [ "$tot_508" -eq 0 ]; then
        echo "- Cross-registered queries terminated cleanly without infinite recursion or 508 errors."
      else
        echo "- Recursion issues detected: $tot_fail failed requests, $tot_508 loop detected (508) responses."
      fi
    } > "$SCEN_DIR/loop.md"

    if [ "$tot_fail" -eq 0 ] && [ "$tot_508" -eq 0 ]; then
      record_verdict "loop" "PASS" "200 rps" "0 failed, 0 loops up to 200 rps (Via chain cut cycle)"
    else
      record_verdict "loop" "FAIL" "loop" "loop: $tot_fail failed, $tot_508 508 errors"
      $FLEET stop s7-a; $FLEET stop s7-b
      return 1
    fi
  fi
  $FLEET stop s7-a; $FLEET stop s7-b
}

# ================= S8: distributed-subscription =================
scenario_distributed_subscription() {
  local ph=9112 pa=9113
  $FLEET start s8-hub $ph ANTARES_HOST_ALIAS=s8_hub ANTARES_PUBLIC_URL="http://127.0.0.1:$ph"
  $FLEET start s8-srca $pa ANTARES_HOST_ALIAS=s8_srca ANTARES_PUBLIC_URL="http://127.0.0.1:$pa"
  tenant="dist_sub"

  # Entity at source
  curl -sf -X POST "http://127.0.0.1:$pa/ngsi-ld/v1/entities" -H "NGSILD-Tenant: $tenant" -H "Content-Type: application/ld+json" \
    -d '{"id":"urn:ngsi-ld:Vehicle:'$tenant':srca0","type":"Vehicle","speed":{"type":"Property","value":10},"@context":"'"$CTX"'"}' >/dev/null

  # Registration at hub
  curl -sf -X POST "http://127.0.0.1:$ph/ngsi-ld/v1/csourceRegistrations" -H "NGSILD-Tenant: $tenant" -H "Content-Type: application/ld+json" \
    -d '{"id":"urn:ngsi-ld:ContextSourceRegistration:'$tenant':srca","type":"ContextSourceRegistration","tenant":"'$tenant'","operations":["federationOps"],"information":[{"entities":[{"type":"Vehicle"}]}],"endpoint":"http://127.0.0.1:'$pa'","@context":"'"$CTX"'"}' >/dev/null

  # Subscription at hub
  curl -sf -X POST "http://127.0.0.1:$ph/ngsi-ld/v1/subscriptions" -H "NGSILD-Tenant: $tenant" -H "Content-Type: application/ld+json" \
    -d '{"id":"urn:ngsi-ld:Subscription:'$tenant':sub","type":"Subscription","entities":[{"type":"Vehicle"}],"notification":{"endpoint":{"uri":"'$SINK_URL'/sub/distsub","accept":"application/json"}},"@context":"'"$CTX"'"}' >/dev/null

  sleep 1
  if [ "$MODE" = "check" ]; then
    if $CHECK distributed-subscription --broker "http://127.0.0.1:$ph" --source "http://127.0.0.1:$pa" --sink "$SINK_URL"; then
      record_verdict "distributed-subscription" "PASS" "5.8.1.4 verified" "remote change notified to hub subscriber"
    else
      record_verdict "distributed-subscription" "FAIL" "notification" "distributed subscription did not notify"
      return 1
    fi
  else
    if ! $CHECK distributed-subscription --broker "http://127.0.0.1:$ph" --source "http://127.0.0.1:$pa" --sink "$SINK_URL"; then
      record_verdict "distributed-subscription" "FAIL" "check failed" "remote change did not notify the hub subscriber"
      $FLEET stop s8-hub; $FLEET stop s8-srca
      return 1
    fi

    local first_bad="" first_bad_pct="" first_bad_extra=0
    local rows=()
    for r in 20 50 100; do
      sink_reset
      run_k6 distributed-subscription run $r 20s -e BROKER_URL="http://127.0.0.1:$ph" -e SOURCE_URL="http://127.0.0.1:$pa" -e TENANT="$tenant"
      local rps p99 ok fail s508 warn posts expected extra pct is_pass
      read -r rps p99 ok fail s508 warn < <(k6_metrics "$SCEN_DIR/distributed-subscription-run-$r.json")
      wait_sink_quiet 30
      posts=$(sink_stats entities)
      expected=$ok
      extra=$(python3 -c "print(max(0, $posts - $expected))")
      pct=$(python3 -c "print(f'{(100.0 * float($posts) / max(1, $expected)):.1f}')")
      rows+=("| $r rps | $ok | $posts | $expected | $extra | $pct% |")
      is_pass=$(python3 -c "print(1 if float('$pct') >= 99.0 and $extra == 0 and $ok > 0 else 0)")
      if [ "$is_pass" -ne 1 ] && [ -z "$first_bad" ]; then
        first_bad=$r
        first_bad_pct=$pct
        first_bad_extra=$extra
      fi
    done

    {
      echo "Two brokers: a hub with a mirrored subscription delegating entity change notifications from a remote source back to the subscriber."
      echo
      echo "| rate | updates ok | entities notified | expected | extra deliveries | delivered % |"
      echo "|---|---|---|---|---|---|"
      for row in "${rows[@]}"; do
        echo "$row"
      done
      echo
      echo "### What this shows"
      if [ -z "$first_bad" ]; then
        echo "- Delegated subscription updates triggered 1:1 notifications without loss or duplicate delivery."
      else
        echo "- Delivery anomalies detected: delivered $first_bad_pct%, extra deliveries $first_bad_extra at $first_bad rps."
      fi
    } > "$SCEN_DIR/distributed-subscription.md"

    if [ -z "$first_bad" ]; then
      record_verdict "distributed-subscription" "PASS" "100 rps" "delivered >= 99% with 0 extra deliveries up to 100 rps"
    else
      record_verdict "distributed-subscription" "FAIL" "notification" "delivered $first_bad_pct%, extra=$first_bad_extra at $first_bad rps"
      $FLEET stop s8-hub; $FLEET stop s8-srca
      return 1
    fi
  fi
  $FLEET stop s8-hub; $FLEET stop s8-srca
}

# ================= S9: ha-pair =================
scenario_ha_pair() {
  if [ -z "${NATS_URL:-}" ] || [ -z "${PG_URL_BASE:-}" ]; then
    echo "skipped: ha-pair requires NATS_URL and PG_URL_BASE"
    record_verdict "ha-pair" "SKIP" "none" "requires NATS and PostgreSQL"
    return 0
  fi

  local p1=9114 p2=9115
  tenant="ha_pair"
  DB_NAME="antares_ha_pair"
  if [ -n "${PG_ADMIN_URL:-}" ]; then
    psql "$PG_ADMIN_URL" -c "CREATE DATABASE \"$DB_NAME\";" >/dev/null 2>&1 || true
  fi
  PG_SHARED="$PG_URL_BASE/$DB_NAME"

  $FLEET start s9-pod1 $p1 ANTARES_HOST_ALIAS=ha_pair ANTARES_STORE=postgres ANTARES_DATABASE_URL="$PG_SHARED" ANTARES_BUS=nats ANTARES_NATS_URL="$NATS_URL"
  $FLEET start s9-pod2 $p2 ANTARES_HOST_ALIAS=ha_pair ANTARES_STORE=postgres ANTARES_DATABASE_URL="$PG_SHARED" ANTARES_BUS=nats ANTARES_NATS_URL="$NATS_URL"

  # Seed entity on pod1
  curl -sf -X POST "http://127.0.0.1:$p1/ngsi-ld/v1/entities" -H "NGSILD-Tenant: $tenant" -H "Content-Type: application/ld+json" \
    -d '{"id":"urn:ngsi-ld:Vehicle:'$tenant':0","type":"Vehicle","speed":{"type":"Property","value":10},"@context":"'"$CTX"'"}' >/dev/null

  # Subscription on pod1
  curl -sf -X POST "http://127.0.0.1:$p1/ngsi-ld/v1/subscriptions" -H "NGSILD-Tenant: $tenant" -H "Content-Type: application/ld+json" \
    -d '{"id":"urn:ngsi-ld:Subscription:'$tenant':sub","type":"Subscription","entities":[{"type":"Vehicle"}],"notification":{"endpoint":{"uri":"'$SINK_URL'/sub/ha","accept":"application/json"}},"@context":"'"$CTX"'"}' >/dev/null

  sleep 1
  if [ "$MODE" = "check" ]; then
    if $CHECK ha-pair --broker "http://127.0.0.1:$p1" --source "http://127.0.0.1:$p2" --sink "$SINK_URL"; then
      record_verdict "ha-pair" "PASS" "NATS shared ok" "writes across both pods delivered without duplicates"
    else
      record_verdict "ha-pair" "FAIL" "sync" "ha-pair notifications duplicated or lost"
      return 1
    fi
  else
    if ! $CHECK ha-pair --broker "http://127.0.0.1:$p1" --source "http://127.0.0.1:$p2" --sink "$SINK_URL"; then
      record_verdict "ha-pair" "FAIL" "check failed" "writes across both pods were not delivered exactly once"
      $FLEET stop s9-pod1; $FLEET stop s9-pod2
      return 1
    fi

    local first_bad="" first_bad_posts=0 first_bad_expected=0
    local rows=()
    for r in 50 100; do
      sink_reset
      run_k6 ha-pair run $r 20s -e BROKER_URL="http://127.0.0.1:$p1" -e SOURCE_URL="http://127.0.0.1:$p2"
      local rps p99 ok fail s508 warn posts expected extra is_pass
      read -r rps p99 ok fail s508 warn < <(k6_metrics "$SCEN_DIR/ha-pair-run-$r.json")
      wait_sink_quiet 30
      posts=$(sink_stats entities)
      expected=$ok
      extra=$(python3 -c "print(max(0, $posts - $expected))")
      rows+=("| $r rps | $ok | $posts | $expected | $extra | $p99 ms |")
      is_pass=$(python3 -c "print(1 if $posts == $expected and $ok > 0 else 0)")
      if [ "$is_pass" -ne 1 ] && [ -z "$first_bad" ]; then
        first_bad=$r
        first_bad_posts=$posts
        first_bad_expected=$expected
      fi
    done

    {
      echo "Two broker pods sharing one PostgreSQL database and NATS JetStream bus, receiving interleaved writes with shared subscription delivery."
      echo
      echo "| rate | writes ok | entities notified | expected | extra deliveries | PATCH p99 |"
      echo "|---|---|---|---|---|---|"
      for row in "${rows[@]}"; do
        echo "$row"
      done
      echo
      echo "### What this shows"
      if [ -z "$first_bad" ]; then
        echo "- Interleaved writes alternating across pod-1 and pod-2 triggered exactly one notification per change."
      else
        echo "- Delivery mismatch across HA pair: got $first_bad_posts notifications, expected $first_bad_expected at $first_bad rps."
      fi
    } > "$SCEN_DIR/ha-pair.md"

    if [ -z "$first_bad" ]; then
      record_verdict "ha-pair" "PASS" "100 rps" "notifications == expected across all rates with zero duplicates"
    else
      record_verdict "ha-pair" "FAIL" "sync" "got $first_bad_posts, expected $first_bad_expected at $first_bad rps"
      $FLEET stop s9-pod1; $FLEET stop s9-pod2
      return 1
    fi
  fi
  $FLEET stop s9-pod1; $FLEET stop s9-pod2
}

# Run requested scenarios
SCENARIOS="${*:-hot-entity noisy-tenant slow-subscriber fan-in hub-sources collision loop distributed-subscription ha-pair}"
if [ "$SCENARIOS" = "all" ]; then
  SCENARIOS="hot-entity noisy-tenant slow-subscriber fan-in hub-sources collision loop distributed-subscription ha-pair"
fi

rm -f "$SCEN_DIR/verdicts.md"
echo "| scenario | verdict | limit or key number | note |" > "$SCEN_DIR/verdicts.md"
echo "|---|---|---|---|" >> "$SCEN_DIR/verdicts.md"

overall=0
for s in $SCENARIOS; do
  fn="scenario_${s//-/_}"
  echo "=== Running scenario: $s (MODE=$MODE STORE=$STORE) ==="
  if declare -f "$fn" >/dev/null; then
    if ! "$fn"; then
      echo "FAILED: $s"
      overall=1
    fi
  else
    echo "unknown scenario: $s"
  fi
done

cat "$SCEN_DIR/verdicts.md"
exit $overall
