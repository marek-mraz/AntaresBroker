#!/usr/bin/env bash
# API-level broker state reset (broker-agnostic; rows are the only truth in
# Antares, so this is a complete reset — the phantom-state trap from the
# Scorpio campaign does not apply, but the harness stays API-level anyway).
# -e stays off: a resource with nothing to delete answers non-zero.
set -uo pipefail
BASE="${1:-http://localhost:9090/ngsi-ld/v1}"

ids() { python3 -c 'import sys,json
try:
    d=json.load(sys.stdin)
    print("\n".join(e["id"] for e in d if isinstance(e,dict) and "id" in e))
except Exception: pass'; }

for res in subscriptions csourceSubscriptions csourceRegistrations; do
  curl -sf "$BASE/$res?limit=1000" | ids | while read -r id; do
    curl -sf -X DELETE "$BASE/$res/$id" -o /dev/null
  done
done

# current-state entities (purge-all, local mode) — also clears mirrored temporal
curl -sf -X DELETE "$BASE/entities/?local=true" -o /dev/null

# temporal-only entities
curl -sf "$BASE/temporal/entities?timerel=before&timeAt=2100-01-01T00:00:00Z&local=true&limit=1000" \
  | ids | while read -r id; do
    curl -sf -X DELETE "$BASE/temporal/entities/$id" -o /dev/null
  done
exit 0
