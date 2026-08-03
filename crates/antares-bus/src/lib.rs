//! Change-event bus (docs/deep-analysis.md §7).
//!
//! v0 ships the `local` implementation (single-node mode: no infrastructure
//! beyond Postgres). The NATS JetStream implementation (`ANTARES_CHANGES`
//! stream, durable pull consumers, KV mirror) lands in phase 2.

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

/// One entity change. Self-contained: carries payload AND prev_payload so
/// consumers (matcher, temporal recorder) never re-read the DB per event.
/// `version` is the entity row version bumped under the write lock (§3.1) —
/// state-projecting consumers apply last-writer-wins on (entity, version).
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
    }
}
