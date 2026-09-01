#!/usr/bin/env bash
# MQTT-notification variant (needs mqtt-compose.yml up + mosquitto_sub).
set -euo pipefail
URL="${BROKER_URL:-http://localhost:9090}/ngsi-ld/v1"
CTX="https://uri.etsi.org/ngsi-ld/v1/ngsi-ld-core-context-v1.8.jsonld"
mosquitto_sub -h "${MQTT_HOST:-localhost}" -t antares/door -C 1 > mqtt-notification.json & sub=$!
sleep 0.3
curl -sf -o /dev/null -X POST "$URL/entities" -H 'Content-Type: application/ld+json' \
  -d '{"id":"urn:ngsi-ld:Door:mqtt:1","type":"Door",
       "state":{"type":"Property","value":"closed"},"@context":"'"$CTX"'"}'
curl -sf -o /dev/null -X POST "$URL/subscriptions" -H 'Content-Type: application/ld+json' \
  -d '{"id":"urn:ngsi-ld:Subscription:door-mqtt","type":"Subscription",
       "entities":[{"type":"Door"}],"watchedAttributes":["state"],
       "notification":{"endpoint":{"uri":"mqtt://mosquitto:1883/antares/door"}},"@context":"'"$CTX"'"}'
curl -sf -o /dev/null -X PATCH "$URL/entities/urn:ngsi-ld:Door:mqtt:1/attrs/state" \
  -H 'Content-Type: application/ld+json' \
  -d '{"type":"Property","value":"open","@context":"'"$CTX"'"}'
wait $sub
grep -q '"state"' mqtt-notification.json && echo "OK: MQTT notification received"
