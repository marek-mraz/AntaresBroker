#!/usr/bin/env bash
# Live Helsinki tram KPIs: HFP MQTT -> NGSI-LD tenant -> wasm -> KPI tenant -> map UI.
#
#   ./run.sh start     build the wasm module and bring the whole stack up
#   ./run.sh stop      stop everything this script started
#   ./run.sh status    what is running, and whether data is fresh
#
# The broker, both Bento pipelines and the UI run natively. Only the default
# store needs a container for its database: STORE=file or STORE=memory
# removes even that.
set -euo pipefail
cd "$(dirname "$0")"

# rustup installs here and non-login shells often miss it.
[ -d "$HOME/.cargo/bin" ] && PATH="$HOME/.cargo/bin:$PATH"

# The two Bento configs address the broker by literal port, so moving this
# means editing the url: lines in ingest.yaml and kpi.yaml with it.
BROKER_PORT="${BROKER_PORT:-42020}"
UI_PORT="${UI_PORT:-42030}"
# The memory store grows without bound under a continuous firehose (measured at
# ~450 MB/min with 110 trams), so this defaults to timescale: history lands in
# the database instead of the heap, and it is the store the temporal API and its
# aggrMethods path are actually meant to run on.
# STORE=file needs no container; STORE=memory is fine for a short throwaway run.
STORE="${STORE:-timescale}"
DB_CONTAINER="${DB_CONTAINER:-hfp-kpi-timescale}"
DATABASE_URL="${ANTARES_DATABASE_URL:-postgres://antares:antares@localhost:5432/antares}"
BROKER_BIN="${BROKER_BIN:-../../../target/release/antares}"
BENTO="${BENTO:-$(command -v bento || true)}"
LOGS=".run"

need_bento() {
  if [ -z "$BENTO" ] || [ ! -x "$BENTO" ]; then
    cat >&2 <<'EOF'
bento not found. Install the MIT fork (warpstreamlabs/bento), e.g.

  curl -sL https://github.com/warpstreamlabs/bento/releases/download/v1.20.0/bento_1.20.0_linux_arm64.tar.gz \
    | tar xz -C /usr/local/bin bento

then re-run, or point BENTO=/path/to/bento at it.
EOF
    exit 1
  fi
}

start() {
  need_bento
  [ -x "$BROKER_BIN" ] || { echo "broker not built: cargo build --release -p antares-broker" >&2; exit 1; }
  mkdir -p "$LOGS"

  echo "building the wasm module (tests first)…"
  ( cd wasm-kpi && cargo test -q && cargo build --release --target wasm32-unknown-unknown -q )

  if [ "$STORE" = "timescale" ] || [ "$STORE" = "postgres" ]; then
    if ! docker exec "$DB_CONTAINER" pg_isready -U antares >/dev/null 2>&1; then
      echo "starting $DB_CONTAINER…"
      docker rm -f "$DB_CONTAINER" >/dev/null 2>&1 || true
      docker run -d --name "$DB_CONTAINER" \
        -e POSTGRES_USER=antares -e POSTGRES_PASSWORD=antares -e POSTGRES_DB=antares \
        -e TS_TUNE_MEMORY=2GB -e TS_TUNE_NUM_CPUS=2 \
        -p 127.0.0.1:5432:5432 timescale/timescaledb-ha:pg17 >/dev/null
      until docker exec "$DB_CONTAINER" pg_isready -U antares >/dev/null 2>&1; do sleep 2; done
    fi
    echo "database ready"
  fi

  echo "starting broker on :$BROKER_PORT (store=$STORE)…"
  # ANTARES_SWEEP_SECS is the clause 4.22 expiry GC interval. Left at its
  # default, expired entities linger for many minutes and a dead feed looks
  # like a live one; 10 s makes staleness visible.
  ANTARES_STORE="$STORE" \
  ANTARES_DATABASE_URL="$DATABASE_URL" \
  ANTARES_ALLOW_SHARED_LOCAL=1 \
  ANTARES_SWEEP_SECS=10 \
  ANTARES_MAX_BATCH_ITEMS=5000 \
  ANTARES_HTTP_PORT="$BROKER_PORT" \
    nohup "$BROKER_BIN" > "$LOGS/broker.log" 2>&1 &
  sleep 5

  echo "starting HFP ingest (live mqtt.hsl.fi)…"
  nohup "$BENTO" -c ingest.yaml > "$LOGS/ingest.log" 2>&1 &

  echo "starting KPI pass…"
  nohup "$BENTO" -c kpi.yaml > "$LOGS/kpi.log" 2>&1 &

  echo "starting UI on :$UI_PORT…"
  ( cd ui && nohup python3 serve.py > "../$LOGS/ui.log" 2>&1 & )

  sleep 2
  echo
  echo "  open http://localhost:$UI_PORT"
  echo "  KPIs appear within ~20 s; the headway column needs ~15 min of arrivals."
}

stop() {
  # Match on the config/script name so this never kills an unrelated broker.
  for p in /proc/[0-9]*; do
    c=$(tr '\0' ' ' < "$p/cmdline" 2>/dev/null) || continue
    case "$c" in
      *release/antares*|*-c\ ingest.yaml*|*-c\ kpi.yaml*|*serve.py*)
        kill "${p#/proc/}" 2>/dev/null || true ;;
    esac
  done
  echo "stopped"
}

status() {
  for p in /proc/[0-9]*; do
    c=$(tr '\0' ' ' < "$p/cmdline" 2>/dev/null) || continue
    rss=$(awk '/VmRSS/{print $2/1024}' "$p/status" 2>/dev/null || echo 0)
    case "$c" in
      *release/antares*)     [ "${rss%.*}" -gt 50 ] && printf '  broker  %6.0f MB\n' "$rss" ;;
      *-c\ ingest.yaml*)     printf '  ingest  %6.0f MB\n' "$rss" ;;
      *-c\ kpi.yaml*)        printf '  kpi     %6.0f MB\n' "$rss" ;;
      *serve.py*)            printf '  ui      %6.0f MB\n' "$rss" ;;
    esac
  done
  echo
  for t in "helsinki:Vehicle" "helsinki:StopArrival" "helsinki-kpi:TransportKPI"; do
    tenant="${t%%:*}"; type="${t##*:}"
    n=$(curl -s -m 5 "http://localhost:$BROKER_PORT/ngsi-ld/v1/entities?type=$type&format=keyValues&limit=1000" \
        -H "NGSILD-Tenant: $tenant" 2>/dev/null \
        | python3 -c 'import json,sys;d=json.load(sys.stdin);print(len(d) if isinstance(d,list) else 0)' 2>/dev/null || echo "?")
    printf '  %-14s %-14s %s\n' "$tenant" "$type" "$n"
  done
}

case "${1:-start}" in
  start) start ;;
  stop) stop ;;
  status) status ;;
  *) echo "usage: $0 {start|stop|status}" >&2; exit 2 ;;
esac
