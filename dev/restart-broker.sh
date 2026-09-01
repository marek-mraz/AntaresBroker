#!/usr/bin/env bash
# Rebuild (release) + restart the local Antares broker on :9090.
set -euo pipefail
cd "$(dirname "$0")/.."
export PATH="$HOME/.cargo/bin:$PATH"
cargo build --release -p antares-broker
for p in $(ls /proc | grep -E '^[0-9]+$'); do
  case "$(tr '\0' ' ' < /proc/$p/cmdline 2>/dev/null)" in
    *target/release/antares*) kill "$p" 2>/dev/null || true;;
  esac
done
sleep 0.5
LOG="${BROKER_LOG:-/tmp/broker.log}"
nohup ./target/release/antares > "$LOG" 2>&1 &
sleep 0.7
curl -sf localhost:9090/q/health >/dev/null && echo "broker up"
