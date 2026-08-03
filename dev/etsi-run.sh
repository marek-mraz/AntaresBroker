#!/usr/bin/env bash
# Run the ETSI NGSI-LD Robot suite against a local Antares broker.
# Adapted from ScorpioBroker's dev/etsi-serial.sh (the reference recipe).
#
# Env knobs (defaults = local dev box):
#   BROKER_URL     default http://localhost:9090/ngsi-ld/v1
#   SUITE          default ../ngsi-ld-test-suite (sibling checkout)
#   CALLBACK_HOST  host the broker POSTs notifications to (default: localhost)
#   SUITES         robot suite dirs, default: the Scorpio serial order
#   STOP_ON_ERROR=1  robot --exitonfailure (stop at the FIRST failing TP)
set -uo pipefail
cd "$(dirname "$0")/.."

BROKER_URL="${BROKER_URL:-http://localhost:9090/ngsi-ld/v1}"
SUITE="${SUITE:-../ngsi-ld-test-suite}"
CALLBACK_HOST="${CALLBACK_HOST:-localhost}"
SUITES="${SUITES:-CommonBehaviours ContextInformation/Consumption ContextInformation/Provision ContextInformation/Subscription ContextSource jsonldContext}"

[ -d "$SUITE" ] || { echo "test suite not found at $SUITE"; exit 1; }

# venv with the suite's requirements (robotframework + vendored HttpCtrl).
# pip runs FROM the suite dir: requirements.txt uses relative editable paths.
VENV="$PWD/.venv"
if [ ! -x "$VENV/bin/robot" ]; then
  python3 -m venv "$VENV"
  (cd "$SUITE" && "$VENV/bin/pip" -q install -r requirements.txt)
fi

# Point the suite at this broker (same sed recipe as Scorpio's runner).
( cd "$SUITE/resources"
  sed -i "s|^url = .*|url = '$BROKER_URL'|" variables.py
  sed -i "s|^temporal_api_url = .*|temporal_api_url = '$BROKER_URL'|" variables.py
  sed -i "s|^notification_server_host = .*|notification_server_host = '$CALLBACK_HOST'|" variables.py
  sed -i "s|^context_source_host = .*|context_source_host = '$CALLBACK_HOST'|" variables.py
  sed -i "s|^context_server_host = .*|context_server_host = '$CALLBACK_HOST'|" variables.py )

mkdir -p results
EXTRA=()
[ "${STOP_ON_ERROR:-}" = 1 ] && EXTRA+=(--exitonfailure)

status=0
for s in $SUITES; do
  name="${s//\//_}"
  echo "=== $s ==="
  (cd "$SUITE" && "$VENV/bin/robot" \
      --outputdir "$OLDPWD/results/$name" \
      --exclude iop --exclude '*mqtt*' \
      "${EXTRA[@]}" \
      "TP/NGSI-LD/$s") || { status=$?; [ "${STOP_ON_ERROR:-}" = 1 ] && break; }
done
exit $status
