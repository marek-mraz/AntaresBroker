//! F-phase wiring (§9.3): roles × bus. The composition root is the ONLY
//! place that knows both the bus variant and which consumers exist.
//!
//! bus=local — single process, all roles: the store's change hook feeds the
//! in-process matcher (`notify::wire`), temporal recording stays synchronous
//! in the write path. Exactly v0's behaviour; the ETSI pipeline runs this.
//!
//! bus=nats — the scale-out spine (F1..F8):
//!   api role      produces: same-tx outbox rows (F3 producer switch), the
//!                 outbox drain publishing to `ANTARES_CHANGES` with
//!                 `Nats-Msg-Id` dedup, subscription CUD → KV (F4),
//!                 registration CUD → `ANTARES_REGISTRY` (F5), and the
//!                 per-instance registration mirror its federation path reads.
//!   matcher /     one shared DURABLE ("matcher"): decode → process_change →
//!   notifier      ack AFTER processing; the KV-watched subscription mirror;
//!                 the interval loop (single-winner by row-lock claim).
//!   temporal      no bus consumer — auto-recording is synchronous in the
//!                 write path (K8 lesson); the role only carries the
//!                 plain-mode partition job, wired in main.rs.
//!
//! Concurrent drains on N api pods double-publish only within the stream's
//! duplicate window, where `Nats-Msg-Id` = outbox seq absorbs them — that is
//! the design, not an accident (§6.4 at-least-once, engineered idempotent).

use antares_api::AppState;
use antares_bus::nats::{self, NatsBus};
use antares_bus::ChangeEvent;
use antares_model::TenantId;
use antares_sql::store::Kind;
use futures_util::StreamExt;
use std::sync::Arc;

#[derive(Clone, Copy, Debug)]
pub struct Roles {
    pub api: bool,
    pub matcher: bool,
    pub notifier: bool,
    pub temporal: bool,
    pub registry: bool,
}

impl Roles {
    pub fn parse(spec: &str) -> Result<Self, String> {
        if spec == "all" {
            return Ok(Self {
                api: true,
                matcher: true,
                notifier: true,
                temporal: true,
                registry: true,
            });
        }
        let mut r = Self {
            api: false,
            matcher: false,
            notifier: false,
            temporal: false,
            registry: false,
        };
        for part in spec.split(',') {
            match part.trim() {
                "api" => r.api = true,
                "matcher" => r.matcher = true,
                "notifier" => r.notifier = true,
                "temporal" => r.temporal = true,
                "registry" => r.registry = true,
                other => {
                    return Err(format!(
                        "unknown role {other:?} (api|matcher|notifier|temporal|registry|all)"
                    ))
                }
            }
        }
        Ok(r)
    }

    pub fn all(&self) -> bool {
        self.api && self.matcher && self.notifier && self.temporal && self.registry
    }
}

/// KV key for one subscription: tenant verbatim (token-safe by construction),
/// id hashed (URNs carry `:` — illegal in KV keys). The VALUE carries the
/// real tenant/id, so the key only needs uniqueness.
fn kv_key(tenant: &str, id: &str) -> String {
    format!(
        "{tenant}.{:016x}",
        antares_bus::subjects::fnv1a64(id.as_bytes())
    )
}

/// Wire everything bus=nats needs onto the state. Async: connects, hydrates
/// mirrors, creates consumers, asserts topology (F7) — all before the broker
/// starts accepting traffic, so a mis-shapen topology is a startup failure,
/// never a silent runtime drift.
pub async fn wire_nats(
    state: &mut AppState,
    url: &str,
    roles: Roles,
) -> Result<(), Box<dyn std::error::Error>> {
    let bus = Arc::new(NatsBus::connect(url).await?);
    // F3: entity writes now enqueue their events in the write transaction.
    state.store.set_outbox(true);
    // Auto-recording stays SYNCHRONOUS in the write path in every bus mode
    // (K8 lesson): every write goes through an api-role pod that has the
    // shared store, so recording in-request gives read-your-writes — the
    // ETSI suite asserts history immediately after a write — and kills the
    // late-replay resurrection race (a consumer re-applying a pre-delete
    // event AFTER a direct temporal delete). The F8 recorder consumer this
    // replaced double-applied by design; it bought nothing but the races.

    // The drain nudge: a same-process write pokes its own drain, so publish
    // latency is ~1 ms, not the idle-poll interval. Cross-pod writes are
    // still covered by each pod's own nudge; the poll below stays as the
    // crash-recovery fallback.
    let nudge = Arc::new(tokio::sync::Notify::new());
    {
        let n = nudge.clone();
        state
            .store
            .set_change_hook(Box::new(move |_, _, _| n.notify_one()));
    }

    let mut durables: Vec<&'static str> = Vec::new();

    if roles.api {
        // F4 write side: subscription CUD → KV (tombstone = null doc).
        let kv = bus.subs_kv().await?;
        let kv_for_hook = kv.clone();
        state.sub_sync = Some(Arc::new(move |tenant: &TenantId, id: &str, doc| {
            let kv = kv_for_hook.clone();
            let key = kv_key(tenant.as_str(), id);
            let value = serde_json::json!({
                "tenant": tenant.as_str(), "id": id, "doc": doc,
            });
            tokio::spawn(async move {
                let bytes = serde_json::to_vec(&value).unwrap_or_default();
                if let Err(e) = kv.put(key, bytes.into()).await {
                    tracing::warn!("sub KV sync failed: {e}");
                }
            });
        }));

        // F5 write side: registration CUD → ANTARES_REGISTRY delta.
        let bus_for_reg = bus.clone();
        state.reg_sync = Some(Arc::new(move |tenant: &TenantId, id: &str, doc| {
            let bus = bus_for_reg.clone();
            let delta = serde_json::json!({
                "tenant": tenant.as_str(), "id": id, "doc": doc,
            });
            let tenant = tenant.as_str().to_owned();
            tokio::spawn(async move {
                if let Err(e) = bus.publish_registry(&tenant, &delta).await {
                    tracing::warn!("registry delta publish failed: {e}");
                }
            });
        }));

        // F5 read side: the ONE compiled registration mirror this instance's
        // federation path reads. Consumer created BEFORE the hydrate so no
        // delta can fall between them; last-writer-wins per key converges.
        let reg_mirror = Arc::new(antares_api::notify::DocMirror::default());
        let reg_consumer = bus.consume_registry_broadcast().await?;
        hydrate(&reg_mirror, &state.store, Kind::Registration);
        state.reg_mirror = Some(reg_mirror.clone());
        tokio::spawn(async move {
            let mut msgs = match reg_consumer.messages().await {
                Ok(m) => m,
                Err(e) => {
                    tracing::error!("registry broadcast consumer died at start: {e}");
                    return;
                }
            };
            while let Some(delta) = nats::next_delta(&mut msgs).await {
                apply_delta(&reg_mirror, &delta);
            }
            tracing::warn!("registry broadcast consumer stream ended");
        });

        // F3: the outbox drain. Runs on every api pod; concurrent drains are
        // absorbed by Nats-Msg-Id dedup within the duplicate window.
        // ANTARES_OUTBOX_DRAIN=off leaves the rows for another pod's drain —
        // the K9 crash-drill lever and the dedicated-drainer split.
        let drain_on = std::env::var("ANTARES_OUTBOX_DRAIN")
            .map(|v| v != "off")
            .unwrap_or(true);
        if !drain_on {
            tracing::warn!("outbox drain OFF on this pod (ANTARES_OUTBOX_DRAIN=off)");
        }
        if drain_on {
            let store = state.store.clone();
            let bus_for_drain = bus.clone();
            tokio::spawn(async move {
                loop {
                    let rows = match store.outbox_peek(64) {
                        Ok(r) => r,
                        Err(e) => {
                            tracing::warn!("outbox peek failed: {e}");
                            tokio::time::sleep(std::time::Duration::from_secs(1)).await;
                            continue;
                        }
                    };
                    if rows.is_empty() {
                        // woken by the same-process write hook, or the fallback
                        // poll for rows another pod failed to publish
                        let _ = tokio::time::timeout(
                            std::time::Duration::from_millis(250),
                            nudge.notified(),
                        )
                        .await;
                        continue;
                    }
                    let mut acked = 0i64;
                    for (seq, _tenant, event) in rows {
                        match serde_json::from_value::<ChangeEvent>(event) {
                            Ok(mut ev) => {
                                ev.seq = seq;
                                match bus_for_drain.publish(&ev).await {
                                    Ok(()) => acked = seq,
                                    Err(e) => {
                                        tracing::warn!("outbox publish of seq {seq} failed: {e}");
                                        break; // retry from here next round
                                    }
                                }
                            }
                            Err(e) => {
                                // an undecodable row would wedge the drain forever
                                tracing::error!("outbox row {seq} undecodable ({e}) — skipped");
                                acked = seq;
                            }
                        }
                    }
                    if acked > 0 {
                        if let Err(e) = store.outbox_ack(acked) {
                            tracing::warn!("outbox ack {acked} failed: {e}");
                        }
                    }
                }
            });
        }
    }

    if roles.matcher || roles.notifier {
        // F4 read side: consumer-before-hydrate, same convergence argument.
        let sub_mirror = Arc::new(antares_api::notify::SubMirror::default());
        let kv = bus.subs_kv().await?;
        let watch = kv.watch_all().await?;
        hydrate(&sub_mirror, &state.store, Kind::Subscription);
        state.sub_mirror = Some(sub_mirror.clone());
        tokio::spawn(async move {
            let mut watch = watch;
            while let Some(entry) = watch.next().await {
                let Ok(entry) = entry else { continue };
                if let Ok(v) = serde_json::from_slice::<serde_json::Value>(&entry.value) {
                    apply_delta(&sub_mirror, &v);
                }
            }
            tracing::warn!("subscription KV watch ended");
        });

        // The balanced matcher durable: decode → process_change → ack AFTER.
        durables.push("matcher");
        let consumer = bus.consume_balanced("matcher").await?;
        let st = state.clone();
        tokio::spawn(async move {
            loop {
                let mut msgs = match consumer.messages().await {
                    Ok(m) => m,
                    Err(e) => {
                        tracing::warn!("matcher consumer stream failed: {e}");
                        tokio::time::sleep(std::time::Duration::from_secs(1)).await;
                        continue;
                    }
                };
                while let Some(Ok(msg)) = msgs.next().await {
                    if let Some(ev) = nats::decode(&msg) {
                        let (before, after) = resolve_payloads(&st, &ev);
                        antares_api::notify::process_change(&st, ev.tenant.as_str(), before, after)
                            .await;
                    }
                    let _ = msg.ack().await;
                }
            }
        });

        // Interval subscriptions: every matcher pod ticks; the row-lock claim
        // in interval_tick makes each firing single-winner (§3.1.6).
        let st = state.clone();
        tokio::spawn(async move {
            let mut tick = tokio::time::interval(std::time::Duration::from_millis(500));
            loop {
                tick.tick().await;
                antares_api::notify::interval_tick(&st).await;
            }
        });
    }

    // The temporal role carries no bus consumer: auto-recording is
    // synchronous in the write path (see above), and plain-mode partition
    // maintenance runs from main.rs regardless of bus mode.

    // F7: the server must agree these are shared durables (R10 lesson).
    bus.assert_topology(&durables).await?;
    tracing::info!(?roles, "bus=nats wired");
    Ok(())
}

/// Hydrate a mirror from the system of record (Postgres) at startup.
fn hydrate(
    mirror: &antares_api::notify::DocMirror,
    store: &antares_sql::store::any::AnyStore,
    kind: Kind,
) {
    // subscription_tenants lists ALL tenants on the Pg arm — serves both kinds
    let tenants = store.subscription_tenants().unwrap_or_default();
    for tenant_str in tenants {
        let Ok(tenant) = TenantId::new(&tenant_str) else {
            continue;
        };
        for doc in store.list(&tenant, kind).unwrap_or_default() {
            if let Some(id) = doc.get("id").and_then(serde_json::Value::as_str) {
                mirror.apply(&tenant_str, id, Some(doc.clone()));
            }
        }
    }
}

/// Apply one `{tenant, id, doc|null}` delta to a mirror.
fn apply_delta(mirror: &antares_api::notify::DocMirror, delta: &serde_json::Value) {
    let (Some(tenant), Some(id)) = (
        delta.get("tenant").and_then(serde_json::Value::as_str),
        delta.get("id").and_then(serde_json::Value::as_str),
    ) else {
        return;
    };
    let doc = delta.get("doc").filter(|d| !d.is_null()).cloned();
    mirror.apply(tenant, id, doc);
}

/// Resolve claim-check references (§7): consumers fetch oversized bodies
/// from the store. The current row may be newer than the referenced version —
/// ordinary at-least-once reality; the matcher is ordering-tolerant (§3.1).
fn resolve_payloads(
    st: &AppState,
    ev: &ChangeEvent,
) -> (Option<serde_json::Value>, Option<serde_json::Value>) {
    let fetch = |r: &antares_bus::PayloadRef| {
        st.store
            .get(&ev.tenant, Kind::Entity, r.entity_id.as_str())
            .ok()
            .flatten()
    };
    let before = ev
        .prev_payload
        .clone()
        .or_else(|| ev.prev_payload_ref.as_ref().and_then(&fetch));
    let after = ev
        .payload
        .clone()
        .or_else(|| ev.payload_ref.as_ref().and_then(&fetch));
    (before, after)
}
