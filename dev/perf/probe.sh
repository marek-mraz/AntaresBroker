# Sourced by the perf scripts: what one broker process cost during a
# measured window, from /proc alone (no runtime metrics feature needed).
#
#   probe_start <pid>          # begin the window
#   probe_stop                 # prints "<cores used> <peak threads>"
#
# cores used = CPU seconds (utime+stime, all threads) over wall seconds;
# peak threads = the largest `Threads:` line of /proc/<pid>/status at
# 1 Hz, which is where a blocking-thread ceiling shows: a parked store
# call is one OS thread, so a count near max_blocking_threads (the
# connection cap + 1024) means the cliff, not the cores, bounded the run.

probe_start() {
  PROBE_PID=$1
  PROBE_T0=$(date +%s.%N)
  PROBE_J0=$(awk '{print $14+$15}' "/proc/$PROBE_PID/stat")
  PROBE_FILE=$(mktemp)
  ( while [ -d "/proc/$PROBE_PID" ]; do
      awk '/^Threads:/ {print $2}' "/proc/$PROBE_PID/status" 2>/dev/null
      sleep 1
    done > "$PROBE_FILE" ) &
  PROBE_SAMPLER=$!
}

probe_stop() {
  local j1 t1
  j1=$(awk '{print $14+$15}' "/proc/$PROBE_PID/stat")
  t1=$(date +%s.%N)
  kill "$PROBE_SAMPLER" 2>/dev/null; wait "$PROBE_SAMPLER" 2>/dev/null || true
  python3 - "$PROBE_J0" "$j1" "$PROBE_T0" "$t1" "$(getconf CLK_TCK)" "$PROBE_FILE" <<'PY'
import sys
j0, j1, t0, t1, tick, path = sys.argv[1:]
cores = (int(j1) - int(j0)) / int(tick) / max(1e-9, float(t1) - float(t0))
threads = max((int(x) for x in open(path).read().split()), default=0)
print(f"{cores:.2f} {threads}")
PY
  rm -f "$PROBE_FILE"
}
