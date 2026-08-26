#!/usr/bin/env bash
# Run the ETSI NGSI-LD Robot suite against a local Antares broker.
# Adapted from ScorpioBroker's dev/etsi-serial.sh (the reference recipe).
#
# Env knobs (defaults = local dev box):
#   BROKER_URL     default http://localhost:9090/ngsi-ld/v1
#   SUITE          default ./ngsi-ld-test-suite (vendored copy in this repo)
#   CALLBACK_HOST  host the broker POSTs notifications to (default: localhost)
#   SUITES         robot suite dirs, default: the Scorpio serial order
#   STOP_ON_ERROR  default 1 (local loop: stop at the FIRST failing TP);
#                  CI sets 0 to run the whole suite and report everything
set -uo pipefail
cd "$(dirname "$0")/.."

BROKER_URL="${BROKER_URL:-http://localhost:9090/ngsi-ld/v1}"
SUITE="${SUITE:-./ngsi-ld-test-suite}"
STOP_ON_ERROR="${STOP_ON_ERROR:-1}"
CALLBACK_HOST="${CALLBACK_HOST:-localhost}"
RESULTS_DIR="${RESULTS_DIR:-results}"
. dev/etsi-suites.sh
SUITES="${SUITES:-$SERIAL_ALL}"

[ -d "$SUITE" ] || { echo "test suite not found at $SUITE"; exit 1; }
check_suites_complete "$SUITE" || exit 1

# venv with the suite's requirements (robotframework + vendored HttpCtrl).
# pip runs FROM the suite dir: requirements.txt uses relative editable paths.
# The bootstrap gate VERIFIES the vendored HttpCtrl actually imports — a venv
# that predates a suite move keeps a working `robot` but a stale editable
# path (easy-install.pth), and every mock-server keyword then fails with
# "No keyword with name 'Start Server'" (two full runs
# poisoned). Heal with a forced editable reinstall, not just presence checks.
VENV="$PWD/.venv"
if [ ! -x "$VENV/bin/robot" ]; then
  python3 -m venv "$VENV"
  (cd "$SUITE" && "$VENV/bin/pip" -q install -r requirements.txt)
fi
if ! "$VENV/bin/python" -c "import HttpCtrl" >/dev/null 2>&1; then
  echo "venv: vendored HttpCtrl not importable — reinstalling editable"
  (cd "$SUITE" && "$VENV/bin/pip" -q install -r requirements.txt \
    && "$VENV/bin/pip" -q install --no-deps --force-reinstall \
         -e ./libraries/robotframework-httpctrl)
  "$VENV/bin/python" -c "import HttpCtrl" \
    || { echo "venv: HttpCtrl still broken — aborting"; exit 1; }
fi

# Point the suite at this broker (same sed recipe as Scorpio's runner).
# Restore the sed-ed file on exit (E7): a dirty suite tree should mean a real
# change, not a leftover run configuration.
trap 'git -C "$SUITE" checkout -- resources/variables.py 2>/dev/null || true' EXIT
( cd "$SUITE/resources"
  sed -i "s|^url = .*|url = '$BROKER_URL'|" variables.py
  sed -i "s|^temporal_api_url = .*|temporal_api_url = '$BROKER_URL'|" variables.py
  sed -i "s|^notification_server_host = .*|notification_server_host = '$CALLBACK_HOST'|" variables.py
  sed -i "s|^context_source_host = .*|context_source_host = '$CALLBACK_HOST'|" variables.py
  sed -i "s|^context_server_host = .*|context_server_host = '$CALLBACK_HOST'|" variables.py )

mkdir -p "$RESULTS_DIR"
EXTRA=()
[ "$STOP_ON_ERROR" = 1 ] && EXTRA+=(--exitonfailure)
# MQTT TPs (058_*) launch mosquitto via `docker run` (MqttUtils.resource), so
# they run wherever docker works. MQTT=0 excludes them on
# dockerless boxes; the CI definition of green INCLUDES them.
[ "${MQTT:-1}" = 1 ] || EXTRA+=(--exclude '*mqtt*')
# TPs needing a specially-configured broker (ANTARES_TEMPORAL=none) never
# belong in a default conformance run — they get their own harness.
EXTRA+=(--exclude config_no_temporal)

status=0
for s in $SUITES; do
  name="${s//\//_}"
  echo "=== $s ==="
  # Tell the 1 Hz sampler which suite owns the samples from here on (E9g) —
  # written before the reset so the reset's own churn is attributed too.
  [ -n "${PHASE_FILE:-}" ] && echo "$name" > "$PHASE_FILE"
  bash dev/reset-broker.sh "$BROKER_URL"   # state reset between suites
  # Console tee'd next to output.xml: when robot dies before writing it,
  # the CI artifact must still say why (see the IOP step's twin note).
  (cd "$SUITE" && "$VENV/bin/robot" \
      --outputdir "$OLDPWD/$RESULTS_DIR/$name" \
      "${EXTRA[@]}" \
      "TP/NGSI-LD/$s") 2>&1 | tee "$RESULTS_DIR/$name-console.log" \
    || { status=$?; [ "$STOP_ON_ERROR" = 1 ] && break; }
done
exit $status
