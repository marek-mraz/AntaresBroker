//! The system attributes of a write: `createdAt`/`modifiedAt` on the
//! entity and on every attribute instance (4.8, 5.2.4), stamped at the
//! moment the operation is applied.

use antares_model::is_meta;
use serde_json::Value;

/// Inject server-managed timestamps into a freshly expanded doc.
pub fn stamp_new(doc: &mut Value, ts: &str) {
    if let Some(obj) = doc.as_object_mut() {
        obj.insert("createdAt".into(), Value::String(ts.to_owned()));
        obj.insert("modifiedAt".into(), Value::String(ts.to_owned()));
        for (k, v) in obj.iter_mut() {
            if !is_meta(k) {
                stamp_instances(v, ts);
            }
        }
    }
}

/// Stamp one Attribute's instances, and their sub-Attributes, with the
/// 4.8 timestamps: createdAt and modifiedAt are "the temporal Property at
/// which the Entity, Property or Relationship was entered into"/"last
/// modified in an NGSI-LD system", a sub-Property is a Property, and the
/// value is server-generated — whatever the client sent is overwritten.
/// Every write path that brings a new Attribute in uses this one: 5.6.1
/// through `stamp_new`, 5.6.2 and 5.6.3 through `attrs.rs`, so the served
/// representation does not depend on which operation wrote the Attribute.
/// The temporal write path stamps differently (`temporal::stamp_instances`):
/// there an instance is the unit of history and carries an instanceId, and
/// sub-Attributes are part of the instance, not stamped separately.
pub fn stamp_instances(v: &mut Value, ts: &str) {
    if let Some(arr) = v.as_array_mut() {
        for inst in arr {
            if let Some(o) = inst.as_object_mut() {
                o.insert("createdAt".into(), Value::String(ts.to_owned()));
                o.insert("modifiedAt".into(), Value::String(ts.to_owned()));
                for (k, sub) in o.iter_mut() {
                    if sub.is_array() && !antares_jsonld::RESERVED_MEMBERS.contains(&k.as_str()) {
                        stamp_instances(sub, ts);
                    }
                }
            }
        }
    }
}
