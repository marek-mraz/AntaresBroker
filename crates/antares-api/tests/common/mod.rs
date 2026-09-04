// SPDX-License-Identifier: EUPL-1.2
//! Doubles shared by the integration tests: store drivers, and the network
//! fixtures a two-organization deployment needs — the proxy an organization
//! runs in front of its broker, and the endpoint a notification lands on.
//!
//! One copy: a second hand-written driver would drift from the trait the
//! moment a method is added, and the drift is invisible until a test that
//! was meant to exercise a refusal quietly delegates instead.
#![allow(dead_code)] // every test binary compiles this module, none uses all of it

pub mod net;

use antares_model::{NgsiError, TenantId};
use antares_store::{CurrentStateDriver, Kind};
use serde_json::Value;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::Arc;

/// One delegating driver, misbehaving in whichever single way a test asks
/// for. Everything it does not intercept delegates, so the data it guards is
/// the real store's.
pub struct Double {
    inner: Arc<dyn CurrentStateDriver>,
    fail_next: AtomicUsize,
    delete_on_get: bool,
    racing_write: Option<Value>,
    raced: AtomicBool,
    refuse_registrations: bool,
    refuse_doc: Option<(Kind, String)>,
    refuse_create: Option<(Kind, String)>,
    overloaded: bool,
    unpaged_queries: AtomicUsize,
}

impl Double {
    /// `list` fails its first `n` calls and then behaves. It stands for both
    /// reachable causes at once: the transient connection failure, and the
    /// `TooManyResults` ceiling a tenant crosses and then drops back under.
    pub fn flaky_list(inner: Arc<dyn CurrentStateDriver>, fail_next: usize) -> Self {
        Self {
            inner,
            fail_next: AtomicUsize::new(fail_next),
            delete_on_get: false,
            racing_write: None,
            raced: AtomicBool::new(false),
            refuse_registrations: false,
            refuse_doc: None,
            refuse_create: None,
            overloaded: false,
            unpaged_queries: AtomicUsize::new(0),
        }
    }

    /// Every read answers the way a storage driver answers when its
    /// connection pool ran out of time: the operation was never attempted,
    /// and the detail is the constant the HTTP binding turns into 503 with
    /// `Retry-After`.
    pub fn overloaded(inner: Arc<dyn CurrentStateDriver>) -> Self {
        Self {
            inner,
            fail_next: AtomicUsize::new(0),
            delete_on_get: false,
            racing_write: None,
            raced: AtomicBool::new(false),
            refuse_registrations: false,
            refuse_doc: None,
            refuse_create: None,
            overloaded: true,
            unpaged_queries: AtomicUsize::new(0),
        }
    }

    /// Every `get` answers with the document and then deletes it — the
    /// concurrent DELETE that lands between a handler's read and its write,
    /// scheduled instead of hoped for. A handler that reads, decides, and
    /// then writes without the row lock resurrects what this deleted.
    pub fn deleting_get(inner: Arc<dyn CurrentStateDriver>) -> Self {
        Self {
            inner,
            fail_next: AtomicUsize::new(0),
            delete_on_get: true,
            racing_write: None,
            raced: AtomicBool::new(false),
            refuse_registrations: false,
            refuse_doc: None,
            refuse_create: None,
            overloaded: false,
            unpaged_queries: AtomicUsize::new(0),
        }
    }

    /// One concurrent write, landing in the window a handler leaves between
    /// its read and its write: after a `get` has answered, and before a
    /// conditional delete reaches the store. It replaces the document under
    /// the same id and fires once, whichever entry point comes first. A
    /// handler that decides on what it read cannot see it; one that decides
    /// inside the store cannot miss it.
    pub fn racing_write(inner: Arc<dyn CurrentStateDriver>, replacement: Value) -> Self {
        Self {
            inner,
            fail_next: AtomicUsize::new(0),
            delete_on_get: false,
            racing_write: Some(replacement),
            raced: AtomicBool::new(false),
            refuse_registrations: false,
            refuse_doc: None,
            refuse_create: None,
            overloaded: false,
            unpaged_queries: AtomicUsize::new(0),
        }
    }

    async fn race(&self, tenant: &TenantId, kind: Kind, id: &str) -> Result<(), NgsiError> {
        let Some(doc) = &self.racing_write else {
            return Ok(());
        };
        if kind != Kind::Entity || self.raced.swap(true, Ordering::SeqCst) {
            return Ok(());
        }
        if self.inner.get(tenant, kind, id).await?.is_some() {
            self.inner.upsert(tenant, kind, id, doc.clone()).await?;
        }
        Ok(())
    }

    /// The registration read fails while everything else keeps working —
    /// the shape of a connection lost between two statements, or a backend
    /// that refuses this one query. Entity data still reads, so a handler
    /// that treats the refusal as "no Context Source is registered" answers
    /// a distributed operation with local data alone and calls it complete.
    pub fn refusing_registrations(inner: Arc<dyn CurrentStateDriver>) -> Self {
        Self {
            inner,
            fail_next: AtomicUsize::new(0),
            delete_on_get: false,
            racing_write: None,
            raced: AtomicBool::new(false),
            refuse_registrations: true,
            refuse_doc: None,
            refuse_create: None,
            overloaded: false,
            unpaged_queries: AtomicUsize::new(0),
        }
    }

    /// Reads of ONE document fail while the rest of the store keeps working —
    /// the row a backend refuses, or a document whose stored bytes no longer
    /// parse. A handler that reads the refusal as "there is no such document"
    /// answers a fault as absence.
    pub fn refusing_doc(inner: Arc<dyn CurrentStateDriver>, kind: Kind, id: &str) -> Self {
        Self {
            inner,
            fail_next: AtomicUsize::new(0),
            delete_on_get: false,
            racing_write: None,
            raced: AtomicBool::new(false),
            refuse_registrations: false,
            refuse_doc: Some((kind, id.to_owned())),
            refuse_create: None,
            overloaded: false,
            unpaged_queries: AtomicUsize::new(0),
        }
    }

    /// The INSERT of one document fails while the rest of the store keeps
    /// working — a backend that refuses one write, or loses the connection
    /// between two of them. A caller that discards the result writes nothing
    /// and reports the document as stored.
    pub fn refusing_create(inner: Arc<dyn CurrentStateDriver>, kind: Kind, id: &str) -> Self {
        Self {
            inner,
            fail_next: AtomicUsize::new(0),
            delete_on_get: false,
            racing_write: None,
            raced: AtomicBool::new(false),
            refuse_registrations: false,
            refuse_doc: None,
            refuse_create: Some((kind, id.to_owned())),
            overloaded: false,
            unpaged_queries: AtomicUsize::new(0),
        }
    }

    /// Delegates everything. It still counts the reads below, which is what
    /// a bound is asserted from.
    pub fn passthrough(inner: Arc<dyn CurrentStateDriver>) -> Self {
        Self::flaky_list(inner, 0)
    }

    /// How many `query_entities` calls arrived with no page — reads the store
    /// was asked to answer whole, however many rows that is.
    pub fn unpaged_queries(&self) -> usize {
        self.unpaged_queries.load(Ordering::SeqCst)
    }

    fn refused_create(&self, kind: Kind, id: &str) -> Result<(), NgsiError> {
        if self
            .refuse_create
            .as_ref()
            .is_some_and(|(k, i)| *k == kind && i == id)
        {
            return Err(NgsiError::InternalError(format!("{id} not writable")));
        }
        Ok(())
    }

    /// The connection pool gave up before this call could start.
    fn pool_wall(&self) -> Result<(), NgsiError> {
        if self.overloaded {
            return Err(NgsiError::InternalError(
                antares_model::error::DB_OVERLOADED.into(),
            ));
        }
        Ok(())
    }

    fn refused(&self, kind: Kind, id: &str) -> Result<(), NgsiError> {
        if self
            .refuse_doc
            .as_ref()
            .is_some_and(|(k, i)| *k == kind && i == id)
        {
            return Err(NgsiError::InternalError(format!("{id} unreadable")));
        }
        Ok(())
    }
}

#[async_trait::async_trait]
impl CurrentStateDriver for Double {
    async fn list(&self, tenant: &TenantId, kind: Kind) -> Result<Vec<Value>, NgsiError> {
        self.pool_wall()?;
        if self
            .fail_next
            .fetch_update(Ordering::SeqCst, Ordering::SeqCst, |n| n.checked_sub(1))
            .is_ok()
        {
            return Err(NgsiError::TooManyResults("list refused".into()));
        }
        self.inner.list(tenant, kind).await
    }

    /// The paged read is the one the Postgres arm leaves uncapped, so it
    /// keeps working past the ceiling that refuses `list`.
    async fn list_page(
        &self,
        tenant: &TenantId,
        kind: Kind,
        after: Option<&str>,
        limit: usize,
    ) -> Result<Vec<Value>, NgsiError> {
        self.pool_wall()?;
        self.inner.list_page(tenant, kind, after, limit).await
    }

    /// The windowed read is not refused either: it is bounded by the
    /// client's own limit, so there is nothing for a ceiling to protect.
    async fn list_slice(
        &self,
        tenant: &TenantId,
        kind: Kind,
        offset: usize,
        limit: usize,
    ) -> Result<(Vec<Value>, usize), NgsiError> {
        self.pool_wall()?;
        self.inner.list_slice(tenant, kind, offset, limit).await
    }

    async fn ping(&self) -> Result<(), NgsiError> {
        self.inner.ping().await
    }
    async fn close(&self) {
        self.inner.close().await;
    }
    fn set_change_hook(&self, h: antares_store::ChangeHook) {
        self.inner.set_change_hook(h);
    }
    async fn create(
        &self,
        tenant: &TenantId,
        kind: Kind,
        id: &str,
        doc: Value,
    ) -> Result<bool, NgsiError> {
        self.pool_wall()?;
        self.refused_create(kind, id)?;
        self.inner.create(tenant, kind, id, doc).await
    }
    async fn batch_create(
        &self,
        tenant: &TenantId,
        items: Vec<(String, Value)>,
    ) -> Result<Vec<bool>, NgsiError> {
        self.inner.batch_create(tenant, items).await
    }
    async fn batch_delete(
        &self,
        tenant: &TenantId,
        ids: &[String],
    ) -> Result<Vec<bool>, NgsiError> {
        self.inner.batch_delete(tenant, ids).await
    }
    async fn batch_upsert(
        &self,
        tenant: &TenantId,
        items: Vec<(String, Value)>,
    ) -> Result<Vec<bool>, NgsiError> {
        self.inner.batch_upsert(tenant, items).await
    }
    async fn upsert(
        &self,
        tenant: &TenantId,
        kind: Kind,
        id: &str,
        doc: Value,
    ) -> Result<bool, NgsiError> {
        self.pool_wall()?;
        self.inner.upsert(tenant, kind, id, doc).await
    }
    async fn get(
        &self,
        tenant: &TenantId,
        kind: Kind,
        id: &str,
    ) -> Result<Option<Value>, NgsiError> {
        self.pool_wall()?;
        self.refused(kind, id)?;
        let doc = self.inner.get(tenant, kind, id).await?;
        if self.delete_on_get && doc.is_some() {
            self.inner.delete(tenant, kind, id).await?;
        }
        self.race(tenant, kind, id).await?;
        Ok(doc)
    }
    async fn delete(&self, tenant: &TenantId, kind: Kind, id: &str) -> Result<bool, NgsiError> {
        self.pool_wall()?;
        self.refused(kind, id)?;
        self.inner.delete(tenant, kind, id).await
    }
    async fn delete_entity_if(
        &self,
        tenant: &TenantId,
        id: &str,
        keep: &(dyn for<'v> Fn(&'v Value) -> bool + Sync),
    ) -> Result<bool, NgsiError> {
        self.race(tenant, Kind::Entity, id).await?;
        self.inner.delete_entity_if(tenant, id, keep).await
    }
    async fn matching_registrations(
        &self,
        tenant: &TenantId,
        ids: Option<&[String]>,
        types: Option<&[String]>,
    ) -> Result<Vec<Value>, NgsiError> {
        self.pool_wall()?;
        if self.refuse_registrations {
            return Err(NgsiError::InternalError("registrations unreadable".into()));
        }
        self.inner.matching_registrations(tenant, ids, types).await
    }
    async fn query_entities(
        &self,
        tenant: &TenantId,
        f: &antares_store::filter::EntityFilter<'_>,
    ) -> Result<antares_store::filter::QueryOutcome, NgsiError> {
        self.pool_wall()?;
        if f.page.is_none() {
            self.unpaged_queries.fetch_add(1, Ordering::SeqCst);
        }
        self.inner.query_entities(tenant, f).await
    }
    async fn mutate_boxed<'a>(
        &self,
        tenant: &TenantId,
        kind: Kind,
        id: &str,
        f: antares_store::MutateFn<'a>,
    ) -> Result<Option<Result<(), ()>>, NgsiError> {
        self.inner.mutate_boxed(tenant, kind, id, f).await
    }
    async fn batch_mutate_boxed<'a>(
        &self,
        tenant: &TenantId,
        ids: &[String],
        f: antares_store::BatchMutateFn<'a>,
    ) -> Result<Vec<Option<Result<(), ()>>>, NgsiError> {
        self.inner.batch_mutate_boxed(tenant, ids, f).await
    }
    async fn record_delivery(
        &self,
        tenant: &TenantId,
        kind: Kind,
        id: &str,
        now: &str,
    ) -> Result<Option<antares_store::Delivery>, NgsiError> {
        self.inner.record_delivery(tenant, kind, id, now).await
    }
    async fn tenant_exists(&self, tenant: &TenantId) -> Result<bool, NgsiError> {
        self.inner.tenant_exists(tenant).await
    }
    async fn subscription_tenants(&self) -> Result<Vec<String>, NgsiError> {
        self.inner.subscription_tenants().await
    }
    async fn context_put(
        &self,
        tenant: Option<&TenantId>,
        id: &str,
        doc: Value,
    ) -> Result<(), NgsiError> {
        self.inner.context_put(tenant, id, doc).await
    }
    async fn context_get(
        &self,
        tenant: Option<&TenantId>,
        id: &str,
    ) -> Result<Option<Value>, NgsiError> {
        self.inner.context_get(tenant, id).await
    }
    async fn context_delete(&self, tenant: Option<&TenantId>, id: &str) -> Result<bool, NgsiError> {
        self.inner.context_delete(tenant, id).await
    }
    async fn context_list_meta(&self, tenant: Option<&TenantId>) -> Result<Vec<Value>, NgsiError> {
        self.inner.context_list_meta(tenant).await
    }
    // The trait's PROVIDED methods delegate too. Left out, each one answers
    // the trait's own default — `OperationNotSupported`, an empty inventory,
    // a no-op sweep — under a double whose contract is that everything it
    // does not intercept behaves like the real store. The failure is silent
    // and reads as the broker refusing the operation.
    fn commit_queue(&self) -> Option<(usize, usize)> {
        self.inner.commit_queue()
    }
    fn set_outbox(&self, on: bool) {
        self.inner.set_outbox(on);
    }
    async fn outbox_peek(&self, limit: i64) -> Result<Vec<(i64, String, Value)>, NgsiError> {
        self.inner.outbox_peek(limit).await
    }
    async fn outbox_ack(&self, seqs: &[i64]) -> Result<u64, NgsiError> {
        self.inner.outbox_ack(seqs).await
    }
    async fn outbox_retain(&self, tenant: &TenantId, seqs: &[i64]) -> Result<u64, NgsiError> {
        self.inner.outbox_retain(tenant, seqs).await
    }
    async fn outbox_event(&self, seq: i64, tenant: &TenantId) -> Result<Option<Value>, NgsiError> {
        self.inner.outbox_event(seq, tenant).await
    }
    fn version_info(&self) -> Value {
        self.inner.version_info()
    }
    async fn sweep_expired(&self) -> usize {
        self.inner.sweep_expired().await
    }
    async fn tenant_ids(&self) -> Result<Vec<String>, NgsiError> {
        self.inner.tenant_ids().await
    }
    async fn tenant_stats_one(
        &self,
        tenant: &TenantId,
    ) -> Result<Option<antares_store::TenantStats>, NgsiError> {
        self.inner.tenant_stats_one(tenant).await
    }
    async fn purge_tenant(&self, tenant: &TenantId) -> Result<bool, NgsiError> {
        self.inner.purge_tenant(tenant).await
    }
}
