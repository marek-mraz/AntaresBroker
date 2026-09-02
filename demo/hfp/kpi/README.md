# Helsinki demo: Bento + wasm + tenant-to-tenant KPIs

Live HSL tram telemetry -> NGSI-LD tenant `helsinki` -> a Rust/wasm module ->
NGSI-LD tenant `helsinki-kpi`. No SQL, no second data plane, no privileged
database reader: every hop crosses the tenant boundary through the NGSI-LD API.

Built and run against live `mqtt.hsl.fi:8883`.

## Pieces

| File | What it is |
|---|---|
| `ingest.yaml` | HFP MQTT -> change-only NGSI-LD upserts + derived arrival events |
| `kpi.yaml` | reads two entity types -> wasm -> upserts KPI entities to the KPI tenant |
| `wasm-kpi/src/lib.rs` | the algorithm: p95 speed, per-stop headway, bunching |
| `ui/` | the map + KPI panel, and the tenant-aware proxy that serves it |
| `run.sh` | builds the module and brings the whole stack up, down, or reports it |

Broker: `antares` release build on `:42020`. The store defaults to `timescale`,
because the memory store grows without bound under a continuous firehose;
`STORE=file` keeps everything native, `STORE=memory` suits a throwaway run.
Bento: v1.20.0 (`warpstreamlabs/bento`, the MIT fork).

## Run it

```bash
./run.sh start     # module tests, wasm build, broker, both pipelines, UI
./run.sh status    # what is running, and how many entities each tenant holds
./run.sh stop
```

Or by hand:

```bash
# 1. broker
ANTARES_HTTP_PORT=42020 ./target/release/antares &

# 2. build the module (needs: rustup target add wasm32-unknown-unknown)
cd wasm-kpi && cargo test && cargo build --release --target wasm32-unknown-unknown

# 3. ingest live Helsinki trams
bento -c ingest.yaml &

# 4. compute KPIs into the second tenant
bento -c kpi.yaml
```

## What the ingest pipeline proves

**Change-only writes.** HFP re-sends a full VP for every tram every second. The
pipeline keeps the last dynamic state per vehicle in a Bento memory cache and
drops every attribute whose value is unchanged, dropping the message entirely
when nothing moved.

**Per-attribute `expiresAt`.** CIM 009 clause 4.22 defines `expiresAt` as a
system temporal Property of "a certain Entity, Property or Relationship", so it
is set on the individual attributes being written, not on the entity. Observed
on one tram after 30 s of ingest:

| attribute | `expiresAt` | `modifiedAt` |
|---|---|---|
| `occupancy` | 09:36:09 | 09:31:11 — never changed since creation |
| `delay` | 09:36:19 | 09:31:21 |
| `heading` | 09:36:42 | 09:31:43 |
| `location` | 09:36:43 | 09:31:45 |
| `nextStop` | none | static context, no expiry |

Different expiry per attribute, because each was stamped only when that value
actually changed. Static context (`route`, `transportMode`) is written once on
first sighting and never expires.

**Change detection earns a second keep.** A vehicle whose `nextStop` changes has
just left the previous stop, which is a real arrival event. The pipeline emits
those as `StopArrival` entities with an entity-level 30-minute `expiresAt`
(the whole event is transient). Snapshot timestamps cannot produce headway;
arrival events can.

## What the wasm module computes, and why it exists

CIM 009 clause 4.5.19.1 defines exactly eight aggregation methods
(`avg`, `distinctCount`, `max`, `min`, `stddev`, `sum`, `sumsq`, `totalCount`),
each applied per Entity per Attribute per time bucket. That covers "mean speed
of tram 7 last hour". It cannot express:

- **p95 speed** — percentiles are not in the vocabulary;
- **headway** — the gap between consecutive arrivals of *different* vehicles at
  the same stop, which is ordering-dependent and crosses entities;
- **bunching** — a threshold over adjacent gaps.

So the module is the tier-3 escape hatch, and nothing more: everything the
temporal API *can* aggregate should be left to the broker.

### Host ABI (Bento `wasm` processor)

Undocumented outside the source, so recorded here. The host
(`internal/impl/wasm/processor_wazero.go`) looks for module exports
`allocate`/`deallocate` (Rust naming; `malloc`/`free` for Go) and calls the
configured function with **no arguments**. The module imports from module
`bento_wasm`:

- `v0_msg_as_bytes() -> u64` — packed `(ptr << 32) | len` into the module's own memory
- `v0_msg_set_bytes(ptr: u32, size: u32)` — hands the result back

Bento's docs call this ecosystem "delicate" and ship a TinyGo helper only; the
Rust side of the ABI is implemented by hand in `lib.rs`.

## Results

Both tenants populated from live data:

```
tenant helsinki      : 110 Vehicle, 144 StopArrival
tenant helsinki-kpi  :  11 TransportKPI

line      n  p95 km/h
2015     39      69.7
1004     20      38.1
1008T    22      37.5
1007     21      34.1
...
```

### Honest gap: headway needs a longer window

`meanHeadwaySeconds` is absent from every KPI entity, and that is correct
behaviour rather than a bug. Verified directly:

```
arrival events      : 144
distinct line+stop  : 144
stops seen twice+   : 0
```

Helsinki tram headway is around 10 minutes, so no stop had been visited twice by
the same line within the collection window. Headway appears once ingest has run
roughly 15-20 minutes. The per-stop grouping itself is covered by a unit test
(`headway_is_per_stop_not_pooled_across_the_line`) which asserts that pooling
stops would report 2 false bunching incidents.

## Live UI

`ui/` — the original HFP map (viewport-bounded `georel=within` paging,
transport-mode colours, click a vehicle for its temporal trail plus speed/delay
spark charts and a data table) with a KPI panel beside it, polling the second
tenant every 4 s and flashing rows that changed.

```bash
python3 ui/serve.py   # http://localhost:42030
```

`ui/serve.py` is a tenant-aware sibling of `../map/serve.py`: it proxies
`/api/<tenant>/<path>` to the broker with the `NGSILD-Tenant` header set from
the path segment, because the broker sends no CORS headers and sits outside the
published port range.

## Trap: HFP `nextStop` flaps

The first version treated any `nextStop` change as an arrival. One vehicle then
produced **82 "arrivals" at a single stop, one per second**, because HFP
oscillates `nextStop` between the current and next stop. Line 1002 reported 167
bunching incidents and a 0.0 s minimum headway, which is nonsense.

Fixed with a second cache used as a gate: `operator: add` fails when the key
exists, so the first arrival of a given vehicle at a given stop wins and the
rest are dropped for the TTL. Dedupe is per **vehicle per stop**, so other
vehicles still register and headway between them stays measurable.

Result: duplicates fell from ~80 per vehicle-stop to a handful, and headway
became plausible (7-19 min across lines, bunching mostly zero). A residue
remains on lines whose vehicles alternate A→B→A: the key alternates too, so two
events slip through per flap pair. Tightening that needs a per-vehicle debounce
across stops rather than per stop.

## Footprint

| Process | RSS |
|---|---|
| antares broker (release, memory store, 110 entities) | ~286 MB |
| bento (ingest, live MQTT firehose) | ~171 MB |
| wasm module on disk | 82 KB |

## Traps hit while building this

1. **Debug broker cannot serve the demo.** With a debug build, `GET /entities?limit=1000`
   timed out while ingest ran. Release build fixed it. Never demo on debug.
2. **`archive: json_array` double-wraps.** The KPI mapping already emits one
   message whose content is a JSON array, so archiving produced `[[...]]` and
   the batch upsert 400'd. Drop the archive when the message is already the array.
3. **Bloblang forbids a leading-dot method chain on a new line.** `}.filter(...)`
   then `.map_each(...)` on the next line is a parse error; keep the chain on one line.
4. **Cache miss must be caught.** A first-sighting vehicle has no cached state;
   without `try`/`catch` around the `cache` get the whole message fails.
5. **Store the full state, not the delta.** Caching only the changed attributes
   makes the next comparison run against a partial record and re-emit unchanged
   values.
