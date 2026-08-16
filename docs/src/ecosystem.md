# Antares in the NGSI-LD ecosystem

## A compliant peer, not a fork

NGSI-LD (ETSI GS CIM 009) is the context-management standard behind
FIWARE-style smart-city platforms, and its whole point is that brokers
are interchangeable: the same entities, queries, subscriptions and
federation registrations work against any compliant implementation.
Antares is a from-scratch Rust implementation of that standard — it
shares no code with Orion-LD, Scorpio or Stellio, follows their naming
tradition, and federates with them over the standard distributed-
operations API. A deployment can mix brokers per site and migrate
between them by replaying declarative state; conformance is the
contract, and Antares publishes its evidence continuously
([conformance](conformance.md)).

## Where Antares fits best

- **Resource-constrained and edge deployments** — a ~35 MiB broker
  reaches places a JVM stack does not: industrial gateways, in-vehicle
  units, one-per-site municipal boxes ([deployment](deployment.md)).
- **The browser and offline-first tooling** — the wasm build is an
  NGSI-LD broker with zero installation: training environments, demos
  that need no backend, per-user sandboxes, edge UIs that keep working
  disconnected ([wasm](wasm.md)).
- **High-density multi-tenancy** — one shared schema with Row-Level
  Security and a 10,000-tenant design target makes per-user or
  per-department context spaces cheap, instead of one broker per tenant.

## The configuration plane

Antares deliberately stays a vanilla data-plane engine: no YAML
bootstrap, no vendor config API. Declarative city configuration —
entities, subscriptions, registrations, pipelines as Git-versioned
manifests, reconciled through the standard API — is a companion-project
concern (the "city-as-code" pattern described in
[operations → upgrades](operations.md#upgrades)). That split is what
keeps the broker upgradable and replaceable, and it works with any
compliant broker, not just Antares.

## Standards posture

Implementation is spec-first against CIM 009 V1.9.1 (per-clause ledger,
[conformance](conformance.md)); suspected defects in the official test
suite are proven from the spec text and raised upstream at ETSI rather
than worked around. Smart Data Models payloads work as-is — see the
smart-city example dataset in the repository.
