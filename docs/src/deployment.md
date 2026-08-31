# Deployment

Every shape on this page is exercised by CI: the compose stacks run the
full ETSI suite in the matrix cells, the HA/rolling shape runs weekly
(`roll-weekly`) and in the two `-nats` cells, and the k8s manifests encode
the same constraints. Resource numbers are measured by the matrix run
(1 Hz sampling of every broker process, tables in each run's CI summary).

## Sizing (measured, full ETSI suite as workload)

| Shape | RSS avg | RSS peak | Notes |
|---|---|---|---|
| Native broker, any store | ~35 MiB | 41–64 MiB | peak is the Subscription suite |
| Idle | ~9 MiB | — | memory store, no traffic |
| wasm Node shim | 74–111 MiB | up to 185 MiB | Node runtime overhead, not the broker |

The CI resource gate enforces 350 MiB during the suite. Postgres sizing
follows standard PostgreSQL tuning; the weekly scale run publishes the
measured resident set of both.

## Sizing under load (measured, weekly scale run)

One broker process holding every role, 1 000 000 entities over 100 tenants,
10 000 subscriptions and 10 000 registrations, on eight cores:

| Workload | broker RSS peak |
|---|---|
| Queries and retrieves, saturated | 102 MiB |
| Notifications, 100–500 updates/s | 388 → 786 MiB |
| Forwarded reads, 50 queries/s (34 sources each) | 809 MiB |
| Forwarded reads, 500 queries/s | 5297 MiB |

The last row is a queue of unfinished requests, not a working set: the p99
is 34 s and one query in seven answers 5xx, while 50 queries/s answers every
one with a p99 of 385 ms. Between those rows the broker's resident set grows
by about 1.8 MiB per forwarded read in flight, and what bounds that number is
`ANTARES_MAX_CONNECTIONS` — so a pod's memory limit and its connection
ceiling are one decision, not two. The default ceiling of 10 000 admits far
more than a small pod can hold: the reference manifests set 512 against a
1 GiB limit.

## Single node, no database

```bash
# memory: state dies with the process — tests, demos
docker run --rm -p 9090:9090 ghcr.io/marek-mraz/antares-broker:dev

# file: durable via redb, fsync-before-ack; the data dir MUST be a volume
docker run --rm -p 9090:9090 \
  -e ANTARES_STORE=file -e ANTARES_DATA_DIR=/data \
  -v antares-data:/data \
  ghcr.io/marek-mraz/antares-broker:dev
```

`file` mode constraints (measured, documented in the README store table):
queries run on in-memory maps (~19 KB RSS per typical entity — comfortable
to ~10k entities), one writer at ~3.1k fsynced writes/s, backup is
stop-copy only (redb holds an exclusive lock). Beyond that, move to
`postgres`.

## Single node with PostgreSQL

```bash
docker compose -f compose-files/docker-compose.yml up
```

Broker + PostGIS. `timescale` differs only in the image and
`ANTARES_STORE`; temporal data lands in a hypertable. Set
`ANTARES_REQUIRE_RLS=1` in shared-schema multi-tenant deployments so the
broker refuses a DB role that bypasses Row-Level Security.
Tenants are created implicitly by the first write; listing and purging
them is described under [operations](operations.md#tenants).

## HA: replicas behind a load balancer

```bash
docker compose -f compose-files/docker-compose-ha.yml up
```

Two broker replicas + haproxy + NATS JetStream + PostGIS — the
rolling-update shape. The contract that makes rolls invisible:

1. `stop_grace_period` (30 s in the compose file) MUST exceed
   `ANTARES_DRAIN_DELAY_MS` + `ANTARES_DRAIN_DEADLINE_SECS` (default
   0.5 s + 20 s), or `docker stop` turns the drain into a kill.
2. Replicas of one logical broker share `ANTARES_HOST_ALIAS` — behind the
   LB they are one hop for federation loop detection.
3. `/q/health` answers 503 DRAINING during the drain window; the LB pulls
   the pod before the socket closes.

## Role-split fleet (scale-out)

Five roles × two replicas from the same binary — only `api` pods serve
HTTP; matcher/notifier/temporal/registry consume the JetStream streams:

```bash
STORE=postgres docker compose -f compose-files/docker-compose-etsi.yml \
  -f compose-files/docker-compose-roles.yml --profile db up -d
dev/roles-smoke.sh                                       # notify chain fires EXACTLY once
STORE=postgres ROLES_SPLIT=1 bash dev/rolling-update.sh  # roll all 10 in role-group order
```

This is the exact shape the `postgres-nats`/`timescale-nats` CI cells run
the whole ETSI suite against — while the fleet rolls continuously.

## Kubernetes

Reference manifests in [`deploy/k8s/`](https://github.com/marek-mraz/AntaresBroker/tree/master/deploy/k8s)
encode the constraints the store mode dictates instead of leaving them as
deployment choices:

- `broker-file.yaml` is **hard-coded `Recreate`**: redb takes an exclusive
  file lock — a rolling update would deadlock on the volume.
- `broker-postgres.yaml` rolls normally; readiness = `/q/ready` (store
  ping + bus connected).
- `nats.yaml` is a 3-node JetStream StatefulSet (`ANTARES_NATS_REPLICAS=3`).
- `postgres-cnpg.yaml` uses the CloudNativePG operator for primary/replica
  failover; `postgres-dev.yaml` is a single labelled-dev pod.

## Upgrades

Blue/green is the recommended path for major upgrades: deploy the new
version empty, replay declarative state (entities/subscriptions/
registrations) through the standard API, verify, switch traffic — the
broker's config-plane companion pattern. In-place minor upgrades follow
the rolling contract above. The `file` store carries a format version and
refuses a mismatched file rather than serving partial data.
