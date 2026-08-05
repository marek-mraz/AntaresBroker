#!/usr/bin/env bash
# Rolling update across the HA pair (tasks.md K3) — one instance at a time,
# using whatever antares-local:latest currently is (i.e. the image the
# pipeline just built). Same script locally and in CI (the §E one-pipeline
# rule).
#
# Precondition: the HA stack is up —
#   docker compose -f compose-files/docker-compose-etsi.yml \
#                  -f compose-files/docker-compose-ha.yml [--profile db] up -d
#
# Per instance the sequence is exactly the K1 contract:
#   docker compose stop  -> SIGTERM -> broker flips /q/health to 503, haproxy
#   ejects it within 400 ms, in-flight requests finish, process exits 0
#   (stop_grace_period 30 s > drain delay + deadline, set in the overlay)
#   docker compose up -d -> recreates on the current image
#   wait for /q/health 200 on its private port, then a rise window so the LB
#   has re-admitted it BEFORE the next instance goes down — never 0 healthy.
#
# Env:
#   STORE=memory|postgres|timescale   selects the db profile (default memory)
#   ROLL_SERVICES="antares1 antares1b" which instances to roll, in order
set -euo pipefail
cd "$(dirname "$0")/.."

STORE="${STORE:-memory}"
PROFILE=()
case "$STORE" in
  postgres|timescale) PROFILE=(--profile db) ;;
  file) echo "file mode cannot roll: redb allows ONE process per volume (K10). Use Recreate."; exit 1 ;;
esac
COMPOSE=(docker compose -f compose-files/docker-compose-etsi.yml \
                        -f compose-files/docker-compose-ha.yml "${PROFILE[@]}")

port_of() { case "$1" in antares1) echo 9095;; antares1b) echo 9096;; *) echo "unknown service $1" >&2; exit 1;; esac; }

wait_healthy() { # $1=port $2=deadline-secs
  local start=$SECONDS
  until curl -sf "localhost:$1/q/health" >/dev/null; do
    if (( SECONDS - start > $2 )); then
      echo "instance on :$1 never became healthy"; return 1
    fi
    sleep 0.5
  done
}

for svc in ${ROLL_SERVICES:-antares1 antares1b}; do
  port=$(port_of "$svc")
  echo "=== rolling $svc (:$port) ==="

  # Sanity: the OTHER instance must be healthy before we take this one down,
  # or the roll turns into an outage.
  for other in ${ROLL_SERVICES:-antares1 antares1b}; do
    [ "$other" = "$svc" ] && continue
    wait_healthy "$(port_of "$other")" 30 || { echo "peer $other unhealthy — aborting roll"; exit 1; }
  done

  t0=$SECONDS
  "${COMPOSE[@]}" stop "$svc"          # SIGTERM -> K1 drain -> clean exit
  "${COMPOSE[@]}" up -d "$svc"         # recreate on the current image
  wait_healthy "$port" 60
  # haproxy needs `rise 2` consecutive passing checks (200 ms apart) before it
  # routes here again; wait that out so the next stop never leaves 0 backends.
  sleep 1
  echo "=== $svc rolled in $((SECONDS - t0))s ==="
done
echo "rolling update complete"
