// SPDX-License-Identifier: EUPL-1.2
//! An operation that reads, decides and then writes has to decide under the
//! row lock. The driver contract states it of `mutate` — "a get+upsert
//! implementation lets a bookkeeping writeback racing a DELETE resurrect the
//! deleted row" (ADR-0005, ETSI 047_06) — and every operation with that
//! shape inherits the rule, whatever the read was for.
//!
//! The concurrent DELETE is scheduled rather than hoped for: `Double`'s
//! deleting `get` answers with the document and removes it, which is exactly
//! the interleaving a racing client produces and a test cannot otherwise
//! reach on purpose.

mod common;

use antares_api::AppState;
use axum::body::Body;
use axum::http::{Request, StatusCode};
use common::Double;
use serde_json::json;
use std::sync::Arc;
use tower::ServiceExt;

const ID: &str = "urn:ngsi-ld:Vehicle:race1";

async fn send(st: &AppState, req: Request<Body>) -> StatusCode {
    antares_api::router(st.clone())
        .oneshot(req)
        .await
        .expect("response")
        .status()
}

fn json_req(method: &str, uri: &str, body: String) -> Request<Body> {
    Request::builder()
        .method(method)
        .uri(uri)
        .header("Content-Type", "application/json")
        .header("Content-Length", body.len())
        .body(Body::from(body))
        .expect("request")
}

/// 5.6.18 Replace Entity reads the target — to answer 404 before the body is
/// validated, and to carry `createdAt` forward — and then writes. With the
/// write outside the row lock, a DELETE landing in between is undone: the
/// replace puts the row back, and the entity the client deleted is alive
/// again, holding a body it never asked to be stored under that id.
#[tokio::test]
async fn a_replace_does_not_resurrect_an_entity_deleted_under_it() {
    let mut st = AppState::new("antares-test".into());
    let created = send(
        &st,
        json_req(
            "POST",
            "/ngsi-ld/v1/entities",
            format!(r#"{{"id":"{ID}","type":"Vehicle","speed":{{"type":"Property","value":1}}}}"#),
        ),
    )
    .await;
    assert_eq!(created, StatusCode::CREATED);

    st.store = Arc::new(Double::deleting_get(st.store.clone()));
    let replaced = send(
        &st,
        json_req(
            "PUT",
            &format!("/ngsi-ld/v1/entities/{ID}"),
            format!(
                r#"{{"id":"{ID}","type":"Vehicle","speed":{{"type":"Property","value":999}}}}"#
            ),
        ),
    )
    .await;
    assert_eq!(
        replaced,
        StatusCode::NOT_FOUND,
        "the target was deleted before the write: 5.6.18 has no target left"
    );

    let after = send(
        &st,
        Request::builder()
            .uri(format!("/ngsi-ld/v1/entities/{ID}"))
            .body(Body::empty())
            .expect("request"),
    )
    .await;
    assert_eq!(
        after,
        StatusCode::NOT_FOUND,
        "a deleted entity may not come back as the loser of a race"
    );
}

/// 5.6.6.4 Delete Entity: the `?type` selector narrows the target — an
/// Entity of another type "is not known" for this delete. Tested on the
/// document that was read, and then deleted by id, the verdict applies to a
/// document that may no longer be there: a write landing in that window
/// swaps in an Entity the selector excludes, and the delete removes it
/// anyway. Deciding inside the store, under the row lock, is the only place
/// where the document judged and the document deleted are the same one.
#[tokio::test]
async fn a_typed_delete_does_not_remove_an_entity_the_selector_excludes() {
    let mut st = AppState::new("antares-test".into());
    let created = send(
        &st,
        json_req(
            "POST",
            "/ngsi-ld/v1/entities",
            format!(r#"{{"id":"{ID}","type":"Vehicle"}}"#),
        ),
    )
    .await;
    assert_eq!(created, StatusCode::CREATED);

    // what the concurrent write leaves behind: the same id, another type
    let building = json!({
        "id": ID,
        "type": ["https://uri.etsi.org/ngsi-ld/default-context/Building"],
        "createdAt": "2026-01-01T00:00:00.000Z",
        "modifiedAt": "2026-01-01T00:00:00.000Z",
    });
    st.store = Arc::new(Double::racing_write(st.store.clone(), building));

    let deleted = send(
        &st,
        Request::builder()
            .method("DELETE")
            .uri(format!("/ngsi-ld/v1/entities/{ID}?type=Vehicle"))
            .body(Body::empty())
            .expect("request"),
    )
    .await;
    assert_eq!(
        deleted,
        StatusCode::NOT_FOUND,
        "the Entity under that id is a Building: not known for a Vehicle delete"
    );

    let after = send(
        &st,
        Request::builder()
            .uri(format!("/ngsi-ld/v1/entities/{ID}"))
            .body(Body::empty())
            .expect("request"),
    )
    .await;
    assert_eq!(
        after,
        StatusCode::OK,
        "an Entity the selector excludes may not be deleted by it"
    );
}
