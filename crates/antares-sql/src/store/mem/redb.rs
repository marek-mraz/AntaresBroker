//! `file` mode durability: the redb write-through shadow behind the
//! in-memory maps, its tables and key layout.

use ::redb::{Database, Durability, TableDefinition};
use antares_store::Kind;

// ---- `file` mode: redb write-through shadow --------------------------------
//
// redb is durability only — queries and the matcher keep running on the
// in-memory maps. Every mutation commits to redb (Durability::Immediate,
// fsync) INSIDE the store's write-critical section, so redb apply order is
// exactly memory apply order, and the commit happens before the store call
// returns — i.e. before the HTTP ack (commit-before-ack). Boot rebuilds
// the maps from the file and refuses to start on a format mismatch.
//
// Table per resource family, named after the spec resource, snake_cased.
// The v0 memory store keeps one temporal doc per entity, so `attr_instances`
// has no separate table; entityMaps are TTL-ephemeral and not durable state.
pub(super) const T_ENTITIES: TableDefinition<&[u8], &[u8]> = TableDefinition::new("entities");
pub(super) const T_SUBSCRIPTIONS: TableDefinition<&[u8], &[u8]> =
    TableDefinition::new("subscriptions");
pub(super) const T_CSOURCE_REGISTRATIONS: TableDefinition<&[u8], &[u8]> =
    TableDefinition::new("csource_registrations");
pub(super) const T_CSOURCE_SUBSCRIPTIONS: TableDefinition<&[u8], &[u8]> =
    TableDefinition::new("csource_subscriptions");
pub(super) const T_TEMPORAL_ENTITIES: TableDefinition<&[u8], &[u8]> =
    TableDefinition::new("temporal_entities");
pub(super) const T_JSONLD_CONTEXTS: TableDefinition<&[u8], &[u8]> =
    TableDefinition::new("jsonld_contexts");
pub(super) const T_SNAPSHOTS: TableDefinition<&[u8], &[u8]> = TableDefinition::new("snapshots");
pub(super) const T_ENTITY_MAP_DOCS: TableDefinition<&[u8], &[u8]> =
    TableDefinition::new("entity_map_docs");
pub(super) const T_DIST_SUBS: TableDefinition<&[u8], &[u8]> = TableDefinition::new("dist_subs");
pub(super) const T_DEAD_LETTERS: TableDefinition<&[u8], &[u8]> =
    TableDefinition::new("dead_letters");
pub(super) const T_META: TableDefinition<&str, &str> = TableDefinition::new("meta");
/// On-disk format version: bump on any key/value shape change; an older
/// or newer file refuses to load rather than being misread as valid data.
pub(super) const FORMAT_VERSION: &str = "1";

pub(super) fn table_for(kind: Kind) -> TableDefinition<'static, &'static [u8], &'static [u8]> {
    match kind {
        Kind::Entity => T_ENTITIES,
        Kind::Subscription => T_SUBSCRIPTIONS,
        Kind::Registration => T_CSOURCE_REGISTRATIONS,
        Kind::CSourceSubscription => T_CSOURCE_SUBSCRIPTIONS,
        Kind::Temporal => T_TEMPORAL_ENTITIES,
        Kind::Snapshot => T_SNAPSHOTS,
        Kind::EntityMap => T_ENTITY_MAP_DOCS,
        Kind::DistSub => T_DIST_SUBS,
        Kind::DeadLetter => T_DEAD_LETTERS,
    }
}

/// Key = `tenant \0 id`. Unambiguous: TenantId is `[A-Za-z0-9_-]{1,64}`
/// by construction, so it can never contain the separator. Takes the tenant
/// as the plain string the maps are keyed by, so a persisted removal can
/// never be skipped for want of a re-parse.
pub(super) fn key_bytes(tenant: &str, id: &str) -> Vec<u8> {
    let mut k = Vec::with_capacity(tenant.len() + 1 + id.len());
    k.extend_from_slice(tenant.as_bytes());
    k.push(0);
    k.extend_from_slice(id.as_bytes());
    k
}

pub(super) fn split_key(key: &[u8]) -> Option<(String, String)> {
    let pos = key.iter().position(|&b| b == 0)?;
    Some((
        String::from_utf8(key[..pos].to_vec()).ok()?,
        String::from_utf8(key[pos + 1..].to_vec()).ok()?,
    ))
}

pub(super) struct Shadow {
    pub(super) db: Database,
}

impl Shadow {
    /// One txn per mutation, fsynced before return. A failed commit
    /// aborts the process: the alternative is acking writes the file does not
    /// hold, which is the one lie a durable store must never tell.
    /// (Deliberately abort-on-commit-failure; per-request error plumbing only
    /// if a recoverable commit failure mode ever shows up in practice.)
    pub(super) fn write(
        &self,
        table: TableDefinition<&[u8], &[u8]>,
        key: &[u8],
        value: Option<&[u8]>,
    ) {
        let result = (|| -> Result<(), String> {
            let mut tx = self.db.begin_write().map_err(|e| e.to_string())?;
            tx.set_durability(Durability::Immediate)
                .map_err(|e| e.to_string())?;
            {
                let mut t = tx.open_table(table).map_err(|e| e.to_string())?;
                match value {
                    Some(v) => {
                        t.insert(key, v).map_err(|e| e.to_string())?;
                    }
                    None => {
                        t.remove(key).map_err(|e| e.to_string())?;
                    }
                }
            }
            tx.commit().map_err(|e| e.to_string())
        })();
        if let Err(e) = result {
            tracing::error!("redb commit failed: {e} — aborting: an acked write must be durable");
            std::process::abort();
        }
    }
}
