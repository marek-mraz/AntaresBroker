#!/usr/bin/env bash
# ONE ETSI pipeline — identical locally and in CI (the ScorpioBroker
# dev/etsi-serial.sh pattern: the workflow is a thin wrapper around THIS).
#
# Stack: compose-files/docker-compose-etsi.yml — 5 brokers + 5 databases.
# Serial suites run against broker1, the IOP suite against all five.
#
# Env knobs (defaults = local dev loop; CI overrides them):
#   STORE=memory|file|postgres|timescale  mode under test         (default memory)
#   STOP_ON_ERROR=1                  halt at the FIRST failing TP  (CI sets 0)
#   SKIP_BUILD=1                     reuse antares-local:latest    (default: build)
#   KEEP_UP=1                        leave the stack running after the run
#   MEM_LIMIT_MB=350                 per-broker peak-RSS gate (Scorpio's limit)
#   CALLBACK_HOST=localhost          host the brokers POST notifications to
#   MQTT=1                           include the 058_* MQTT TPs (G4); 0 skips
#                                    them on boxes where docker can't run
#   SUITES=                          comma list filtering which suites run
#                                    (E9a; substring match on the serial suite
#                                    names, "IOP" selects the IOP step).
#                                    Default: everything. Example:
#                                    STORE=file SUITES=Consumption …
#   RESULTS_DIR=results/$STORE       where this invocation writes its results
#                                    (dev/etsi-local.sh gives each serial
#                                    matrix cell its own dir)
#   SAMPLE_INTERVAL=1                seconds between CPU/RSS samples (E9g)
#
# Locally run ONE cell at a time:   STORE=postgres SUITES=Consumption dev/etsi-pipeline.sh
# (dev/etsi-local.sh loops the full store × suite matrix serially; CI builds
# the image ONCE and runs the same 32 cells in parallel with SKIP_BUILD=1.)
#
# Output per mode, under results/$STORE/ (all uploaded as CI artifacts):
#   <suite>/output.xml + log.html   Robot's own results and drill-down
#   resource-samples.csv            EVERY 1 Hz CPU/RSS sample, each labelled
#                                   with the suite AND the TP that was running
#   resource-by-test.csv            per test × broker rollup: avg/peak CPU+RSS
#   failures.csv                    every failing TP with its full message
#   run-summary.md                  the human view incl. the top-10 spike
#                                   tables (which test caused the peak)
#   gate-status.txt                 PASS/FAIL — suites green AND RSS ≤ limit
set -uo pipefail
cd "$(dirname "$0")/.."

STORE="${STORE:-memory}"
case "$STORE" in
  memory)    DB_IMAGE="" ; PROFILE=() ;;
  file)      DB_IMAGE="" ; PROFILE=() ;;   # brokers only; redb lives on the per-broker volume
  postgres)  DB_IMAGE="ghcr.io/baosystems/postgis:17-3.5" ; PROFILE=(--profile db) ;;  # multi-arch (postgis/postgis has no arm64)
  timescale) DB_IMAGE="timescale/timescaledb-ha:pg17" ; PROFILE=(--profile db) ;;
  *) echo "unknown STORE=$STORE (memory|file|postgres|timescale)"; exit 2 ;;
esac

# Honesty banner: which modes actually have a store backend TODAY. Update this
# list as §B (file) and §C/§D (postgres/timescale) land — a green column for a
# mode whose backend does not exist yet validates infrastructure, not storage.
BACKED="memory file postgres timescale"
case "$STORE" in file) SECTION="B" ;; postgres) SECTION="C" ;; timescale) SECTION="D" ;; *) SECTION="?" ;; esac
case " $BACKED " in *" $STORE "*) BACKED_NOTE="" ;;
  *) BACKED_NOTE="store backend for \`$STORE\` is NOT implemented yet (tasks.md §$SECTION) — this run validates the stack, not the storage" ;;
esac
export BACKED_NOTE
export STORE DB_IMAGE
COMPOSE=(docker compose -f compose-files/docker-compose-etsi.yml "${PROFILE[@]}")
RESULTS="${RESULTS_DIR:-results/$STORE}"
MEM_LIMIT_MB="${MEM_LIMIT_MB:-350}"
mkdir -p "$RESULTS"

# 1. The image under test (the exact artifact CI publishes on green).
[ "${SKIP_BUILD:-}" = 1 ] || docker build -t antares-local:latest .

# 2. MQTT prerequisites (G4). The suite launches its OWN mosquitto per 058
# test (MqttUtils.resource: `docker run --network compose-files_default
# --name ngsi-ld-test-suite-mosquitto-container scorpio-test-mosquitto`).
# Provide what it expects: the image (built from the suite's own confs — the
# single source), the network (fixed subnet: the only container on it always
# gets .2), and name resolution for the runner; the host-networked brokers
# get the same mapping via extra_hosts in the compose.
if [ "${MQTT:-1}" = 1 ]; then
  docker build -t scorpio-test-mosquitto:latest \
    -f compose-files/mosquitto/Dockerfile \
    ngsi-ld-test-suite/resources/mqttUtils/mosquitto
  docker network inspect compose-files_default >/dev/null 2>&1 \
    || docker network create --subnet 172.29.9.0/24 compose-files_default
  grep -q ngsi-ld-test-suite-mosquitto-container /etc/hosts \
    || echo "172.29.9.2 ngsi-ld-test-suite-mosquitto-container" | sudo tee -a /etc/hosts >/dev/null
fi
export MQTT="${MQTT:-1}"

# TLS trust for the SUITE's own fetches: forge.etsi.org serves an incomplete
# chain (leaf only — error.md 2026-08-05), which python/requests and curl
# reject. Hand them the system bundle + the missing intermediate. The brokers
# get the same anchor via ANTARES_EXTRA_CA_FILE in the compose.
CA_BUNDLE="$RESULTS/ca-bundle.crt"
cat /etc/ssl/certs/ca-certificates.crt dev/ca-extra.pem > "$CA_BUNDLE"
export REQUESTS_CA_BUNDLE="$PWD/$CA_BUNDLE" CURL_CA_BUNDLE="$PWD/$CA_BUNDLE" SSL_CERT_FILE="$PWD/$CA_BUNDLE"

# E9a: which suites this invocation runs (default: all serial + IOP).
SERIAL_ALL="CommonBehaviours ContextInformation/Consumption ContextInformation/Provision ContextInformation/Subscription ContextSource jsonldContext DistributedOperations"
RUN_IOP=1
SERIAL_SUITES="$SERIAL_ALL"
if [ -n "${SUITES:-}" ]; then
  RUN_IOP=0
  SERIAL_SUITES=""
  IFS=',' read -ra _parts <<<"$SUITES"
  for _p in "${_parts[@]}"; do
    if [ "$_p" = "IOP" ]; then RUN_IOP=1; continue; fi
    hit=""
    for _s in $SERIAL_ALL; do
      case "$_s" in *"$_p"*) SERIAL_SUITES="$SERIAL_SUITES $_s"; hit=1 ;; esac
    done
    [ -n "$hit" ] || { echo "SUITES: '$_p' matches no suite (of: $SERIAL_ALL IOP)"; exit 2; }
  done
fi

# 3. The ONE stack.
"${COMPOSE[@]}" up -d --wait
for port in 9090 9091 9092 9093 9094; do
  for t in $(seq 1 30); do curl -sf "localhost:$port/q/health" >/dev/null && break || sleep 1; done
  curl -sf "localhost:$port/q/health" >/dev/null || { echo "broker on :$port not healthy"; exit 1; }
done

# 4. Resource monitor: CPU + RSS of every antares container, every second, for
# the whole run. PHASE_FILE carries the suite currently under test so each
# sample can be traced back to what caused it; the report step then joins the
# samples to individual TPs on their Robot timestamps.
PHASE_FILE="$RESULTS/.current-phase"
echo "startup" > "$PHASE_FILE"
export PHASE_FILE
python3 dev/etsi-sampler.py \
  --out "$RESULTS/resource-samples.csv" \
  --phase-file "$PHASE_FILE" \
  --interval "${SAMPLE_INTERVAL:-1}" &
MONITOR_PID=$!

teardown() {
  kill "$MONITOR_PID" 2>/dev/null || true
  rm -f "$PHASE_FILE"
  # Leave the suite submodule clean (E7) — the IOP step seds variables.py.
  git -C ngsi-ld-test-suite checkout -- resources/variables.py 2>/dev/null || true
  [ "${KEEP_UP:-}" = 1 ] || "${COMPOSE[@]}" down -v >/dev/null 2>&1 || true
}
trap teardown EXIT

# 5. Serial suites against broker1 (STOP_ON_ERROR respected by etsi-run.sh).
serial_status=0
if [ -n "${SERIAL_SUITES// /}" ]; then
  BROKER_URL=http://localhost:9090/ngsi-ld/v1 \
  CALLBACK_HOST="${CALLBACK_HOST:-localhost}" \
  RESULTS_DIR="$RESULTS" \
  SUITES="$SERIAL_SUITES" \
  STOP_ON_ERROR="${STOP_ON_ERROR:-1}" \
    ./dev/etsi-run.sh
  serial_status=$?
  if [ "$serial_status" != 0 ] && [ "${STOP_ON_ERROR:-1}" = 1 ]; then
    echo "stopped at first failing TP (STOP_ON_ERROR=1) — see $RESULTS/<suite>/log.html"
    exit "$serial_status"
  fi
fi

# 6. IOP suite against all five brokers of the same stack. Configure
# variables.py HERE — etsi-run.sh restores it on exit (E7), so this step must
# never rely on that sed surviving.
if [ "$RUN_IOP" = 1 ]; then
echo "IOP" > "$PHASE_FILE"
( cd ngsi-ld-test-suite/resources
  sed -i "s|^url = .*|url = 'http://localhost:9090/ngsi-ld/v1'|" variables.py
  sed -i "s|^temporal_api_url = .*|temporal_api_url = 'http://localhost:9090/ngsi-ld/v1'|" variables.py
  sed -i "s|^notification_server_host = .*|notification_server_host = '${CALLBACK_HOST:-localhost}'|" variables.py
  sed -i "s|^context_source_host = .*|context_source_host = '${CALLBACK_HOST:-localhost}'|" variables.py
  sed -i "s|^context_server_host = .*|context_server_host = '${CALLBACK_HOST:-localhost}'|" variables.py )
( cd ngsi-ld-test-suite && ../.venv/bin/robot --outputdir "../$RESULTS/IOP" \
    --variable b1_url:http://localhost:9090/ngsi-ld/v1 \
    --variable b2_url:http://localhost:9091/ngsi-ld/v1 \
    --variable b3_url:http://localhost:9092/ngsi-ld/v1 \
    --variable b4_url:http://localhost:9093/ngsi-ld/v1 \
    --variable b5_url:http://localhost:9094/ngsi-ld/v1 \
    IOP_TP ) || true
fi

# 7. Report: suite table, per-broker CPU/RSS, spike attribution, downloadable
# failure + sample CSVs, image size and the memory gate.
kill "$MONITOR_PID" 2>/dev/null || true
IMAGE_BYTES=$(docker image inspect antares-local:latest --format '{{.Size}}' 2>/dev/null || echo 0)
RESULTS="$RESULTS" STORE="$STORE" MEM_LIMIT_MB="$MEM_LIMIT_MB" IMAGE_BYTES="$IMAGE_BYTES" \
  python3 dev/etsi-report.py

grep -q '^PASS$' "$RESULTS/gate-status.txt"
