//! F9 — live-NATS integration (env-gated, same pattern as the live-PG
//! tests): the R10 broadcast-vs-balanced assertion, the §7 claim check on
//! the wire, and Nats-Msg-Id dedup. Skips loudly without
//! ANTARES_TEST_NATS_URL (CI runs a nats:2 -js service).

use antares_bus::nats::NatsBus;
use antares_bus::{ChangeEvent, ChangeOp};
use antares_model::{EntityId, TenantId};
use futures_util::StreamExt;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

macro_rules! require_nats {
    () => {
        match std::env::var("ANTARES_TEST_NATS_URL") {
            Ok(url) => url,
            Err(_) => {
                eprintln!("SKIP: ANTARES_TEST_NATS_URL not set");
                return;
            }
        }
    };
}

fn nonce() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("clock")
        .as_nanos()
}

fn event(tenant: &TenantId, id: &str, seq: i64) -> ChangeEvent {
    ChangeEvent {
        tenant: tenant.clone(),
        entity_id: EntityId::new(id).expect("id"),
        types: vec!["https://x/T".into()],
        op: ChangeOp::Create,
        changed_attrs: vec![],
        payload: Some(serde_json::json!({"id": id, "type": ["https://x/T"]})),
        prev_payload: None,
        version: 1,
        incarnation: "2026-08-05T00:00:00Z".into(),
        seq,
        payload_ref: None,
        prev_payload_ref: None,
    }
}

/// R10: two instances on the SAME durable split the work — each message is
/// delivered to exactly one of them; two ephemeral registry consumers each
/// see EVERY delta. The distinction that Scorpio's `$[quarkus.uuid}` typo
/// silently destroyed, asserted against a live server.
#[tokio::test(flavor = "multi_thread")]
async fn balanced_splits_and_broadcast_duplicates() {
    let url = require_nats!();
    let bus = NatsBus::connect(&url).await.expect("connect");
    let n = nonce();
    let tenant = TenantId::new(&format!("bal{n}")).expect("tenant");

    // two consumers, ONE durable — a shared work queue
    let durable = format!("bal-{n}");
    let c1 = bus.consume_balanced(&durable).await.expect("consumer 1");
    let c2 = bus.consume_balanced(&durable).await.expect("consumer 2");

    const N: usize = 8;
    for i in 0..N {
        bus.publish(&event(&tenant, &format!("urn:bal:{n}:{i}"), i as i64 + 1))
            .await
            .expect("publish");
    }

    // both instances pull; the union must be exactly the N messages, no
    // message seen twice (each counted by its unique entity id)
    let mut seen = std::collections::HashSet::new();
    let mut total = 0usize;
    let deadline = tokio::time::Instant::now() + Duration::from_secs(10);
    let mut s1 = c1.messages().await.expect("stream 1");
    let mut s2 = c2.messages().await.expect("stream 2");
    while total < N && tokio::time::Instant::now() < deadline {
        tokio::select! {
            Some(Ok(m)) = s1.next() => {
                if let Some(ev) = antares_bus::nats::decode(&m) {
                    if ev.tenant.as_str() == tenant.as_str() {
                        assert!(seen.insert(ev.entity_id.as_str().to_owned()),
                            "balanced durable delivered {} twice", ev.entity_id.as_str());
                        total += 1;
                    }
                }
                m.ack().await.expect("ack");
            }
            Some(Ok(m)) = s2.next() => {
                if let Some(ev) = antares_bus::nats::decode(&m) {
                    if ev.tenant.as_str() == tenant.as_str() {
                        assert!(seen.insert(ev.entity_id.as_str().to_owned()),
                            "balanced durable delivered {} twice", ev.entity_id.as_str());
                        total += 1;
                    }
                }
                m.ack().await.expect("ack");
            }
            _ = tokio::time::sleep(Duration::from_millis(100)) => {}
        }
    }
    assert_eq!(total, N, "balanced durable must deliver every message once");

    // broadcast: two EPHEMERAL registry consumers each see the same delta
    let b1 = bus.consume_registry_broadcast().await.expect("bc 1");
    let b2 = bus.consume_registry_broadcast().await.expect("bc 2");
    let delta =
        serde_json::json!({"tenant": tenant.as_str(), "id": format!("urn:reg:{n}"), "doc": {}});
    bus.publish_registry(tenant.as_str(), &delta)
        .await
        .expect("registry publish");
    for (label, c) in [("bc1", b1), ("bc2", b2)] {
        let mut s = c.messages().await.expect("stream");
        let got = tokio::time::timeout(Duration::from_secs(5), async {
            loop {
                let Some(Ok(m)) = s.next().await else {
                    continue;
                };
                let v: serde_json::Value = serde_json::from_slice(&m.payload).expect("json");
                m.ack().await.expect("ack");
                if v["tenant"] == tenant.as_str() {
                    return v;
                }
            }
        })
        .await
        .unwrap_or_else(|_| panic!("{label} never saw the broadcast delta"));
        assert_eq!(got["id"], format!("urn:reg:{n}"));
    }
}

/// §7: a payload over the claim-check threshold travels as a reference, and
/// the whole message stays far under NATS's ~1 MB cap.
#[tokio::test(flavor = "multi_thread")]
async fn claim_check_strips_oversized_payloads_on_the_wire() {
    let url = require_nats!();
    let bus = NatsBus::connect(&url).await.expect("connect");
    let n = nonce();
    let tenant = TenantId::new(&format!("cc{n}")).expect("tenant");
    let durable = format!("cc-{n}");
    let c = bus.consume_balanced(&durable).await.expect("consumer");

    let mut ev = event(&tenant, &format!("urn:cc:{n}"), 1);
    ev.payload = Some(serde_json::Value::String(
        "x".repeat(antares_bus::CLAIM_CHECK_BYTES + 1),
    ));
    bus.publish(&ev).await.expect("publish");

    let mut s = c.messages().await.expect("stream");
    let got = tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            let Some(Ok(m)) = s.next().await else {
                continue;
            };
            let ev = antares_bus::nats::decode(&m);
            m.ack().await.expect("ack");
            if let Some(ev) = ev {
                if ev.tenant.as_str() == tenant.as_str() {
                    return ev;
                }
            }
        }
    })
    .await
    .expect("claim-checked event never arrived");
    assert!(
        got.payload.is_none(),
        "oversized body must not travel inline"
    );
    let r = got.payload_ref.expect("reference set");
    assert_eq!(r.entity_id.as_str(), format!("urn:cc:{n}"));
    assert_eq!(r.version, 1);
}

/// §6.4: a drain retry republishing the same outbox seq is absorbed by the
/// stream's duplicate window — one delivery, not two.
#[tokio::test(flavor = "multi_thread")]
async fn msg_id_dedup_absorbs_republish() {
    let url = require_nats!();
    let bus = NatsBus::connect(&url).await.expect("connect");
    let n = nonce();
    let tenant = TenantId::new(&format!("dd{n}")).expect("tenant");
    let durable = format!("dd-{n}");
    let c = bus.consume_balanced(&durable).await.expect("consumer");

    let ev = event(&tenant, &format!("urn:dd:{n}"), 7);
    bus.publish(&ev).await.expect("publish 1");
    bus.publish(&ev)
        .await
        .expect("publish 2 (same Nats-Msg-Id)");

    let mut s = c.messages().await.expect("stream");
    let mut count = 0;
    let deadline = tokio::time::Instant::now() + Duration::from_secs(4);
    while tokio::time::Instant::now() < deadline {
        tokio::select! {
            Some(Ok(m)) = s.next() => {
                if let Some(got) = antares_bus::nats::decode(&m) {
                    if got.tenant.as_str() == tenant.as_str() {
                        count += 1;
                    }
                }
                m.ack().await.expect("ack");
            }
            _ = tokio::time::sleep(Duration::from_millis(200)) => {}
        }
    }
    assert_eq!(
        count, 1,
        "duplicate-window dedup must swallow the republish"
    );
}
