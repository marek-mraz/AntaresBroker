#!/usr/bin/env bash
# N6/N7a: prove the built wasm artifact in BOTH runtimes — the Node shim
# (health + entity round-trip + a real HTTP notification) and headless
# Chromium (React playground: OPFS worker broker, demo board, in-page
# notification, second tab, federation). Needs www/pkg (dev/wasm-build.sh),
# npm deps in www/, a built www/dist, and a playwright chromium.
# One script, local and CI (§E rule).
set -euo pipefail
cd "$(dirname "$0")/.."

PORT="${PORT:-9391}"
RECV_PORT=$((PORT + 1))

# --- Node tier smoke ------------------------------------------------------
node www/node-shim.mjs "$PORT" & SHIM=$!
node -e "
  require('node:http').createServer((req,res)=>{let b='';req.on('data',c=>b+=c);req.on('end',()=>{console.log('RECEIVED-NOTIFICATION');res.end();});}).listen($RECV_PORT);
" & RECV=$!
trap 'kill $SHIM $RECV 2>/dev/null || true' EXIT
for _ in $(seq 1 50); do curl -sf "http://127.0.0.1:$PORT/q/health" >/dev/null && break; sleep 0.2; done

CTX="https://uri.etsi.org/ngsi-ld/v1/ngsi-ld-core-context-v1.8.jsonld"
code=$(curl -s -o /dev/null -w '%{http_code}' -X POST "http://127.0.0.1:$PORT/ngsi-ld/v1/entities" \
  -H 'Content-Type: application/ld+json' \
  -d "{\"id\":\"urn:ngsi-ld:Smoke:1\",\"type\":\"Smoke\",\"v\":{\"type\":\"Property\",\"value\":1},\"@context\":\"$CTX\"}")
[ "$code" = 201 ] || { echo "node tier: create expected 201, got $code" >&2; exit 1; }
code=$(curl -s -o /dev/null -w '%{http_code}' -X POST "http://127.0.0.1:$PORT/ngsi-ld/v1/subscriptions" \
  -H 'Content-Type: application/ld+json' \
  -d "{\"id\":\"urn:ngsi-ld:Subscription:smoke\",\"type\":\"Subscription\",\"entities\":[{\"type\":\"Smoke\"}],\"notification\":{\"endpoint\":{\"uri\":\"http://127.0.0.1:$RECV_PORT/n\",\"accept\":\"application/json\"}},\"@context\":\"$CTX\"}")
[ "$code" = 201 ] || { echo "node tier: subscribe expected 201, got $code" >&2; exit 1; }
curl -s -o /dev/null -X PATCH "http://127.0.0.1:$PORT/ngsi-ld/v1/entities/urn:ngsi-ld:Smoke:1/attrs/v" \
  -H 'Content-Type: application/ld+json' \
  -d "{\"type\":\"Property\",\"value\":2,\"@context\":\"$CTX\"}"
for _ in $(seq 1 50); do
  ts=$(curl -s "http://127.0.0.1:$PORT/ngsi-ld/v1/subscriptions/urn:ngsi-ld:Subscription:smoke" | grep -o '"timesSent":[0-9]*' | cut -d: -f2)
  [ "${ts:-0}" -ge 1 ] && break; sleep 0.2
done
[ "${ts:-0}" -ge 1 ] || { echo "node tier: notification never sent" >&2; exit 1; }
echo "node tier OK (create 201, notification sent over real HTTP)"
kill $SHIM $RECV 2>/dev/null || true

# --- File-store tier (N4 persistence outside the browser) -----------------
# ANTARES_STORE=file: the SAME .wasm over an fs-backed sync-access handle
# (the Node stand-in for OPFS). Proof is restart survival; the negative half
# proves memory mode does NOT survive — otherwise this tier tests nothing.
FDIR="$(mktemp -d)"
FURN="urn:ngsi-ld:Smoke:persist"
ANTARES_STORE=file ANTARES_FILE="$FDIR/antares.redb" node www/node-shim.mjs "$PORT" & SHIM=$!
for _ in $(seq 1 50); do curl -sf "http://127.0.0.1:$PORT/q/health" >/dev/null && break; sleep 0.2; done
curl -s "http://127.0.0.1:$PORT/q/health" | grep -q '"store":"file"' \
  || { echo "file tier: /q/health must report store=file" >&2; exit 1; }
code=$(curl -s -o /dev/null -w '%{http_code}' -X POST "http://127.0.0.1:$PORT/ngsi-ld/v1/entities" \
  -H 'Content-Type: application/ld+json' \
  -d "{\"id\":\"$FURN\",\"type\":\"Smoke\",\"v\":{\"type\":\"Property\",\"value\":1},\"@context\":\"$CTX\"}")
[ "$code" = 201 ] || { echo "file tier: create expected 201, got $code" >&2; exit 1; }
kill $SHIM 2>/dev/null || true; wait $SHIM 2>/dev/null || true
ANTARES_STORE=file ANTARES_FILE="$FDIR/antares.redb" node www/node-shim.mjs "$PORT" & SHIM=$!
for _ in $(seq 1 50); do curl -sf "http://127.0.0.1:$PORT/q/health" >/dev/null && break; sleep 0.2; done
code=$(curl -s -o /dev/null -w '%{http_code}' "http://127.0.0.1:$PORT/ngsi-ld/v1/entities/$FURN")
[ "$code" = 200 ] || { echo "file tier: entity must survive a restart, got $code" >&2; exit 1; }
kill $SHIM 2>/dev/null || true; wait $SHIM 2>/dev/null || true
# negative: the memory shim restarted on the same port loses everything
node www/node-shim.mjs "$PORT" & SHIM=$!
for _ in $(seq 1 50); do curl -sf "http://127.0.0.1:$PORT/q/health" >/dev/null && break; sleep 0.2; done
code=$(curl -s -o /dev/null -w '%{http_code}' "http://127.0.0.1:$PORT/ngsi-ld/v1/entities/$FURN")
[ "$code" = 404 ] || { echo "file tier negative: memory restart must 404, got $code" >&2; exit 1; }
kill $SHIM 2>/dev/null || true
rm -rf "$FDIR"
echo "file tier OK (store=file reported, restart survival held, memory negative held)"

# --- Browser tier ---------------------------------------------------------
# BROWSER_TIER=0 skips it (ci's per-commit wasm publish gate: node + file
# tiers only, no chromium; the full browser tier runs in wasm.yml/full.yml).
if [ "${BROWSER_TIER:-1}" = 1 ]; then
  node www/test/browser-test.mjs
fi
