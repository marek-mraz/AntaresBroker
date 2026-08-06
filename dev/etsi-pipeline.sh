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
#   HA=1                             layer the K2 overlay: haproxy owns 9090,
#                                    two broker1 replicas behind it — the
#                                    suite talks to the LB and cannot tell
#   ROLL_DURING_RUN=1                (needs HA=1) K8: roll the two replicas in
#                                    a loop for the whole run via
#                                    dev/rolling-update.sh. The suite has no
#                                    retries and asserts exact single
#                                    responses, so ANY failure is a real K1
#                                    drain bug, not flake. postgres/timescale
#                                    only (shared state makes replicas one
#                                    broker; K10: file cannot roll)
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
#   WASM=1                           N7a Node tier: the suite runs against the
#                                    BROWSER artifact — five node shims
#                                    (www/node-shim.mjs, the same .wasm a page
#                                    loads) on 9090..9094 instead of the
#                                    docker brokers. Forces STORE=memory
#                                    semantics (the wasm build has no other
#                                    backend), MQTT=0 (no MQTT sink in a
#                                    browser build — documented N7c/N8), and
#                                    refuses HA/ROLL (nothing to roll).
#   BROWSER=1                        (needs WASM=1) N7b browser tier: ONE
#                                    headless-Chromium page hosts the module
#                                    (www/test/etsi-proxy.mjs forwards suite
#                                    HTTP into it) instead of the node shims.
#                                    Run with the N7b green set:
#                                    SUITES=CommonBehaviours,Provision,Consumption,jsonldContext
#                                    (Subscription/ContextSource/DO/IOP stay
#                                    Node-tier-only — N7c).
set -uo pipefail
cd "$(dirname "$0")/.."

STORE="${STORE:-memory}"
if [ "${WASM:-0}" = 1 ]; then
  [ "${HA:-0}" = 1 ] || [ "${ROLL_DURING_RUN:-0}" = 1 ] && { echo "WASM=1 has no HA/roll story"; exit 2; }
  [ "$STORE" = memory ] || { echo "WASM=1 implies STORE=memory (the artifact's only backend here)"; exit 2; }
  export MQTT=0
elif [ "${BROWSER:-0}" = 1 ]; then
  echo "BROWSER=1 needs WASM=1 (it is the browser tier of the wasm artifact)"; exit 2
fi
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
COMPOSE=(docker compose -f compose-files/docker-compose-etsi.yml)
# K2/K8: the HA overlay moves antares1 behind an LB on 9090; with
# ROLL_DURING_RUN the replicas roll continuously under the running suite.
if [ "${HA:-0}" = 1 ]; then
  COMPOSE+=(-f compose-files/docker-compose-ha.yml)
  # F10: replicas of one broker talk over NATS (shared matcher durable,
  # claimed interval firings). memory mode keeps bus=local — it exercises
  # only the LB/drain mechanics, and ANTARES_BUS=nats refuses per-process
  # state by design.
  case "$STORE" in
    postgres|timescale) export HA_BUS=nats ;;
    *) export HA_BUS=local ;;
  esac
  if [ "${ROLL_DURING_RUN:-0}" = 1 ]; then
    case "$STORE" in
      postgres|timescale) ;;
      *) echo "ROLL_DURING_RUN needs shared state: STORE=postgres|timescale (file: K10 lock; memory: replicas diverge)"; exit 2 ;;
    esac
  fi
elif [ "${ROLL_DURING_RUN:-0}" = 1 ]; then
  echo "ROLL_DURING_RUN=1 requires HA=1 (nothing to roll without the LB)"; exit 2
fi
COMPOSE+=("${PROFILE[@]}")
RESULTS="${RESULTS_DIR:-results/$STORE}"
MEM_LIMIT_MB="${MEM_LIMIT_MB:-350}"
mkdir -p "$RESULTS"

# 1. The artifact under test: the docker image — or, WASM=1, the browser
# build (the exact bytes a page loads, served through the Node shim).
if [ "${WASM:-0}" = 1 ]; then
  # A failed build MUST abort — the script runs under set -uo (no -e), and
  # falling through here once ran the whole suite against a stale artifact.
  if [ "${SKIP_BUILD:-}" != 1 ]; then
    ./dev/wasm-build.sh || { echo "wasm build failed — aborting"; exit 1; }
  fi
  [ -f www/pkg/antares_wasm_bg.wasm ] || { echo "www/pkg missing — run dev/wasm-build.sh"; exit 1; }
else
  if [ "${SKIP_BUILD:-}" != 1 ]; then
    docker build -t antares-local:latest . || { echo "image build failed — aborting"; exit 1; }
  fi
fi

# 2. The mosquitto network. The compose file references it as external, so it
# must exist for EVERY run (MQTT or not): the suite's MqttUtils launches its
# mosquitto container onto it by name, and the db containers deliberately live
# on their own bridge so mosquitto stays this network's only occupant and
# always lands on .2 (the brokers' extra_hosts mapping counts on that).
[ "${WASM:-0}" = 1 ] || docker network inspect compose-files_default >/dev/null 2>&1 \
  || docker network create --subnet 172.29.9.0/24 compose-files_default

# MQTT prerequisites (G4). The suite launches its OWN mosquitto per 058 test
# (MqttUtils.resource: `docker run --network compose-files_default
# --name ngsi-ld-test-suite-mosquitto-container scorpio-test-mosquitto`).
# Provide the image (built from the suite's own confs — the single source) and
# name resolution for the runner.
if [ "${MQTT:-1}" = 1 ]; then
  docker build -t scorpio-test-mosquitto:latest \
    -f compose-files/mosquitto/Dockerfile \
    ngsi-ld-test-suite/resources/mqttUtils/mosquitto
  grep -q ngsi-ld-test-suite-mosquitto-container /etc/hosts \
    || echo "172.29.9.2 ngsi-ld-test-suite-mosquitto-container" | sudo tee -a /etc/hosts >/dev/null
  # Vendored overlay (error.md 2026-08-05): the suite's Start Mqtt Server has
  # no readiness wait after `docker run -d`, so the first connect races the
  # mosquitto start and loses on a cold daemon. Same E7 pattern as
  # variables.py — applied for the run, restored in teardown.
  cp dev/MqttUtils.resource ngsi-ld-test-suite/resources/mqttUtils/MqttUtils.resource
fi
export MQTT="${MQTT:-1}"

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

# 3. The ONE stack (WASM=1: five node shims instead — same ports, same suite;
# BROWSER=1: one Chromium page behind www/test/etsi-proxy.mjs).
WASM_PIDS=()
if [ "${WASM:-0}" = 1 ]; then
  mkdir -p "$RESULTS"
  if [ "${BROWSER:-0}" = 1 ]; then
    node www/test/etsi-proxy.mjs > "$RESULTS/browser-proxy.log" 2>&1 &
    WASM_PIDS+=($!)
  else
    for port in 9090 9091 9092 9093 9094; do
      node www/node-shim.mjs "$port" > "$RESULTS/shim-$port.log" 2>&1 &
      WASM_PIDS+=($!)
    done
  fi
else
  "${COMPOSE[@]}" up -d --wait
fi
for port in 9090 9091 9092 9093 9094; do
  for t in $(seq 1 30); do curl -sf "localhost:$port/q/health" >/dev/null && break || sleep 1; done
  curl -sf "localhost:$port/q/health" >/dev/null || { echo "broker on :$port not healthy"; exit 1; }
done

# K8: roll the HA pair in a loop underneath the whole run. The suite is a
# brutally strict drain client (no retries, exact single responses) — any red
# TP here is a real K1 bug. The loop's log lands in the results dir.
ROLL_PID=""
if [ "${ROLL_DURING_RUN:-0}" = 1 ]; then
  ( while :; do
      STORE="$STORE" bash dev/rolling-update.sh || echo "ROLL FAILED rc=$? at $(date +%T)"
      sleep 5
    done > "$RESULTS/roll-loop.log" 2>&1 ) &
  ROLL_PID=$!
  echo "K8: rolling antares1/antares1b continuously (pid $ROLL_PID)"
fi

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
  [ -n "$ROLL_PID" ] && { kill "$ROLL_PID" 2>/dev/null || true; }
  rm -f "$PHASE_FILE"
  # Leave the suite submodule clean (E7) — the IOP step seds variables.py and
  # the MQTT step overlays MqttUtils.resource.
  git -C ngsi-ld-test-suite checkout -- resources/variables.py \
    resources/mqttUtils/MqttUtils.resource 2>/dev/null || true
  if [ "${WASM:-0}" = 1 ]; then
    kill "${WASM_PIDS[@]}" 2>/dev/null || true
  else
    [ "${KEEP_UP:-}" = 1 ] || "${COMPOSE[@]}" down -v >/dev/null 2>&1 || true
  fi
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
# Console tee'd into the results dir: when robot dies before writing
# output.xml (import error, unreachable broker), the artifact must still
# say why — a 1 KB artifact with no clue is undebuggable in CI.
( cd ngsi-ld-test-suite && ../.venv/bin/robot --outputdir "../$RESULTS/IOP" \
    --variable b1_url:http://localhost:9090/ngsi-ld/v1 \
    --variable b2_url:http://localhost:9091/ngsi-ld/v1 \
    --variable b3_url:http://localhost:9092/ngsi-ld/v1 \
    --variable b4_url:http://localhost:9093/ngsi-ld/v1 \
    --variable b5_url:http://localhost:9094/ngsi-ld/v1 \
    IOP_TP ) 2>&1 | tee "$RESULTS/IOP-console.log" || true
fi

# 7. Report: suite table, per-broker CPU/RSS, spike attribution, downloadable
# failure + sample CSVs, image size and the memory gate.
kill "$MONITOR_PID" 2>/dev/null || true
IMAGE_BYTES=$(docker image inspect antares-local:latest --format '{{.Size}}' 2>/dev/null || echo 0)
RESULTS="$RESULTS" STORE="$STORE" MEM_LIMIT_MB="$MEM_LIMIT_MB" IMAGE_BYTES="$IMAGE_BYTES" \
  python3 dev/etsi-report.py

grep -q '^PASS$' "$RESULTS/gate-status.txt"
