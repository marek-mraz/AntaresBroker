#!/usr/bin/env bash
# One benchmark pass: release broker (memory store) + the k6 baseline
# scenario, summary exported as JSON. Repeat N times with REPEATS to feed
# dev/perf/variance.py; the noise profile from repeated same-commit passes
# must exist before anyone writes a regression gate against these numbers.
#
# Env: REPEATS (default 1), RATE, DURATION (k6 knobs), OUT (default
#      results/perf), ANTARES_HTTP_PORT (default 9090).
# Record alongside the numbers which tuning knobs the host actually
# honoured (governor, no_turbo, ASLR) — inside a VM several are read-only.
set -euo pipefail
cd "$(dirname "$0")/../.."

OUT="${OUT:-results/perf}"
PORT="${ANTARES_HTTP_PORT:-9090}"
REPEATS="${REPEATS:-1}"
mkdir -p "$OUT"

command -v k6 >/dev/null || { echo "k6 missing"; exit 1; }

{
  echo "commit: $(git rev-parse HEAD)"
  echo "host: $(uname -m) $(nproc) cpus"
  for k in /sys/devices/system/cpu/cpu0/cpufreq/scaling_governor \
           /sys/devices/system/cpu/intel_pstate/no_turbo \
           /proc/sys/kernel/randomize_va_space; do
    [ -r "$k" ] && echo "$k: $(cat "$k")" || echo "$k: unavailable"
  done
} > "$OUT/run-metadata.txt"

cargo build -q --release -p antares-broker -j 2

for i in $(seq "$REPEATS"); do
  ANTARES_HTTP_PORT="$PORT" ./target/release/antares \
    > "$OUT/broker-$i.log" 2>&1 &
  BROKER=$!
  trap 'kill -TERM $BROKER 2>/dev/null || true' EXIT
  for _ in $(seq 60); do
    curl -sf "http://localhost:$PORT/q/health" >/dev/null && break
    sleep 1
  done
  BROKER_URL="http://localhost:$PORT" \
    k6 run --summary-export "$OUT/summary-$i.json" --quiet \
    dev/perf/k6-baseline.js
  kill -TERM "$BROKER"; wait "$BROKER" 2>/dev/null || true
  trap - EXIT
done

[ "$REPEATS" -gt 1 ] && python3 dev/perf/variance.py "$OUT"/summary-*.json \
  | tee "$OUT/noise-profile.txt"
echo "results in $OUT/"
