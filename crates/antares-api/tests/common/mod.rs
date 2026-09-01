// SPDX-License-Identifier: EUPL-1.2
//! Store doubles shared by the integration tests.
//!
//! One copy: a second hand-written driver would drift from the trait the
//! moment a method is added, and the drift is invisible until a test that
//! was meant to exercise a refusal quietly delegates instead.
#![allow(dead_code)]

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
        }
    }

    fn race(&self, tenant: &TenantId, kind: Kind, id: &str) -> Result<(), NgsiError> {
        let Some(doc) = &self.racing_write else {
            return Ok(());
        };
        if kind != Kind::Entity || self.raced.swap(true, Ordering::SeqCst) {
            return Ok(());
        }
        if self.inner.get(tenant, kind, id)?.is_some() {
            self.inner.upsert(tenant, kind, id, doc.clone())?;
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
        }
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

impl CurrentStateDriver for Double {
    fn list(&self, tenant: &TenantId, kind: Kind) -> Result<Vec<Value>, NgsiError> {
        if self
            .fail_next
            .fetch_update(Ordering::SeqCst, Ordering::SeqCst, |n| n.checked_sub(1))
            .is_ok()
        {
            return Err(NgsiError::TooManyResults("list refused".into()));
        }
        self.inner.list(tenant, kind)
    }

    /// The paged read is the one the Postgres arm leaves uncapped, so it
    /// keeps working past the ceiling that refuses `list`.
    fn list_page(
        &self,
        tenant: &TenantId,
        kind: Kind,
        after: Option<&str>,
        limit: usize,
    ) -> Result<Vec<Value>, NgsiError> {
        self.inner.list_page(tenant, kind, after, limit)
    }

    /// The windowed read is not refused either: it is bounded by the
    /// client's own limit, so there is nothing for a ceiling to protect.
    fn list_slice(
        &self,
        tenant: &TenantId,
        kind: Kind,
        offset: usize,
        limit: usize,
    ) -> Result<(Vec<Value>, usize), NgsiError> {
        self.inner.list_slice(tenant, kind, offset, limit)
    }

    fn ping(&self) -> Result<(), NgsiError> {
        self.inner.ping()
    }
    fn close<'a>(&'a self) -> std::pin::Pin<Box<dyn std::future::Future<Output = ()> + Send + 'a>> {
        self.inner.close()
    }
    fn set_change_hook(&self, h: antares_store::ChangeHook) {
        self.inner.set_change_hook(h);
    }
    fn create(
        &self,
        tenant: &TenantId,
        kind: Kind,
        id: &str,
        doc: Value,
    ) -> Result<bool, NgsiError> {
        self.inner.create(tenant, kind, id, doc)
    }
    fn batch_create(
        &self,
        tenant: &TenantId,
        items: Vec<(String, Value)>,
    ) -> Result<Vec<bool>, NgsiError> {
        self.inner.batch_create(tenant, items)
    }
    fn batch_delete(&self, tenant: &TenantId, ids: &[String]) -> Result<Vec<bool>, NgsiError> {
        self.inner.batch_delete(tenant, ids)
    }
    fn batch_upsert(
        &self,
        tenant: &TenantId,
        items: Vec<(String, Value)>,
    ) -> Result<Vec<bool>, NgsiError> {
        self.inner.batch_upsert(tenant, items)
    }
    fn upsert(
        &self,
        tenant: &TenantId,
        kind: Kind,
        id: &str,
        doc: Value,
    ) -> Result<bool, NgsiError> {
        self.inner.upsert(tenant, kind, id, doc)
    }
    fn get(&self, tenant: &TenantId, kind: Kind, id: &str) -> Result<Option<Value>, NgsiError> {
        self.refused(kind, id)?;
        let doc = self.inner.get(tenant, kind, id)?;
        if self.delete_on_get && doc.is_some() {
            self.inner.delete(tenant, kind, id)?;
        }
        self.race(tenant, kind, id)?;
        Ok(doc)
    }
    fn delete(&self, tenant: &TenantId, kind: Kind, id: &str) -> Result<bool, NgsiError> {
        self.refused(kind, id)?;
        self.inner.delete(tenant, kind, id)
    }
    fn delete_entity_if(
        &self,
        tenant: &TenantId,
        id: &str,
        keep: &dyn Fn(&Value) -> bool,
    ) -> Result<bool, NgsiError> {
        self.race(tenant, Kind::Entity, id)?;
        self.inner.delete_entity_if(tenant, id, keep)
    }
    fn matching_registrations(
        &self,
        tenant: &TenantId,
        ids: Option<&[String]>,
        types: Option<&[String]>,
    ) -> Result<Vec<Value>, NgsiError> {
        if self.refuse_registrations {
            return Err(NgsiError::InternalError("registrations unreadable".into()));
        }
        self.inner.matching_registrations(tenant, ids, types)
    }
    fn query_entities(
        &self,
        tenant: &TenantId,
        f: &antares_store::filter::EntityFilter<'_>,
    ) -> Result<antares_store::filter::QueryOutcome, NgsiError> {
        self.inner.query_entities(tenant, f)
    }
    fn mutate_boxed<'a>(
        &self,
        tenant: &TenantId,
        kind: Kind,
        id: &str,
        f: antares_store::MutateFn<'a>,
    ) -> Result<Option<Result<(), ()>>, NgsiError> {
        self.inner.mutate_boxed(tenant, kind, id, f)
    }
    fn batch_mutate_boxed<'a>(
        &self,
        tenant: &TenantId,
        ids: &[String],
        f: antares_store::BatchMutateFn<'a>,
    ) -> Result<Vec<Option<Result<(), ()>>>, NgsiError> {
        self.inner.batch_mutate_boxed(tenant, ids, f)
    }
    fn record_delivery(
        &self,
        tenant: &TenantId,
        kind: Kind,
        id: &str,
        now: &str,
    ) -> Result<Option<antares_store::Delivery>, NgsiError> {
        self.inner.record_delivery(tenant, kind, id, now)
    }
    fn tenant_exists(&self, tenant: &TenantId) -> Result<bool, NgsiError> {
        self.inner.tenant_exists(tenant)
    }
    fn subscription_tenants(&self) -> Result<Vec<String>, NgsiError> {
        self.inner.subscription_tenants()
    }
    fn context_put(&self, id: &str, doc: Value) -> Result<(), NgsiError> {
        self.inner.context_put(id, doc)
    }
    fn context_get(&self, id: &str) -> Result<Option<Value>, NgsiError> {
        self.inner.context_get(id)
    }
    fn context_delete(&self, id: &str) -> Result<bool, NgsiError> {
        self.inner.context_delete(id)
    }
    fn context_list_meta(&self) -> Result<Vec<Value>, NgsiError> {
        self.inner.context_list_meta()
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
    fn outbox_peek(&self, limit: i64) -> Result<Vec<(i64, String, Value)>, NgsiError> {
        self.inner.outbox_peek(limit)
    }
    fn outbox_ack(&self, seqs: &[i64]) -> Result<u64, NgsiError> {
        self.inner.outbox_ack(seqs)
    }
    fn version_info(&self) -> Value {
        self.inner.version_info()
    }
    fn sweep_expired(&self) -> usize {
        self.inner.sweep_expired()
    }
    fn tenant_ids(&self) -> Result<Vec<String>, NgsiError> {
        self.inner.tenant_ids()
    }
    fn tenant_stats_one(
        &self,
        tenant: &TenantId,
    ) -> Result<Option<antares_store::TenantStats>, NgsiError> {
        self.inner.tenant_stats_one(tenant)
    }
    fn purge_tenant(&self, tenant: &TenantId) -> Result<bool, NgsiError> {
        self.inner.purge_tenant(tenant)
    }
}
