//! F8 — temporal auto-recording as a durable change-stream consumer.
//!
//! In bus=local mode the api layer records synchronously (`mirror_record`);
//! in bus=nats mode api pods skip that and THIS consumer reproduces it from
//! `ChangeEvent`s. Delivery is at-least-once, so every write here must be
//! idempotent: instance ids are uuid5 over the instance's identity + the
//! event's `(incarnation, version)` — a redelivered event computes the same
//! ids and the append skips instances the doc already holds. That is the
//! "idempotent upserts on the unique key" the design demands (§6.4); the
//! store's dual-write then lands them in `attr_instances` in the same tx.
//!
//! ponytail: attribute DELETION history stays with the api's synchronous
//! `mirror_delete_attr` in every mode (its result feeds the 404-vs-204
//! decision, so it cannot leave the request path); this recorder appends
//! created/updated instances and applies the entityDeleted fence only.

use antares_bus::{ChangeEvent, ChangeOp};
use antares_model::TenantId;
use antares_sql::store::any::AnyStore;
use antares_sql::store::Kind;
use serde_json::{Map, Value};

/// Entity members that never become temporal attribute instance arrays.
const META: &[&str] = &[
    "id",
    "type",
    "scope",
    "createdAt",
    "modifiedAt",
    "deletedAt",
    "expiresAt",
    "@context",
];

/// The instance identity the dedup key is built from: datasetId + the
/// timestamps + the value body, with volatile members stripped.
fn identity(inst: &Value) -> String {
    let mut clean = inst.clone();
    if let Some(o) = clean.as_object_mut() {
        o.remove("instanceId");
    }
    clean.to_string()
}

fn instance_id(
    tenant: &TenantId,
    entity: &str,
    attr: &str,
    ev: &ChangeEvent,
    ident: &str,
) -> String {
    let name = format!(
        "{}|{}|{}|{}|{}|{}",
        tenant.as_str(),
        entity,
        attr,
        ev.incarnation,
        ev.version,
        ident
    );
    format!(
        "urn:ngsi-ld:Instance:{}",
        uuid::Uuid::new_v5(&uuid::Uuid::NAMESPACE_OID, name.as_bytes())
    )
}

/// Apply one change event to the temporal representation. Idempotent by
/// construction — replaying the same event is a no-op.
pub fn apply(store: &AnyStore, ev: &ChangeEvent) {
    let tenant = &ev.tenant;
    let id = ev.entity_id.as_str();
    if ev.op == ChangeOp::Delete || (ev.payload.is_none() && ev.payload_ref.is_none()) {
        // the entityDeleted fence: temporal representation goes with the row
        if let Err(e) = store.delete(tenant, Kind::Temporal, id) {
            tracing::warn!("recorder: temporal delete of {id} failed: {e}");
        }
        return;
    }
    // claim check: an oversized payload travels as a reference (§7)
    let fetched;
    let payload = match (&ev.payload, &ev.payload_ref) {
        (Some(p), _) => p,
        (None, Some(r)) => match store.get(tenant, Kind::Entity, r.entity_id.as_str()) {
            Ok(Some(doc)) => {
                fetched = doc;
                &fetched
            }
            _ => return,
        },
        _ => return,
    };
    let Some(obj) = payload.as_object() else {
        return;
    };
    let prev = ev.prev_payload.as_ref().and_then(Value::as_object);

    let r = (|| -> Result<(), antares_model::NgsiError> {
        if store.get(tenant, Kind::Temporal, id)?.is_none() {
            let mut doc = Map::new();
            for k in ["id", "type", "createdAt", "modifiedAt", "scope"] {
                if let Some(v) = obj.get(k) {
                    doc.insert(k.into(), v.clone());
                }
            }
            store.create(tenant, Kind::Temporal, id, Value::Object(doc))?;
        }
        store.mutate(tenant, Kind::Temporal, id, |doc| {
            let target = doc.as_object_mut().expect("temporal doc");
            for (k, v) in obj {
                if META.contains(&k.as_str()) {
                    continue;
                }
                let instances: Vec<&Value> =
                    v.as_array().map(|a| a.iter().collect()).unwrap_or_default();
                let prev_ids: Vec<String> = prev
                    .and_then(|p| p.get(k))
                    .and_then(Value::as_array)
                    .map(|a| a.iter().map(identity).collect())
                    .unwrap_or_default();
                for inst in instances {
                    let ident = identity(inst);
                    if prev_ids.contains(&ident) {
                        continue; // unchanged instance — not part of this write
                    }
                    let iid = instance_id(tenant, id, k, ev, &ident);
                    let arr = match target.get_mut(k).and_then(Value::as_array_mut) {
                        Some(a) => a,
                        None => {
                            target.insert(k.clone(), Value::Array(Vec::new()));
                            target
                                .get_mut(k)
                                .and_then(Value::as_array_mut)
                                .expect("arr")
                        }
                    };
                    let already = arr.iter().any(|existing| {
                        existing.get("instanceId").and_then(Value::as_str) == Some(iid.as_str())
                    });
                    if already {
                        continue; // redelivery — the idempotence property
                    }
                    let mut stamped = inst.clone();
                    if let Some(o) = stamped.as_object_mut() {
                        o.insert("instanceId".into(), Value::String(iid));
                    }
                    arr.push(stamped);
                }
            }
            Ok::<(), std::convert::Infallible>(())
        })?;
        Ok(())
    })();
    if let Err(e) = r {
        tracing::warn!("recorder: temporal apply for {id} failed: {e}");
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use antares_model::EntityId;
    use antares_sql::store::Store;
    use serde_json::json;

    fn ev(version: i64, prev: Option<Value>, payload: Option<Value>) -> ChangeEvent {
        ChangeEvent {
            tenant: TenantId::default(),
            entity_id: EntityId::new("urn:x:1").expect("id"),
            types: vec!["T".into()],
            op: if payload.is_some() {
                ChangeOp::Update
            } else {
                ChangeOp::Delete
            },
            changed_attrs: vec![],
            payload,
            prev_payload: prev,
            version,
            incarnation: "2026-08-05T00:00:00Z".into(),
            seq: version,
            payload_ref: None,
            prev_payload_ref: None,
        }
    }

    fn doc(val: i64, at: &str) -> Value {
        json!({
            "id": "urn:x:1", "type": ["T"],
            "createdAt": "2026-08-05T00:00:00Z", "modifiedAt": at,
            "https://x/temp": [{"type": "Property", "value": val, "observedAt": at}]
        })
    }

    #[test]
    fn replay_is_idempotent_and_changes_append() {
        let store = AnyStore::Mem(Store::default());
        let t = TenantId::default();
        let e1 = ev(1, None, Some(doc(1, "2026-08-05T01:00:00Z")));
        apply(&store, &e1);
        apply(&store, &e1); // redelivery
        let tdoc = store
            .get(&t, Kind::Temporal, "urn:x:1")
            .expect("get")
            .expect("present");
        assert_eq!(
            tdoc["https://x/temp"].as_array().expect("arr").len(),
            1,
            "redelivered event must not duplicate instances"
        );

        // a second write appends only the CHANGED instance
        let e2 = ev(
            2,
            Some(doc(1, "2026-08-05T01:00:00Z")),
            Some(doc(2, "2026-08-05T02:00:00Z")),
        );
        apply(&store, &e2);
        let tdoc = store
            .get(&t, Kind::Temporal, "urn:x:1")
            .expect("get")
            .expect("present");
        assert_eq!(tdoc["https://x/temp"].as_array().expect("arr").len(), 2);
    }

    #[test]
    fn delete_is_the_fence() {
        let store = AnyStore::Mem(Store::default());
        let t = TenantId::default();
        apply(&store, &ev(1, None, Some(doc(1, "2026-08-05T01:00:00Z"))));
        apply(&store, &ev(2, Some(doc(1, "2026-08-05T01:00:00Z")), None));
        assert!(store
            .get(&t, Kind::Temporal, "urn:x:1")
            .expect("get")
            .is_none());
    }
}
