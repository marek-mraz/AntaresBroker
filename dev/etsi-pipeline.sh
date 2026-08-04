#!/usr/bin/env bash
# ONE ETSI pipeline — identical locally and in CI (the ScorpioBroker
# dev/etsi-serial.sh pattern: the workflow is a thin wrapper around THIS).
#
# Stack: compose-files/docker-compose-etsi.yml — 5 brokers + 5 databases.
# Serial suites run against broker1, the IOP suite against all five.
#
# Env knobs (defaults = local dev loop; CI overrides them):
#   STORE=memory|postgres|timescale  store mode under test        (default memory)
#   STOP_ON_ERROR=1                  halt at the FIRST failing TP  (CI sets 0)
#   SKIP_BUILD=1                     reuse antares-local:latest    (default: build)
#   KEEP_UP=1                        leave the stack running after the run
#   MEM_LIMIT_MB=350                 per-broker peak-RSS gate (Scorpio's limit)
#   CALLBACK_HOST=localhost          host the brokers POST notifications to
#
# Locally run ONE mode at a time:   STORE=postgres STOP_ON_ERROR=1 dev/etsi-pipeline.sh
# CI runs all three modes in parallel via the workflow matrix.
#
# Output per mode: results/$STORE/{<suite>/output.xml, resource-samples.csv,
#                  run-summary.md, gate-status.txt}
set -uo pipefail
cd "$(dirname "$0")/.."

STORE="${STORE:-memory}"
case "$STORE" in
  memory)    DB_IMAGE="" ; PROFILE=() ;;
  postgres)  DB_IMAGE="postgis/postgis:17-3.5"       ; PROFILE=(--profile db) ;;
  timescale) DB_IMAGE="timescale/timescaledb-ha:pg17" ; PROFILE=(--profile db) ;;
  *) echo "unknown STORE=$STORE (memory|postgres|timescale)"; exit 2 ;;
esac
export STORE DB_IMAGE
COMPOSE=(docker compose -f compose-files/docker-compose-etsi.yml "${PROFILE[@]}")
RESULTS="results/$STORE"
MEM_LIMIT_MB="${MEM_LIMIT_MB:-350}"
mkdir -p "$RESULTS"

# 1. The image under test (the exact artifact CI publishes on green).
[ "${SKIP_BUILD:-}" = 1 ] || docker build -t antares-local:latest .

# 2. The ONE stack.
"${COMPOSE[@]}" up -d --wait
for port in 9090 9091 9092 9093 9094; do
  for t in $(seq 1 30); do curl -sf "localhost:$port/q/health" >/dev/null && break || sleep 1; done
  curl -sf "localhost:$port/q/health" >/dev/null || { echo "broker on :$port not healthy"; exit 1; }
done

# 3. Resource monitor (CPU + RSS, every antares container) for the whole run.
( while :; do
    ids=$(docker ps -q --filter name=antares)
    [ -n "$ids" ] && docker stats --no-stream --format '{{.Name}},{{.CPUPerc}},{{.MemUsage}}' $ids \
      | sed "s/^/$(date +%s),/"
    sleep 2
  done > "$RESULTS/resource-samples.csv" ) &
MONITOR_PID=$!

teardown() {
  kill "$MONITOR_PID" 2>/dev/null || true
  [ "${KEEP_UP:-}" = 1 ] || "${COMPOSE[@]}" down -v >/dev/null 2>&1 || true
}
trap teardown EXIT

# 4. Serial suites against broker1 (STOP_ON_ERROR respected by etsi-run.sh).
BROKER_URL=http://localhost:9090/ngsi-ld/v1 \
CALLBACK_HOST="${CALLBACK_HOST:-localhost}" \
RESULTS_DIR="$RESULTS" \
STOP_ON_ERROR="${STOP_ON_ERROR:-1}" \
  ./dev/etsi-run.sh
serial_status=$?
if [ "$serial_status" != 0 ] && [ "${STOP_ON_ERROR:-1}" = 1 ]; then
  echo "stopped at first failing TP (STOP_ON_ERROR=1) — see $RESULTS/<suite>/log.html"
  exit "$serial_status"
fi

# 5. IOP suite against all five brokers of the same stack.
( cd ngsi-ld-test-suite && ../.venv/bin/robot --outputdir "../$RESULTS/IOP" \
    --variable b1_url:http://localhost:9090/ngsi-ld/v1 \
    --variable b2_url:http://localhost:9091/ngsi-ld/v1 \
    --variable b3_url:http://localhost:9092/ngsi-ld/v1 \
    --variable b4_url:http://localhost:9093/ngsi-ld/v1 \
    --variable b5_url:http://localhost:9094/ngsi-ld/v1 \
    IOP_TP ) || true

# 6. Report: suite table + per-broker CPU/RSS + image size + memory gate.
kill "$MONITOR_PID" 2>/dev/null || true
IMAGE_BYTES=$(docker image inspect antares-local:latest --format '{{.Size}}' 2>/dev/null || echo 0)
RESULTS="$RESULTS" STORE="$STORE" MEM_LIMIT_MB="$MEM_LIMIT_MB" IMAGE_BYTES="$IMAGE_BYTES" \
python3 - <<'EOF'
import collections, glob, os, re, xml.etree.ElementTree as ET

results, store = os.environ["RESULTS"], os.environ["STORE"]
limit = float(os.environ["MEM_LIMIT_MB"])
image_mb = int(os.environ["IMAGE_BYTES"]) / 1024 / 1024

suites, fails, total_pass, total_fail = [], [], 0, 0
for path in sorted(glob.glob(f"{results}/*/output.xml")):
    name = path.split("/")[-2]
    try:
        root = ET.parse(path).getroot()
    except Exception as e:
        suites.append((name, "—", "—", f"unreadable: {e}")); total_fail += 1; continue
    stat = root.find("./statistics/total/stat")
    p, f, s = (int(stat.get(k, "0")) for k in ("pass", "fail", "skip"))
    suites.append((name, p, f, s)); total_pass += p; total_fail += f + s
    for test in root.iter("test"):
        st = test.find("status")
        txt = (st.text or "").strip()
        if st.get("status") == "FAIL" and "exit-on-failure" not in txt:
            fails.append((name, test.get("name"), txt[:200]))

rows = collections.defaultdict(lambda: {"cpu": [], "mem": []})
def mib(v):
    m = re.match(r"\s*([\d.]+)\s*([KMG])iB", v)
    if not m: return None
    x, u = float(m.group(1)), m.group(2)
    return x / 1024 if u == "K" else x * 1024 if u == "G" else x
try:
    for line in open(f"{results}/resource-samples.csv"):
        parts = line.strip().split(",")
        if len(parts) < 4: continue
        try: rows[parts[1]]["cpu"].append(float(parts[2].rstrip("%")))
        except ValueError: pass
        v = mib(parts[3].split("/")[0])
        if v is not None: rows[parts[1]]["mem"].append(v)
except OSError:
    pass

peaks = {n: max(d["mem"], default=0) for n, d in rows.items()}
mem_ok = bool(peaks) and all(p <= limit for p in peaks.values())
gate = "PASS" if mem_ok and total_fail == 0 and total_pass > 0 else "FAIL"
open(f"{results}/gate-status.txt", "w").write(gate + "\n")

avg = lambda xs: sum(xs) / len(xs) if xs else 0.0
with open(f"{results}/run-summary.md", "w") as out:
    out.write(f"## ETSI results — store: `{store}`\n\n")
    out.write(f"**{total_pass} passed, {total_fail} failed/skipped — gate {gate}** · "
              f"image {image_mb:.0f} MB · peak RSS limit {limit:.0f} MiB\n\n")
    out.write("| Suite | Pass | Fail | Skip |\n|---|---|---|---|\n")
    for name, p, f, s in suites:
        out.write(f"| {name} | {p} | {f} | {s} |\n")
    if fails:
        out.write(f"\n### Failures ({len(fails)}) — first 50\n\n")
        for name, test, msg in fails[:50]:
            out.write(f"- **{name} / {test}**: {msg}\n")
    out.write("\n### Broker resources\n\n")
    out.write("| Broker | Samples | CPU avg | CPU peak | RSS avg | RSS peak |\n|---|---|---|---|---|---|\n")
    for name in sorted(rows):
        c, m = rows[name]["cpu"], rows[name]["mem"]
        out.write(f"| {name} | {max(len(c), len(m))} | {avg(c):.1f}% | {max(c, default=0):.1f}% "
                  f"| {avg(m):.0f} MiB | {max(m, default=0):.0f} MiB |\n")
    if not mem_ok and peaks:
        worst = max(peaks, key=peaks.get)
        out.write(f"\n**memory gate: {worst} peaked at {peaks[worst]:.0f} MiB vs limit {limit:.0f} MiB**\n")

print(open(f"{results}/run-summary.md").read())
EOF

grep -q '^PASS$' "$RESULTS/gate-status.txt"
