#!/usr/bin/env bash
# Seed three entities and walk the basic queries.
set -euo pipefail
URL="${BROKER_URL:-http://localhost:9090}/ngsi-ld/v1"
CTX="https://uri.etsi.org/ngsi-ld/v1/ngsi-ld-core-context-v1.9.jsonld"

curl -sf -o /dev/null -X POST "$URL/entityOperations/create" \
  -H 'Content-Type: application/ld+json' -d '[
  {"id":"urn:ngsi-ld:TemperatureSensor:qs:1","type":"TemperatureSensor",
   "temperature":{"type":"Property","value":21.5,"unitCode":"CEL"},
   "location":{"type":"GeoProperty","value":{"type":"Point","coordinates":[19.15,48.73]}},
   "@context":"'"$CTX"'"},
  {"id":"urn:ngsi-ld:TemperatureSensor:qs:2","type":"TemperatureSensor",
   "temperature":{"type":"Property","value":28.0,"unitCode":"CEL"},
   "location":{"type":"GeoProperty","value":{"type":"Point","coordinates":[19.16,48.74]}},
   "@context":"'"$CTX"'"},
  {"id":"urn:ngsi-ld:TemperatureSensor:qs:3","type":"TemperatureSensor",
   "temperature":{"type":"Property","value":35.2,"unitCode":"CEL"},
   "location":{"type":"GeoProperty","value":{"type":"Point","coordinates":[19.17,48.75]}},
   "@context":"'"$CTX"'"}]'
echo "seeded 3 entities"

echo "— all TemperatureSensors:"
curl -sf "$URL/entities?type=TemperatureSensor" | python3 -m json.tool | head -8
echo "— hot ones (q=temperature>30):"
curl -sf "$URL/entities?type=TemperatureSensor&q=temperature%3E30"
echo
echo "— near a point (geo query, 2 km):"
curl -sf "$URL/entities?type=TemperatureSensor&georel=near%3BmaxDistance==2000&geometry=Point&coordinates=%5B19.15,48.73%5D"
echo
