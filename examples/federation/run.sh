#!/usr/bin/env bash
# Two brokers, one CSR: query A, get B's entity.
# BROKER_A/BROKER_B env override the compose defaults (for local binaries).
set -euo pipefail
A="${BROKER_A:-http://localhost:9090}"
B="${BROKER_B:-http://localhost:9091}"
# B_FROM_A: how broker A reaches broker B (compose: the service name)
B_FROM_A="${B_FROM_A:-http://broker-b:9090}"
CTX="https://uri.etsi.org/ngsi-ld/v1/ngsi-ld-core-context-v1.9.jsonld"

curl -sf -o /dev/null -X POST "$B/ngsi-ld/v1/entities" \
  -H 'Content-Type: application/ld+json' \
  -d '{"id":"urn:ngsi-ld:ParkingSpot:fed:042","type":"ParkingSpot",
       "status":{"type":"Property","value":"free"},"@context":"'"$CTX"'"}'
echo "entity created on B"

curl -sf -o /dev/null -X POST "$A/ngsi-ld/v1/csourceRegistrations" \
  -H 'Content-Type: application/ld+json' \
  -d '{"id":"urn:ngsi-ld:ContextSourceRegistration:broker-b","type":"ContextSourceRegistration",
       "information":[{"entities":[{"type":"ParkingSpot"}]}],
       "endpoint":"'"$B_FROM_A"'","@context":"'"$CTX"'"}'
echo "CSR registered on A -> B"

echo "federated query via A:"
out=$(curl -sf "$A/ngsi-ld/v1/entities?type=ParkingSpot")
echo "$out"
echo "$out" | grep -q "urn:ngsi-ld:ParkingSpot:fed:042" && echo "OK: B's entity served by A"
