# Temporal API

The temporal API (CIM 009 clauses 5.6.11–5.6.16, 5.7.3, 5.7.4) serves the
history of attribute values. Every example below runs against the
quickstart broker seeded by `examples/quickstart/seed.sh`; `$U` is
`http://localhost:9090/ngsi-ld/v1` and `$L` the `Link` header carrying the
core `@context`.

## How history is recorded

Every write through the entity endpoints produces temporal events for the
attribute instances it changed. The events are buffered for the request
and drained to the temporal driver after the handler returns and before
the response leaves, so a temporal read that follows a write sees it. An
update that changes no value produces no event; `ANTARES_TEMPORAL_RECORD`
narrows what enters history to observed instances or to nothing. A driver
failure in the drain never changes the response already produced; it is
counted as `temporalDrainErrors` on `/q/health`. The gates, the driver
choice `ANTARES_TEMPORAL` and retention are described in
[Storage drivers](storage.md#what-enters-history).

Three patches of `temperature` with an `observedAt` each:

```bash
curl -X POST $U/entities/urn:ngsi-ld:TemperatureSensor:qs:1/attrs \
  -H 'Content-Type: application/json' -H "$L" \
  -d '{"temperature":{"type":"Property","value":24.1,"unitCode":"CEL","observedAt":"2026-08-26T09:00:00Z"}}'
```

The seed value `21.5` carried no `observedAt`, so it appears only under
`timeproperty=modifiedAt` (below), never in the default `observedAt`
history.

## Querying

`timerel` is `before`, `after` or `between` around `timeAt` (and
`endTimeAt` for `between`), applied to `timeproperty` (`observedAt` by
default; `createdAt`, `modifiedAt`, `deletedAt`). `after` and `before`
include the `timeAt` instant itself. `attrs` restricts the attributes.

```bash
curl -si "$U/temporal/entities/urn:ngsi-ld:TemperatureSensor:qs:1?timerel=after&timeAt=2026-08-26T08:30:00Z&attrs=temperature"
```

```json
{"id": "urn:ngsi-ld:TemperatureSensor:qs:1", "type": "TemperatureSensor",
 "temperature": [
   {"type": "Property", "instanceId": "urn:ngsi-ld:Instance:6e0d857e-8845-57ee-9e5b-bbcc34d89a0b",
    "observedAt": "2026-08-26T09:00:00Z", "unitCode": "CEL", "value": 24.1},
   {"type": "Property", "instanceId": "urn:ngsi-ld:Instance:19536e56-b044-51cd-8658-2600fb344d98",
    "observedAt": "2026-08-26T10:00:00Z", "unitCode": "CEL", "value": 26.8}]}
```

Each instance carries an `instanceId`; it is the handle for the
instance-level operations below. `GET $U/temporal/entities?type=…`
queries several entities with the same parameters plus `q`, `geoQ`,
`scopeQ`, `id`, `idPattern` and paging; `POST
$U/temporal/entityOperations/query` takes the same query as a `Query`
body with a `temporalQ` member:

```bash
curl -X POST $U/temporal/entityOperations/query -H 'Content-Type: application/json' -H "$L" -d '{
  "type": "Query", "entities": [{"type": "TemperatureSensor"}],
  "temporalQ": {"timerel": "after", "timeAt": "2026-08-25T10:00:00Z"},
  "attrs": ["temperature"], "q": "temperature>25"}'
```

Errors are `BadRequestData` 400 with the reason in `detail`:

```text
{"detail":"invalid timerel \"since\"", ...}
{"detail":"timeAt must be a valid ISO 8601 DateTime (4.11)", ...}
```

### Representation

`format=temporalValues` (or `options=temporalValues`) collapses each
attribute to `[value, time]` pairs:

```json
{"id": "urn:ngsi-ld:TemperatureSensor:qs:1", "type": "TemperatureSensor",
 "temperature": {"type": "Property",
   "values": [[22.4, "2026-08-26T08:00:00Z"], [24.1, "2026-08-26T09:00:00Z"], [26.8, "2026-08-26T10:00:00Z"]]}}
```

`timeproperty=modifiedAt` keys the history on the write time instead, so
the seed value shows up:

```json
"values": [[21.5, "2026-08-26T15:59:33.332Z"], [22.4, "2026-08-26T15:59:33.409Z"],
           [24.1, "2026-08-26T15:59:33.416Z"], [26.8, "2026-08-26T15:59:33.423Z"]]
```

`lastN=2` keeps the two newest instances of each attribute, newest first:

```json
"temperature": [
  {"type": "Property", "observedAt": "2026-08-26T10:00:00Z", "value": 26.8, ...},
  {"type": "Property", "observedAt": "2026-08-26T09:00:00Z", "value": 24.1, ...}]
```

`lastN` must be a positive integer; `pick`/`omit` and `sysAttrs` apply as
on the entity endpoints.

### Aggregation

`aggrMethods` (`totalCount`, `distinctCount`, `sum`, `avg`, `min`, `max`,
`stddev`, `sumsq`) with `aggrPeriodDuration` returns one row per bucket
as `[value, bucketStart, bucketEnd]`, buckets anchored at `timeAt`:

```bash
curl "$U/temporal/entities/urn:ngsi-ld:TemperatureSensor:qs:1?timerel=between&timeAt=2026-08-26T08:00:00Z&endTimeAt=2026-08-26T11:00:00Z&attrs=temperature&aggrMethods=avg,max&aggrPeriodDuration=PT1H"
```

```json
"temperature": {"type": "Property",
  "avg": [[22.4, "2026-08-26T08:00:00Z", "2026-08-26T09:00:00Z"],
          [24.1, "2026-08-26T09:00:00Z", "2026-08-26T10:00:00Z"],
          [26.8, "2026-08-26T10:00:00Z", "2026-08-26T11:00:00Z"]],
  "max": [[22.4, "2026-08-26T08:00:00Z", "2026-08-26T09:00:00Z"], ...]}
```

Without `aggrPeriodDuration` (or with `PT0S`) the whole range is one
bucket, ending one second after the last instance:

```json
"avg": [[24.433333333333334, "2026-08-26T08:00:00Z", "2026-08-26T10:00:01Z"]],
"sum": [[73.3, "2026-08-26T08:00:00Z", "2026-08-26T10:00:01Z"]],
"totalCount": [[3, "2026-08-26T08:00:00Z", "2026-08-26T10:00:01Z"]]
```

Which methods apply to which value type follows Table 4.5.19.1-1; a
method that does not apply to the attribute's values is a 400.

On postgres and timescale the aggregation runs in SQL when the query is
exact there: no `q`, `geoQ` or `scopeQ`, the page pushed down, no `omit`,
second-granular period, and only numeric or boolean values in the
window. Any other shape, and every query on the memory and file drivers,
reconstructs the instances and aggregates in the broker. The result is
the same; the SQL path was measured at 5.7 s → 0.59 s for 50 entities
with 150k instances.

### Pagination

One response carries at most nine instances per attribute. Beyond that
the broker cuts the whole entity at one instant, answers `206 Partial
Content` and names the window it served in `Content-Range`. Twelve hourly
instances of a sensor:

```text
HTTP/1.1 206 Partial Content
Content-Range: date-time 2026-08-25T00:00:00Z-2026-08-25T08:00:00Z/*
{"id": "urn:ngsi-ld:TemperatureSensor:qs:2", "type": "TemperatureSensor",
 "temperature": {"type": "Property", "values": [[20.0, "2026-08-25T00:00:00Z"], ..., [28.0, "2026-08-25T08:00:00Z"]]}}
```

Continue from the instant after the range end; the last page answers 200:

```text
GET …?timerel=after&timeAt=2026-08-25T09:00:00Z&attrs=temperature&format=temporalValues
HTTP/1.1 200 OK
"values": [[29.0, "2026-08-25T09:00:00Z"], [30.0, "2026-08-25T10:00:00Z"], [31.0, "2026-08-25T11:00:00Z"]]
```

Every attribute is trimmed to the same boundary, so no instance of any
attribute falls between two pages. Aggregated representations are
computed over the whole evolution and are never cut. Entity-level paging
of `GET /temporal/entities` uses `limit`/`offset` and `Link rel="next"`
as the entity endpoints do.

### Temporal entity maps

`entityMap=true` on a multi-entity query pins the matched id set for the
following pages, so a client walking `rel="next"` links sees a stable set
even while entities change:

```text
HTTP/1.1 201 Created
Link: </ngsi-ld/v1/temporal/entities?entityMap=true&limit=1&offset=1&…>; rel="next";type="application/json"
```

The map lives under `/temporal/entityMaps/{id}` for one hour by default;
a client may set `expiresAt` on the map, capped at 24 hours (6.4.3.2-1).
It is stored in the current-state driver, so it survives a restart on
file and postgres.

## Writing and deleting history

| operation | clause | request |
|---|---|---|
| create a temporal entity with its instances | 5.6.11 | `POST $U/temporal/entities` |
| add instances to an attribute | 5.6.12 | `POST $U/temporal/entities/{id}/attrs` with `{"temperature": [instance, …]}` |
| delete an attribute's history | 5.6.13 | `DELETE $U/temporal/entities/{id}/attrs/{attr}`, `?datasetId=` for one instance set, `?deleteAll=true` for every set |
| modify one instance | 5.6.14 | `PATCH $U/temporal/entities/{id}/attrs/{attr}/{instanceId}` |
| delete one instance | 5.6.15 | `DELETE $U/temporal/entities/{id}/attrs/{attr}/{instanceId}` |
| purge an entity's history | 5.6.16 | `DELETE $U/temporal/entities/{id}` |

All answer 204; a missing instance or entity is `ResourceNotFound` 404:

```text
{"detail":"instance urn:ngsi-ld:Instance:cbcc1cff-46bb-4b7f-b82e-f42a60d75542 not found","status":404,"title":"ResourceNotFound", ...}
```

Purging a temporal entity removes its history only; `GET
$U/entities/{id}` still answers 200 with the current state. Deleting the
current-state entity mirrors a deletion instance into history, so the
entity's last state remains queryable under `timeproperty=deletedAt`.

Instances added through the temporal endpoints are stored as sent.
Instances the entity endpoints record with an `observedAt` are keyed on
(entity, attribute, `datasetId`, `observedAt`), so a sensor re-sending
the same measurement replaces the instance instead of duplicating it.

## Retention and `none`

`ANTARES_TEMPORAL_RETENTION_DAYS` starts a sweep on the postgres or
timescale half that drops instances older than the horizon; unset keeps
everything. With `ANTARES_TEMPORAL=none` every temporal endpoint answers
`OperationNotSupported` 422 (Table 6.3.2-1) and the entity endpoints
record nothing:

```text
HTTP/1.1 422 Unprocessable Entity
{"detail":"no temporal store is configured","status":422,"title":"OperationNotSupported","type":"https://uri.etsi.org/ngsi-ld/errors/OperationNotSupported"}
```

`/q/health` names both halves: `"store": "memory", "temporal": "none"`.
