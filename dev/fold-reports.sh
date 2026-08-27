#!/usr/bin/env bash
# Fold the newest ETSI-matrix-results bundle + weekly coverage into site/
# (site/reports/latest, badge.json + per-cell badge-<cell>.json,
# site/reports/coverage + coverage-badge.json). Shared by wasm.yml and
# pages.yml so the two Pages paths cannot drift. Needs: GH_TOKEN, REPO.
# A missing bundle renders a placeholder instead of failing the deploy.
set -e
id=$(gh api "repos/$REPO/actions/artifacts?name=ETSI-matrix-results&per_page=1" \
     -q '.artifacts[0].id' 2>/dev/null || true)
mkdir -p site/reports/latest
if [ -n "$id" ] && [ "$id" != "null" ]; then
  gh api "repos/$REPO/actions/artifacts/$id/zip" > matrix.zip
  mkdir -p cells && unzip -q matrix.zip -d cells
  python3 dev/etsi-report-site.py cells site/reports/latest
else
  echo '<!doctype html><title>ETSI report</title><p>No ETSI-matrix-results bundle in the retention window — dispatch the etsi-matrix workflow.' \
    > site/reports/latest/index.html
  echo '{"schemaVersion":1,"label":"ETSI CIM 009","message":"no recent run","color":"lightgrey"}' \
    > site/reports/badge.json
fi
# the unit/integration JUnit bundle from the newest `workspace` job —
# rendered as its own page so the ETSI half keeps Robot's HTML untouched
uid=$(gh api "repos/$REPO/actions/artifacts?name=unit-junit&per_page=1" \
      -q '.artifacts[0].id' 2>/dev/null || true)
mkdir -p site/reports/unit
if [ -n "$uid" ] && [ "$uid" != "null" ]; then
  gh api "repos/$REPO/actions/artifacts/$uid/zip" > unit.zip
  mkdir -p unit-junit && unzip -q -o unit.zip -d unit-junit
  python3 dev/unit-report-site.py unit-junit site/reports/unit
else
  echo '<!doctype html><title>Unit tests</title><p>No unit-junit bundle in the retention window — run the ci workflow.' \
    > site/reports/unit/index.html
  echo '{"schemaVersion":1,"label":"unit tests","message":"no recent run","color":"lightgrey"}' \
    > site/reports/badge-unit.json
fi
# the weekly merged coverage (html + % badge)
cid=$(gh api "repos/$REPO/actions/artifacts?name=etsi-coverage-merged&per_page=1" \
      -q '.artifacts[0].id' 2>/dev/null || true)
if [ -n "$cid" ] && [ "$cid" != "null" ]; then
  gh api "repos/$REPO/actions/artifacts/$cid/zip" > cov.zip
  mkdir -p cov && unzip -q cov.zip -d cov
  mkdir -p site/reports/coverage
  [ -d cov/html ] && cp -r cov/html/. site/reports/coverage/ || true
  pct=$(awk -F: '/^LF:/{lf+=$2} /^LH:/{lh+=$2} END{printf "%.1f", (lf?100*lh/lf:0)}' cov/merged.info)
  color=$(awk -v p="$pct" 'BEGIN{print (p>=80)?"brightgreen":((p>=60)?"yellow":"red")}')
  printf '{"schemaVersion":1,"label":"coverage (ETSI+unit)","message":"%s%%","color":"%s"}' "$pct" "$color" \
    > site/reports/coverage-badge.json
else
  echo '{"schemaVersion":1,"label":"coverage","message":"no recent run","color":"lightgrey"}' \
    > site/reports/coverage-badge.json
fi

# the performance runs: newest weekly (shapes) and scale (design targets)
# bundles, each with its index.html + perf.json + CSVs
for name in perf-weekly-results scale-weekly-results; do
  pid=$(gh api "repos/$REPO/actions/artifacts?name=$name&per_page=1" \
        -q '.artifacts[0].id' 2>/dev/null || true)
  if [ -n "$pid" ] && [ "$pid" != "null" ]; then
    gh api "repos/$REPO/actions/artifacts/$pid/zip" > "$name.zip" \
      && mkdir -p "site/reports/perf/latest/${name%-results}" \
      && unzip -qo "$name.zip" -d "site/reports/perf/latest/${name%-results}" || true
  fi
done
[ -d site/reports/perf/latest ] && cat > site/reports/perf/latest/index.html <<'HTML'
<!doctype html><meta charset=utf-8><title>Antares performance</title>
<p><a href="perf-weekly/">Weekly shapes (memory store)</a> · <a href="scale-weekly/">Scale run (design targets on PostgreSQL)</a></p>
HTML
true
