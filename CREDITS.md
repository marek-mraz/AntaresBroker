# Credits

Antares stands on the work of others. The people and projects below are
named because the broker would not exist, or would not be checkable,
without them.

## Specifications and tests

- **ETSI ISG CIM** for NGSI-LD, [ETSI GS CIM 009 V1.9.1](https://www.etsi.org/deliver/etsi_gs/CIM/001_099/009/01.09.01_60/gs_CIM009v010901p.pdf),
  and for the [NGSI-LD test suite](https://forge.etsi.org/rep/cim/ngsi-ld-test-suite)
  that this repository vendors and runs in every CI cell. Test-side fixes
  the fork carries are listed in `docs/upstream/etsi-raises.md`.
- **ScorpioBroker** (NEC Laboratories Europe), whose serial suite recipe
  this repository's `dev/etsi-*.sh` scripts follow, and whose behaviour
  served as the reference implementation while the ledger was audited.

## Libraries the broker is built from

- [tokio](https://tokio.rs/), [axum](https://github.com/tokio-rs/axum) and
  [hyper](https://hyper.rs/) for the runtime and the HTTP binding.
- [sqlx](https://github.com/launchbadge/sqlx) for parameterized Postgres access.
- [redb](https://www.redb.org/) for the durable single-node file store.
- [rumqttc](https://github.com/bytebeamio/rumqtt) for MQTT notifications.
- [async-nats](https://github.com/nats-io/nats.rs) for the JetStream bus.
- [geo](https://github.com/georust/geo) and [geojson](https://github.com/georust/geojson) for geo-queries.
- [moka](https://github.com/moka-rs/moka) for the @context cache.
- [wasm-bindgen](https://github.com/rustwasm/wasm-bindgen) for the browser build.
- jemalloc via [tikv-jemallocator](https://github.com/tikv/jemallocator).

## Infrastructure the tests run on

- [PostgreSQL](https://www.postgresql.org/) with [PostGIS](https://postgis.net/)
  and [TimescaleDB](https://www.timescale.com/).
- [NATS](https://nats.io/) JetStream.
- [Eclipse Mosquitto](https://mosquitto.org/).
- [Robot Framework](https://robotframework.org/), which the ETSI suite is written in.
- [k6](https://k6.io/) for the performance runs.

## Naming

Antares, the brightest star in Scorpius, follows the NGSI-LD broker
tradition of Orion, Scorpio and Stellio.
