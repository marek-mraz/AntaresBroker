#!/usr/bin/env bash
# Start and stop N Antares brokers from one binary.
#
#   dev/perf/fleet.sh start <name> <port> [KEY=VALUE ...]
#   dev/perf/fleet.sh stop <name>
#   dev/perf/fleet.sh stop-all
#
# Env: BIN (target/release/antares), STORE (memory|postgres),
#      PG_URL_BASE (postgresql://antares:antares@127.0.0.1:5432),
#      PG_ADMIN_URL, OUT (results/perf).
set -euo pipefail

BIN="${BIN:-target/release/antares}"
STORE="${STORE:-memory}"
OUT="${OUT:-results/perf}"
SCEN_DIR="$OUT/scenarios"
mkdir -p "$SCEN_DIR"

case "${1:-}" in
  start)
    NAME="${2:?broker name}"
    PORT="${3:?port}"
    shift 3

    PIDFILE="$SCEN_DIR/$NAME.pid"
    LOGFILE="$SCEN_DIR/$NAME.log"

    if [ -f "$PIDFILE" ]; then
      pid=$(cat "$PIDFILE" 2>/dev/null || true)
      if [ -n "$pid" ] && [ -d "/proc/$pid" ]; then
        echo "fleet: broker $NAME already running (pid $pid)"
        exit 0
      fi
      rm -f "$PIDFILE"
    fi

    # Set up per-broker environment
    export ANTARES_HTTP_PORT="$PORT"
    export ANTARES_EGRESS_ALLOW_PRIVATE=true
    export ANTARES_STORE="$STORE"
    # the Via pseudonym every forward carries (6.3.18): one alias per broker
    export ANTARES_HOST_ALIAS="${ANTARES_HOST_ALIAS:-$NAME}"

    BUS_NATS=0
    DB_URL_GIVEN=0
    for kv in "$@"; do
      case "$kv" in
        ANTARES_BUS=nats) BUS_NATS=1 ;;
        ANTARES_DATABASE_URL=*) DB_URL_GIVEN=1 ;;
      esac
      export "$kv"
    done

    if [ "$STORE" = "postgres" ]; then
      # one database per broker unless the caller names one: pods of an
      # HA pair share theirs
      if [ "$DB_URL_GIVEN" -eq 0 ]; then
        PG_BASE="${PG_URL_BASE:-postgresql://antares:antares@127.0.0.1:5432}"
        DB_NAME="antares_${NAME//-/_}"
        export ANTARES_DATABASE_URL="$PG_BASE/$DB_NAME"
        if [ -n "${PG_ADMIN_URL:-}" ]; then
          psql "$PG_ADMIN_URL" -c "CREATE DATABASE \"$DB_NAME\";" >/dev/null 2>&1 || true
        fi
      fi
      if [ "$BUS_NATS" -eq 0 ]; then
        export ANTARES_ALLOW_SHARED_LOCAL=1
      fi
    fi

    "$BIN" > "$LOGFILE" 2>&1 &
    PID=$!
    echo "$PID" > "$PIDFILE"

    # Wait for /q/health up to 30 s
    ready=0
    for _ in $(seq 60); do
      if curl -sf -o /dev/null "http://127.0.0.1:$PORT/q/health"; then
        ready=1
        break
      fi
      if ! kill -0 "$PID" 2>/dev/null; then
        echo "fleet: broker $NAME died on startup; check $LOGFILE" >&2
        cat "$LOGFILE" >&2
        exit 1
      fi
      sleep 0.5
    done

    if [ "$ready" -ne 1 ]; then
      echo "fleet: broker $NAME on port $PORT failed to answer /q/health within 30 s" >&2
      kill "$PID" 2>/dev/null || true
      rm -f "$PIDFILE"
      exit 1
    fi
    ;;

  stop)
    NAME="${2:?broker name}"
    PIDFILE="$SCEN_DIR/$NAME.pid"
    if [ -f "$PIDFILE" ]; then
      pid=$(cat "$PIDFILE" 2>/dev/null || true)
      if [ -n "$pid" ] && [ -d "/proc/$pid" ]; then
        kill "$pid" 2>/dev/null || true
        for _ in $(seq 30); do
          [ -d "/proc/$pid" ] || break
          sleep 0.1
        done
        if [ -d "/proc/$pid" ]; then
          kill -9 "$pid" 2>/dev/null || true
        fi
      fi
      rm -f "$PIDFILE"
    fi
    ;;

  stop-all)
    for pf in "$SCEN_DIR"/*.pid; do
      [ -f "$pf" ] || continue
      pid=$(cat "$pf" 2>/dev/null || true)
      if [ -n "$pid" ] && [ -d "/proc/$pid" ]; then
        kill "$pid" 2>/dev/null || true
        for _ in $(seq 20); do
          [ -d "/proc/$pid" ] || break
          sleep 0.1
        done
        if [ -d "/proc/$pid" ]; then
          kill -9 "$pid" 2>/dev/null || true
        fi
      fi
      rm -f "$pf"
    done
    ;;

  *)
    echo "usage: fleet.sh start <name> <port> [KEY=VALUE ...] | stop <name> | stop-all" >&2
    exit 2
    ;;
esac
