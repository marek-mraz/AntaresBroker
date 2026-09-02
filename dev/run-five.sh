#!/usr/bin/env bash
# Launch 5 Antares brokers (ports 9090..9094) — the Scorpio IOP-stack
# equivalent for DistributedOperations and IOP suites, no Docker needed
# (in-memory store, bus=local per instance).
set -euo pipefail
cd "$(dirname "$0")/.."
export PATH="$HOME/.cargo/bin:$PATH"
cargo build --release -p antares-broker
# a shared build.target-dir puts the binary outside the repository
TARGET=$(cargo metadata --format-version 1 --no-deps | python3 -c 'import json,sys;print(json.load(sys.stdin)["target_directory"])')
BIN="$TARGET/release/antares"

for p in $(ls /proc | grep -E '^[0-9]+$'); do
  case "$(tr '\0' ' ' < /proc/$p/cmdline 2>/dev/null)" in
    "$BIN"*) kill "$p" 2>/dev/null || true;;
  esac
done
sleep 0.5

LOGDIR="${BROKER_LOG_DIR:-/tmp/antares-logs}"
mkdir -p "$LOGDIR"
for i in 0 1 2 3 4; do
  port=$((9090 + i))
  # ANTARES_PUBLIC_URL: distributed subscriptions (5.8.1.4) hand this URL to
  # the peer broker as the notification endpoint — the default
  # http://{host_alias} is not resolvable between local processes.
  ANTARES_HTTP_PORT=$port ANTARES_HOST_ALIAS="antares$((i + 1))" \
    ANTARES_PUBLIC_URL="http://localhost:$port" \
    nohup "$BIN" > "$LOGDIR/broker$((i + 1)).log" 2>&1 &
done
sleep 1
for i in 0 1 2 3 4; do
  curl -sf "localhost:$((9090 + i))/q/health" >/dev/null \
    && echo "broker$((i + 1)) up on :$((9090 + i))" \
    || { echo "broker$((i + 1)) FAILED"; exit 1; }
done
