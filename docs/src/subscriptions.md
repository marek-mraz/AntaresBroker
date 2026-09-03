# Subscriptions and notifications

Every example below runs against the quickstart broker
(`examples/quickstart/compose.yml`, or `cargo run -p antares-broker`) with
its three `TemperatureSensor` entities from `seed.sh`. Notifications are
received by a small HTTP server on port 9380 that answers 200 and prints
each body. `$U` is `http://localhost:9090/ngsi-ld/v1` and `$CTX` the core
context URL used by `seed.sh`.

## Create a subscription

```bash
curl -si -X POST $U/subscriptions -H 'Content-Type: application/ld+json' -d '{
  "id": "urn:ngsi-ld:Subscription:hot", "type": "Subscription",
  "entities": [{"type": "TemperatureSensor"}],
  "q": "temperature>30",
  "watchedAttributes": ["temperature"],
  "notification": {
    "attributes": ["temperature"],
    "endpoint": {"uri": "http://localhost:9380/notify", "accept": "application/json"}
  },
  "@context": "'$CTX'"}'
```

```text
HTTP/1.1 201 Created
Location: /ngsi-ld/v1/subscriptions/urn:ngsi-ld:Subscription:hot
```

A change that matches sends one notification. After
`PATCH $U/entities/urn:ngsi-ld:TemperatureSensor:qs:1/attrs` with
`temperature` 31.7, the receiver gets:

```json
{
  "id": "urn:ngsi-ld:Notification:e5fb42a7-f1c4-49d2-bbb8-bf2255448669",
  "type": "Notification",
  "subscriptionId": "urn:ngsi-ld:Subscription:hot",
  "notifiedAt": "2026-08-26T15:55:28.489Z",
  "data": [
    {"id": "urn:ngsi-ld:TemperatureSensor:qs:1", "type": "TemperatureSensor",
     "temperature": {"type": "Property", "unitCode": "CEL", "value": 31.7}}
  ]
}
```

With `accept: application/json` the `@context` travels in the `Link`
header:

```text
Content-Type: application/json
Link: <https://uri.etsi.org/ngsi-ld/v1/ngsi-ld-core-context-v1.9.jsonld>; rel="http://www.w3.org/ns/json-ld#context"; type="application/ld+json"
```

With `accept: application/ld+json` it is a member of the body instead.

## What triggers a notification

- `entities` (type, id, idPattern) and `q`, `geoQ`, `scopeQ` select the
  entities; `watchedAttributes` limits which attribute changes count; with
  no `watchedAttributes` every attribute of a matching entity counts.
- `notificationTrigger` defaults to `["attributeCreated",
  "attributeUpdated"]`, as the stored subscription above shows; add
  `attributeDeleted`, `entityCreated`, `entityUpdated` or `entityDeleted`
  to hear about those.
- An update that changes nothing sends nothing.
- `timeInterval` replaces change-driven delivery with a periodic one, see
  below. A subscription cannot carry both `timeInterval` and
  `watchedAttributes`.

## Notification shape

`notification.format` picks the entity representation:

| format | data entry |
|---|---|
| `normalized` (default) | full attributes, as above |
| `keyValues` (alias `simplified`) | `"temperature": 29.0`, GeoProperty values as bare GeoJSON |
| `concise` | attributes without the `type` member where it can be inferred |

`notification.attributes` restricts the attributes in each entry.
`sysAttrs: true` adds `createdAt`/`modifiedAt` at entity and attribute
level. `showChanges: true` adds `previousValue` (or `previousObject`,
`previousLanguageMap`) next to the new value; it requires a normalized
format:

```text
HTTP/1.1 400 Bad Request
{"detail":"showChanges cannot be true when format is keyValues (5.2.14)","title":"BadRequestData",...}
```

One patch of `temperature` to 29.0, seen by a `showChanges` + `sysAttrs`
subscription and by a `keyValues` one:

```json
{"id": "urn:ngsi-ld:TemperatureSensor:qs:1", "type": "TemperatureSensor",
 "createdAt": "2026-08-26T15:55:28.402Z", "modifiedAt": "2026-08-26T15:56:04.370Z",
 "temperature": {"type": "Property", "value": 29.0, "previousValue": 31.7, "unitCode": "CEL",
                 "createdAt": "2026-08-26T15:55:28.402Z", "modifiedAt": "2026-08-26T15:56:04.370Z"}}
```

```json
{"id": "urn:ngsi-ld:TemperatureSensor:qs:1", "type": "TemperatureSensor",
 "location": {"type": "Point", "coordinates": [19.15, 48.73]}, "temperature": 29.0}
```

## Grouped delivery

One request produces at most one notification per subscription, however
many entities it touched. A batch upsert of two sensors reaches the
`keyValues` subscription as one POST:

```json
{
  "id": "urn:ngsi-ld:Notification:c06c6141-a3fc-4553-8369-ddeb69aa56a2",
  "type": "Notification",
  "subscriptionId": "urn:ngsi-ld:Subscription:kv",
  "notifiedAt": "2026-08-26T15:56:06.437Z",
  "data": [
    {"id": "urn:ngsi-ld:TemperatureSensor:qs:2", "type": "TemperatureSensor", "temperature": 27.5},
    {"id": "urn:ngsi-ld:TemperatureSensor:qs:3", "type": "TemperatureSensor", "temperature": 34.9}
  ]
}
```

and `timesSent` moves by one. A notification body is capped at 4 MiB,
the same limit the broker accepts on inbound bodies. A grouped delivery
over the cap is split at whole-entity boundaries into several
notifications; a single entity larger than the cap is sent alone.

## Periodic delivery: `timeInterval`

```json
{"id": "urn:ngsi-ld:Subscription:tick", "type": "Subscription",
 "entities": [{"type": "TemperatureSensor"}], "timeInterval": 2,
 "notification": {"format": "keyValues",
   "endpoint": {"uri": "http://localhost:9380/tick", "accept": "application/json"}}}
```

Every 2 seconds the broker sends all matching entities, changed or not;
five seconds after creation the receiver holds two notifications with all
three sensors each. Any interval greater than zero is accepted, fractions
included; the broker checks due subscriptions twice a second, so an
interval below that is served at the tick rate. With NATS and several
broker pods, one pod claims each tick, so an interval fires once per
fleet.

## Throttling

`"throttling": 30` sends at most one notification per 30 seconds per
subscription; changes inside the window are dropped, not queued. Three
patches in a row produced one delivery and `timesSent: 1`. With several
pods on NATS the window is kept per pod.

## Delivery bookkeeping

`GET $U/subscriptions/{id}` shows the fields of CIM 009 5.8.6. After the
first successful notification:

```json
"notification": {
  "endpoint": {"accept": "application/json", "uri": "http://localhost:9380/notify"},
  "lastNotification": "2026-08-26T15:55:28.489Z",
  "lastSuccess": "2026-08-26T15:55:28.489Z",
  "status": "ok",
  "timesSent": 1
}
```

A subscription pointing at a closed port after one matching change:

```json
"notification": {
  "endpoint": {"accept": "application/json", "uri": "http://localhost:9399/nobody"},
  "lastNotification": "2026-08-26T15:55:37.662Z",
  "lastFailure": "2026-08-26T15:55:37.663Z",
  "status": "failed",
  "timesFailed": 1,
  "timesSent": 1
}
```

`timesSent` counts notifications, not attempts: a delivery that is retried
and then succeeds still adds one. `status` flips back to `ok` on the next
successful delivery.

### Retry and dead letters

The default is one attempt per notification. `ANTARES_NOTIFY_ATTEMPTS`,
`ANTARES_NOTIFY_BACKOFF_MS` and `ANTARES_NOTIFY_MAX_AGE_SECS` turn on
retries with exponential backoff on a separate task; a notification whose
attempts or age run out becomes a dead letter, listed and replayable
through `/q/dead-letters` and counted on `/q/health` as `deadLetters`.
See [Operations](operations.md#notification-delivery) and the
[Admin API](admin-api.md).

### Egress

Delivery goes through the same egress policy as `@context` fetches and
federation forwards: `http`, `https`, `mqtt` and `mqtts` only, redirects
capped, DNS pinned, response size capped, and a per-host breaker that
pauses a failing endpoint. Private and loopback destinations are allowed
by default; `ANTARES_EGRESS_ALLOW_PRIVATE=false` denies them for an
internet-facing deployment, and a refused delivery is booked as a failure.
A scheme no notification binding serves is refused when the subscription is
created — the endpoint is input data that does not meet the requirements of
the operation (5.8.1.4, Table 5.5.2-1), so the error is BadRequestData:

```json
{"detail":"no notification binding registered for endpoint scheme \"ftp\" (6.3.8)","status":400,"title":"BadRequestData",...}
```

## MQTT endpoints

An endpoint URI of the form `mqtt[s]://[user[:pass]@]host[:port]/topic`
delivers notifications as MQTT publishes (CIM 009 clause 7). The message
is `{"metadata": {...}, "body": <Notification>}`; protocol parameters go
in `notifierInfo`:

```json
"endpoint": {
  "uri": "mqtt://localhost:1883/antares/hot",
  "accept": "application/json",
  "notifierInfo": [{"key": "MQTT-Version", "value": "mqtt5.0"}, {"key": "MQTT-QoS", "value": "1"}]
}
```

The broker validates the URI and the parameters at creation (201 above,
with no MQTT broker running) and connects on the first delivery. Sessions
are pooled per endpoint and credentials; the password never appears in
error bodies or logs. The binding is the `mqtt` cargo feature, on by
default.
