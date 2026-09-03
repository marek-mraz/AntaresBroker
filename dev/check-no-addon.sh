#!/usr/bin/env bash
# Release gate: no addon in the shipped build.
#
# An engine, a façade or a driver from outside `crates/` is an addon behind
# an off-by-default feature (ADR-0017, ADR-0020, docs/src/extending.md). The
# shipped image, the release binaries and every CI gate run the core alone,
# and conformance is asserted against the built-in allow-all engine — so an
# addon reaching a release build would make the conformance claim untrue for
# whoever ran that binary.
#
# Today that holds by construction: the Dockerfile and the release build use
# `-p antares-broker --locked` with the default features, and every addon is
# an optional dependency. This makes it a GATE, so a future feature or a
# change of `default` cannot undo it quietly.
set -euo pipefail
cd "$(dirname "$0")/.."

# What may never appear in a default-feature build of the broker. Anything
# under `examples/` is an addon by construction; the names are the addon
# crates this repository knows about, so a rename that dodges the path rule
# is still caught.
FORBIDDEN='examples/|antares-plugin-|antares-policy-|antares-facade-|antares-sensorthings|antares-ogcapi|antares-wfs|antares-odata'

tree() { cargo tree -p antares-broker -e normal --locked "$@"; }

shipped=$(tree)
[ "$(printf '%s' "$shipped" | wc -l)" -gt 100 ] || {
  echo "cargo tree returned $(printf '%s' "$shipped" | wc -l) lines — the gate would pass vacuously"
  exit 1
}

# Self-test: the pattern must actually FIND an addon when one is there.
# Without this the gate is a grep that has never matched anything.
tree --features plugin-example | grep -Eq "$FORBIDDEN" || {
  echo "self-test failed: the pattern does not match a build that DOES carry the addon"
  exit 1
}

if found=$(printf '%s\n' "$shipped" | grep -E "$FORBIDDEN"); then
  echo "ADDON IN THE SHIPPED BUILD — the release runs the core alone:"
  printf '%s\n' "$found"
  exit 1
fi
echo "no-addon gate: OK ($(printf '%s' "$shipped" | wc -l) normal dependencies, none an addon)"
