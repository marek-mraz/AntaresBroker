#!/usr/bin/env bash
# Resident-set sampler for a whole rig run: the broker process and the
# Postgres backends, plus CPU % of both, at 1 Hz to CSV, and a verdict against the design
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
    echo "t,broker_kib,postgres_kib,broker_cpu_pct,postgres_cpu_pct,host_busy_cores,host_cores" > "$CSV"
    (
      # CPU % = jiffies burnt per wall second (utime+stime, all threads);
      # 100 = one core. Postgres: every process in the container, summed.
      # host_busy_cores: whole-machine non-idle share of /proc/stat times
      # the core count — the "how many cores are saturated" number.
      tick=$(getconf CLK_TCK); bj0=0; pj0=0; t0=$(date +%s.%N)
      cores=$(nproc); hb0=0; ht0=0
      while [ -d "/proc/$BROKER" ]; do
        read -r hb ht < <(awk '/^cpu / {busy=$2+$3+$4+$7+$8+$9; total=busy+$5+$6; print busy, total}' /proc/stat)
        b=$(awk '/VmRSS/ {print $2}' "/proc/$BROKER/status" 2>/dev/null || echo 0)
        bj=$(awk '{print $14+$15}' "/proc/$BROKER/stat" 2>/dev/null || echo 0)
        p=0; pj=0
        if [ -n "$PG" ]; then
          # every backend of the server, summed (shared buffers count once
          # per process here; the ceiling is generous for that reason)
          p=$(docker exec "$PG" sh -c "awk '/VmRSS/ {s+=\$2} END {print s+0}' /proc/[0-9]*/status" 2>/dev/null || echo 0)
          pj=$(docker exec "$PG" sh -c "cat /proc/[0-9]*/stat 2>/dev/null | awk '{s+=\$14+\$15} END {print s+0}'" 2>/dev/null || echo 0)
        fi
        t1=$(date +%s.%N)
        read -r bc pc < <(awk -v bj="$bj" -v bj0="$bj0" -v pj="$pj" -v pj0="$pj0" -v t0="$t0" -v t1="$t1" -v tick="$tick" \
          'BEGIN { dt = t1 - t0; if (dt <= 0) dt = 1; printf "%.1f %.1f\n", (bj-bj0)*100/tick/dt, (pj-pj0)*100/tick/dt }')
        hc=$(awk -v hb="$hb" -v hb0="$hb0" -v ht="$ht" -v ht0="$ht0" -v cores="$cores" \
          'BEGIN { d = ht - ht0; if (d <= 0) d = 1; printf "%.2f", (hb - hb0) / d * cores }')
        echo "$(date +%s),$b,$p,$bc,$pc,$hc,$cores" >> "$CSV"
        bj0=$bj; pj0=$pj; t0=$t1; hb0=$hb; ht0=$ht
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
cpu = lambda k: max((float(r.get(k) or 0) for r in rows), default=0.0)
avg = lambda k: (sum(float(r.get(k) or 0) for r in rows) / len(rows)) if rows else 0.0
print(f"| broker RSS peak | {b:.0f} MiB | budget {bmax} MiB |")
print(f"| Postgres RSS peak | {p:.2f} GiB | budget {pmax} GiB |")
cores = int(float(rows[0].get("host_cores") or 0)) if rows else 0
print(f"| broker CPU peak / mean | {cpu('broker_cpu_pct')/100:.1f} / {avg('broker_cpu_pct')/100:.1f} cores | of {cores} |")
print(f"| Postgres CPU peak / mean | {cpu('postgres_cpu_pct')/100:.1f} / {avg('postgres_cpu_pct')/100:.1f} cores | of {cores} |")
print(f"| host busy peak / mean | {cpu('host_busy_cores'):.1f} / {avg('host_busy_cores'):.1f} cores | of {cores}: saturated when peak ≈ {cores} |")
print(f"| samples | {len(rows)} | 1 Hz, rss.csv |")
if scale >= 1.0 and (b > bmax or p > pmax):
    print("BUDGET EXCEEDED"); sys.exit(1)
EOF
    ;;
  *) echo "usage: rss.sh start <broker-pid> [pg-container] | stop"; exit 2;;
esac
