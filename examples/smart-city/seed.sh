#!/usr/bin/env bash
# Seed the smart-city dataset (50 entities, Smart Data Models types) and
# run the city questions.
set -euo pipefail
cd "$(dirname "$0")"
URL="${BROKER_URL:-http://localhost:9090}/ngsi-ld/v1"

curl -sf -o /dev/null -X POST "$URL/entityOperations/upsert" \
  -H 'Content-Type: application/ld+json' -d @entities.json
echo "seeded $(python3 -c 'import json;print(len(json.load(open("entities.json"))))') entities"

q() { echo "— $1"; curl -sf "$URL/entities?$2" | python3 -c 'import json,sys;d=json.load(sys.stdin);print(f"  {len(d)} results:", ", ".join(e["id"].rsplit(":",1)[-1] for e in d[:8]), "..." if len(d)>8 else "")'; }
q "free parking spots"                "type=ParkingSpot&q=status==%22free%22&limit=100"
q "streetlights switched off"         "type=Streetlight&q=powerState==%22off%22&limit=100"
q "air quality: pm25 over 25"         "type=AirQualityObserved&q=pm25%3E25&limit=100"
q "waste containers at least 70% full" "type=WasteContainer&q=fillingLevel%3E=0.7&limit=100"
q "anything within 500 m of the square" "georel=near%3BmaxDistance==500&geometry=Point&coordinates=%5B19.145,48.735%5D&limit=100&type=ParkingSpot,Streetlight,AirQualityObserved,WasteContainer"
