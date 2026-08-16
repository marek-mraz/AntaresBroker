# Antares

An NGSI-LD Context Broker (ETSI GS CIM 009 V1.9.1) in Rust: one native
binary, a store ladder from in-memory to TimescaleDB, NATS JetStream
scale-out, HTTP + MQTT notifications, federation — and a WebAssembly build
that runs the same broker inside a web page.

This book is the user documentation. For the engineering internals see the
repository: [architecture deep-analysis](https://github.com/marek-mraz/AntaresBroker/blob/master/docs/deep-analysis.md),
[ADRs](https://github.com/marek-mraz/AntaresBroker/tree/master/docs/adr),
[per-clause conformance ledger](https://github.com/marek-mraz/AntaresBroker/tree/master/docs/spec).

- Live conformance report: <https://antares-ngsi-ld-demo.marek-mraz.com/reports/latest/>
- Browser playground: <https://antares-ngsi-ld-demo.marek-mraz.com/>
- Source: <https://github.com/marek-mraz/AntaresBroker>
