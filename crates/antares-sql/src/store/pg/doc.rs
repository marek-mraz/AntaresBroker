// SPDX-License-Identifier: EUPL-1.2
//! PgStore slice two: the doc-table kinds —
//! `subscriptions`, `csource_registrations`, `csource_subscriptions` — plus
//! cross-tenant `jsonld_contexts`. Same sync-facade shape as `pg_entity`.
//!
//! The v0 interchange form stores ONE doc per resource; the bookkeeping
//! columns (`expires_at`, `is_active`, `times_sent`, `last_*`) are EXTRACTED
//! from the doc on every write, so the row stays the truth while the
//! API layer keeps its doc-shaped view until the cutover completes.

use antares_model::TenantId;
use serde_json::Value;
use sqlx::postgres::PgPool;
use sqlx::Row;

use super::entity::{check_ceiling, wait, MAX_UNDECIDED_ROWS};

/// Which doc table a resource kind lives in.
#[derive(Clone, Copy, Debug)]
pub enum DocKind {
    Subscription,
    Registration,
    CSourceSubscription,
    Snapshot,
    EntityMap,
    DistSub,
    DeadLetter,
}

impl DocKind {
    fn table(self) -> &'static str {
        match self {
            DocKind::Subscription => "subscriptions",
            DocKind::Registration => "csource_registrations",
            DocKind::CSourceSubscription => "csource_subscriptions",
            DocKind::Snapshot => "snapshots",
            DocKind::EntityMap => "entity_map_docs",
            DocKind::DistSub => "dist_subs",
            DocKind::DeadLetter => "dead_letters",
        }
    }
    fn doc_column(self) -> &'static str {
        match self {
            DocKind::Subscription | DocKind::CSourceSubscription => "subscription",
            DocKind::Registration => "registration",
            DocKind::Snapshot | DocKind::EntityMap | DocKind::DistSub | DocKind::DeadLetter => {
                "doc"
            }
        }
    }
    fn has_bookkeeping(self) -> bool {
        matches!(self, DocKind::Subscription | DocKind::CSourceSubscription)
    }
}

/// Bookkeeping columns, derived from the doc (5.2.14.2 output members).
fn bookkeeping(
    doc: &Value,
) -> (
    Option<String>,
    bool,
    i64,
    Option<String>,
    Option<String>,
    Option<String>,
) {
    let s = |v: Option<&Value>| v.and_then(Value::as_str).map(str::to_owned);
    let n = doc.get("notification");
    (
        s(doc.get("expiresAt")),
        doc.get("isActive") != Some(&Value::Bool(false)),
        n.and_then(|n| n.get("timesSent"))
            .and_then(Value::as_i64)
            .unwrap_or(0),
        s(n.and_then(|n| n.get("lastNotification"))),
        s(n.and_then(|n| n.get("lastSuccess"))),
        s(n.and_then(|n| n.get("lastFailure"))),
    )
}

// ---- csource_index maintenance ---------------------------------------------
// The flattened federation match table, rebuilt in Rust inside the same
// transaction as every registration write (no triggers). Deleting a
// registration cleans its rows via the FK ON DELETE CASCADE.

/// Table 4.20-1 operation names; bit position in `csource_index.ops` = index.
/// Order is append-only: a bitmask is stored data, so renumbering is a
/// migration.
pub const OPERATIONS: &[&str] = &[
    "createEntity",
    "updateEntity",
    "appendAttrs",
    "updateAttrs",
    "deleteAttrs",
    "deleteEntity",
    "createBatch",
    "upsertBatch",
    "updateBatch",
    "deleteBatch",
    "upsertTemporal",
    "appendAttrsTemporal",
    "deleteAttrsTemporal",
    "updateAttrInstanceTemporal",
    "deleteAttrInstanceTemporal",
    "deleteTemporal",
    "mergeEntity",
    "replaceEntity",
    "replaceAttrs",
    "mergeBatch",
    "purgeEntity",
    "retrieveEntity",
    "queryEntity",
    "queryBatch",
    "retrieveTemporal",
    "queryTemporal",
    "retrieveEntityTypes",
    "retrieveEntityTypeDetails",
    "retrieveEntityTypeInfo",
    "retrieveAttrTypes",
    "retrieveAttrTypeDetails",
    "retrieveAttrTypeInfo",
    "createSubscription",
    "updateSubscription",
    "retrieveSubscription",
    "querySubscription",
    "deleteSubscription",
    "retrieveEntityMap",
    "updateEntityMap",
    "deleteEntityMap",
    "createEntityMapQueryEntity",
    "createEntityMapQueryTemporal",
    "retrieveContextSourceIdentity",
];

/// The bit position is stored data (`csource_index.ops` is a `bigint`), so
/// appending a 64th operation would overflow the shift in `ops_mask` and
/// write a mask no later migration could distinguish from a real one. Fail at
/// compile time on the day it is appended, not on a corrupted row later.
const _: () = assert!(OPERATIONS.len() < 64, "ops bitmask is i64");

/// Table 4.20-2 named groups, expanded to their members before masking.
fn group_members(name: &str) -> Option<&'static [&'static str]> {
    const FEDERATION: &[&str] = &[
        "retrieveEntity",
        "queryEntity",
        "queryBatch",
        "retrieveEntityTypes",
        "retrieveEntityTypeDetails",
        "retrieveEntityTypeInfo",
        "retrieveAttrTypes",
        "retrieveAttrTypeDetails",
        "retrieveAttrTypeInfo",
        "createSubscription",
        "updateSubscription",
        "retrieveSubscription",
        "querySubscription",
        "deleteSubscription",
        "retrieveEntityMap",
        "updateEntityMap",
        "deleteEntityMap",
        "createEntityMapQueryEntity",
        "retrieveContextSourceIdentity",
    ];
    const ASSOCIATION: &[&str] = &[
        "retrieveEntity",
        "queryEntity",
        "queryBatch",
        "retrieveEntityTypes",
        "retrieveEntityTypeDetails",
        "retrieveEntityTypeInfo",
        "retrieveAttrTypes",
        "retrieveAttrTypeDetails",
        "retrieveAttrTypeInfo",
        "createSubscription",
        "updateSubscription",
        "retrieveSubscription",
        "querySubscription",
        "deleteSubscription",
        "retrieveContextSourceIdentity",
    ];
    const UPDATE: &[&str] = &[
        "updateEntity",
        "updateAttrs",
        "replaceEntity",
        "replaceAttrs",
    ];
    const RETRIEVE: &[&str] = &["retrieveEntity", "queryEntity"];
    const REDIRECTION: &[&str] = &[
        "createEntity",
        "updateEntity",
        "appendAttrs",
        "updateAttrs",
        "deleteAttrs",
        "deleteEntity",
        "mergeEntity",
        "replaceEntity",
        "replaceAttrs",
        "retrieveEntity",
        "queryEntity",
        "purgeEntity",
        "retrieveEntityTypes",
        "retrieveEntityTypeDetails",
        "retrieveEntityTypeInfo",
        "retrieveAttrTypes",
        "retrieveAttrTypeDetails",
        "retrieveAttrTypeInfo",
        "retrieveEntityMap",
        "updateEntityMap",
        "deleteEntityMap",
        "createEntityMapQueryEntity",
        "retrieveContextSourceIdentity",
    ];
    match name {
        "federationOps" => Some(FEDERATION),
        "associationOps" => Some(ASSOCIATION),
        "updateOps" => Some(UPDATE),
        "retrieveOps" => Some(RETRIEVE),
        "redirectionOps" => Some(REDIRECTION),
        _ => None,
    }
}

/// Registration `operations` → bitmask; absent defaults to federationOps
/// (5.2.9).
pub fn ops_mask(reg: &Value) -> i64 {
    let names: Vec<&str> = reg
        .get("operations")
        .and_then(Value::as_array)
        .map(|a| a.iter().filter_map(Value::as_str).collect())
        .unwrap_or_else(|| vec!["federationOps"]);
    let mut mask = 0i64;
    let mut set = |op: &str| {
        if let Some(bit) = OPERATIONS.iter().position(|o| *o == op) {
            mask |= 1 << bit;
        }
    };
    for n in names {
        match group_members(n) {
            Some(members) => members.iter().for_each(|m| set(m)),
            None => set(n),
        }
    }
    mask
}

fn mode_code(reg: &Value) -> i16 {
    match reg.get("mode").and_then(Value::as_str) {
        Some("auxiliary") => 0,
        Some("redirect") => 2,
        Some("exclusive") => 3,
        _ => 1, // inclusive is the default (5.2.9)
    }
}

/// Hard ceiling on rows one registration may explode into. The API caps
/// cardinality at the validation boundary, but a document written
/// through any other path — a restored dump, a future importer — must not be
/// able to drive this quadratically. Truncating loses federation matches for
/// an absurd registration; OOM loses the process.
pub const MAX_INDEX_ROWS: usize = 10_000;

/// Explode one registration document into csource_index rows: each
/// RegistrationInfo element yields entities × (propertyNames ∪
/// relationshipNames) rows, with NULL placeholders when a dimension is
/// absent — the Scorpio csourceinformation shape minus the 46
/// boolean columns. Attribute/type names are stored as they appear in the
/// document; canonical-IRI storage lands with the SQL matching path.
/// NGSI-LD 2.0 readiness: when propertyNames/relationshipNames
/// merge into attributeNames, the migration is a coalesce of the two name
/// columns into one attribute_name column — no reshape.
pub fn index_rows(reg: &Value) -> Vec<Value> {
    let endpoint = reg
        .get("endpoint")
        .and_then(Value::as_str)
        .unwrap_or_default();
    let common = |entity: Option<&Value>, prop: Option<&str>, rel: Option<&str>| {
        serde_json::json!({
            "entity_id": entity.and_then(|e| e.get("id")).and_then(Value::as_str),
            "id_pattern": entity.and_then(|e| e.get("idPattern")).and_then(Value::as_str),
            "entity_type": entity.and_then(|e| e.get("type")).and_then(Value::as_str),
            "property_name": prop,
            "relationship_name": rel,
            // A registration carries its geo scope as a RAW GeoJSON
            // geometry under `location` (not instance-wrapped like an entity
            // attribute) — see antares_api::csource::csr_matches_subscription,
            // which hands exactly this value to `matches_geometry`.
            "location": reg.get("location").filter(|g| g.get("type").is_some())
                           .map(|g| g.to_string()),
            "scopes": reg.get("scope").map(|s| match s {
                Value::String(one) => vec![one.clone()],
                Value::Array(a) => a.iter().filter_map(Value::as_str).map(str::to_owned).collect(),
                _ => vec![],
            }),
            "expires_at": reg.get("expiresAt").and_then(Value::as_str),
            "endpoint": endpoint,
            "mode": mode_code(reg),
            "ops": ops_mask(reg),
            "tenant_at_peer": reg.get("tenant").and_then(Value::as_str),
            "headers": reg.get("contextSourceInfo"),
            // Table 5.2.9-1 names this member `contextSourceAlias` — the
            // peer's tenant-specific loop pseudonym. (`hostAlias` is the
            // prose spelling 6.3.18 uses and the csource_index column name;
            // it is not a payload member and was never sent by any client.)
            "host_alias": reg.get("contextSourceAlias").and_then(Value::as_str),
        })
    };
    let mut rows = Vec::new();
    let infos = reg
        .get("information")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    for info in &infos {
        let names = |k: &str| -> Vec<String> {
            info.get(k)
                .and_then(Value::as_array)
                .map(|a| {
                    a.iter()
                        .filter_map(Value::as_str)
                        .map(str::to_owned)
                        .collect()
                })
                .unwrap_or_default()
        };
        let props = names("propertyNames");
        let rels = names("relationshipNames");
        let entities: Vec<Option<&Value>> = match info.get("entities").and_then(Value::as_array) {
            Some(a) if !a.is_empty() => a.iter().map(Some).collect(),
            _ => vec![None],
        };
        for ent in entities {
            // Checked per ROW, not per entity: one `information` element with
            // one entity and a million propertyNames never reaches a
            // per-entity check twice, so the vector grew unbounded — the OOM
            // this ceiling exists to prevent.
            for (p, r) in std::iter::once((None, None))
                .filter(|_| props.is_empty() && rels.is_empty())
                .chain(props.iter().map(|p| (Some(p.as_str()), None)))
                .chain(rels.iter().map(|r| (None, Some(r.as_str()))))
            {
                if rows.len() >= MAX_INDEX_ROWS {
                    tracing::warn!(
                        "registration explodes past {MAX_INDEX_ROWS} index rows; truncating"
                    );
                    return rows;
                }
                rows.push(common(ent, p, r));
            }
        }
    }
    if infos.is_empty() {
        rows.push(common(None, None, None));
    }
    rows
}

/// Rebuild one registration's `csource_index` rows (delete + multi-row
/// insert) inside the CALLER's transaction, so the extracted match rows are
/// never a version behind the document. Shared by `upsert` and `mutate` —
/// the row lock lives in here, so no caller can do it atomically itself.
///
/// The geometry goes through `try_geomfromgeojson` (migration 0009): a
/// location PostGIS cannot parse leaves the column NULL instead of aborting
/// the write.
async fn rebuild_csource_index(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    tenant: &TenantId,
    id: &str,
    doc: &Value,
) -> Result<(), sqlx::Error> {
    sqlx::query("DELETE FROM csource_index WHERE tenant_id = $1 AND registration_id = $2")
        .bind(tenant.as_str())
        .bind(id)
        .execute(&mut **tx)
        .await?;
    sqlx::query(
        "INSERT INTO csource_index
           (tenant_id, registration_id, entity_id, id_pattern, entity_type,
            property_name, relationship_name, scopes, expires_at, endpoint,
            mode, ops, tenant_at_peer, headers, host_alias, location)
         SELECT $1, $2, e->>'entity_id', e->>'id_pattern', e->>'entity_type',
                e->>'property_name', e->>'relationship_name',
                CASE WHEN e->'scopes' = 'null'::jsonb THEN NULL
                     ELSE ARRAY(SELECT jsonb_array_elements_text(e->'scopes')) END,
                (e->>'expires_at')::timestamptz, e->>'endpoint',
                (e->>'mode')::smallint, (e->>'ops')::bigint,
                e->>'tenant_at_peer', e->'headers', e->>'host_alias',
                CASE WHEN ST_IsValid(try_geomfromgeojson(e->>'location'))
                     THEN try_geomfromgeojson(e->>'location') END
         FROM jsonb_array_elements($3::jsonb) AS e",
    )
    .bind(tenant.as_str())
    .bind(id)
    .bind(Value::Array(index_rows(doc)))
    .execute(&mut **tx)
    .await
    .map(|_| ())
}

/// The INSERT every doc write shares — same columns, same binds, differing
/// only in the caller's `ON CONFLICT …` tail. `None` = the tail took a
/// DO NOTHING path (the row was already there).
async fn insert_doc(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    tenant: &TenantId,
    kind: DocKind,
    id: &str,
    doc: &Value,
    conflict: &str,
) -> Result<Option<bool>, sqlx::Error> {
    let table = kind.table();
    let col = kind.doc_column();
    let head = if kind.has_bookkeeping() {
        format!(
            "INSERT INTO {table} (tenant_id, id, {col}, context, expires_at, is_active,
               times_sent, last_notification, last_success, last_failure)
             VALUES ($1, $2, $3, $4, $5::timestamptz, $6, $7,
               $8::timestamptz, $9::timestamptz, $10::timestamptz)"
        )
    } else {
        format!("INSERT INTO {table} (tenant_id, id, {col}) VALUES ($1, $2, $3)")
    };
    // literals from `DocKind` plus the caller's literal tail, values bound
    let mut q = sqlx::query(sqlx::AssertSqlSafe(format!("{head}{conflict}")))
        .bind(tenant.as_str())
        .bind(id)
        .bind(doc);
    let bk;
    if kind.has_bookkeeping() {
        let context = doc
            .get("@context")
            .cloned()
            .unwrap_or(Value::Object(Default::default()));
        bk = (bookkeeping(doc), context);
        let ((expires, active, sent, last_n, last_s, last_f), context) = &bk;
        q = q
            .bind(context)
            .bind(expires)
            .bind(*active)
            .bind(*sent)
            .bind(last_n)
            .bind(last_s)
            .bind(last_f);
    }
    Ok(q.fetch_optional(&mut **tx)
        .await?
        .map(|r| r.get::<bool, _>(0)))
}

pub struct PgDocStore {
    pool: PgPool,
}

impl PgDocStore {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }

    pub fn pool(&self) -> &PgPool {
        &self.pool
    }

    /// Create one doc: `Ok(false)` when a document with that id already
    /// exists. 5.8.1.4 (and 5.9.2.4 for registrations): "If the NGSI-LD
    /// endpoint already knows about this Subscription, as there is an
    /// existing Subscription whose id (URI) is equivalent, an error of type
    /// AlreadyExists shall be raised."
    ///
    /// ONE statement, so the answer comes from the unique constraint itself:
    /// a read-then-write would let two concurrent creates of the same
    /// client-supplied id both report created, and the second would silently
    /// overwrite the first.
    pub fn create(
        &self,
        tenant: &TenantId,
        kind: DocKind,
        id: &str,
        doc: &Value,
    ) -> Result<bool, sqlx::Error> {
        let conflict = " ON CONFLICT (tenant_id, id) DO NOTHING RETURNING true AS created";
        wait(async {
            let mut tx = self.pool.begin().await?;
            crate::store::pg::set_tenant(&mut tx, tenant).await?;
            let created = insert_doc(&mut tx, tenant, kind, id, doc, conflict)
                .await?
                .is_some();
            // a losing INSERT must not rebuild the winner's index rows
            if created && matches!(kind, DocKind::Registration) {
                rebuild_csource_index(&mut tx, tenant, id, doc).await?;
            }
            tx.commit().await?;
            Ok(created)
        })
    }

    /// Upsert one doc, refreshing the extracted columns. `Ok(true)` = it
    /// existed before.
    pub fn upsert(
        &self,
        tenant: &TenantId,
        kind: DocKind,
        id: &str,
        doc: &Value,
    ) -> Result<bool, sqlx::Error> {
        let col = kind.doc_column();
        let conflict = if kind.has_bookkeeping() {
            format!(
                " ON CONFLICT (tenant_id, id) DO UPDATE SET {col} = EXCLUDED.{col},
                   context = EXCLUDED.context, expires_at = EXCLUDED.expires_at,
                   is_active = EXCLUDED.is_active, times_sent = EXCLUDED.times_sent,
                   last_notification = EXCLUDED.last_notification,
                   last_success = EXCLUDED.last_success, last_failure = EXCLUDED.last_failure
                 RETURNING (xmax <> 0) AS existed"
            )
        } else {
            format!(
                " ON CONFLICT (tenant_id, id) DO UPDATE SET {col} = EXCLUDED.{col}
                 RETURNING (xmax <> 0) AS existed"
            )
        };
        wait(async {
            let mut tx = self.pool.begin().await?;
            crate::store::pg::set_tenant(&mut tx, tenant).await?;
            let existed = insert_doc(&mut tx, tenant, kind, id, doc, &conflict)
                .await?
                .expect("DO UPDATE always returns the row");
            if matches!(kind, DocKind::Registration) {
                rebuild_csource_index(&mut tx, tenant, id, doc).await?;
            }
            tx.commit().await?;
            Ok(existed)
        })
    }

    pub fn get(
        &self,
        tenant: &TenantId,
        kind: DocKind,
        id: &str,
    ) -> Result<Option<Value>, sqlx::Error> {
        let sql = format!(
            "SELECT {} FROM {} WHERE tenant_id = $1 AND id = $2",
            kind.doc_column(),
            kind.table()
        );
        wait(async {
            let mut tx = self.pool.begin().await?;
            crate::store::pg::set_tenant(&mut tx, tenant).await?;
            let row = sqlx::query(sqlx::AssertSqlSafe(sql.clone()))
                .bind(tenant.as_str())
                .bind(id)
                .fetch_optional(&mut *tx)
                .await?;
            tx.commit().await?;
            Ok(row.map(|r| r.get::<Value, _>(0)))
        })
    }

    /// Read-modify-write in ONE transaction under the row lock (the entity
    /// pattern applied to doc kinds): `SELECT … FOR UPDATE` → apply → `UPDATE`.
    /// A missing row returns `None` and is NEVER inserted — a concurrent
    /// DELETE must win, not be resurrected by a bookkeeping writeback
    /// (the 047_06 leftover-subscription bug).
    pub fn mutate<T, E>(
        &self,
        tenant: &TenantId,
        kind: DocKind,
        id: &str,
        f: impl FnOnce(&mut Value) -> Result<T, E>,
    ) -> Result<Option<Result<T, E>>, sqlx::Error> {
        let col = kind.doc_column();
        let table = kind.table();
        let select =
            format!("SELECT {col} FROM {table} WHERE tenant_id = $1 AND id = $2 FOR UPDATE");
        let update = if kind.has_bookkeeping() {
            format!(
                "UPDATE {table} SET {col} = $3, expires_at = $4::timestamptz,
                   is_active = $5, times_sent = $6, last_notification = $7::timestamptz,
                   last_success = $8::timestamptz, last_failure = $9::timestamptz,
                   context = $10
                 WHERE tenant_id = $1 AND id = $2"
            )
        } else {
            format!("UPDATE {table} SET {col} = $3 WHERE tenant_id = $1 AND id = $2")
        };
        wait(async move {
            let mut tx = self.pool.begin().await?;
            crate::store::pg::set_tenant(&mut tx, tenant).await?;
            let row = sqlx::query(sqlx::AssertSqlSafe(select.clone()))
                .bind(tenant.as_str())
                .bind(id)
                .fetch_optional(&mut *tx)
                .await?;
            let Some(row) = row else {
                return Ok(None);
            };
            let mut doc: Value = row.get(0);
            match f(&mut doc) {
                Ok(t) => {
                    let context = doc
                        .get("@context")
                        .cloned()
                        .unwrap_or(Value::Object(Default::default()));
                    let (expires, active, sent, last_n, last_s, last_f) = bookkeeping(&doc);
                    let mut q = sqlx::query(sqlx::AssertSqlSafe(update.clone()))
                        .bind(tenant.as_str())
                        .bind(id)
                        .bind(&doc);
                    if kind.has_bookkeeping() {
                        q = q
                            .bind(&expires)
                            .bind(active)
                            .bind(sent)
                            .bind(&last_n)
                            .bind(&last_s)
                            .bind(&last_f)
                            .bind(&context);
                    }
                    q.execute(&mut *tx).await?;
                    // 5.9.3 Update Registration: a patch may flip the mode,
                    // rewrite `information` or move the endpoint, so the
                    // extracted match rows have to be rebuilt with the doc —
                    // in this transaction, under the same row lock.
                    if matches!(kind, DocKind::Registration) {
                        rebuild_csource_index(&mut tx, tenant, id, &doc).await?;
                    }
                    tx.commit().await?;
                    Ok(Some(Ok(t)))
                }
                // closure rejected the change: nothing written, lock released
                Err(e) => Ok(Some(Err(e))),
            }
        })
    }

    pub fn delete(&self, tenant: &TenantId, kind: DocKind, id: &str) -> Result<bool, sqlx::Error> {
        let sql = format!(
            "DELETE FROM {} WHERE tenant_id = $1 AND id = $2",
            kind.table()
        );
        wait(async {
            let mut tx = self.pool.begin().await?;
            crate::store::pg::set_tenant(&mut tx, tenant).await?;
            let done = sqlx::query(sqlx::AssertSqlSafe(sql.clone()))
                .bind(tenant.as_str())
                .bind(id)
                .execute(&mut *tx)
                .await?
                .rows_affected();
            tx.commit().await?;
            Ok(done == 1)
        })
    }

    /// Every doc of one kind for one tenant, id-ordered.
    ///
    /// Bounded: without a LIMIT this statement materializes a whole tenant's
    /// subscriptions/registrations into one `Vec`, which at the 100 000-per-
    /// broker target is the broker's memory, not the database's. A tenant that
    /// reaches the ceiling is refused with TooManyResults (5.5.6) rather than
    /// served a silent prefix.
    pub fn list(&self, tenant: &TenantId, kind: DocKind) -> Result<Vec<Value>, sqlx::Error> {
        let sql = format!(
            "SELECT {} FROM {} WHERE tenant_id = $1 ORDER BY id LIMIT $2",
            kind.doc_column(),
            kind.table()
        );
        wait(async {
            let mut tx = self.pool.begin().await?;
            crate::store::pg::set_tenant(&mut tx, tenant).await?;
            let rows = sqlx::query(sqlx::AssertSqlSafe(sql.clone()))
                .bind(tenant.as_str())
                .bind(MAX_UNDECIDED_ROWS)
                .fetch_all(&mut *tx)
                .await?;
            tx.commit().await?;
            check_ceiling(false, rows.len(), MAX_UNDECIDED_ROWS)?;
            Ok(rows.into_iter().map(|r| r.get::<Value, _>(0)).collect())
        })
    }

    /// 5.12: the registrations that may take part in an operation on these
    /// entity ids / types, read through the `csource_index` rows every
    /// registration write maintains — the alternative is listing every
    /// registration document of the tenant and scanning it in Rust, which at
    /// the 100 000-registrations target is a full table read per federated
    /// request.
    ///
    /// The narrowing is one-directional, exactly like the entity pushdown: SQL
    /// may only REMOVE rows the caller's matcher would reject anyway, and the
    /// matcher stays the arbiter of every other 5.12 condition (csf, geo,
    /// intervals, datasetId, the Via chain, the idPattern regex). Hence an
    /// index dimension left NULL is unconstrained and always survives, and an
    /// `idPattern` row survives every id query. `None` means "do not narrow on
    /// that dimension". A registration whose explosion hit `MAX_INDEX_ROWS`
    /// carries only the rows that fit — the same truncation the index write
    /// already accepts.
    ///
    /// `types` must be EXPANDED plain type IRIs, because that is what the
    /// registration write stored (each EntityInfo `type` goes through
    /// `expand_key` before the index row is built). A term, or a 4.17 Entity
    /// Type Selection expression, matches no stored value and would narrow away
    /// registrations that do match — a caller holding either passes `None`.
    ///
    /// Bounded like every other read (5.5.6): a tenant whose candidate set
    /// reaches the ceiling is refused rather than served a silent prefix.
    pub fn matching_registrations(
        &self,
        tenant: &TenantId,
        ids: Option<&[String]>,
        types: Option<&[String]>,
    ) -> Result<Vec<Value>, sqlx::Error> {
        // An absent dimension is OMITTED from the statement rather than bound
        // as NULL and escaped in SQL: `$2 IS NULL OR …` is unfoldable in a
        // GENERIC plan, and Postgres switches a repeatedly executed prepared
        // statement to one — measured as a sequential scan of csource_index,
        // exactly the read this function exists to avoid.
        let mut wheres = String::new();
        let mut n = 1; // $1 = tenant_id
        if types.is_some() {
            n += 1;
            wheres.push_str(&format!(
                " AND (x.entity_type IS NULL OR x.entity_type = ANY(${n}))"
            ));
        }
        if ids.is_some() {
            n += 1;
            wheres.push_str(&format!(
                " AND (x.entity_id IS NULL OR x.id_pattern IS NOT NULL \
                   OR x.entity_id = ANY(${n}))"
            ));
        }
        // literals from this function plus `$n` placeholders — no caller text
        let sql = format!(
            "SELECT DISTINCT r.id, r.registration
               FROM csource_registrations r
               JOIN csource_index x
                 ON x.tenant_id = r.tenant_id AND x.registration_id = r.id
              WHERE r.tenant_id = $1{wheres}
              ORDER BY r.id LIMIT ${}",
            n + 1
        );
        wait(async {
            let mut tx = self.pool.begin().await?;
            crate::store::pg::set_tenant(&mut tx, tenant).await?;
            let mut q = sqlx::query(sqlx::AssertSqlSafe(sql.clone())).bind(tenant.as_str());
            if let Some(types) = types {
                q = q.bind(types);
            }
            if let Some(ids) = ids {
                q = q.bind(ids);
            }
            let rows = q.bind(MAX_UNDECIDED_ROWS).fetch_all(&mut *tx).await?;
            tx.commit().await?;
            check_ceiling(false, rows.len(), MAX_UNDECIDED_ROWS)?;
            Ok(rows.into_iter().map(|r| r.get::<Value, _>(1)).collect())
        })
    }

    /// Bookkeeping columns straight from the row (test hook: rows are truth).
    pub fn status_row(
        &self,
        tenant: &TenantId,
        kind: DocKind,
        id: &str,
    ) -> Result<Option<(bool, i64)>, sqlx::Error> {
        let sql = format!(
            "SELECT is_active, times_sent FROM {} WHERE tenant_id = $1 AND id = $2",
            kind.table()
        );
        wait(async {
            let mut tx = self.pool.begin().await?;
            crate::store::pg::set_tenant(&mut tx, tenant).await?;
            let row = sqlx::query(sqlx::AssertSqlSafe(sql.clone()))
                .bind(tenant.as_str())
                .bind(id)
                .fetch_optional(&mut *tx)
                .await?;
            tx.commit().await?;
            Ok(row.map(|r| (r.get(0), r.get(1))))
        })
    }

    // jsonldContexts — the ONE cross-tenant table, no RLS by design: Cached
    // rows are copies of public documents every tenant shares. The
    // tenant-authored kinds (Hosted, ImplicitlyCreated, 5.13.1) carry their
    // owning tenant in the stored document's "owner" member, enforced where
    // they are served, listed and deleted (5.13).
    ///
    /// 5.13.1: "Implementations shall periodically invalidate the 'Cached'
    /// @contexts." A Cached row is written per distinct external URL a request
    /// references — client-controlled input — and the broker warms every
    /// stored row at startup, so an insert that pushes the cache past its
    /// ceiling evicts the oldest Cached rows. `Hosted` and
    /// `ImplicitlyCreated` rows are resources the broker serves on demand
    /// (5.13.2, 5.13.4), not cache, and are never evicted.
    pub fn context_put(&self, id: &str, doc: &Value, kind: &str) -> Result<(), sqlx::Error> {
        wait(async {
            // `xmax` is zero on a fresh row and the locking transaction's id
            // on the conflict path: the eviction then runs only when the table
            // actually grew, never on a usage bump rewriting a row in place.
            let inserted: bool = sqlx::query_scalar(
                "INSERT INTO jsonld_contexts (id, body, kind) VALUES ($1, $2, $3)
                 ON CONFLICT (id) DO UPDATE SET body = EXCLUDED.body, kind = EXCLUDED.kind
                 RETURNING xmax::text = '0'",
            )
            .bind(id)
            .bind(doc)
            .bind(kind)
            .fetch_one(&self.pool)
            .await?;
            if inserted && kind == "Cached" {
                sqlx::query(
                    "DELETE FROM jsonld_contexts WHERE id IN (
                       SELECT id FROM jsonld_contexts WHERE kind = 'Cached'
                        ORDER BY created_at DESC, id DESC OFFSET $1)",
                )
                .bind(crate::store::MAX_CACHED_CONTEXTS as i64)
                .execute(&self.pool)
                .await?;
            }
            Ok(())
        })
    }

    pub fn context_get(&self, id: &str) -> Result<Option<Value>, sqlx::Error> {
        wait(async {
            let row = sqlx::query("SELECT body FROM jsonld_contexts WHERE id = $1")
                .bind(id)
                .fetch_optional(&self.pool)
                .await?;
            Ok(row.map(|r| r.get::<Value, _>(0)))
        })
    }

    pub fn context_delete(&self, id: &str) -> Result<bool, sqlx::Error> {
        wait(async {
            Ok(sqlx::query("DELETE FROM jsonld_contexts WHERE id = $1")
                .bind(id)
                .execute(&self.pool)
                .await?
                .rows_affected()
                == 1)
        })
    }

    pub fn context_list(&self) -> Result<Vec<Value>, sqlx::Error> {
        wait(async {
            let rows = sqlx::query("SELECT body FROM jsonld_contexts ORDER BY id")
                .fetch_all(&self.pool)
                .await?;
            Ok(rows.into_iter().map(|r| r.get::<Value, _>(0)).collect())
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn ops_mask_expands_groups_and_defaults() {
        // absent operations = federationOps (5.2.9)
        let default_mask = ops_mask(&json!({}));
        let fed_mask = ops_mask(&json!({"operations": ["federationOps"]}));
        assert_eq!(default_mask, fed_mask);
        let bit = |op: &str| 1i64 << OPERATIONS.iter().position(|o| *o == op).expect(op);
        assert_ne!(fed_mask & bit("retrieveEntity"), 0);
        assert_eq!(
            fed_mask & bit("createEntity"),
            0,
            "provision op not in federationOps"
        );
        let m = ops_mask(&json!({"operations": ["createEntity", "retrieveOps"]}));
        assert_ne!(m & bit("createEntity"), 0);
        assert_ne!(m & bit("queryEntity"), 0, "retrieveOps expands");
        assert_eq!(m & bit("deleteEntity"), 0);
        assert_eq!(ops_mask(&json!({"operations": ["notARealOp"]})), 0);
    }

    /// Table 4.20-2 defines `redirectionOps` as 23 operations — the provision
    /// and retrieve set PLUS type/attribute introspection, the EntityMap
    /// operations and `retrieveContextSourceIdentity`. A short group writes a
    /// mask that stops those requests being redirected at all.
    #[test]
    fn redirection_ops_expands_to_the_whole_table_4_20_2_group() {
        let m = ops_mask(&json!({"operations": ["redirectionOps"]}));
        let bit = |op: &str| 1i64 << OPERATIONS.iter().position(|o| *o == op).expect(op);
        for op in [
            "createEntity",
            "deleteEntity",
            "purgeEntity",
            "retrieveEntity",
            "queryEntity",
            "retrieveEntityTypes",
            "retrieveEntityTypeDetails",
            "retrieveEntityTypeInfo",
            "retrieveAttrTypes",
            "retrieveAttrTypeDetails",
            "retrieveAttrTypeInfo",
            "retrieveEntityMap",
            "updateEntityMap",
            "deleteEntityMap",
            "createEntityMapQueryEntity",
            "retrieveContextSourceIdentity",
        ] {
            assert_ne!(m & bit(op), 0, "redirectionOps is missing {op}");
        }
        assert_eq!(m.count_ones(), 23, "Table 4.20-2 lists 23 operations");
        // and NOT the members the table leaves out
        for op in [
            "createSubscription",
            "queryBatch",
            "createBatch",
            "retrieveTemporal",
            "createEntityMapQueryTemporal",
        ] {
            assert_eq!(m & bit(op), 0, "{op} is not a redirectionOp");
        }
    }

    /// The bit position IS stored data (`csource_index.ops` is a `bigint`), so
    /// the list can only grow to 63 entries; at 64 the shift silently writes a
    /// wrong mask no migration could tell from a real one.
    #[test]
    fn the_operation_list_still_fits_the_bitmask() {
        assert!(OPERATIONS.len() < 64, "ops bitmask is i64");
        // every name is unique: a duplicate would give one operation two bits
        let mut seen = OPERATIONS.to_vec();
        seen.sort_unstable();
        seen.dedup();
        assert_eq!(seen.len(), OPERATIONS.len(), "duplicate operation name");
    }

    /// The explosion ceiling has to bound the shape its own doc comment names
    /// — a document written through a path with no cardinality validation (a
    /// restored dump, an importer). ONE entity with a huge `propertyNames` is
    /// exactly that shape, and a per-entity check never sees it twice.
    #[test]
    fn the_index_ceiling_bounds_one_entity_too() {
        let names: Vec<String> = (0..MAX_INDEX_ROWS + 500).map(|i| format!("p{i}")).collect();
        let reg = json!({
            "endpoint": "http://cs.example:9090",
            "information": [{"entities": [{"type": "T"}], "propertyNames": names}]
        });
        assert_eq!(index_rows(&reg).len(), MAX_INDEX_ROWS);
        // and across many entities, where it already held
        let many: Vec<Value> = (0..MAX_INDEX_ROWS + 500)
            .map(|i| json!({"id": format!("urn:e:{i}")}))
            .collect();
        let reg = json!({"endpoint": "e", "information": [{"entities": many}]});
        assert_eq!(index_rows(&reg).len(), MAX_INDEX_ROWS);
    }

    #[test]
    fn index_rows_explode_information() {
        // 2 entities × (1 property + 1 relationship) = 4 rows
        let reg = json!({
            "endpoint": "http://cs.example:9090",
            "mode": "exclusive",
            "information": [{
                "entities": [
                    {"id": "urn:a", "type": "T1"},
                    {"idPattern": "urn:.*", "type": "T2"}
                ],
                "propertyNames": ["speed"],
                "relationshipNames": ["isParked"]
            }],
            "expiresAt": "2030-01-01T00:00:00Z"
        });
        let rows = index_rows(&reg);
        assert_eq!(rows.len(), 4);
        assert!(rows
            .iter()
            .all(|r| r["endpoint"] == "http://cs.example:9090"
                && r["mode"] == 3
                && r["expires_at"] == "2030-01-01T00:00:00Z"));
        assert!(rows
            .iter()
            .any(|r| r["entity_id"] == "urn:a" && r["property_name"] == "speed"));
        assert!(rows
            .iter()
            .any(|r| r["id_pattern"] == "urn:.*" && r["relationship_name"] == "isParked"));
        // attribute-less info: one row per entity, both attr columns NULL
        let bare = json!({"endpoint": "e", "information": [{"entities": [{"type": "T"}]}]});
        let rows = index_rows(&bare);
        assert_eq!(rows.len(), 1);
        assert!(rows[0]["property_name"].is_null() && rows[0]["relationship_name"].is_null());
    }
}
