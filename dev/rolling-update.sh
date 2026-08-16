#!/usr/bin/env bash
# Rolling update across the HA pair — one instance at a time,
# using whatever antares-local:latest currently is (i.e. the image the
# pipeline just built). Same script locally and in CI (the one-pipeline
# rule).
#
# Precondition: the HA stack is up —
#   docker compose -f compose-files/docker-compose-etsi.yml \
#                  -f compose-files/docker-compose-ha.yml [--profile db] up -d
#
# ROLES_SPLIT=1: roll the 10-pod role fleet
# (docker-compose-roles.yml) instead of the HA pair, in role-group order —
# api, matcher, notifier, temporal, registry. The invariant is per GROUP:
# the peer of the same role must be healthy before its twin goes down, so no
# role ever has 0 live pods. api pods are judged by /q/health + the LB rise
# window (the drain contract); workers serve ops endpoints only and are
# judged by /q/ready.
#
# Per instance the sequence is exactly the drain contract:
#   docker compose stop  -> SIGTERM -> broker flips /q/health to 503, haproxy
#   ejects it within 400 ms, in-flight requests finish, process exits 0
#   (stop_grace_period 30 s > drain delay + deadline, set in the overlay)
#   docker compose up -d -> recreates on the current image
#   wait for /q/health 200 on its private port, then a rise window so the LB
#   has re-admitted it BEFORE the next instance goes down — never 0 healthy.
#
# Env:
#   STORE=memory|postgres|timescale   selects the db profile (default memory)
#   ROLES_SPLIT=1                     roll the role fleet (postgres/timescale
#                                     only — the overlay is bus=nats)
#   ROLL_SERVICES="antares1 antares1b" which instances to roll, in order
set -euo pipefail
cd "$(dirname "$0")/.."

STORE="${STORE:-memory}"
PROFILE=()
case "$STORE" in
  postgres|timescale) PROFILE=(--profile db) ;;
  file) echo "file mode cannot roll: redb allows ONE process per volume. Use Recreate."; exit 1 ;;
esac

if [ "${ROLES_SPLIT:-0}" = 1 ]; then
  case "$STORE" in
    postgres|timescale) ;;
    *) echo "ROLES_SPLIT=1 needs STORE=postgres|timescale (the roles overlay is ANTARES_BUS=nats)"; exit 1 ;;
  esac
  OVERLAY=compose-files/docker-compose-roles.yml
  DEFAULT_SERVICES="antares1 api2 matcher1 matcher2 notifier1 notifier2 temporal1 temporal2 registry1 registry2"
else
  OVERLAY=compose-files/docker-compose-ha.yml
  DEFAULT_SERVICES="antares1 antares1b"
  # `up -d` re-resolves the overlay's ${HA_BUS} — pin it to the mode's
  # correct bus here, or a memory-mode roll would recreate brokers with
  # bus=nats and they would refuse to boot (per-process state).
  case "$STORE" in
    postgres|timescale) export HA_BUS="${HA_BUS:-nats}" ;;
    *) export HA_BUS="${HA_BUS:-local}" ;;
  esac
fi
COMPOSE=(docker compose -f compose-files/docker-compose-etsi.yml \
                        -f "$OVERLAY" "${PROFILE[@]}")

port_of() {
  case "$1" in
    antares1) echo 9095;; antares1b|api2) echo 9096;;
    matcher1) echo 9110;; matcher2) echo 9111;;
    notifier1) echo 9112;; notifier2) echo 9113;;
    temporal1) echo 9114;; temporal2) echo 9115;;
    registry1) echo 9116;; registry2) echo 9117;;
    *) echo "unknown service $1" >&2; exit 1;;
  esac
}

# Role group of a service — the never-0-healthy invariant is per group.
group_of() {
  case "$1" in
    antares1|antares1b|api2) echo api;;
    matcher*) echo matcher;; notifier*) echo notifier;;
    temporal*) echo temporal;; registry*) echo registry;;
  esac
}

# api pods carry the NGSI-LD surface -> /q/health; workers are ops-only
# -> /q/ready (which also gates on store/bus, the stronger probe).
probe_of() {
  case "$(group_of "$1")" in api) echo /q/health;; *) echo /q/ready;; esac
}

wait_healthy() { # $1=service $2=deadline-secs
  local port; port=$(port_of "$1")
  local probe; probe=$(probe_of "$1")
  local start=$SECONDS
  until curl -sf "localhost:$port$probe" >/dev/null; do
    if (( SECONDS - start > $2 )); then
      echo "$1 (:$port$probe) never became healthy"; return 1
    fi
    sleep 0.5
  done
}

SERVICES="${ROLL_SERVICES:-$DEFAULT_SERVICES}"

roll_one() { # $1=service — its group peers must be healthy before it goes down
  local svc=$1 port group t0
  port=$(port_of "$svc")
  group=$(group_of "$svc")
  echo "=== rolling $svc (:$port) ==="

  # Sanity: every OTHER member of this role group must be healthy before we
  # take this one down, or the roll turns into a per-role outage.
  for other in $SERVICES; do
    [ "$other" = "$svc" ] && continue
    [ "$(group_of "$other")" = "$group" ] || continue
    wait_healthy "$other" 30 || { echo "peer $other unhealthy — aborting roll"; return 1; }
  done

  t0=$SECONDS
  "${COMPOSE[@]}" stop "$svc"          # SIGTERM -> drain -> clean exit
  "${COMPOSE[@]}" up -d "$svc"         # recreate on the current image
  wait_healthy "$svc" 60 || return 1
  # api only: haproxy needs `rise 2` consecutive passing checks (200 ms apart)
  # before it routes here again; wait that out so the next stop never leaves
  # 0 backends. Workers are not behind the LB — no rise window to wait.
  if [ "$group" = api ]; then sleep 1; fi
  echo "=== $svc rolled in $((SECONDS - t0))s ==="
}

# ROLL_PARALLEL (default: on for the roles fleet, off for the HA pair):
# each role GROUP rolls independently and CONCURRENTLY — k8s rolls its
# Deployments the same way — while inside a group members still go one at a
# time, so the per-group never-0-healthy invariant is untouched. Cycle time
# collapses from the sum of all pods to the slowest group's pair.
if [ "${ROLL_PARALLEL:-${ROLES_SPLIT:-0}}" = 1 ]; then
  groups=$(for s in $SERVICES; do group_of "$s"; done | awk '!seen[$0]++')
  pids=()
  for g in $groups; do
    ( for svc in $SERVICES; do
        [ "$(group_of "$svc")" = "$g" ] || continue
        roll_one "$svc" || exit 1
      done ) &
    pids+=($!)
  done
  rc=0
  for p in "${pids[@]}"; do wait "$p" || rc=1; done
  [ "$rc" = 0 ] || { echo "parallel roll FAILED"; exit 1; }
else
  for svc in $SERVICES; do roll_one "$svc" || exit 1; done
fi
echo "rolling update complete"
