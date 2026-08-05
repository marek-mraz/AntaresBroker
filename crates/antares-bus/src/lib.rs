//! Change-event bus (docs/deep-analysis.md §7).
//!
//! Two implementations behind one closed enum (the AnyStore pattern,
//! ADR-0005): `local` — an in-process broadcast ring, single-node mode, no
//! infrastructure beyond the store — and `nats` — the JetStream spine
//! (`ANTARES_CHANGES`, durable pull consumers, KV subscription mirror) that
//! makes multi-instance roles possible (F1). `bus = local` is the default and
//! what the ETSI pipeline runs; `nats` becomes mandatory only on scale-out.

pub mod nats;
pub mod subjects;

use antares_model::{EntityId, TenantId};
use serde::{Deserialize, Serialize};
use tokio::sync::broadcast;

/// Operation kind — mirrors Scorpio's requestType int registry as an enum (§7).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ChangeOp {
    Create,
    Update,
    Append,
    Merge,
    Replace,
    Delete,
    BatchCreate,
    BatchUpsert,
    BatchUpdate,
    BatchDelete,
    BatchMerge,
}

/// Claim-check reference (§7): events whose payload exceeds
/// [`CLAIM_CHECK_BYTES`] carry this instead of the inline body — consumers
/// fetch the document from the store. NATS caps messages at ~1 MB and Antares
/// never chunks.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PayloadRef {
    pub entity_id: EntityId,
    pub version: i64,
}

/// Inline-payload ceiling before the claim check kicks in (§7: 256 KB).
pub const CLAIM_CHECK_BYTES: usize = 256 * 1024;

/// One entity change. Self-contained: carries payload AND prev_payload so
/// consumers (matcher, temporal recorder) never re-read the DB per event.
/// `version` is the entity row version bumped under the write lock (§3.1) —
/// state-projecting consumers apply last-writer-wins on
/// `(incarnation, version)`; `incarnation` is the row's created_at, which
/// disambiguates delete/recreate (the version restarts at 1, §3.1.3).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChangeEvent {
    pub tenant: TenantId,
    pub entity_id: EntityId,
    pub types: Vec<String>,
    pub op: ChangeOp,
    pub changed_attrs: Vec<String>,
    pub payload: Option<serde_json::Value>,
    pub prev_payload: Option<serde_json::Value>,
    pub version: i64,
    /// The row's created_at — the incarnation half of the ordering key.
    #[serde(default)]
    pub incarnation: String,
    /// Outbox row id (F3) — the `Nats-Msg-Id` dedup key. 0 = local bus.
    #[serde(default)]
    pub seq: i64,
    /// Claim-check (§7): set when `payload` was stripped for size.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub payload_ref: Option<PayloadRef>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub prev_payload_ref: Option<PayloadRef>,
}

impl ChangeEvent {
    /// §7 claim-check: replace any inline body over `limit` bytes with a
    /// reference. Oversized entities are rare; the common path is untouched.
    pub fn claim_check(mut self, limit: usize) -> Self {
        let over = |v: &Option<serde_json::Value>| {
            v.as_ref()
                .is_some_and(|p| serde_json::to_vec(p).map(|b| b.len()).unwrap_or(0) > limit)
        };
        if over(&self.payload) {
            self.payload = None;
            self.payload_ref = Some(PayloadRef {
                entity_id: self.entity_id.clone(),
                version: self.version,
            });
        }
        if over(&self.prev_payload) {
            self.prev_payload = None;
            self.prev_payload_ref = Some(PayloadRef {
                entity_id: self.entity_id.clone(),
                version: self.version - 1,
            });
        }
        self
    }
}

/// In-process bus for single-node mode (`bus = local`).
/// Bounded ring; a lagging consumer drops oldest (backpressure over buffering).
#[derive(Clone)]
pub struct LocalBus {
    tx: broadcast::Sender<ChangeEvent>,
}

impl LocalBus {
    pub fn new(capacity: usize) -> Self {
        let (tx, _) = broadcast::channel(capacity);
        Self { tx }
    }

    /// Publish; returns number of current subscribers.
    pub fn publish(&self, event: ChangeEvent) -> usize {
        // No subscribers is fine (e.g. api-only role in tests).
        self.tx.send(event).unwrap_or(0)
    }

    pub fn subscribe(&self) -> broadcast::Receiver<ChangeEvent> {
        self.tx.subscribe()
    }
}

/// The closed bus seam (`bus = local | nats`, §9.2): core crates see only
/// this; the broker's wiring picks the variant and starts the consumers that
/// match it.
#[derive(Clone)]
pub enum AnyBus {
    Local(LocalBus),
    Nats(std::sync::Arc<nats::NatsBus>),
}

impl AnyBus {
    /// Publish one change event. On the NATS arm this is the DIRECT path
    /// (used by the outbox drain, which owns retry/dedup); producers in
    /// postgres mode never call this straight from a request handler —
    /// they enqueue in the write transaction (F3, §10).
    pub async fn publish(&self, event: ChangeEvent) -> Result<(), nats::BusError> {
        match self {
            AnyBus::Local(b) => {
                b.publish(event);
                Ok(())
            }
            AnyBus::Nats(b) => b.publish(&event).await,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn event(version: i64) -> ChangeEvent {
        ChangeEvent {
            tenant: TenantId::default(),
            entity_id: EntityId::new("urn:ngsi-ld:Vehicle:A1").expect("valid urn"),
            types: vec!["https://uri.etsi.org/ngsi-ld/default-context/Vehicle".into()],
            op: ChangeOp::Create,
            changed_attrs: vec![],
            payload: Some(serde_json::json!({"speed": 80})),
            prev_payload: None,
            version,
            incarnation: "2026-08-05T00:00:00Z".into(),
            seq: 0,
            payload_ref: None,
            prev_payload_ref: None,
        }
    }

    #[tokio::test]
    async fn broadcast_reaches_all_subscribers() {
        let bus = LocalBus::new(16);
        let mut a = bus.subscribe();
        let mut b = bus.subscribe();
        assert_eq!(bus.publish(event(1)), 2);
        assert_eq!(a.recv().await.expect("recv a").version, 1);
        assert_eq!(b.recv().await.expect("recv b").version, 1);
    }

    #[test]
    fn event_round_trips_through_serde() {
        let e = event(7);
        let bytes = serde_json::to_vec(&e).expect("serialize");
        let back: ChangeEvent = serde_json::from_slice(&bytes).expect("deserialize");
        assert_eq!(back.version, 7);
        assert_eq!(back.op, ChangeOp::Create);
        assert_eq!(back.incarnation, "2026-08-05T00:00:00Z");
    }

    #[test]
    fn claim_check_strips_only_oversized_bodies() {
        let mut e = event(3);
        e.prev_payload = Some(serde_json::json!({"small": true}));
        e.payload = Some(serde_json::Value::String("x".repeat(1024)));
        let checked = e.claim_check(512);
        assert!(checked.payload.is_none(), "over-limit body stripped");
        let r = checked.payload_ref.as_ref().expect("ref set");
        assert_eq!(r.version, 3);
        assert!(
            checked.prev_payload.is_some() && checked.prev_payload_ref.is_none(),
            "under-limit body inline"
        );
    }
}
