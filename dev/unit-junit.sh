#!/usr/bin/env bash
# Run one cargo-nextest config and KEEP its JUnit under a per-config name.
#
#   dev/unit-junit.sh <config-name> [nextest args...]
#
# nextest always writes target/nextest/ci/junit.xml, so consecutive configs
# overwrite each other — this copies it out before the next run clobbers it.
# The copy happens even when the run is red (that is the interesting case),
# and the cargo exit code is preserved so the CI step still fails.
set -u
OUT=${UNIT_JUNIT_DIR:-unit-junit}
name=$1
shift
# JUnit carries no provenance — a bundle of XML says WHAT failed but not
# which commit it failed on. Stamp it once so the report page can name the
# revision and link failures to source at the right SHA.
mkdir -p "$OUT"
printf '{"sha":"%s","repo":"%s","ref":"%s","run":"%s"}\n' \
  "${GITHUB_SHA:-$(git rev-parse HEAD 2>/dev/null || echo unknown)}" \
  "${GITHUB_REPOSITORY:-marek-mraz/AntaresBroker}" \
  "${GITHUB_REF_NAME:-local}" \
  "${GITHUB_RUN_ID:-}" > "$OUT/meta.json"
cargo nextest run --profile ci --locked "$@"
rc=$?
if [ -f target/nextest/ci/junit.xml ]; then
  cp target/nextest/ci/junit.xml "$OUT/$name.xml"
else
  # compile failure: no tests ran, so there is no JUnit at all. Leave a
  # marker the report page can render red instead of silently dropping the
  # whole config off the page.
  printf '<testsuites name="%s" tests="0" failures="1" errors="1" time="0">\n  <testsuite name="%s" tests="0" failures="1" errors="1">\n    <testcase name="(build)" classname="%s"><failure type="build">nextest produced no JUnit — the config failed to build</failure></testcase>\n  </testsuite>\n</testsuites>\n' \
    "$name" "$name" "$name" > "$OUT/$name.xml"
fi
exit $rc
