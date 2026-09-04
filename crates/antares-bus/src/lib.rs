// SPDX-License-Identifier: EUPL-1.2
//! Change-event bus.
//!
//! The event and its transport. `bus = local`, the default and what the
//! ETSI pipeline runs, carries changes in-process through the store's change
//! hook and needs nothing from here but `ChangeEvent`. `bus = nats` adds the
//! JetStream spine (`ANTARES_CHANGES`, durable pull consumers, KV
//! subscription mirror) that makes multi-instance roles possible, and
//! becomes mandatory only on scale-out. The composition root
//! (`antares-broker/src/wiring.rs`) is the only place that names either.
#![cfg_attr(not(test), warn(clippy::expect_used))]

pub mod nats;
pub mod subjects;

use antares_model::{EntityId, TenantId};
use serde::{Deserialize, Serialize};

/// Operation kind — mirrors Scorpio's requestType int registry as an enum.
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

/// Claim-check reference: events whose payload exceeds
/// [`CLAIM_CHECK_BYTES`] carry this instead of the inline body. NATS caps
/// messages at ~1 MB and Antares never chunks, so the body travels out of
/// band: the publisher keeps the outbox row that holds the whole event, and
/// the consumer reads it back by the event's `seq`. The store's current row
/// is the after-image and can stand in for `payload` alone — a before-image
/// is not derivable from it, which is why the reference is not a document
/// lookup.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PayloadRef {
    pub entity_id: EntityId,
    pub version: i64,
}

/// Inline-payload ceiling before the claim check kicks in (256 KB).
pub const CLAIM_CHECK_BYTES: usize = 256 * 1024;

/// One entity change. Self-contained: carries payload AND prev_payload so
/// consumers (matcher, temporal recorder) never re-read the DB per event.
/// `version` is the entity row version bumped under the write lock —
/// state-projecting consumers apply last-writer-wins on
/// `(incarnation, version)`; `incarnation` is the row's created_at, which
/// disambiguates delete/recreate (the version restarts at 1).
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
    /// Outbox row id — the `Nats-Msg-Id` dedup key. 0 = local bus.
    #[serde(default)]
    pub seq: i64,
    /// Claim-check: set when `payload` was stripped for size.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub payload_ref: Option<PayloadRef>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub prev_payload_ref: Option<PayloadRef>,
}

/// A body's serialized size against the claim-check ceiling. An
/// unserializable body counts as zero: it cannot be published either way, and
/// treating it as oversized would retain a row nothing can resolve.
fn over(v: &Option<serde_json::Value>, limit: usize) -> bool {
    v.as_ref()
        .is_some_and(|p| serde_json::to_vec(p).map(|b| b.len()).unwrap_or(0) > limit)
}

impl ChangeEvent {
    /// True when [`ChangeEvent::claim_check`] would strip a body at `limit`.
    /// The publisher asks before it publishes: a stripped body has to stay
    /// readable somewhere the consumer can reach, and the drain is the last
    /// holder of the whole event.
    pub fn claim_checked_at(&self, limit: usize) -> bool {
        over(&self.payload, limit) || over(&self.prev_payload, limit)
    }

    /// Claim-check: replace any inline body over `limit` bytes with a
    /// reference. Oversized entities are rare; the common path is untouched.
    pub fn claim_check(mut self, limit: usize) -> Self {
        if over(&self.payload, limit) {
            self.payload = None;
            self.payload_ref = Some(PayloadRef {
                entity_id: self.entity_id.clone(),
                version: self.version,
            });
        }
        if over(&self.prev_payload, limit) {
            self.prev_payload = None;
            self.prev_payload_ref = Some(PayloadRef {
                entity_id: self.entity_id.clone(),
                // saturating: a decoded event's version is whatever the wire
                // said, and wrapping would reference the wrong document
                version: self.version.saturating_sub(1),
            });
        }
        // The ENVELOPE must fit too, or the publish is refused by the bus and
        // the outbox drain retries the same row forever. changed_attrs is the
        // one member that scales with the entity's width, and no consumer
        // reads it off the wire (process_change re-derives the diff from the
        // payloads); types stay — the publish subject is built from them.
        if serde_json::to_vec(&self)
            .map(|b| b.len())
            .unwrap_or(usize::MAX)
            > limit
        {
            self.changed_attrs = Vec::new();
        }
        self
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
    fn claim_check_bounds_the_envelope_not_only_the_bodies() {
        let mut e = event(9);
        e.payload = Some(serde_json::json!({"small": true}));
        // the envelope itself outgrows the limit: thousands of attribute
        // IRIs from one wide entity, with both bodies tiny
        e.changed_attrs = (0..20_000)
            .map(|i| format!("https://example.org/ngsi-ld/attributes/generated/a{i:05}"))
            .collect();
        let limit = 256 * 1024;
        let checked = e.claim_check(limit);
        let wire = serde_json::to_vec(&checked).expect("serialize");
        assert!(
            wire.len() <= limit,
            "the published message must fit the bus limit, got {} bytes",
            wire.len()
        );
        assert!(
            checked.payload.is_some(),
            "a small body is not the thing to strip for an oversized envelope"
        );
        assert!(
            checked.changed_attrs.is_empty(),
            "changed_attrs is re-derived by the consumer from the payloads, so it goes first"
        );
        assert!(
            !checked.types.is_empty(),
            "types must survive — the publish subject is built from them"
        );
    }

    /// The publisher asks `claim_checked_at` BEFORE it publishes and keeps
    /// the outbox row when the answer is yes. An answer that disagrees with
    /// the strip either keeps every row (the outbox never drains) or keeps
    /// none (the consumer resolves a reference to a row that is gone).
    #[test]
    fn claim_checked_at_answers_for_the_bodies_the_strip_takes() {
        let mut fits = event(3);
        fits.payload = Some(serde_json::json!({"small": true}));
        fits.prev_payload = Some(serde_json::json!({"small": false}));
        assert!(!fits.claim_checked_at(512));
        assert!(fits.clone().claim_check(512).payload_ref.is_none());

        let mut prev_only = fits.clone();
        prev_only.prev_payload = Some(serde_json::Value::String("x".repeat(1024)));
        assert!(
            prev_only.claim_checked_at(512),
            "a stripped before-image is the one the store cannot answer for"
        );
        assert!(prev_only.claim_check(512).prev_payload_ref.is_some());

        let mut both = fits;
        both.payload = Some(serde_json::Value::String("x".repeat(1024)));
        both.prev_payload = Some(serde_json::Value::String("y".repeat(1024)));
        assert!(both.claim_checked_at(512));
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

    /// A decoded event carries whatever `version` the wire said. The claim
    /// check derives the previous version from it, and that arithmetic must
    /// not overflow — an overflow is a panic in debug builds and a wrap in
    /// release, i.e. a reference to the wrong document.
    #[test]
    fn claim_check_does_not_underflow_on_an_extreme_version() {
        let mut e = event(i64::MIN);
        e.prev_payload = Some(serde_json::Value::String("x".repeat(1024)));
        let checked = e.claim_check(512);
        assert_eq!(
            checked.prev_payload_ref.expect("ref set").version,
            i64::MIN,
            "must saturate, never wrap to i64::MAX"
        );
    }
}
