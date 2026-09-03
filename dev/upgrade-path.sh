#!/usr/bin/env bash
# Data written by the PREVIOUS release is served by this one.
#
#   dev/upgrade-path.sh OLD_BINARY NEW_BINARY
#
# The old binary seeds a store through the standard API — an entity, three
# temporal instances behind it, and a subscription — and the new binary is
# then pointed at that same store and made to serve all three: the entity
# reads back, the history is still there, and a write through the new
# binary fires the subscription the old one stored.
#
# `file` runs always. `postgres` runs when ANTARES_TEST_DATABASE_URL names a
# reachable database, so the pair is proven across both durable stores; the
# old binary applies its migrations and the new one applies its own on top.
#
# Nothing here is Antares-specific beyond the two env variables: the seed
# and the assertions go through CIM 009 endpoints, so a shape that only the
# old binary understood would fail as loudly as a broken migration.
set -euo pipefail
cd "$(dirname "$0")/.."

OLD=${1:?usage: upgrade-path.sh OLD_BINARY NEW_BINARY}
NEW=${2:?usage: upgrade-path.sh OLD_BINARY NEW_BINARY}
[ -x "$OLD" ] || { echo "upgrade-path: $OLD is not executable"; exit 2; }
[ -x "$NEW" ] || { echo "upgrade-path: $NEW is not executable"; exit 2; }

WORK=$(mktemp -d)
SINK_PORT=${UPGRADE_SINK_PORT:-9481}
PORT=${UPGRADE_BROKER_PORT:-9480}
API="http://127.0.0.1:$PORT/ngsi-ld/v1"
ENTITY="urn:ngsi-ld:UpgradeProbe:1"
failures=0

cleanup() {
    [ -n "${BROKER_PID:-}" ] && kill "$BROKER_PID" 2>/dev/null
    [ -n "${SINK_PID:-}" ] && kill "$SINK_PID" 2>/dev/null
    wait 2>/dev/null || true
    rm -rf "$WORK"
}
trap cleanup EXIT

fail() { echo "  FAIL: $*"; failures=$((failures + 1)); }
ok() { echo "  ok: $*"; }

# One notification sink for the whole run: every POST body is appended to
# $WORK/notifications, one JSON document per line.
start_sink() {
    # A sink left behind by an interrupted run would answer on this port and
    # write its notifications somewhere else, and the assertion below would
    # then fail for a reason that has nothing to do with the upgrade.
    if curl -s -o /dev/null --max-time 2 "http://127.0.0.1:$SINK_PORT/notify" 2>/dev/null; then
        echo "upgrade-path: port $SINK_PORT is already serving; set UPGRADE_SINK_PORT"
        exit 2
    fi
    python3 - "$SINK_PORT" "$WORK/notifications" <<'PY' &
import sys
from http.server import BaseHTTPRequestHandler, HTTPServer

port, path = int(sys.argv[1]), sys.argv[2]


class Sink(BaseHTTPRequestHandler):
    def do_POST(self):
        body = self.rfile.read(int(self.headers.get("Content-Length", 0)))
        with open(path, "ab") as f:
            f.write(body.replace(b"\n", b" ") + b"\n")
        self.send_response(204)
        self.end_headers()

    def log_message(self, *_):
        pass


HTTPServer(("127.0.0.1", port), Sink).serve_forever()
PY
    SINK_PID=$!
}

# Start a broker and wait for it to answer, or give up with its log.
start_broker() {
    local bin=$1 label=$2
    shift 2
    env "$@" ANTARES_HTTP_PORT="$PORT" ANTARES_EGRESS_ALLOW_PRIVATE=true \
        "$bin" > "$WORK/$label.log" 2>&1 &
    BROKER_PID=$!
    for _ in $(seq 1 120); do
        if curl -sf "http://127.0.0.1:$PORT/q/health" > /dev/null 2>&1; then
            return 0
        fi
        if ! kill -0 "$BROKER_PID" 2>/dev/null; then
            echo "  $label exited before it served:"
            tail -20 "$WORK/$label.log"
            return 1
        fi
        sleep 0.5
    done
    echo "  $label never answered /q/health:"
    tail -20 "$WORK/$label.log"
    return 1
}

stop_broker() {
    kill "$BROKER_PID" 2>/dev/null || true
    # the file store must close its redb file, not be torn off it
    for _ in $(seq 1 60); do
        kill -0 "$BROKER_PID" 2>/dev/null || break
        sleep 0.5
    done
    BROKER_PID=
}

json() { curl -s -o "$WORK/body" -w '%{http_code}' "$@"; }

# The seed the old binary writes: an entity, two more observations behind
# it, and a subscription on its type.
seed() {
    local code
    # A durable store the harness has already used still holds the probe; the
    # seed is the same either way, so clear it rather than demand a fresh
    # database (CI hands us one, a local run reuses a container).
    curl -s -o /dev/null -X DELETE "$API/entities/$ENTITY"
    curl -s -o /dev/null -X DELETE "$API/subscriptions/urn:ngsi-ld:Subscription:upgrade"
    code=$(json -X POST "$API/entities" -H 'Content-Type: application/json' -d '{
      "id": "'"$ENTITY"'", "type": "UpgradeProbe",
      "level": {"type": "Property", "value": 1, "observedAt": "2026-01-01T00:00:00Z"}
    }')
    [ "$code" = 201 ] || { fail "seed create returned $code: $(cat "$WORK/body")"; return 1; }
    for n in 2 3; do
        code=$(json -X PATCH "$API/entities/$ENTITY/attrs" -H 'Content-Type: application/json' -d '{
          "level": {"type": "Property", "value": '"$n"', "observedAt": "2026-01-0'"$n"'T00:00:00Z"}
        }')
        [ "$code" = 204 ] || { fail "seed patch $n returned $code: $(cat "$WORK/body")"; return 1; }
    done
    code=$(json -X POST "$API/subscriptions" -H 'Content-Type: application/json' -d '{
      "id": "urn:ngsi-ld:Subscription:upgrade", "type": "Subscription",
      "entities": [{"type": "UpgradeProbe"}],
      "notification": {"endpoint": {"uri": "http://127.0.0.1:'"$SINK_PORT"'/notify"}}
    }')
    [ "$code" = 201 ] || { fail "seed subscription returned $code: $(cat "$WORK/body")"; return 1; }
    ok "the old binary wrote the entity, its history and the subscription"
}

# What the new binary must still serve.
assert_served() {
    local code
    code=$(json "$API/entities/$ENTITY")
    if [ "$code" = 200 ] && grep -q '"value":3' "$WORK/body"; then
        ok "the entity reads back with its last value"
    else
        fail "entity GET returned $code: $(cat "$WORK/body")"
    fi

    code=$(json "$API/temporal/entities/$ENTITY?timerel=after&timeAt=2025-01-01T00:00:00Z")
    local instances
    instances=$(python3 -c '
import json, sys
doc = json.load(open(sys.argv[1]))
level = doc.get("level", [])
print(len(level) if isinstance(level, list) else 1)
' "$WORK/body" 2>/dev/null || echo 0)
    if [ "$code" = 200 ] && [ "$instances" -ge 3 ]; then
        ok "the history the old binary recorded is served ($instances instances)"
    else
        fail "temporal GET returned $code with $instances instances: $(head -c 300 "$WORK/body")"
    fi

    code=$(json "$API/subscriptions/urn:ngsi-ld:Subscription:upgrade")
    if [ "$code" = 200 ]; then
        ok "the subscription survived the upgrade"
    else
        fail "subscription GET returned $code: $(cat "$WORK/body")"
    fi

    : > "$WORK/notifications"
    code=$(json -X PATCH "$API/entities/$ENTITY/attrs" -H 'Content-Type: application/json' -d '{
      "level": {"type": "Property", "value": 4, "observedAt": "2026-01-04T00:00:00Z"}
    }')
    [ "$code" = 204 ] || fail "post-upgrade patch returned $code: $(cat "$WORK/body")"
    local waited=0
    while [ "$waited" -lt 60 ]; do
        grep -q "$ENTITY" "$WORK/notifications" 2>/dev/null && break
        sleep 0.5
        waited=$((waited + 1))
    done
    if grep -q "$ENTITY" "$WORK/notifications" 2>/dev/null; then
        ok "a write through the new binary fires the old binary's subscription"
    else
        fail "the subscription stored by the old binary never fired"
    fi
}

scenario() {
    local label=$1
    shift
    echo "== $label =="
    start_broker "$OLD" "$label-old" "$@" || { failures=$((failures + 1)); return; }
    seed || { stop_broker; return; }
    stop_broker
    start_broker "$NEW" "$label-new" "$@" || { failures=$((failures + 1)); return; }
    assert_served
    stop_broker
}

start_sink
scenario file ANTARES_STORE=file ANTARES_DATA_DIR="$WORK/data"

if [ -n "${ANTARES_TEST_DATABASE_URL:-}" ]; then
    # one broker process at a time, which is what this harness runs and
    # what the local bus needs before it will share a database
    scenario postgres ANTARES_STORE=postgres ANTARES_ALLOW_SHARED_LOCAL=1 \
        ANTARES_DATABASE_URL="$ANTARES_TEST_DATABASE_URL"
else
    echo "== postgres =="
    echo "  skipped: ANTARES_TEST_DATABASE_URL is unset"
fi

echo
if [ "$failures" -eq 0 ]; then
    echo "upgrade path: OK"
else
    echo "upgrade path: $failures assertion(s) failed"
fi
exit $((failures > 0))
