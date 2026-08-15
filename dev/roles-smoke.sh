#!/usr/bin/env bash
# Roles-fleet smoke (backlog 2026-08-15b item 1) — NOT a TP. Asserts:
#   1. every fleet pod answers: api pods /q/health (and via the LB), workers
#      /q/ready — polled from outside because the image is distroless (no
#      shell for a compose healthcheck probe);
#   2. one entity created via the LB notifies through the split
#      matcher/notifier chain — and EXACTLY once: the four matcher/notifier
#      pods share ONE durable, so a duplicate delivery means two consumers
#      processed one change (the negative assertion).
#
# Precondition: the roles stack is up —
#   STORE=postgres DB_IMAGE=ghcr.io/baosystems/postgis:17-3.5 \
#   docker compose -f compose-files/docker-compose-etsi.yml \
#                  -f compose-files/docker-compose-roles.yml --profile db up -d
set -euo pipefail
cd "$(dirname "$0")/.."

API_PORTS="9095 9096"
WORKER_PORTS="9110 9111 9112 9113 9114 9115 9116 9117"
# LB_PORT override: the sandbox has no haproxy binary, so the local
# process-level fleet run points this at api1 directly (LB_PORT=9095);
# the compose stack (CI) uses the real LB on 9090.
LB="${LB_PORT:-9090}"

wait_200() { # $1=url $2=deadline-secs $3=label
  local start=$SECONDS
  until curl -sf "$1" >/dev/null; do
    if (( SECONDS - start > $2 )); then echo "FAIL: $3 ($1) never answered 200"; return 1; fi
    sleep 0.5
  done
  echo "ok: $3"
}

echo "=== fleet readiness ==="
wait_200 "localhost:$LB/q/health" 60 "LB -> api"
for p in $API_PORTS;    do wait_200 "localhost:$p/q/health" 60 "api :$p"; done
for p in $WORKER_PORTS; do wait_200 "localhost:$p/q/ready"  60 "worker :$p"; done

echo "=== notify chain through the split roles ==="
RUN=$(date +%s%N)
TENANT="rolesmoke$RUN"
SEEN="$(mktemp -d)/seen"
: > "$SEEN"
RX_PORT=$(python3 -c "import socket; s=socket.socket(); s.bind(('127.0.0.1',0)); print(s.getsockname()[1]); s.close()")

# notification receiver: appends every POST body to $SEEN, answers 200
python3 - "$SEEN" "$RX_PORT" <<'PY' &
import http.server, sys, socketserver
seen, port = sys.argv[1], int(sys.argv[2])
class H(http.server.BaseHTTPRequestHandler):
    def do_POST(self):
        body = self.rfile.read(int(self.headers.get("Content-Length", 0)))
        with open(seen, "ab") as f:
            f.write(body + b"\n")
        self.send_response(200); self.send_header("Content-Length", "0"); self.end_headers()
    def log_message(self, *a): pass
socketserver.TCPServer.allow_reuse_address = True
with socketserver.TCPServer(("127.0.0.1", port), H) as srv:
    srv.serve_forever()
PY
RX_PID=$!
trap 'kill $RX_PID 2>/dev/null || true' EXIT

# The receiver must be ACCEPTING before the subscription exists: delivery is
# attempted within ms of the entity write and a refused connection is a
# terminal failure (5.8.6 status=failed, no redelivery) — seen live 2026-08-15
# as a 1-in-3 smoke flake (times_sent=1, last_failure 4 ms after send).
until curl -s -o /dev/null "http://127.0.0.1:$RX_PORT/"; do sleep 0.1; done

curl -sf -X POST "localhost:$LB/ngsi-ld/v1/subscriptions" \
  -H "Content-Type: application/json" -H "NGSILD-Tenant: $TENANT" \
  -d "{\"id\":\"urn:ngsi-ld:Subscription:rolesmoke:$RUN\",\"type\":\"Subscription\",
       \"entities\":[{\"type\":\"RoleSmoke\"}],
       \"notification\":{\"endpoint\":{\"uri\":\"http://127.0.0.1:$RX_PORT/notify\"}}}" \
  -o /dev/null -w "subscription: %{http_code}\n"

EID="urn:ngsi-ld:RoleSmoke:$RUN"
curl -sf -X POST "localhost:$LB/ngsi-ld/v1/entities" \
  -H "Content-Type: application/json" -H "NGSILD-Tenant: $TENANT" \
  -d "{\"id\":\"$EID\",\"type\":\"RoleSmoke\",\"temperature\":{\"type\":\"Property\",\"value\":21}}" \
  -o /dev/null -w "entity: %{http_code}\n"

start=$SECONDS
until /usr/bin/grep -a -q "$EID" "$SEEN" 2>/dev/null; do
  if (( SECONDS - start > 30 )); then echo "FAIL: no notification within 30 s"; exit 1; fi
  sleep 0.5
done
echo "ok: notification delivered through the split matcher/notifier chain"

# negative: EXACTLY once — wait out any would-be duplicate, then count
sleep 5
COUNT=$(/usr/bin/grep -a -c "$EID" "$SEEN")
if [ "$COUNT" != 1 ]; then
  echo "FAIL: expected exactly 1 notification, got $COUNT (duplicate delivery across the role pair)"
  exit 1
fi
echo "ok: exactly one notification (no duplicate from the pair)"
echo "roles smoke PASS"
