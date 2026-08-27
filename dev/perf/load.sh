#!/usr/bin/env bash
# Load the design-target dataset, scaled: entities by bulk COPY, then
# subscriptions and context source registrations through the API.
#
#   SCALE=0.0001 DATABASE_URL=postgres://… BROKER_URL=http://localhost:9090 dev/perf/load.sh
#
# SCALE=1 is the README target row: 100,000,000 entities over 10,000
# tenants, 100,000 subscriptions, 100,000 registrations. Every count
# scales linearly (tenants never below 10, the others never below 1).
# The broker must be running with ANTARES_EGRESS_ALLOW_PRIVATE=true so it
# may notify the sink on 127.0.0.1; the entity stage runs while it is up
# only because bulk-load.sh touches no table the broker holds locks on —
# nothing may QUERY during that stage (indexes are down).
#
# Env: SCALE, DATABASE_URL, BROKER_URL, SINK_PORT (9800), MQTT_URL (unset =
#      HTTP only), OUT (results/perf), BULK (the loader command; default
#      dev/bulk-load.sh, set to `docker exec … bash /tmp/bulk-load.sh` when
#      psql lives in a container).
set -euo pipefail
cd "$(dirname "$0")/../.."

SCALE="${SCALE:-0.0001}"
OUT="${OUT:-results/perf}"
SINK_PORT="${SINK_PORT:-9800}"
BROKER_URL="${BROKER_URL:-http://localhost:9090}"
: "${DATABASE_URL:?set DATABASE_URL}"
mkdir -p "$OUT"

n() { python3 -c "import math,sys; print(max(int(sys.argv[2]), int(round($1 * float(sys.argv[1])))))" "$SCALE" "$2"; }
ENTITIES=$(n 100000000 1); TENANTS=$(n 10000 10); SUBS="${SUBS:-$(n 100000 1)}"; CSRS="${CSRS:-$(n 100000 1)}"
echo "scale $SCALE: $ENTITIES entities / $TENANTS tenants / $SUBS subscriptions / $CSRS registrations" | tee "$OUT/load.md"

# the sink outlives this script: the measurements after the load need it
if ! curl -sf "http://127.0.0.1:$SINK_PORT/stats" >/dev/null; then
  nohup python3 dev/perf/sink.py "$SINK_PORT" > "$OUT/sink.log" 2>&1 &
  echo $! > "$OUT/sink.pid"
fi

stage() {  # stage <name> <cmd…>: wall time per stage into load.md
  local name=$1; shift; local t0; t0=$(date +%s)
  "$@"
  echo "- $name: $(( $(date +%s) - t0 )) s" | tee -a "$OUT/load.md"
}

FIFO="$OUT/entities.fifo"; rm -f "$FIFO"; mkfifo "$FIFO"
python3 dev/perf/gen.py --entities "$ENTITIES" --tenants "$TENANTS" > "$FIFO" & GEN=$!
stage "entities ($ENTITIES, bulk COPY)" bash -c "${BULK:-dev/bulk-load.sh} '$FIFO' | tail -1"
wait $GEN; rm -f "$FIFO"

stage "subscriptions ($SUBS)" python3 dev/perf/api-load.py subscriptions --count "$SUBS" --tenants "$TENANTS" --out "$OUT" \
  --broker "$BROKER_URL" --sink "http://127.0.0.1:$SINK_PORT" ${MQTT_URL:+--mqtt "$MQTT_URL"}
stage "registrations ($CSRS)" python3 dev/perf/api-load.py registrations --count "$CSRS" --tenants "$TENANTS" --out "$OUT" \
  --broker "$BROKER_URL" --sink "http://127.0.0.1:$SINK_PORT"

echo "loaded; sink stats at http://127.0.0.1:$SINK_PORT/stats" | tee -a "$OUT/load.md"
