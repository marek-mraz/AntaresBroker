// SPDX-License-Identifier: EUPL-1.2
//! Bus wiring: roles × bus. The composition root is the ONLY
//! place that knows both the bus variant and which consumers exist.
//!
//! bus=local — single process, all roles: the store's change hook feeds the
//! in-process matcher (`antares_api::wire`), temporal recording stays synchronous
//! in the write path. Exactly v0's behaviour; the ETSI pipeline runs this.
//!
//! bus=nats — the scale-out spine:
//!   api role      produces: same-tx outbox rows, the
//!                 outbox drain publishing to `ANTARES_CHANGES` with
//!                 `Nats-Msg-Id` dedup, subscription CUD → KV,
//!                 registration CUD → `ANTARES_REGISTRY`, and the
//!                 per-instance registration mirror its federation path reads.
//!   matcher /     one shared DURABLE ("matcher"): decode → process_change →
//!   notifier      ack AFTER processing; the KV-watched subscription mirror;
//!                 the interval loop (single-winner by row-lock claim).
//!   temporal      no bus consumer — auto-recording is synchronous in the
//!                 write path; the role only carries the
//!                 plain-mode partition job, wired in main.rs.
//!
//! Concurrent drains on N api pods double-publish only within the stream's
//! duplicate window, where `Nats-Msg-Id` = outbox seq absorbs them — that is
//! the design, not an accident (at-least-once, engineered idempotent).

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

/// ANTARES_OUTBOX_DRAIN: `on` (the default) or `off`, and nothing else. A
/// typo'd `of` read as "on" under the old permissive parse, quietly defeating
/// the dedicated-drainer split and the crash drill it exists for.
pub fn outbox_drain_enabled() -> Result<bool, String> {
    match std::env::var("ANTARES_OUTBOX_DRAIN") {
        Err(std::env::VarError::NotPresent) => Ok(true),
        Err(e) => Err(format!("ANTARES_OUTBOX_DRAIN is unreadable: {e}")),
        Ok(v) => match v.as_str() {
            "on" => Ok(true),
            "off" => Ok(false),
            other => Err(format!(
                "ANTARES_OUTBOX_DRAIN must be on|off, got {other:?}"
            )),
        },
    }
}

/// KV key for one mirrored subscription: tenant verbatim (token-safe by
/// construction), id hashed (URNs carry `:` — illegal in KV keys). The VALUE
/// carries the real tenant/id, so the key only needs uniqueness. Kind-scoped:
/// a Subscription and a Context Source Registration Subscription may carry the
/// same client-chosen id in one tenant (5.5.10), and sharing a key would let
/// one overwrite the other's mirror entry.
fn kv_key(tenant: &str, kind: Kind, id: &str) -> String {
    format!(
        "{tenant}.{}{:016x}",
        if kind == Kind::CSourceSubscription {
            "c"
        } else {
            ""
        },
        antares_bus::subjects::fnv1a64(id.as_bytes())
    )
}

/// Wire everything bus=nats needs onto the state. Async: connects, hydrates
/// mirrors, creates consumers, asserts topology — all before the broker
/// starts accepting traffic, so a mis-shapen topology is a startup failure,
/// never a silent runtime drift.
pub async fn wire_nats(
    state: &mut AppState,
    url: &str,
    roles: Roles,
) -> Result<(), Box<dyn std::error::Error>> {
    let bus = Arc::new(NatsBus::connect(url).await?);
    // /q/health `bus` member: live connection state + reconnect count.
    // Installed HERE so bus=local never carries the member at all.
    {
        let b = bus.clone();
        state.bus_stats = Some(Arc::new(move || {
            serde_json::json!({
                "mode": "nats",
                "connected": b.connected(),
                "reconnects": b.reconnects(),
            })
        }));
    }
    // Multi-process mode: interval firings need the single-winner claim
    // — keyed off this flag, not off mirror presence (local mode wires a
    // mirror too).
    state.nats = true;
    // Entity writes now enqueue their events in the write transaction.
    state.store.set_outbox(true);
    // Auto-recording stays SYNCHRONOUS in the write path in every bus mode:
    // every write goes through an api-role pod that has the
    // shared store, so recording in-request gives read-your-writes — the
    // ETSI suite asserts history immediately after a write — and kills the
    // late-replay resurrection race (a consumer re-applying a pre-delete
    // event AFTER a direct temporal delete). The recorder consumer this
    // replaced double-applied by design; it bought nothing but the races.

    // The drain nudge: a same-process write pokes its own drain, so publish
    // latency is ~1 ms, not the idle-poll interval. Cross-pod writes are
    // still covered by each pod's own nudge; the poll below stays as the
    // crash-recovery fallback.
    let nudge = Arc::new(tokio::sync::Notify::new());
    {
        let n = nudge.clone();
        // Auto-recording rides the same synchronous hook here as in bus=local
        // (one choke point for every write, no handler can forget), then nudges
        // the outbox drain.
        let st_rec = state.clone();
        state.store.set_change_hook(Arc::new(
            move |tenant: &TenantId,
                  before: Option<serde_json::Value>,
                  after: Option<serde_json::Value>|
                  -> antares_store::HookFuture<'_> {
                let st_rec = st_rec.clone();
                let n = n.clone();
                Box::pin(async move {
                    antares_api::notify::record_temporal_change(
                        &st_rec,
                        tenant,
                        before.as_ref(),
                        after.as_ref(),
                    )
                    .await;
                    n.notify_one();
                })
            },
        ));
    }

    let mut durables: Vec<&'static str> = Vec::new();

    if roles.api {
        // Subscription write side: subscription CUD → KV (tombstone = null doc).
        let kv = bus.subs_kv().await?;
        let kv_for_hook = kv.clone();
        state.sub_sync = Some(Arc::new(
            move |tenant: &TenantId, kind: Kind, id: &str, doc| {
                let kv = kv_for_hook.clone();
                let key = kv_key(tenant.as_str(), kind, id);
                let value = serde_json::json!({
                    "tenant": tenant.as_str(), "id": id, "doc": doc,
                    "csub": kind == Kind::CSourceSubscription,
                });
                tokio::spawn(async move {
                    let bytes = serde_json::to_vec(&value).unwrap_or_default();
                    // The store row is already committed. A lost put leaves this
                    // subscription invisible to every matcher pod — silently, and
                    // until the next restart, because mirrors hydrate from the
                    // store only at process start. So retry rather than warn once.
                    // Named ceiling: after the last attempt the divergence stands
                    // until a restart; closing that needs periodic reconciliation.
                    for attempt in 0..MIRROR_SYNC_ATTEMPTS {
                        match kv.put(key.clone(), bytes.clone().into()).await {
                            Ok(_) => return,
                            Err(e) => {
                                tracing::warn!("sub KV sync attempt {} failed: {e}", attempt + 1);
                                tokio::time::sleep(mirror_sync_backoff(attempt)).await;
                            }
                        }
                    }
                    tracing::error!(
                        "sub KV sync gave up for {key} — this subscription is not mirrored \
                     until the next restart"
                    );
                });
            },
        ));

        // Registration write side: registration CUD → ANTARES_REGISTRY delta.
        let bus_for_reg = bus.clone();
        state.reg_sync = Some(Arc::new(move |tenant: &TenantId, id: &str, doc| {
            let bus = bus_for_reg.clone();
            let delta = serde_json::json!({
                "tenant": tenant.as_str(), "id": id, "doc": doc,
            });
            let tenant = tenant.as_str().to_owned();
            let id = id.to_owned();
            tokio::spawn(async move {
                // Same contract as the subscription mirror: the row is
                // committed, so a lost delta makes the registration invisible
                // to every federation path until a restart re-hydrates.
                for attempt in 0..MIRROR_SYNC_ATTEMPTS {
                    match bus.publish_registry(&tenant, &delta).await {
                        Ok(()) => return,
                        Err(e) => {
                            tracing::warn!(
                                "registry delta publish attempt {} failed: {e}",
                                attempt + 1
                            );
                            tokio::time::sleep(mirror_sync_backoff(attempt)).await;
                        }
                    }
                }
                tracing::error!(
                    "registry delta publish gave up for {id} — this registration is not \
                     mirrored until the next restart"
                );
            });
        }));

        // 5.2.34 write side: a cooldown stamp is broadcast to the other api
        // pods on the registry stream (seconds-scale state, deliberately not
        // persisted) — per-process stamps re-dial a failed source from every
        // pod behind the LB.
        let bus_for_cool = bus.clone();
        state.reg_fail_sync = Some(Arc::new(move |reg_id: &str, ok: bool| {
            let bus = bus_for_cool.clone();
            let delta = serde_json::json!({"cooldownReg": reg_id, "ok": ok});
            tokio::spawn(async move {
                if let Err(e) = bus.publish_registry("cooldown", &delta).await {
                    tracing::warn!("cooldown stamp publish failed: {e}");
                }
            });
        }));

        // Registration read side: the ONE compiled registration mirror this instance's
        // federation path reads. Consumer created BEFORE the hydrate so no
        // delta can fall between them; last-writer-wins per key converges.
        let reg_mirror = Arc::new(antares_api::mirror::DocMirror::default());
        let reg_consumer = bus.consume_registry_broadcast().await?;
        // Installed only if it is whole. A mirror that is present and SHORT
        // is read as the truth — `reg_docs` asks it and never the store — so
        // half a hydrate silently drops Context Sources for the life of the
        // process. Left uninstalled, federation matching falls back to the
        // store's own indexed narrowing: correct, and merely slower.
        match antares_api::notify::seed_mirror(
            &*state.store,
            reg_mirror.as_ref(),
            Kind::Registration,
        )
        .await
        {
            Ok(()) => state.reg_mirror = Some(reg_mirror.clone()),
            Err(e) => tracing::error!(
                "registration mirror hydrate failed ({e}); \
                 federation matching falls back to a store read per request"
            ),
        }
        let egress_for_cool = state.egress.clone();
        let store_for_reg = state.store.clone();
        tokio::spawn(async move {
            // The consumer is ephemeral, so a NATS restart or an inactivity
            // gap deletes it server-side and the next pull errors. Ending the
            // task there froze this pod's registration mirror — and with it
            // all federation matching — for the process lifetime, while
            // /q/health still reported the bus connected. Re-open instead,
            // and re-hydrate from the store because a fresh consumer starts
            // at NEW and never replays what the gap dropped.
            loop {
                let mut msgs = match reg_consumer.messages().await {
                    Ok(m) => m,
                    Err(e) => {
                        tracing::warn!("registry broadcast consumer stream failed: {e}");
                        tokio::time::sleep(std::time::Duration::from_secs(1)).await;
                        continue;
                    }
                };
                while let Some(delta) = nats::next_delta(&mut msgs).await {
                    // 5.2.34 read side: a cooldown stamp updates this pod's
                    // map (a marker delta has no tenant/id — apply_delta
                    // ignores it on pods that predate the member).
                    if let Some(rid) = delta.get("cooldownReg").and_then(serde_json::Value::as_str)
                    {
                        egress_for_cool.reg_record(rid, delta["ok"].as_bool().unwrap_or(false));
                        continue;
                    }
                    apply_delta(reg_mirror.as_ref(), &delta);
                }
                tracing::warn!("registry broadcast consumer stream ended — reopening");
                tokio::time::sleep(std::time::Duration::from_secs(1)).await;
                // Already installed, so this one cannot be withheld. A failed
                // re-hydrate leaves the mirror holding what it had before the
                // gap, which the federation path will serve as current: say
                // so at error level rather than let it pass as a warning.
                if let Err(e) = antares_api::notify::seed_mirror(
                    &*store_for_reg,
                    reg_mirror.as_ref(),
                    Kind::Registration,
                )
                .await
                {
                    tracing::error!(
                        "registration mirror re-hydrate failed ({e}); \
                         this pod is matching against registrations from before the gap"
                    );
                }
            }
        });

        // The outbox drain. Runs on every api pod; concurrent drains are
        // absorbed by Nats-Msg-Id dedup within the duplicate window.
        // ANTARES_OUTBOX_DRAIN=off leaves the rows for another pod's drain —
        // the crash-drill lever and the dedicated-drainer split.
        let drain_on = outbox_drain_enabled()?;
        if !drain_on {
            tracing::warn!("outbox drain OFF on this pod (ANTARES_OUTBOX_DRAIN=off)");
        }
        if drain_on {
            let store = state.store.clone();
            let bus_for_drain = bus.clone();
            tokio::spawn(async move {
                loop {
                    let rows = match store.outbox_peek(64).await {
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
                    // Ack the EXACT published seqs — a blanket
                    // up-to-max delete loses a lower-seq row whose
                    // transaction commits between peek and ack.
                    let mut acked: Vec<i64> = Vec::new();
                    // Rows whose bodies were too big for the bus: the message
                    // carries a reference, and this row is the only copy of
                    // what it references. Kept, not deleted, and taken out of
                    // the next page by the same stamp.
                    let mut retained: Vec<(TenantId, i64)> = Vec::new();
                    for (seq, _tenant, event) in rows {
                        match serde_json::from_value::<ChangeEvent>(event) {
                            Ok(mut ev) => {
                                ev.seq = seq;
                                let checked = ev.claim_checked_at(antares_bus::CLAIM_CHECK_BYTES);
                                match bus_for_drain.publish(&ev).await {
                                    Ok(()) if checked => retained.push((ev.tenant, seq)),
                                    Ok(()) => acked.push(seq),
                                    Err(e) => {
                                        tracing::warn!("outbox publish of seq {seq} failed: {e}");
                                        break; // retry from here next round
                                    }
                                }
                            }
                            Err(e) => {
                                // an undecodable row would wedge the drain forever
                                tracing::error!("outbox row {seq} undecodable ({e}) — skipped");
                                acked.push(seq);
                            }
                        }
                    }
                    // Retain BEFORE ack: both statements are separate
                    // transactions, and a crash between them must leave a
                    // claim-check row alive rather than published and gone.
                    // One statement per row, under that row's tenant — the
                    // outbox UPDATE takes no service escape (0005), and an
                    // event over the bus ceiling is rare enough that grouping
                    // the page by tenant would cost more code than statements.
                    for (tenant, seq) in &retained {
                        if let Err(e) = store.outbox_retain(tenant, &[*seq]).await {
                            tracing::warn!("outbox retain of seq {seq} failed: {e}");
                        }
                    }
                    if !acked.is_empty() {
                        if let Err(e) = store.outbox_ack(&acked).await {
                            tracing::warn!("outbox ack {acked:?} failed: {e}");
                        }
                    }
                }
            });
        }
    }

    if roles.matcher || roles.notifier {
        // Subscription read side: consumer-before-hydrate, same convergence argument.
        let sub_mirror = Arc::new(antares_api::mirror::SubMirror::default());
        let kv = bus.subs_kv().await?;
        let watch = kv.watch_all().await?;
        // Same rule as the registration mirror, and the same one `bus=local`
        // applies in `antares_api::wire`: a subscription absent from an installed
        // mirror never fires again, because the matcher reads candidates
        // from the mirror alone.
        match antares_api::notify::seed_mirror(
            &*state.store,
            sub_mirror.as_ref(),
            Kind::Subscription,
        )
        .await
        {
            Ok(()) => state.sub_mirror = Some(sub_mirror.clone()),
            Err(e) => tracing::error!(
                "subscription mirror hydrate failed ({e}); \
                 matching falls back to a store scan per change"
            ),
        }
        tokio::spawn(async move {
            // Same restart contract as the registry consumer: the watch ends
            // on a NATS restart, and a task that returns there stops seeing
            // every subscription change — i.e. this pod silently stops
            // notifying — for the process lifetime. `watch_all` replays the
            // bucket's current values, so re-opening also re-converges the
            // mirror over the gap.
            let mut watch = watch;
            loop {
                while let Some(entry) = watch.next().await {
                    let Ok(entry) = entry else { continue };
                    if let Ok(v) = serde_json::from_slice::<serde_json::Value>(&entry.value) {
                        apply_delta(sub_mirror.as_ref(), &v);
                    }
                }
                tracing::warn!("subscription KV watch ended — reopening");
                tokio::time::sleep(std::time::Duration::from_secs(1)).await;
                match kv.watch_all().await {
                    Ok(w) => watch = w,
                    Err(e) => tracing::warn!("subscription KV watch reopen failed: {e}"),
                }
            }
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
                    // Change lag = stream-publish → matcher-processing
                    // age, from the JetStream metadata timestamp.
                    if let Ok(info) = msg.info() {
                        // OffsetDateTime → SystemTime (impl in `time`/std),
                        // so no direct `time` dependency here.
                        let published: std::time::SystemTime = info.published.into();
                        if let Ok(age) = published.elapsed() {
                            metrics::histogram!("antares_change_lag_seconds")
                                .record(age.as_secs_f64());
                        }
                    }
                    if let Some(ev) = nats::decode(&msg) {
                        let (before, after) = resolve_payloads(&st, &ev).await;
                        antares_api::notify::process_change(&st, ev.tenant.as_str(), before, after)
                            .await;
                    }
                    let _ = msg.ack().await;
                }
            }
        });

        // Interval subscriptions: every matcher pod ticks; the row-lock claim
        // in interval_tick makes each firing single-winner.
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

    // The server must agree these are shared durables.
    bus.assert_topology(&durables).await?;
    tracing::info!(?roles, "bus=nats wired");
    Ok(())
}

/// Hydrate a mirror from the system of record (Postgres) at startup.
/// Attempts a mirror write gets before the divergence is logged as an error.
const MIRROR_SYNC_ATTEMPTS: u32 = 5;

/// Exponential backoff between mirror-sync attempts: 0.2 s doubling to 3.2 s,
/// so the whole ladder outlives a bus reconnect without holding a task for
/// minutes.
fn mirror_sync_backoff(attempt: u32) -> std::time::Duration {
    std::time::Duration::from_millis(200u64 << attempt.min(4))
}

/// Apply one `{tenant, id, doc|null, csub}` delta to a mirror. A Context
/// Source Registration Subscription carries no document into the mirror: it
/// is matched against registrations rather than entities, so the delta only
/// wakes the interval sweep (5.11.7).
fn apply_delta(mirror: &dyn antares_api::mirror::Mirror, delta: &serde_json::Value) {
    if delta.get("csub").and_then(serde_json::Value::as_bool) == Some(true) {
        mirror.csub_written();
        return;
    }
    let (Some(tenant), Some(id)) = (
        delta.get("tenant").and_then(serde_json::Value::as_str),
        delta.get("id").and_then(serde_json::Value::as_str),
    ) else {
        return;
    };
    // Hydration validates the tenant before it touches the mirror; the delta
    // path must agree. A name outside the grammar can only add entries that no
    // lookup will ever hit, so an unvalidated one is unbounded growth keyed by
    // whatever reached the bus. The grammar is the whole check here: what
    // arrives is a tenant a peer broker wrote, and a write inside a Snapshot
    // (5.5.15) carries the synthetic tenant a client may not name — refusing
    // that one would drop the mirror delta the snapshot-scoped subscription
    // matches on.
    if TenantId::new_internal(tenant).is_err() {
        return;
    }
    let doc = delta.get("doc").filter(|d| !d.is_null()).cloned();
    mirror.apply(tenant, id, doc);
}

/// Resolve claim-check references: bodies the bus could not carry come back
/// from the outbox row the drain kept, read by the event's own `seq`.
///
/// The store's current row is NOT that source. It answers with the entity as
/// it stands now, which is the after-image: resolving `prev_payload_ref` from
/// it hands the matcher two copies of the same document, `diff` finds nothing
/// changed and the change reaches no subscriber. It stays the fallback for
/// `payload_ref` alone, where being newer than the referenced version is the
/// ordinary at-least-once reality the matcher already tolerates.
async fn resolve_payloads(
    st: &AppState,
    ev: &ChangeEvent,
) -> (Option<serde_json::Value>, Option<serde_json::Value>) {
    if ev.payload_ref.is_none() && ev.prev_payload_ref.is_none() {
        return (ev.prev_payload.clone(), ev.payload.clone());
    }
    // 0 is the local bus, which never claim-checks: it hands the payloads to
    // the matcher in process.
    let kept = match ev.seq {
        0 => None,
        seq => st
            .store
            .outbox_event(seq, &ev.tenant)
            .await
            .ok()
            .flatten()
            .and_then(|v| serde_json::from_value::<ChangeEvent>(v).ok()),
    };
    if let Some(kept) = kept {
        return (kept.prev_payload, kept.payload);
    }
    // Past the retention window, or a deployment whose store keeps no outbox.
    // The after-image is still recoverable; the before-image is not, and a
    // guess in its place is a notification that reports a change that did not
    // happen.
    let after = match ev.payload.clone() {
        Some(v) => Some(v),
        None => match ev.payload_ref.as_ref() {
            Some(r) => st
                .store
                .get(&ev.tenant, Kind::Entity, r.entity_id.as_str())
                .await
                .ok()
                .flatten(),
            None => None,
        },
    };
    if ev.prev_payload_ref.is_some() {
        metrics::counter!("antares_claim_check_unresolved_total").increment(1);
        tracing::warn!(
            "claim-check row for seq {} is gone: the change to {} notifies nobody",
            ev.seq,
            ev.entity_id.as_str()
        );
    }
    (ev.prev_payload.clone(), after)
}

#[cfg(test)]
mod tests {
    use super::*;
    use antares_api::mirror::DocMirror;
    use antares_bus::{ChangeOp, PayloadRef};
    use antares_model::EntityId;
    use serde_json::json;

    #[test]
    fn roles_parse_accepts_the_role_set_and_refuses_anything_else() {
        assert!(Roles::parse("all").expect("all").all());
        let r = Roles::parse("api").expect("api");
        assert!(r.api && !r.matcher && !r.notifier && !r.temporal && !r.registry);
        assert!(
            !r.all(),
            "a single role must never claim to be the full set"
        );
        let r = Roles::parse(" matcher , notifier ").expect("padded list");
        assert!(r.matcher && r.notifier && !r.api);
        // The enumerated full set is the same thing as "all" — a role split
        // that happens to name every role must still pass the bus=local gate.
        assert!(Roles::parse("api,matcher,notifier,temporal,registry")
            .expect("full list")
            .all());

        for bad in ["", "api,", "ALL", "apis", "api;matcher", "worker", " "] {
            let err = Roles::parse(bad).expect_err(&format!("ANTARES_ROLES={bad:?} must be fatal"));
            assert!(err.starts_with("unknown role"), "{bad:?}: {err}");
        }
    }

    /// The KV key must be legal for a NATS KV bucket (`:` from a URN is not),
    /// stable across calls, and collision-free per id, per tenant and per
    /// kind — 5.5.10 leaves the id to the client, so one URN can name both a
    /// Subscription and a Context Source Registration Subscription.
    #[test]
    fn kv_key_is_bucket_legal_and_stable() {
        let sub = Kind::Subscription;
        let k = kv_key("default", sub, "urn:ngsi-ld:Subscription:1");
        assert_eq!(
            k,
            kv_key("default", sub, "urn:ngsi-ld:Subscription:1"),
            "stable"
        );
        assert!(
            k.bytes()
                .all(|b| b.is_ascii_alphanumeric() || b == b'.' || b == b'_' || b == b'-'),
            "illegal KV key character in {k:?}"
        );
        assert!(k.starts_with("default."), "tenant scoping lost: {k}");
        assert_ne!(
            kv_key("default", sub, "urn:ngsi-ld:Subscription:1"),
            kv_key("default", sub, "urn:ngsi-ld:Subscription:2")
        );
        assert_ne!(
            kv_key("t1", sub, "urn:ngsi-ld:Subscription:1"),
            kv_key("t2", sub, "urn:ngsi-ld:Subscription:1")
        );
        assert_ne!(
            kv_key("default", sub, "urn:ngsi-ld:Subscription:1"),
            kv_key(
                "default",
                Kind::CSourceSubscription,
                "urn:ngsi-ld:Subscription:1"
            ),
            "one id naming both kinds must not share a mirror entry"
        );
    }

    /// A Context Source Registration Subscription delta carries no document:
    /// it wakes the interval sweep and touches nothing the matcher reads.
    #[test]
    fn a_csub_delta_only_wakes_the_sweep() {
        let m = antares_api::mirror::SubMirror::default();
        apply_delta(
            &m,
            &serde_json::json!({
                "tenant": "default", "id": "urn:ngsi-ld:CSourceSubscription:1",
                "doc": {"id": "urn:ngsi-ld:CSourceSubscription:1", "timeInterval": 5},
                "csub": true,
            }),
        );
        assert!(
            m.docs("default").is_empty(),
            "a csource subscription must not enter the candidate index"
        );
    }

    /// The KV/registry mirrors are fed from the bus. A malformed or hostile
    /// delta must be dropped — never panic a consumer task, and never grow
    /// the mirror under a key no request can ever address (the tenant is
    /// validated on every request, so an unvalidated one is pure ballast).
    #[test]
    fn apply_delta_ignores_malformed_and_hostile_deltas() {
        let m = DocMirror::default();
        apply_delta(
            &m,
            &json!({"tenant": "default", "id": "urn:x:1", "doc": {"id": "urn:x:1"}}),
        );
        assert_eq!(m.docs("default").len(), 1, "a good delta must apply");

        // tombstone
        apply_delta(
            &m,
            &json!({"tenant": "default", "id": "urn:x:1", "doc": null}),
        );
        assert!(m.docs("default").is_empty(), "null doc must delete");
        assert!(m.tenants().is_empty(), "an emptied tenant must not linger");

        // Shapes that must be ignored without panicking.
        for junk in [
            json!({}),
            json!(null),
            json!(42),
            json!("scalar"),
            json!([1, 2, 3]),
            json!({"tenant": "default"}),
            json!({"id": "urn:x:1"}),
            json!({"tenant": 7, "id": "urn:x:1", "doc": {}}),
            json!({"tenant": "default", "id": null, "doc": {}}),
            json!({"cooldownReg": "urn:reg:1", "ok": false}),
        ] {
            apply_delta(&m, &junk);
        }
        assert!(m.tenants().is_empty(), "junk deltas grew the mirror");

        // Hostile tenants: not addressable by any request (the header is
        // validated to [A-Za-z0-9_-]{1,64}), so they may not take memory.
        for hostile in [
            "a".repeat(4096),
            "../../etc".into(),
            "a b".into(),
            "".into(),
        ] {
            apply_delta(
                &m,
                &json!({"tenant": hostile, "id": "urn:x:1", "doc": {"id": "urn:x:1"}}),
            );
        }
        assert!(
            m.tenants().is_empty(),
            "an unaddressable tenant grew the mirror without bound: {:?}",
            m.tenants()
        );
    }

    async fn state_with(entity: Option<serde_json::Value>) -> AppState {
        let st = AppState::new("antares".into());
        if let Some(doc) = entity {
            let id = doc["id"].as_str().expect("id").to_owned();
            st.store
                .create(&TenantId::default(), Kind::Entity, &id, doc)
                .await
                .expect("seed");
        }
        st
    }

    fn event(payload: Option<serde_json::Value>, r#ref: Option<PayloadRef>) -> ChangeEvent {
        ChangeEvent {
            tenant: TenantId::default(),
            entity_id: EntityId::new("urn:ngsi-ld:T:1").expect("id"),
            types: vec!["T".into()],
            op: ChangeOp::Update,
            changed_attrs: vec![],
            payload,
            prev_payload: None,
            version: 1,
            incarnation: String::new(),
            seq: 0,
            payload_ref: r#ref,
            prev_payload_ref: None,
        }
    }

    /// Claim-check resolution: inline wins, a reference is fetched, and a
    /// reference to a row that is gone resolves to None instead of panicking
    /// the matcher task.
    #[tokio::test]
    async fn resolve_payloads_prefers_inline_and_tolerates_a_dangling_reference() {
        let doc = json!({"id": "urn:ngsi-ld:T:1", "type": "T"});
        let st = state_with(Some(doc.clone())).await;

        let (before, after) =
            resolve_payloads(&st, &event(Some(json!({"inline": true})), None)).await;
        assert_eq!(after, Some(json!({"inline": true})), "inline payload wins");
        assert_eq!(before, None);

        let r = PayloadRef {
            entity_id: EntityId::new("urn:ngsi-ld:T:1").expect("id"),
            version: 1,
        };
        let (_, after) = resolve_payloads(&st, &event(None, Some(r.clone()))).await;
        assert_eq!(
            after.as_ref().and_then(|a| a["id"].as_str()),
            Some("urn:ngsi-ld:T:1"),
            "a claim-check reference must be fetched from the store"
        );

        // The row was deleted between publish and consumption.
        let gone = state_with(None).await;
        let (before, after) = resolve_payloads(&gone, &event(None, Some(r))).await;
        assert_eq!(after, None, "a dangling reference must resolve to None");
        assert_eq!(before, None);

        let (before, after) = resolve_payloads(&st, &event(None, None)).await;
        assert!(before.is_none() && after.is_none(), "no payload, no fetch");
    }

    /// A claim-check fetch is scoped to the EVENT's tenant: a reference must
    /// never resolve against another tenant's row of the same id.
    #[tokio::test]
    async fn resolve_payloads_never_crosses_a_tenant_boundary() {
        let st = state_with(Some(json!({"id": "urn:ngsi-ld:T:1", "type": "T"}))).await;
        let mut ev = event(
            None,
            Some(PayloadRef {
                entity_id: EntityId::new("urn:ngsi-ld:T:1").expect("id"),
                version: 1,
            }),
        );
        ev.tenant = TenantId::new("other").expect("tenant");
        let (_, after) = resolve_payloads(&st, &ev).await;
        assert_eq!(
            after, None,
            "a reference resolved another tenant's entity: {after:?}"
        );
    }

    /// A change whose bodies were both too big for the bus reaches the
    /// matcher with the before-image the write actually replaced. Resolving
    /// the reference against the store's current row instead hands back the
    /// after-image twice, `diff` finds nothing and the change notifies
    /// nobody. Skips without ANTARES_TEST_DATABASE_URL — the outbox is a
    /// Postgres table, and the memory arm has none.
    #[tokio::test(flavor = "multi_thread")]
    async fn a_retained_row_gives_the_matcher_the_before_image_the_write_replaced() {
        let Ok(url) = std::env::var("ANTARES_TEST_DATABASE_URL") else {
            eprintln!("SKIP: ANTARES_TEST_DATABASE_URL not set");
            return;
        };
        let pool = antares_sql::store::pg::connect(&url, 5)
            .await
            .expect("connect");
        let tenant = TenantId::new("claimcheck").expect("tenant");
        antares_sql::store::pg::ensure_tenant(&pool, &tenant)
            .await
            .expect("tenant row");

        let id = "urn:ngsi-ld:T:oversized";
        let wide = |v: &str| {
            json!({"id": id, "type": ["T"],
                   "https://uri.etsi.org/ngsi-ld/default-context/note":
                       [{"type": "Property", "value": format!("{v}{}", "x".repeat(300 * 1024))}]})
        };
        let before_doc = wide("before-");
        let after_doc = wide("after-");

        let mut ev = ChangeEvent {
            tenant: tenant.clone(),
            entity_id: EntityId::new(id).expect("id"),
            types: vec!["T".into()],
            op: ChangeOp::Update,
            changed_attrs: vec![],
            payload: Some(after_doc.clone()),
            prev_payload: Some(before_doc.clone()),
            version: 2,
            incarnation: String::new(),
            seq: 0,
            payload_ref: None,
            prev_payload_ref: None,
        };
        let mut tx = pool.begin().await.expect("tx");
        antares_sql::store::pg::set_tenant(&mut tx, &tenant)
            .await
            .expect("set tenant");
        let seq = antares_sql::store::pg::outbox::enqueue(
            &mut tx,
            &tenant,
            &serde_json::to_value(&ev).expect("event json"),
        )
        .await
        .expect("enqueue");
        tx.commit().await.expect("commit");
        antares_sql::store::pg::outbox::retain(&pool, &tenant, &[seq])
            .await
            .expect("retain");

        let st = AppState::with_store(
            "antares".into(),
            std::sync::Arc::new(antares_sql::store::any::AnyStore::Pg(
                antares_sql::store::any::PgBackend::new(pool.clone()),
            )),
            "postgres",
        );
        // The current row is the AFTER image — the document the old
        // resolution handed back for both halves.
        let _ = st.store.delete(&tenant, Kind::Entity, id).await;
        st.store
            .create(&tenant, Kind::Entity, id, after_doc.clone())
            .await
            .expect("seed current row");

        ev.seq = seq;
        let wire = ev.claim_check(antares_bus::CLAIM_CHECK_BYTES);
        assert!(
            wire.prev_payload_ref.is_some() && wire.payload_ref.is_some(),
            "the fixture must be over the claim-check ceiling"
        );
        let (before, after) = resolve_payloads(&st, &wire).await;
        assert_eq!(before.as_ref(), Some(&before_doc), "before-image lost");
        assert_eq!(after.as_ref(), Some(&after_doc));
        assert_ne!(
            before, after,
            "both halves resolved to the current row: the change notifies nobody"
        );

        let _ = st.store.delete(&tenant, Kind::Entity, id).await;
        let _ = antares_sql::store::pg::outbox::reap_published(&pool, 0).await;
    }

    /// The outbox-drain switch is a config value like any other: the two
    /// documented spellings decide, anything else is fatal instead of
    /// silently leaving the drain on (a typo'd `ANTARES_OUTBOX_DRAIN=of`
    /// would otherwise read as "on" and quietly defeat the crash drill).
    #[test]
    fn outbox_drain_switch_is_total() {
        std::env::remove_var("ANTARES_OUTBOX_DRAIN");
        assert!(outbox_drain_enabled().expect("default"), "default is on");
        std::env::set_var("ANTARES_OUTBOX_DRAIN", "off");
        assert!(!outbox_drain_enabled().expect("off"));
        std::env::set_var("ANTARES_OUTBOX_DRAIN", "on");
        assert!(outbox_drain_enabled().expect("on"));
        for bad in ["", "of", "false", "0", "OFF", "no"] {
            std::env::set_var("ANTARES_OUTBOX_DRAIN", bad);
            let err = outbox_drain_enabled()
                .expect_err(&format!("ANTARES_OUTBOX_DRAIN={bad:?} must be fatal"));
            assert!(err.contains("ANTARES_OUTBOX_DRAIN"), "{err}");
        }
        std::env::remove_var("ANTARES_OUTBOX_DRAIN");
    }
}
