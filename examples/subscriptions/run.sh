#!/usr/bin/env bash
# HTTP-callback subscription end to end: subscribe, change, receive.
set -euo pipefail
URL="${BROKER_URL:-http://localhost:9090}/ngsi-ld/v1"
RECEIVER="${RECEIVER_URL:-http://127.0.0.1:9491}"
CTX="https://uri.etsi.org/ngsi-ld/v1/ngsi-ld-core-context-v1.9.jsonld"

python3 "$(dirname "$0")/receiver.py" 9491 > notifications.log & rcv=$!
trap 'kill $rcv 2>/dev/null' EXIT
sleep 0.3

curl -sf -o /dev/null -X POST "$URL/entities" -H 'Content-Type: application/ld+json' \
  -d '{"id":"urn:ngsi-ld:Door:sub:1","type":"Door",
       "state":{"type":"Property","value":"closed"},"@context":"'"$CTX"'"}'
curl -sf -o /dev/null -X POST "$URL/subscriptions" -H 'Content-Type: application/ld+json' \
  -d '{"id":"urn:ngsi-ld:Subscription:door","type":"Subscription",
       "entities":[{"type":"Door"}],"watchedAttributes":["state"],
       "notification":{"endpoint":{"uri":"'"$RECEIVER"'/notify"}},"@context":"'"$CTX"'"}'
curl -sf -o /dev/null -X PATCH "$URL/entities/urn:ngsi-ld:Door:sub:1/attrs/state" \
  -H 'Content-Type: application/ld+json' \
  -d '{"type":"Property","value":"open","@context":"'"$CTX"'"}'

for _ in $(seq 30); do grep -q "state=open" notifications.log 2>/dev/null && break; sleep 0.2; done
cat notifications.log
grep -q "state=open" notifications.log && echo "OK: notification received"
