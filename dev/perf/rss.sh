#!/usr/bin/env bash
# Resident-set sampler for a whole rig run: the broker process, the
# Postgres container, and the rig's own processes (k6, the sink, mosquitto)
# by name, RSS + CPU % of each at 1 Hz to CSV, and the peaks at the end
# (BROKER_MIB / PG_GIB set, the exit code says whether they held). A
# `phase` column mirrors $OUT/phase, which the perf scripts write as they
# move between stages, so a chart can be cut per stage.
#
#   dev/perf/rss.sh start <broker-pid> [postgres-container]   # background sampler
#   dev/perf/rss.sh stop                                      # verdict + table
#
# Env: METRICS_URL (e.g. http://127.0.0.1:9090/q/metrics — a Prometheus
#      snapshot lands in $OUT/metrics/<epoch>.prom every 15 s, a time
#      series any later analysis can diff), OUT (results/perf), SCALE (asserts only at 1.0; below that the
#      peaks are printed and the exit code stays 0), BROKER_MIB, PG_GIB
#      (ceilings in MiB / GiB; unset = report only).
set -euo pipefail
cd "$(dirname "$0")/../.."
OUT="${OUT:-results/perf}"; mkdir -p "$OUT"
CSV="$OUT/rss.csv"; PIDFILE="$OUT/rss.pid"

case "${1:-}" in
  start)
    BROKER=${2:?broker pid}; PG=${3:-}
    # The last two path components, not argv[0] whole: the stages start their
    # own brokers from the same binary by a different path form (`./target/...`
    # against `target/...`), and a match on the leading form misses every one
    # of them — the broker column then reports the idle first process while a
    # stage's broker does the work. Two components rather than the bare name
    # so a `psql` connecting to a database of the same name cannot match.
    BROKER_NAME=$(tr '\0' ' ' < "/proc/$BROKER/cmdline" | awk '{print $1}')
    BROKER_NAME=$(printf '%s' "$BROKER_NAME" | awk -F/ '{ print (NF > 1 ? $(NF-1) "/" $NF : $NF) }')
    echo "t,broker_kib,postgres_kib,broker_cpu_pct,postgres_cpu_pct,host_busy_cores,host_cores,k6_kib,k6_cpu_pct,sink_kib,sink_cpu_pct,mqtt_kib,mqtt_cpu_pct,phase" > "$CSV"
    (
      # CPU % = jiffies burnt per wall second (utime+stime, all threads);
      # 100 = one core. Postgres: every process in the container, summed.
      # host_busy_cores: whole-machine non-idle share of /proc/stat times
      # the core count — the "how many cores are saturated" number.
      tick=$(getconf CLK_TCK); bj0=0; pj0=0; t0=$(date +%s.%N)
      CG=""
      if [ -n "$PG" ]; then
        id=$(docker inspect -f '{{.Id}}' "$PG" 2>/dev/null || true)
        for c in "/sys/fs/cgroup/system.slice/docker-$id.scope" "/sys/fs/cgroup/docker/$id"; do
          [ -f "$c/memory.current" ] && CG=$c && break
        done
        [ -z "$CG" ] && echo "rss.sh: no cgroup for container $PG, Postgres columns stay 0" >&2
      fi
      cores=$(nproc); hb0=0; ht0=0; kj0=0; sj0=0; mj0=0; n=0
      [ -n "${METRICS_URL:-}" ] && mkdir -p "$OUT/metrics"
      # by_name <substring>: summed RSS KiB and jiffies of every process
      # whose cmdline carries it — one argument, never a phrase: /proc cmdline
      # separates arguments with NUL (k6 is a fresh process per stage)
      by_name() {
        local rss=0 jf=0 d
        for d in $(/usr/bin/grep -l -a -- "$1" /proc/[0-9]*/cmdline 2>/dev/null); do
          d=${d%/cmdline}
          # never the sampler itself (its own command line names every pattern)
          /usr/bin/grep -q -a "rss.sh" "$d/cmdline" 2>/dev/null && continue
          rss=$(( rss + $(awk '/VmRSS/ {print $2}' "$d/status" 2>/dev/null || echo 0) ))
          jf=$(( jf + $(awk '{print $14+$15}' "$d/stat" 2>/dev/null || echo 0) ))
        done
        echo "$rss $jf"
      }
      while [ -d "/proc/$BROKER" ]; do
        read -r hb ht < <(awk '/^cpu / {busy=$2+$3+$4+$7+$8+$9; total=busy+$5+$6; print busy, total}' /proc/stat)
        # every broker process, not only the pid the loop watches: the
        # shapes, saturation and startup stages run their own brokers
        read -r b bj < <(by_name "$BROKER_NAME")
        p=0; pj=0
        if [ -n "$CG" ]; then
          # the container's cgroup, read from the host: memory.current
          # (not a per-backend RSS sum, which counted the shared buffers
          # once per backend: 50 GiB "RSS" on a 32 GB machine) and
          # cpu.stat usage_usec converted to jiffies
          p=$(awk '{print int($1/1024)}' "$CG/memory.current" 2>/dev/null || echo 0)
          pj=$(awk -v tick="$tick" '/^usage_usec/ {print int($2 * tick / 1000000)}' "$CG/cpu.stat" 2>/dev/null || echo 0)
        fi
        read -r k kj < <(by_name "k6")
        read -r sk sj < <(by_name "perf/sink.py")
        read -r mk mj < <(by_name "mosquitto")
        t1=$(date +%s.%N)
        read -r kc sc mc < <(awk -v a="$kj" -v a0="$kj0" -v b="$sj" -v b0="$sj0" -v c="$mj" -v c0="$mj0" -v t0="$t0" -v t1="$t1" -v tick="$tick" \
          'function d(x, y) { return x > y ? x - y : 0 } BEGIN { dt = t1 - t0; if (dt <= 0) dt = 1; printf "%.1f %.1f %.1f\n", d(a,a0)*100/tick/dt, d(b,b0)*100/tick/dt, d(c,c0)*100/tick/dt }')
        read -r bc pc < <(awk -v bj="$bj" -v bj0="$bj0" -v pj="$pj" -v pj0="$pj0" -v t0="$t0" -v t1="$t1" -v tick="$tick" \
          'function d(x, y) { return x > y ? x - y : 0 } BEGIN { dt = t1 - t0; if (dt <= 0) dt = 1; printf "%.1f %.1f\n", d(bj,bj0)*100/tick/dt, d(pj,pj0)*100/tick/dt }')
        hc=$(awk -v hb="$hb" -v hb0="$hb0" -v ht="$ht" -v ht0="$ht0" -v cores="$cores" \
          'BEGIN { d = ht - ht0; if (d <= 0) d = 1; printf "%.2f", (hb - hb0) / d * cores }')
        # The first pass has no earlier reading to difference against: every
        # counter starts at 0, so its deltas are the whole since-boot total
        # over a near-zero wall delta (a 34-core Postgres on an 8-core box).
        # It primes the baselines and writes nothing.
        if [ "$n" -gt 0 ]; then
          echo "$(date +%s),$b,$p,$bc,$pc,$hc,$cores,$k,$kc,$sk,$sc,$mk,$mc,$(cat "$OUT/phase" 2>/dev/null | tr -d ',\n')" >> "$CSV"
        fi
        bj0=$bj; pj0=$pj; t0=$t1; hb0=$hb; ht0=$ht; kj0=$kj; sj0=$sj; mj0=$mj
        n=$((n + 1))
        if [ -n "${METRICS_URL:-}" ] && [ $((n % 15)) -eq 1 ]; then
          # bounded: a broker queueing requests held this curl for good and
          # the sampler with it, so later phases had no rows at all
          curl -sf --max-time 2 "$METRICS_URL" > "$OUT/metrics/$(date +%s).prom" 2>/dev/null || true
        fi
        sleep 1
      done
    ) & echo $! > "$PIDFILE"
    ;;
  stop)
    kill "$(cat "$PIDFILE")" 2>/dev/null || true; rm -f "$PIDFILE"
    python3 - "$CSV" "${SCALE:-0}" "${BROKER_MIB:-0}" "${PG_GIB:-0}" <<'EOF' | tee "$OUT/rss.md"
import csv, sys
path, scale, bmax, pmax = sys.argv[1], float(sys.argv[2]), int(sys.argv[3]), int(sys.argv[4])
rows = list(csv.DictReader(open(path)))
b = max((int(r["broker_kib"]) for r in rows), default=0) / 1024
p = max((int(r["postgres_kib"]) for r in rows), default=0) / 1024 / 1024
cpu = lambda k: max((float(r.get(k) or 0) for r in rows), default=0.0)
avg = lambda k: (sum(float(r.get(k) or 0) for r in rows) / len(rows)) if rows else 0.0
print(f"| broker RSS peak | {b:.0f} MiB | {'ceiling ' + str(bmax) + ' MiB' if bmax else 'no ceiling set'} |")
print(f"| Postgres RSS peak | {p:.2f} GiB | {'ceiling ' + str(pmax) + ' GiB' if pmax else 'no ceiling set'} |")
cores = int(float(rows[0].get("host_cores") or 0)) if rows else 0
print(f"| broker CPU peak / mean | {cpu('broker_cpu_pct')/100:.1f} / {avg('broker_cpu_pct')/100:.1f} cores | of {cores} |")
print(f"| Postgres CPU peak / mean | {cpu('postgres_cpu_pct')/100:.1f} / {avg('postgres_cpu_pct')/100:.1f} cores | of {cores} |")
print(f"| host busy peak / mean | {cpu('host_busy_cores'):.1f} / {avg('host_busy_cores'):.1f} cores | of {cores}: saturated when peak ≈ {cores} |")
for name, key in (("k6", "k6"), ("sink", "sink"), ("mosquitto", "mqtt")):
    if any(r.get(f"{key}_kib") for r in rows):
        print(f"| {name} RSS peak / CPU peak | {max(int(r.get(f'{key}_kib') or 0) for r in rows)/1024:.0f} MiB / {cpu(f'{key}_cpu_pct')/100:.1f} cores | rig, not the broker |")
print(f"| samples | {len(rows)} | rss.csv, {(int(rows[-1]['t'])-int(rows[0]['t']))/max(1,len(rows)-1):.1f} s apart |")
if scale >= 1.0 and ((bmax and b > bmax) or (pmax and p > pmax)):
    print("BUDGET EXCEEDED"); sys.exit(1)
EOF
    ;;
  *) echo "usage: rss.sh start <broker-pid> [pg-container] | stop"; exit 2;;
esac
