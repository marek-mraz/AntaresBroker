# Getting started

Every snippet on this page was executed against a broker built from this
repository before being committed.

## Install

**Docker** (multi-arch, amd64 + arm64):

```bash
docker run --rm -p 9090:9090 ghcr.io/marek-mraz/antares-broker:dev
```

**From source** (Rust toolchain per `rust-toolchain.toml`):

```bash
cargo run -p antares-broker        # serves http://0.0.0.0:9090
```

**Release binary**: attached to GitHub releases (from v0.1.0 on).

The default configuration needs zero infrastructure: in-memory store,
in-process bus, all roles in one process. Check it is up:

```bash
curl -s localhost:9090/q/health    # {"status":"UP","store":"memory",...}
```

## First entity

```bash
curl -i -X POST localhost:9090/ngsi-ld/v1/entities \
  -H 'Content-Type: application/ld+json' \
  -d '{
    "id": "urn:ngsi-ld:TemperatureSensor:001",
    "type": "TemperatureSensor",
    "temperature": {"type": "Property", "value": 21.5, "unitCode": "CEL"},
    "@context": "https://uri.etsi.org/ngsi-ld/v1/ngsi-ld-core-context-v1.8.jsonld"
  }'
# HTTP/1.1 201 Created
# Location: /ngsi-ld/v1/entities/urn:ngsi-ld:TemperatureSensor:001

curl -s 'localhost:9090/ngsi-ld/v1/entities?type=TemperatureSensor'
```

## First subscription

Start something that shows incoming HTTP requests — `nc -l 9491` is enough —
then subscribe:

```bash
curl -i -X POST localhost:9090/ngsi-ld/v1/subscriptions \
  -H 'Content-Type: application/ld+json' \
  -d '{
    "id": "urn:ngsi-ld:Subscription:demo",
    "type": "Subscription",
    "entities": [{"type": "TemperatureSensor"}],
    "watchedAttributes": ["temperature"],
    "notification": {"endpoint": {"uri": "http://127.0.0.1:9491/notify"}},
    "@context": "https://uri.etsi.org/ngsi-ld/v1/ngsi-ld-core-context-v1.8.jsonld"
  }'
# HTTP/1.1 201 Created
```

Change the watched attribute — the fragment is `application/ld+json` (with
`Content-Type: application/json` the `@context` would have to travel in the
`Link` header instead; mixing both is rejected per CIM 009 clause 6.3.5):

```bash
curl -i -X PATCH \
  localhost:9090/ngsi-ld/v1/entities/urn:ngsi-ld:TemperatureSensor:001/attrs/temperature \
  -H 'Content-Type: application/ld+json' \
  -d '{"type": "Property", "value": 42.0,
       "@context": "https://uri.etsi.org/ngsi-ld/v1/ngsi-ld-core-context-v1.8.jsonld"}'
# HTTP/1.1 204 No Content
```

The listener receives a `Notification` whose `data` carries the entity with
`temperature.value = 42.0`. MQTT delivery works the same way with an
`mqtt[s]://` endpoint URI.

## First federation pair

Two brokers, one Context Source Registration. Local processes talk over
private addresses, so the egress guard must be opened for the demo
(`ANTARES_EGRESS_ALLOW_PRIVATE=true` — never do this facing the internet;
the default rejects private egress as SSRF protection):

```bash
ANTARES_HTTP_PORT=9391 ANTARES_EGRESS_ALLOW_PRIVATE=true antares &   # broker A
ANTARES_HTTP_PORT=9392 ANTARES_EGRESS_ALLOW_PRIVATE=true antares &   # broker B
```

Create an entity only broker B knows:

```bash
curl -s -X POST localhost:9392/ngsi-ld/v1/entities \
  -H 'Content-Type: application/ld+json' \
  -d '{"id": "urn:ngsi-ld:ParkingSpot:B:042", "type": "ParkingSpot",
       "status": {"type": "Property", "value": "free"},
       "@context": "https://uri.etsi.org/ngsi-ld/v1/ngsi-ld-core-context-v1.8.jsonld"}'
```

Tell broker A that broker B serves `ParkingSpot` entities:

```bash
curl -i -X POST localhost:9391/ngsi-ld/v1/csourceRegistrations \
  -H 'Content-Type: application/ld+json' \
  -d '{
    "id": "urn:ngsi-ld:ContextSourceRegistration:brokerB",
    "type": "ContextSourceRegistration",
    "information": [{"entities": [{"type": "ParkingSpot"}]}],
    "endpoint": "http://127.0.0.1:9392",
    "@context": "https://uri.etsi.org/ngsi-ld/v1/ngsi-ld-core-context-v1.8.jsonld"
  }'
# HTTP/1.1 201 Created
```

Query broker A — the result comes from broker B through the registration:

```bash
curl -s 'localhost:9391/ngsi-ld/v1/entities?type=ParkingSpot'
# [{"id":"urn:ngsi-ld:ParkingSpot:B:042","type":"ParkingSpot",
#   "status":{"type":"Property","value":"free"}}]
```

That is the whole federation model: registrations route queries (and
writes, subscriptions, temporal queries) to the sources that declared the
matching types and id patterns. The [federation guide](federation.md)
covers distributed operations in depth.
