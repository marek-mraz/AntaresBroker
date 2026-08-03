# ADR-0003: WebSocket binding deferred out of v1

Date: 2026-08-03 · Status: accepted

The ngsi-ld-ws binding (websocket.md draft, WS-01..47) is NOT implemented in
v1. Structural: the whole binding is scoped to a future `antares-ws` crate
behind cargo feature `ws`, plugging into the NotificationSink scheme registry
and a Router::merge — core crates never change (deep-analysis §9.2/§11).
Standards-aligned: a WS Notification Binding is an official NGSI-LD 2.1 work
item (ETSI TC DATA Issue #8); implementing ahead of it risks divergence.
