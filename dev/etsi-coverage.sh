#!/usr/bin/env bash
# ETSI-driven coverage — which broker code the conformance suite (and the
# Rust test suite) NEVER executes.
#
# strict.yml's daily floor measures unit-test coverage only. This script
# instruments the BROKER BINARY itself (cargo llvm-cov / LLVM
# -C instrument-coverage), optionally runs the workspace tests under the same
# instrumentation, then drives the broker with the same Robot suites as
# dev/etsi-run.sh. All profiles merge into ONE report, so a zero-count line
# means: no Rust test and no ETSI TP ever ran it. Per store mode, identical
# locally and in CI (.github/workflows/etsi-coverage.yml is a thin wrapper).
#
# Env knobs:
#   STORE=memory|file|postgres|timescale   mode under test      (default memory)
#   ANTARES_DATABASE_URL   required for postgres/timescale
#   UNIT_TESTS=1           also run `cargo test --workspace` instrumented,
#                          merging Rust-test coverage in (default 0 locally
#                          for the fast loop; CI sets 1)
#   SUITES=…               subset for a quick loop (default: all serial suites)
#   MQTT=0|1               058_* need docker mosquitto — default 0 locally,
#                          CI sets 1 (rule 8: MQTT is CI-only)
#   RESULTS_DIR            default results/coverage-$STORE
#
# Output under $RESULTS_DIR: lcov.info, html/ (line-level drill-down),
# coverage.json, summary.txt, uncovered-functions.txt (the point of it all),
# plus the per-suite Robot logs. Report-only — no floor here; ratchet gates
# belong in strict.yml once numbers stabilise.
set -euo pipefail
cd "$(dirname "$0")/.."

STORE="${STORE:-memory}"
RESULTS_DIR="${RESULTS_DIR:-results/coverage-$STORE}"
PORT="${ANTARES_HTTP_PORT:-9090}"   # 050_04/051_03 hardcode 9090 — keep it

command -v cargo-llvm-cov >/dev/null || {
  echo "cargo-llvm-cov missing — cargo install cargo-llvm-cov --locked"; exit 1; }

case "$STORE" in
  postgres|timescale)
    : "${ANTARES_DATABASE_URL:?$STORE mode needs ANTARES_DATABASE_URL}" ;;
  file)
    export ANTARES_DATA_DIR="${ANTARES_DATA_DIR:-$(mktemp -d)}" ;;
esac

# Instrumentation env: cargo-llvm-cov 0.8 injects -C instrument-coverage via
# a RUSTC_WRAPPER, which does NOT change cargo fingerprints — correctness
# depends entirely on `clean` wiping stale uninstrumented artifacts. clean
# refuses (but still exits 0! — hence the hard check below) when the target
# dir lacks cargo's CACHEDIR.TAG marker; a target/ created by anything other
# than cargo itself is missing it. Seen 2026-08-14: the silent clean failure
# reused uninstrumented rlibs and the coverage map covered 4 of 11 crates.
mkdir -p target
[ -f target/CACHEDIR.TAG ] || printf 'Signature: 8a477f597d28d172789f06886806bc55\n' > target/CACHEDIR.TAG
source <(cargo llvm-cov show-env --export-prefix)
CLEAN_OUT=$(cargo llvm-cov clean --workspace 2>&1) || { echo "$CLEAN_OUT"; exit 1; }
case "$CLEAN_OUT" in *"cannot clean"*)
  echo "$CLEAN_OUT"
  echo "clean failed — stale uninstrumented artifacts would corrupt the report"; exit 1;;
esac

if [ "${UNIT_TESTS:-0}" = 1 ]; then
  echo "=== workspace tests (instrumented) ==="
  # Failures don't abort: coverage measures what ran, the per-push gate for
  # test green is ci.yml. -j 2: default parallelism OOM-kills the linker here.
  cargo test --workspace -j 2 || true
fi

echo "=== build instrumented broker ==="
cargo build -p antares-broker
BIN="${CARGO_LLVM_COV_TARGET_DIR}/debug/antares"

echo "=== start broker ($STORE) on :$PORT ==="
ANTARES_STORE="$STORE" \
ANTARES_HTTP_PORT="$PORT" \
ANTARES_PUBLIC_URL="http://localhost:$PORT" \
ANTARES_EGRESS_ALLOW_PRIVATE=true \
ANTARES_SWEEP_SECS=2 \
"$BIN" > "${TMPDIR:-/tmp}/antares-coverage-$STORE.log" 2>&1 &
BROKER_PID=$!
trap 'kill -TERM "$BROKER_PID" 2>/dev/null || true; wait "$BROKER_PID" 2>/dev/null || true' EXIT
for _ in $(seq 60); do
  curl -sf "http://localhost:$PORT/q/health" >/dev/null && break
  kill -0 "$BROKER_PID" 2>/dev/null || { echo "broker died on startup:"; tail -20 "${TMPDIR:-/tmp}/antares-coverage-$STORE.log"; exit 1; }
  sleep 1
done
curl -sf "http://localhost:$PORT/q/health" >/dev/null || { echo "broker never became healthy"; exit 1; }

echo "=== ETSI suite against the instrumented broker ==="
# STOP_ON_ERROR=0 always: a red TP still exercises code, and coverage gates
# on data produced, not on suite green (the matrix gates green).
BROKER_URL="http://localhost:$PORT/ngsi-ld/v1" \
CALLBACK_HOST="${CALLBACK_HOST:-127.0.0.1}" \
MQTT="${MQTT:-0}" STOP_ON_ERROR=0 RESULTS_DIR="$RESULTS_DIR" \
  dev/etsi-run.sh || true

# Graceful stop: the .profraw is written by the exit hook — SIGTERM drains
# and exits cleanly; SIGKILL would lose the whole broker profile.
kill -TERM "$BROKER_PID"
wait "$BROKER_PID" || true
trap - EXIT

echo "=== reports ==="
mkdir -p "$RESULTS_DIR"
cargo llvm-cov report --lcov --output-path "$RESULTS_DIR/lcov.info"
cargo llvm-cov report --html --output-dir "$RESULTS_DIR/html"
cargo llvm-cov report --json --output-path "$RESULTS_DIR/coverage.json"
cargo llvm-cov report --summary-only | tee "$RESULTS_DIR/summary.txt"

# The list this exercise exists for: functions with execution count 0 —
# code neither the Rust tests (UNIT_TESTS=1) nor any ETSI TP ever ran.
# Names are mangled unless rustfilt is installed; html/ has the demangled
# line-level view either way.
python3 - "$RESULTS_DIR/coverage.json" <<'PY' > "$RESULTS_DIR/uncovered-functions.txt"
import json, shutil, subprocess, sys
data = json.load(open(sys.argv[1]))
rows = []
for export in data["data"]:
    for fn in export.get("functions", []):
        if fn.get("count", 0) == 0:
            files = [f for f in fn.get("filenames", []) if "/crates/" in f]
            if files:
                rows.append((files[0].split("/crates/")[-1], fn["name"]))
out = "\n".join(f"{f}\t{n}" for f, n in sorted(set(rows)))
if shutil.which("rustfilt"):
    out = subprocess.run(["rustfilt"], input=out, capture_output=True, text=True).stdout
print(out)
PY
echo "=== functions no test ran ($STORE): $(grep -c . "$RESULTS_DIR/uncovered-functions.txt" || true) ==="
head -30 "$RESULTS_DIR/uncovered-functions.txt"
echo "full list: $RESULTS_DIR/uncovered-functions.txt — line level: $RESULTS_DIR/html/index.html"
