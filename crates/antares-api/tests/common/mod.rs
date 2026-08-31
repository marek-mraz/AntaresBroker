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
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

/// One delegating driver, misbehaving in whichever single way a test asks
/// for. Everything it does not intercept delegates, so the data it guards is
/// the real store's.
pub struct Double {
    inner: Arc<dyn CurrentStateDriver>,
    fail_next: AtomicUsize,
    delete_on_get: bool,
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
        }
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
        let doc = self.inner.get(tenant, kind, id)?;
        if self.delete_on_get && doc.is_some() {
            self.inner.delete(tenant, kind, id)?;
        }
        Ok(doc)
    }
    fn delete(&self, tenant: &TenantId, kind: Kind, id: &str) -> Result<bool, NgsiError> {
        self.inner.delete(tenant, kind, id)
    }
    fn matching_registrations(
        &self,
        tenant: &TenantId,
        ids: Option<&[String]>,
        types: Option<&[String]>,
    ) -> Result<Vec<Value>, NgsiError> {
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
}
