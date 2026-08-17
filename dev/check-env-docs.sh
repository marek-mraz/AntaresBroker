#!/usr/bin/env bash
# Gate: every ANTARES_* env var read by non-test code must have a row in
# docs/src/configuration.md. Grep-based on purpose — cheap and loud.
set -euo pipefail
cd "$(dirname "$0")/.."

# Vars the check ignores: test-only, negative-test typo strings, k8s
# service-env artifacts filtered by the code itself, NATS subject names.
# PORTAL/DB_PORT/WORKER_PROT are the kubelet-shape fixtures of
# the_service_link_exemption_does_not_over_reach — never real knobs.
IGNORE='^ANTARES_(TEST_|BOGUS_|STROE|HTPT_|HTTP_PROT$|CHANGES$|REGISTRY$|SERVICE|PORT$|PORT_|API_SERVICE|FILE_PORT|FILE$|GIT_HASH$|_|PORTAL$|DB_PORT$|WORKER_PROT$)'

code_vars=$(/usr/bin/grep -rhoa 'ANTARES_[A-Z0-9_]*' crates/ --include='*.rs' \
  | sort -u | /usr/bin/grep -Ev "$IGNORE" | /usr/bin/grep -Ev '^ANTARES_$' || true)
missing=0
for v in $code_vars; do
  if ! /usr/bin/grep -q "\`$v\`" docs/src/configuration.md; then
    echo "UNDOCUMENTED env var: $v (add a row to docs/src/configuration.md)"
    missing=1
  fi
done
[ "$missing" -eq 0 ] && echo "env docs check: OK ($(echo "$code_vars" | wc -l) vars documented)"
exit "$missing"
