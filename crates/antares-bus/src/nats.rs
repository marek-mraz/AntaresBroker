//! The JetStream implementation of the bus.
//!
//! One `ANTARES_CHANGES` stream (Interest retention, subjects `changes.>`),
//! one `ANTARES_REGISTRY` stream for registration deltas, one KV bucket
//! (`antares_subscriptions`) for the compiled-subscription mirror.
//!
//! Consumer discipline, asserted not assumed (a lesson from Scorpio):
//! *balanced* work (matcher, temporal recorder) = a shared DURABLE pull
//! consumer — instances joining the same durable load-balance; *broadcast*
//! work (per-instance mirrors) = an EPHEMERAL pull consumer — every instance
//! sees every message. The distinction is explicit in the method you call,
//! and `assert_topology` verifies the server agrees at startup.
//!
//! Delivery is at-least-once engineered to idempotent: publish-side dedup via
//! `Nats-Msg-Id` inside the stream's duplicate window, explicit ack AFTER
//! processing (never Scorpio's PRE_PROCESSING commit-before-work), bounded
//! prefetch.

use crate::{subjects, ChangeEvent};
use async_nats::jetstream::{self, consumer, stream};
use futures_util::StreamExt;

/// Bus failures. String-typed on purpose: callers either retry (drain loop)
/// or die loudly (startup) — nobody branches on the variant.
#[derive(Debug)]
pub struct BusError(pub String);

impl std::fmt::Display for BusError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "bus error: {}", self.0)
    }
}
impl std::error::Error for BusError {}

fn err<E: std::fmt::Display>(e: E) -> BusError {
    BusError(e.to_string())
}

pub const CHANGES_STREAM: &str = "ANTARES_CHANGES";
pub const REGISTRY_STREAM: &str = "ANTARES_REGISTRY";
pub const SUBS_BUCKET: &str = "antares_subscriptions";

/// Bounded prefetch: how many unacked messages one consumer may
/// hold. Any unbounded queue is a 3am page.
pub const MAX_ACK_PENDING: i64 = 256;

pub struct NatsBus {
    js: jetstream::Context,
    client: async_nats::Client,
    /// `Event::Connected` occurrences (the initial connect included; the
    /// getter subtracts it — surfaced on /q/health as `reconnects`).
    reconnects: std::sync::Arc<std::sync::atomic::AtomicU64>,
}

impl NatsBus {
    /// Connect and ensure the streams + KV bucket exist (idempotent).
    pub async fn connect(url: &str) -> Result<Self, BusError> {
        let reconnects = std::sync::Arc::new(std::sync::atomic::AtomicU64::new(0));
        let counter = reconnects.clone();
        let client = async_nats::ConnectOptions::new()
            .event_callback(move |ev| {
                let counter = counter.clone();
                async move {
                    match ev {
                        // fires per successful (re)connect after the initial
                        // ConnectOptions::connect returned
                        async_nats::Event::Connected => {
                            counter.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                            tracing::info!("bus reconnected to NATS");
                        }
                        async_nats::Event::Disconnected => {
                            tracing::warn!("bus lost the NATS connection — reconnecting");
                        }
                        other => tracing::debug!("bus event: {other}"),
                    }
                }
            })
            .connect(url)
            .await
            .map_err(err)?;
        // The change stream carries every tenant's ChangeEvent bodies
        // and MUST stay internal. If the server requires no auth, anything that
        // reaches it can read or forge all-tenant events — warn loudly so an
        // unauthenticated JetStream cluster is not shipped by accident.
        if !client.server_info().auth_required {
            tracing::warn!(
                "connected to a NATS server that requires NO authentication — the \
                 ANTARES_CHANGES stream exposes all tenants' change events; require \
                 nkey/creds/mTLS and network-isolate the JetStream cluster in production"
            );
        }
        let js = jetstream::new(client.clone());
        let bus = Self {
            js,
            client,
            reconnects,
        };
        bus.ensure_streams().await?;
        Ok(bus)
    }

    /// Live connection state for /q/health.
    pub fn connected(&self) -> bool {
        self.client.connection_state() == async_nats::connection::State::Connected
    }

    /// Successful reconnects since startup. `Event::Connected` fires on
    /// EVERY successful connect including the initial one (connector.rs
    /// emits it unconditionally), so the first event is subtracted.
    pub fn reconnects(&self) -> u64 {
        self.reconnects
            .load(std::sync::atomic::Ordering::Relaxed)
            .saturating_sub(1)
    }

    async fn ensure_streams(&self) -> Result<(), BusError> {
        // Production runs replicas=3 on a 3-node JetStream cluster. Stream
        // replication is a CLIENT-side stream setting, so the deployment
        // manifests set ANTARES_NATS_REPLICAS=3; single-node dev/CI keeps 1.
        // Garbage is FATAL like every other config value: a typo silently
        // running replicas=1 on a 3-node cluster is a durability downgrade
        // nobody chose.
        let replicas = replicas_from(std::env::var("ANTARES_NATS_REPLICAS").ok().as_deref())?;
        self.js
            .get_or_create_stream(stream::Config {
                name: CHANGES_STREAM.into(),
                subjects: vec!["changes.>".into()],
                // Interest retention: each durable sees every message; a
                // message dies once every interested consumer acked it.
                // (WorkQueue would forbid multiple consumer groups.)
                retention: stream::RetentionPolicy::Interest,
                duplicate_window: std::time::Duration::from_secs(120),
                num_replicas: replicas,
                ..Default::default()
            })
            .await
            .map_err(err)?;
        self.js
            .get_or_create_stream(stream::Config {
                name: REGISTRY_STREAM.into(),
                subjects: vec!["registry.>".into()],
                // Broadcast deltas for per-instance mirrors: ephemeral
                // consumers carry no interest, so bound by age instead.
                retention: stream::RetentionPolicy::Limits,
                max_age: std::time::Duration::from_secs(600),
                num_replicas: replicas,
                ..Default::default()
            })
            .await
            .map_err(err)?;
        self.js
            .create_key_value(async_nats::jetstream::kv::Config {
                bucket: SUBS_BUCKET.into(),
                num_replicas: replicas,
                ..Default::default()
            })
            .await
            .map_err(err)?;
        Ok(())
    }

    /// Publish one change event with `Nats-Msg-Id` dedup. The id is
    /// the outbox seq so a drain retry after a crash is absorbed by the
    /// stream's duplicate window, not delivered twice.
    pub async fn publish(&self, ev: &ChangeEvent) -> Result<(), BusError> {
        let ev = ev.clone().claim_check(crate::CLAIM_CHECK_BYTES);
        let subject = subjects::change_subject(
            &ev.tenant,
            ev.types.first().map(String::as_str).unwrap_or(""),
            ev.entity_id.as_str(),
        );
        let mut headers = async_nats::HeaderMap::new();
        headers.insert(
            "Nats-Msg-Id",
            format!("{}:{}", ev.tenant.as_str(), ev.seq).as_str(),
        );
        let bytes = serde_json::to_vec(&ev).map_err(err)?;
        // double-ack: await the JetStream publish ack, not just the TCP write
        self.js
            .publish_with_headers(subject, headers, bytes.into())
            .await
            .map_err(err)?
            .await
            .map_err(err)?;
        Ok(())
    }

    /// Publish a registration CUD delta: the full registration document
    /// (or a `{"deleted": id}` tombstone) on the tenant's registry subject.
    /// The tenant is re-validated here because it becomes a subject token.
    pub async fn publish_registry(
        &self,
        tenant: &str,
        delta: &serde_json::Value,
    ) -> Result<(), BusError> {
        let tenant = antares_model::TenantId::new(tenant).map_err(err)?;
        let bytes = serde_json::to_vec(delta).map_err(err)?;
        self.js
            .publish(subjects::registry_subject(&tenant), bytes.into())
            .await
            .map_err(err)?
            .await
            .map_err(err)?;
        Ok(())
    }

    /// BALANCED consumption (matcher, temporal recorder): a shared durable —
    /// instances with the same `durable` name split the work. Explicit-ack,
    /// bounded prefetch; the caller acks AFTER processing.
    pub async fn consume_balanced(
        &self,
        durable: &str,
    ) -> Result<consumer::PullConsumer, BusError> {
        let s = self.js.get_stream(CHANGES_STREAM).await.map_err(err)?;
        s.get_or_create_consumer(
            durable,
            consumer::pull::Config {
                durable_name: Some(durable.into()),
                ack_policy: consumer::AckPolicy::Explicit,
                max_ack_pending: MAX_ACK_PENDING,
                ..Default::default()
            },
        )
        .await
        .map_err(err)
    }

    /// BROADCAST consumption (per-instance registry mirror): an ephemeral
    /// consumer — every instance sees every delta, and the consumer dies
    /// with the instance.
    pub async fn consume_registry_broadcast(&self) -> Result<consumer::PullConsumer, BusError> {
        let s = self.js.get_stream(REGISTRY_STREAM).await.map_err(err)?;
        s.create_consumer(consumer::pull::Config {
            // no durable name = ephemeral = broadcast
            durable_name: None,
            deliver_policy: consumer::DeliverPolicy::New,
            ack_policy: consumer::AckPolicy::Explicit,
            max_ack_pending: MAX_ACK_PENDING,
            ..Default::default()
        })
        .await
        .map_err(err)
    }

    /// The KV bucket holding the compiled-subscription mirror.
    pub async fn subs_kv(&self) -> Result<async_nats::jetstream::kv::Store, BusError> {
        self.js.get_key_value(SUBS_BUCKET).await.map_err(err)
    }

    /// The Scorpio `$[quarkus.uuid}` lesson: assert at startup that every
    /// balanced concern really is a shared durable on the server — a typo'd
    /// durable name would silently turn work-sharing into a private queue
    /// (or broadcast into load-balancing). Fatal on mismatch.
    pub async fn assert_topology(&self, balanced_durables: &[&str]) -> Result<(), BusError> {
        let s = self.js.get_stream(CHANGES_STREAM).await.map_err(err)?;
        for durable in balanced_durables {
            let info = s.consumer_info(durable).await.map_err(|e| {
                BusError(format!(
                    "topology: balanced durable '{durable}' missing on {CHANGES_STREAM}: {e}"
                ))
            })?;
            if info.config.durable_name.as_deref() != Some(*durable) {
                return Err(BusError(format!(
                    "topology: consumer '{durable}' is not durable — balanced work would \
                     not be shared across instances"
                )));
            }
        }
        tracing::info!(
            durables = ?balanced_durables,
            "bus topology asserted: balanced durables present on {CHANGES_STREAM}"
        );
        Ok(())
    }
}

/// Decode one JetStream message into a `ChangeEvent`. `None` = alien bytes —
/// log-and-ack territory for the consumer (redelivering garbage forever is
/// the alternative). Defence in depth: the subject's tenant segment must agree with
/// the event body — consumers re-verify so a subject-mapping bug can never
/// route one tenant's change into another tenant's processing.
/// `ANTARES_NATS_REPLICAS`: absent = 1; present must be a positive integer.
fn replicas_from(v: Option<&str>) -> Result<usize, BusError> {
    match v {
        None => Ok(1),
        Some(raw) => raw
            .trim()
            .parse::<usize>()
            .ok()
            .filter(|n| *n >= 1)
            .ok_or_else(|| {
                BusError(format!(
                    "ANTARES_NATS_REPLICAS must be a positive integer, got {raw:?}"
                ))
            }),
    }
}

pub fn decode(msg: &async_nats::jetstream::Message) -> Option<ChangeEvent> {
    let ev: ChangeEvent = serde_json::from_slice(&msg.payload).ok()?;
    if !subject_tenant_agrees(msg.subject.as_str(), &ev) {
        tracing::error!(
            subject = %msg.subject,
            tenant = %ev.tenant.as_str(),
            "dropping change event: subject tenant segment disagrees with body"
        );
        return None;
    }
    Some(ev)
}

/// The tenant-agreement check, unit-testable: `changes.{tenant}.…` must carry the
/// event's own tenant. Non-`changes` subjects pass (registry deltas carry
/// tenant in the body only).
pub fn subject_tenant_agrees(subject: &str, ev: &ChangeEvent) -> bool {
    let mut parts = subject.split('.');
    if parts.next() != Some("changes") {
        return true;
    }
    parts.next() == Some(ev.tenant.as_str())
}

/// Drive a pull consumer as a message stream. Thin wrapper so wiring code
/// does not import futures/consumer types everywhere.
pub async fn messages(
    consumer: &consumer::PullConsumer,
) -> Result<
    impl futures_util::Stream<
            Item = Result<
                async_nats::jetstream::Message,
                async_nats::error::Error<consumer::pull::MessagesErrorKind>,
            >,
        > + '_,
    BusError,
> {
    consumer.messages().await.map_err(err)
}

/// One decoded registry delta from the broadcast consumer, acked in place.
pub async fn next_delta(
    stream: &mut (impl futures_util::Stream<
        Item = Result<
            async_nats::jetstream::Message,
            async_nats::error::Error<consumer::pull::MessagesErrorKind>,
        >,
    > + Unpin),
) -> Option<serde_json::Value> {
    loop {
        let msg = stream.next().await?.ok()?;
        let parsed = serde_json::from_slice(&msg.payload).ok();
        let _ = msg.ack().await;
        if parsed.is_some() {
            return parsed;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ChangeOp;
    use antares_model::{EntityId, TenantId};

    /// Config fatality: a typo'd replica count must never silently run a
    /// 3-node deployment at replicas=1 — that is a durability downgrade
    /// nobody chose. Absent stays 1; whitespace is tolerated; garbage and
    /// zero name the key in the error.
    #[test]
    fn replica_count_garbage_is_fatal_not_a_silent_default() {
        assert_eq!(replicas_from(None).map_err(|e| e.0), Ok(1));
        assert_eq!(replicas_from(Some("3")).map_err(|e| e.0), Ok(3));
        assert_eq!(replicas_from(Some(" 3 ")).map_err(|e| e.0), Ok(3));
        for bad in ["three", "", "0", "-1", "1.5", "3 nodes"] {
            let err = replicas_from(Some(bad))
                .map(|_| ())
                .expect_err("garbage must be refused")
                .0;
            assert!(
                err.contains("ANTARES_NATS_REPLICAS") && err.contains(bad.trim()),
                "the error must name the key and the value: {err}"
            );
        }
    }

    #[test]
    fn subject_tenant_reverification_drops_mismatches() {
        let ev = ChangeEvent {
            tenant: TenantId::new("acme").expect("tenant"),
            entity_id: EntityId::new("urn:x:1").expect("id"),
            types: vec!["T".into()],
            op: ChangeOp::Create,
            changed_attrs: vec![],
            payload: None,
            prev_payload: None,
            version: 1,
            incarnation: String::new(),
            seq: 1,
            payload_ref: None,
            prev_payload_ref: None,
        };
        assert!(subject_tenant_agrees("changes.acme.aa.bb", &ev));
        assert!(
            !subject_tenant_agrees("changes.other.aa.bb", &ev),
            "a mis-mapped subject must be dropped, not processed"
        );
        assert!(
            subject_tenant_agrees("registry.other", &ev),
            "non-changes subjects carry tenant in the body only"
        );
    }

    /// The check compares whole tokens: a subject whose tenant segment only
    /// starts with, contains or is missing the event's tenant must not pass.
    #[test]
    fn tenant_agreement_is_not_fooled_by_partial_tokens() {
        let ev = ChangeEvent {
            tenant: TenantId::new("acme").expect("tenant"),
            entity_id: EntityId::new("urn:x:1").expect("id"),
            types: vec![],
            op: ChangeOp::Delete,
            changed_attrs: vec![],
            payload: None,
            prev_payload: None,
            version: 1,
            incarnation: String::new(),
            seq: 1,
            payload_ref: None,
            prev_payload_ref: None,
        };
        for subject in [
            "changes.acmeX.aa.bb", // longer token
            "changes.acm.aa.bb",   // prefix of the tenant
            "changes..aa.bb",      // empty tenant segment
            "changes.aa.acme.bb",  // tenant present, wrong position
            "changes",             // no tenant segment at all
            "changes.",
        ] {
            assert!(
                !subject_tenant_agrees(subject, &ev),
                "must not accept {subject:?}"
            );
        }
    }
}
