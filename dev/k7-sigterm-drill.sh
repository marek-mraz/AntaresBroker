#!/usr/bin/env bash
# SIGTERM drill: roll every HA instance while the
# continuity harness hammers the LB. Expected: ZERO failed requests, zero
# lost writes, notifications at-least-once. This is the only real test of
# the drain — a drain bug shows up here and nowhere else.
#
# Precondition: the HA stack is up (docker-compose-etsi.yml + docker-compose-ha.yml).
#   STORE=memory|postgres|timescale   (file cannot roll — exclusive redb
#                                      lock; rolling-update.sh refuses)
set -euo pipefail
cd "$(dirname "$0")/.."

BASE="${BASE:-http://localhost:9090}"
RECEIVER_PORT="${RECEIVER_PORT:-9299}"
SECONDS_UNDER_LOAD="${SECONDS_UNDER_LOAD:-45}"
RUN_ID="$(date +%s)"

# One subscription to the harness's receiver, so at-least-once is asserted too.
sub=$(cat <<EOF
{"id": "urn:ngsi-ld:Subscription:k7:$RUN_ID", "type": "Subscription",
 "entities": [{"type": "ContinuityProbe"}],
 "notification": {"endpoint": {"uri": "http://localhost:$RECEIVER_PORT/notify"}}}
EOF
)
curl -sf -X POST "$BASE/ngsi-ld/v1/subscriptions" \
     -H 'Content-Type: application/json' -d "$sub" > /dev/null
trap 'curl -sf -X DELETE "$BASE/ngsi-ld/v1/subscriptions/urn:ngsi-ld:Subscription:k7:'"$RUN_ID"'" >/dev/null || true' EXIT

python3 dev/k6-continuity.py \
  --base "$BASE" --seconds "$SECONDS_UNDER_LOAD" --rate 20 \
  --listen-port "$RECEIVER_PORT" --expect-notifications --run-id "$RUN_ID" &
K6_PID=$!

sleep 3                      # traffic flowing before the chaos starts
STORE="${STORE:-memory}" bash dev/rolling-update.sh

wait "$K6_PID"               # k6 exits 1 on any violation
echo "SIGTERM drill PASS: rolled every instance under load with zero failures"
