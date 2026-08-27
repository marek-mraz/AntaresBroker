#!/usr/bin/env bash
# Resident-set sampler for a whole rig run: the broker process and the
# Postgres backends at 1 Hz to CSV, and a verdict against the design
# budgets at the end (broker < 500 MiB, Postgres < 16 GiB).
#
#   dev/perf/rss.sh start <broker-pid> [postgres-container]   # background sampler
#   dev/perf/rss.sh stop                                      # verdict + table
#
# Env: OUT (results/perf), SCALE (asserts only at 1.0; below that the
#      peaks are printed and the exit code stays 0), BROKER_MIB (500),
#      PG_GIB (16).
set -euo pipefail
cd "$(dirname "$0")/../.."
OUT="${OUT:-results/perf}"; mkdir -p "$OUT"
CSV="$OUT/rss.csv"; PIDFILE="$OUT/rss.pid"

case "${1:-}" in
  start)
    BROKER=${2:?broker pid}; PG=${3:-}
    echo "t,broker_kib,postgres_kib" > "$CSV"
    (
      while [ -d "/proc/$BROKER" ]; do
        b=$(awk '/VmRSS/ {print $2}' "/proc/$BROKER/status" 2>/dev/null || echo 0)
        p=0
        if [ -n "$PG" ]; then
          # every backend of the server, summed (shared buffers count once
          # per process here; the ceiling is generous for that reason)
          p=$(docker exec "$PG" sh -c "awk '/VmRSS/ {s+=\$2} END {print s+0}' /proc/[0-9]*/status" 2>/dev/null || echo 0)
        fi
        echo "$(date +%s),$b,$p" >> "$CSV"
        sleep 1
      done
    ) & echo $! > "$PIDFILE"
    ;;
  stop)
    kill "$(cat "$PIDFILE")" 2>/dev/null || true; rm -f "$PIDFILE"
    python3 - "$CSV" "${SCALE:-0}" "${BROKER_MIB:-500}" "${PG_GIB:-16}" <<'EOF' | tee "$OUT/rss.md"
import csv, sys
path, scale, bmax, pmax = sys.argv[1], float(sys.argv[2]), int(sys.argv[3]), int(sys.argv[4])
rows = list(csv.DictReader(open(path)))
b = max((int(r["broker_kib"]) for r in rows), default=0) / 1024
p = max((int(r["postgres_kib"]) for r in rows), default=0) / 1024 / 1024
print(f"| broker RSS peak | {b:.0f} MiB | budget {bmax} MiB |")
print(f"| Postgres RSS peak | {p:.2f} GiB | budget {pmax} GiB |")
print(f"| samples | {len(rows)} | |")
if scale >= 1.0 and (b > bmax or p > pmax):
    print("BUDGET EXCEEDED"); sys.exit(1)
EOF
    ;;
  *) echo "usage: rss.sh start <broker-pid> [pg-container] | stop"; exit 2;;
esac
