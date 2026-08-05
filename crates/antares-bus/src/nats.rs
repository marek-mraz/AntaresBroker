//! F1 — the JetStream implementation (§6.4).
//!
//! One `ANTARES_CHANGES` stream (Interest retention, subjects `changes.>`),
//! one `ANTARES_REGISTRY` stream for registration deltas, one KV bucket
//! (`antares_subscriptions`) for the compiled-subscription mirror (F4).
//!
//! Consumer discipline, asserted not assumed (F7, the R10 lesson):
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

/// Bounded prefetch (§6.4/§2.1): how many unacked messages one consumer may
/// hold. Any unbounded queue is a 3am page.
pub const MAX_ACK_PENDING: i64 = 256;

pub struct NatsBus {
    js: jetstream::Context,
}

impl NatsBus {
    /// Connect and ensure the streams + KV bucket exist (idempotent).
    pub async fn connect(url: &str) -> Result<Self, BusError> {
        let client = async_nats::connect(url).await.map_err(err)?;
        let js = jetstream::new(client);
        let bus = Self { js };
        bus.ensure_streams().await?;
        Ok(bus)
    }

    async fn ensure_streams(&self) -> Result<(), BusError> {
        self.js
            .get_or_create_stream(stream::Config {
                name: CHANGES_STREAM.into(),
                subjects: vec!["changes.>".into()],
                // Interest retention: each durable sees every message; a
                // message dies once every interested consumer acked it.
                // (WorkQueue would forbid multiple consumer groups, §6.4.)
                retention: stream::RetentionPolicy::Interest,
                duplicate_window: std::time::Duration::from_secs(120),
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
                ..Default::default()
            })
            .await
            .map_err(err)?;
        self.js
            .create_key_value(async_nats::jetstream::kv::Config {
                bucket: SUBS_BUCKET.into(),
                ..Default::default()
            })
            .await
            .map_err(err)?;
        Ok(())
    }

    /// Publish one change event with `Nats-Msg-Id` dedup (§6.4). The id is
    /// the outbox seq (F3) so a drain retry after a crash is absorbed by the
    /// stream's duplicate window, not delivered twice.
    pub async fn publish(&self, ev: &ChangeEvent) -> Result<(), BusError> {
        let ev = ev.clone().claim_check(crate::CLAIM_CHECK_BYTES);
        let subject = subjects::change_subject(
            ev.tenant.as_str(),
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

    /// Publish a registration CUD delta (F5): the full registration document
    /// (or a `{"deleted": id}` tombstone) on the tenant's registry subject.
    pub async fn publish_registry(
        &self,
        tenant: &str,
        delta: &serde_json::Value,
    ) -> Result<(), BusError> {
        let bytes = serde_json::to_vec(delta).map_err(err)?;
        self.js
            .publish(subjects::registry_subject(tenant), bytes.into())
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

    /// The KV bucket holding the compiled-subscription mirror (F4).
    pub async fn subs_kv(&self) -> Result<async_nats::jetstream::kv::Store, BusError> {
        self.js.get_key_value(SUBS_BUCKET).await.map_err(err)
    }

    /// F7 (the R10 `$[quarkus.uuid}` lesson): assert at startup that every
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
                     not be shared across instances (R10 class)"
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
/// the alternative).
pub fn decode(msg: &async_nats::jetstream::Message) -> Option<ChangeEvent> {
    serde_json::from_slice(&msg.payload).ok()
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
