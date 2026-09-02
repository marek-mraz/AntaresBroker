// SPDX-License-Identifier: EUPL-1.2
//! 6.3.10 Pagination behaviour: "the Link Target shall be a URI-reference
//! that could be dereferenced by an NGSI-LD Client to retrieve the next page
//! of NGSI-LD Elements". The parameters of the original request reach the
//! handler percent-decoded (4.9: a filter carried in the URI "shall be
//! URI-encoded"), so the link has to encode them again -- a value carrying
//! `&`, `=` or `>` otherwise rewrites the query the link runs, and `>` ends
//! the link-value itself (IETF RFC 8288 clause 3).

use antares_api::AppState;
use axum::body::Body;
use axum::http::{Request, StatusCode};
use http_body_util::BodyExt;
use serde_json::Value;
use tower::ServiceExt;

async fn send(
    st: &AppState,
    method: &str,
    uri: &str,
    body: &str,
) -> (StatusCode, Vec<String>, String) {
    let req = Request::builder()
        .method(method)
        .uri(uri)
        .header("Content-Type", "application/json")
        .header("Content-Length", body.len())
        .body(Body::from(body.to_owned()))
        .expect("request");
    let resp = antares_api::router(st.clone())
        .oneshot(req)
        .await
        .expect("response");
    let status = resp.status();
    let links = resp
        .headers()
        .get_all(axum::http::header::LINK)
        .iter()
        .map(|v| String::from_utf8_lossy(v.as_bytes()).into_owned())
        .collect();
    let bytes = resp.into_body().collect().await.expect("body").to_bytes();
    (status, links, String::from_utf8_lossy(&bytes).into_owned())
}

/// Three entities carrying the given `cat` values, so a `limit=1` page over a
/// query that matches more than one of them has a next page to point at.
async fn seeded(cats: [&str; 3]) -> AppState {
    let mut st = AppState::new("test".into());
    antares_api::wire(&mut st);
    for (n, cat) in (1..=3).zip(cats) {
        let body = format!(
            r#"{{"id":"urn:ngsi-ld:Bldg:{n}","type":"Bldg",
                "cat":{{"type":"Property","value":{cat}}}}}"#
        );
        let (status, _, resp) = send(&st, "POST", "/ngsi-ld/v1/entities", &body).await;
        assert_eq!(status, StatusCode::CREATED, "{resp}");
    }
    st
}

/// The Link Target of the `next` link, without the angle brackets.
fn next_target(links: &[String]) -> String {
    let next = links
        .iter()
        .find(|l| l.contains("rel=\"next\""))
        .unwrap_or_else(|| panic!("no next link in {links:?}"));
    let (open, rest) = next.split_once('<').expect("link-value opens");
    assert_eq!(open, "", "the link-value starts with the target");
    rest.split_once('>')
        .expect("link-value closes")
        .0
        .to_owned()
}

#[tokio::test(flavor = "multi_thread")]
async fn the_next_link_reruns_the_query_it_came_from() {
    let st = seeded([r#""a&b""#; 3]).await;
    // q=cat=="a&b" — the value carries the query-string delimiters
    let uri = "/ngsi-ld/v1/entities?type=Bldg&limit=1&q=cat%3D%3D%22a%26b%22";
    let (status, links, body) = send(&st, "GET", uri, "").await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let first: Value = serde_json::from_str(&body).expect("page");
    assert_eq!(first.as_array().map(Vec::len), Some(1), "{body}");

    let (status, _, body) = send(&st, "GET", &next_target(&links), "").await;
    assert_eq!(
        status,
        StatusCode::OK,
        "the next link is not dereferenceable: {body}"
    );
    let second: Value = serde_json::from_str(&body).expect("page");
    assert_eq!(second.as_array().map(Vec::len), Some(1), "{body}");
    assert_ne!(
        second[0]["id"], first[0]["id"],
        "the next link served page one again"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn a_greater_than_in_a_parameter_does_not_end_the_link_value() {
    // `cat>1` matches two of the three; a link-value truncated at the raw `>`
    // (RFC 8288 clause 3: the target ends at the first `>`) runs `cat`, the
    // existence term, and pages through all three instead
    let st = seeded(["1", "2", "3"]).await;
    let uri = "/ngsi-ld/v1/entities?type=Bldg&limit=1&q=cat%3E1";
    let (status, links, body) = send(&st, "GET", uri, "").await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let first: Value = serde_json::from_str(&body).expect("page");
    assert_eq!(first[0]["id"], "urn:ngsi-ld:Bldg:2", "{body}");

    let (status, _, body) = send(&st, "GET", &next_target(&links), "").await;
    assert_eq!(
        status,
        StatusCode::OK,
        "the next link is not dereferenceable: {body}"
    );
    let second: Value = serde_json::from_str(&body).expect("page");
    assert_eq!(
        second[0]["id"], "urn:ngsi-ld:Bldg:3",
        "the next link ran a different query: {body}"
    );
}
