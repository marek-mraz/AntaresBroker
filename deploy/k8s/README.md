# Reference Kubernetes manifests

The deployable shapes of the K-section HA table, with the update strategy the
store mode DICTATES baked in — not chosen per deployment:

| File | What | Strategy | Why |
|---|---|---|---|
| `namespace.yaml` | the `antares` namespace | — | — |
| `nats.yaml` | 3-node JetStream cluster (StatefulSet) | RollingUpdate | R3 stream replication survives one node; brokers set `ANTARES_NATS_REPLICAS=3` |
| `postgres-cnpg.yaml` | CloudNativePG `Cluster`, primary + streaming replica | operator-managed | Patroni or CNPG for failover; requires the [CNPG operator](https://cloudnative-pg.io) installed first |
| `postgres-dev.yaml` | ONE plain PostGIS pod | Recreate | kind/CI smoke only — no HA, clearly labelled |
| `broker-file.yaml` | `file`-mode broker, 1 replica + PVC | **Recreate, hard-coded** | redb takes an exclusive file lock: a rolling update would deadlock on the volume — the second pod dies on `Database already open` |
| `broker-postgres.yaml` | `postgres`-mode: 2× api + 2× worker (matcher,notifier,temporal) over NATS | **RollingUpdate** | stateless pods, shared store, shared durables — the rolling-update path the roll drill exercises |
| `networkpolicy.yaml` | deny-by-default ingress + one allow per flow | — | egress stays open: notification endpoints, Context Sources and `@context` URLs are client data, gated by `ANTARES_EGRESS_ALLOW_PRIVATE` and the scheme allowlist, not by a CIDR list |

Every pod carries explicit memory limits (the Scorpio lesson: no
manifest ships without them) and the broker's limit is the same 350 MiB the
CI RSS gate enforces. Readiness probe `/q/ready`, liveness `/q/health`;
drain wired:
`terminationGracePeriodSeconds` exceeds `ANTARES_DRAIN_DELAY_MS` +
`ANTARES_DRAIN_DEADLINE_SECS`, so a pod termination is a drain, never a kill.
Every pod runs non-root with all capabilities dropped and a read-only root
filesystem, except the plain PostGIS pod, whose entrypoint initialises
PGDATA as root before dropping privileges.

Connections inside the namespace are plaintext: the DSN carries no
`sslmode` and NATS is reached over `nats://`. A cluster that does not treat
its pod network as trusted terminates TLS at the database and the message
bus and changes the Secret and `ANTARES_NATS_URL` to match — the broker
passes both through unchanged, so nothing in it has to be rebuilt for that.

## CI coverage

The `k8s-manifests` job lints everything with kubeconform
(`-ignore-missing-schemas` covers the CNPG CRD) and smoke-deploys the
self-contained subset — nats + postgres-dev + broker-postgres + broker-file —
on a kind cluster with the image the pipeline just built: `rollout status`
green means the readiness probes (`/q/ready`) answered.

🖥 The real-cluster validation (a genuine 3-node NATS spread across nodes,
CNPG failover, the kill drills) needs a cluster you point it at — the
manifests are authored for that day; kind proves they deploy, not that they
fail over.

## Not here on purpose

Ingress/LB flavor, TLS termination, PEP/reverse-proxy policy: deployment
territory (authn/rate limiting live in front of the broker,
not in it).
