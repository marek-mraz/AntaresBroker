#!/usr/bin/env bash
# Startup time and idle footprint per store: exec → /q/health answers 200,
# median of five, and the resident set right after.
#
#   dev/perf/startup.sh                      # memory, file
#   DATABASE_URL=postgres://… dev/perf/startup.sh   # + postgres
#
# Env: BIN (target/release/antares), OUT (results/perf), PORT (9470),
#      DATABASE_URL (adds the postgres row when set).
set -euo pipefail
cd "$(dirname "$0")/../.."

BIN="${BIN:-target/release/antares}"
OUT="${OUT:-results/perf}"
PORT="${PORT:-9470}"
mkdir -p "$OUT"
[ -x "$BIN" ] || { echo "missing $BIN (cargo build --release -p antares-broker)"; exit 1; }

DATA=$(mktemp -d)
trap 'rm -rf "$DATA"' EXIT

one() {  # one <store>: prints "<ms> <rss_kib>"
  local store=$1 t0 t1 pid rss
  t0=$(date +%s%N)
  ANTARES_STORE="$store" ANTARES_DATA_DIR="$DATA" ANTARES_HTTP_PORT="$PORT" \
    ANTARES_DATABASE_URL="${DATABASE_URL:-}" ANTARES_ALLOW_SHARED_LOCAL=1 \
    "$BIN" >/dev/null 2>&1 & pid=$!
  until curl -sf -o /dev/null "http://127.0.0.1:$PORT/q/health"; do sleep 0.005; done
  t1=$(date +%s%N)
  rss=$(awk '/VmRSS/ {print $2}' "/proc/$pid/status")
  kill "$pid"; wait "$pid" 2>/dev/null || true
  echo "$(( (t1 - t0) / 1000000 )) $rss"
}

{
  echo "| store | ready in (median of 5) | RSS after start |"
  echo "|---|---|---|"
  for store in memory file ${DATABASE_URL:+postgres}; do
    ms=(); rss=0
    for _ in 1 2 3 4 5; do
      read -r m r < <(one "$store"); ms+=("$m"); rss=$r
    done
    med=$(printf '%s\n' "${ms[@]}" | sort -n | sed -n 3p)
    echo "| $store | $med ms | $(( rss / 1024 )) MiB |"
  done
} | tee "$OUT/startup.md"
