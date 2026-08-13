#!/usr/bin/env bash
# One metrics sample line: epoch, entity count, broker RSS/CPU, pg RSS, DB size.
BASE="${1:-http://localhost:42010/ngsi-ld/v1}"
count=$(curl -s -o /dev/null -D- "$BASE/entities?type=Vehicle&count=true&limit=1" \
  | tr -d '\r' | awk -F': ' 'tolower($1)=="ngsild-results-count"{print $2}')
stats=$(docker stats --no-stream --format '{{.Name}} {{.MemUsage}} {{.CPUPerc}}' 2>/dev/null | tr '\n' '|')
dbsize=$(docker exec hfp-timescale-1 psql -U antares -tAc \
  "select pg_size_pretty(pg_database_size('antares'))" 2>/dev/null)
hist=$(docker exec hfp-timescale-1 psql -U antares -tAc \
  "select count(*) from attr_instances" 2>/dev/null)
echo "$(date -u +%H:%M:%S) entities=${count:-?} db=${dbsize:-?} hist_rows=${hist:-?} :: $stats"
