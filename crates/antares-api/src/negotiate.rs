// SPDX-License-Identifier: EUPL-1.2
//! HTTP negotiation (CIM 009 6.3.4/6.3.5/6.3.6): content types, Accept,
//! Link-header @context resolution, response building.

use antares_jsonld::{Context, Loader, CORE_CONTEXT};
use antares_model::{NgsiError, TenantId};
use axum::http::{header, HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use serde_json::{Map, Value};
use std::sync::Arc;

pub const JSONLD_CONTEXT_REL: &str = "http://www.w3.org/ns/json-ld#context";

/// Query-string extractor that drops empty-valued parameters — the Robot
/// suite's keywords frequently send `datasetId=`/`options=` as empty strings
/// meaning "absent".
pub struct CleanParams(pub std::collections::HashMap<String, String>);

impl<S: Send + Sync> axum::extract::FromRequestParts<S> for CleanParams {
    type Rejection = std::convert::Infallible;

    async fn from_request_parts(
        parts: &mut axum::http::request::Parts,
        _state: &S,
    ) -> Result<Self, Self::Rejection> {
        let raw = parts.uri.query().unwrap_or("");
        let mut map = std::collections::HashMap::new();
        for pair in raw.split('&') {
            if pair.is_empty() {
                continue;
            }
            let (k, v) = pair.split_once('=').unwrap_or((pair, ""));
            let dec = |s: &str| percent_decode(s.replace('+', " ").as_bytes());
            let (k, v) = (dec(k), dec(v));
            if !v.is_empty() {
                map.insert(k, v);
            }
        }
        Ok(Self(map))
    }
}

pub(crate) use antares_ql::percent_decode;

/// Handler-level error: an NGSI-LD ProblemDetails or a bare status (6.3.4).
#[derive(Debug)]
pub enum ApiError {
    Ngsi(NgsiError),
    Bare(StatusCode),
    /// 6.3.4: 406 whose body lists the available representations.
    NotAcceptable(&'static [&'static str]),
}

impl From<NgsiError> for ApiError {
    fn from(e: NgsiError) -> Self {
        Self::Ngsi(e)
    }
}

/// 6.3.3 Reporting errors: Content-Type application/json, HTTP status per
/// Table 6.3.2-1, payload = the RFC 7807 object with the 5.5.3 terms.
impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        match self {
            Self::Ngsi(e) => {
                let pd = e.to_problem_details();
                (
                    StatusCode::from_u16(pd.status).unwrap_or(StatusCode::INTERNAL_SERVER_ERROR),
                    [(header::CONTENT_TYPE, "application/json")],
                    axum::Json(serde_json::json!({
                        "type": pd.r#type,
                        "title": pd.title,
                        "status": pd.status,
                        "detail": pd.detail,
                    })),
                )
                    .into_response()
            }
            Self::Bare(code) => code.into_response(),
            // 6.3.4: "the body of the message shall contain the list of the
            // available representations of the resources"
            Self::NotAcceptable(available) => (
                StatusCode::NOT_ACCEPTABLE,
                [(header::CONTENT_TYPE, "application/json")],
                axum::Json(serde_json::json!({
                    "availableRepresentations": available,
                })),
            )
                .into_response(),
        }
    }
}

pub type ApiResult<T> = Result<T, ApiError>;

/// Tenant from the NGSILD-Tenant header (6.3.14).
pub fn tenant_from(headers: &HeaderMap) -> ApiResult<TenantId> {
    // NGSILD-Tenant carries one value (6.3.14), so it is not a list-type
    // field: repeated field lines cannot be joined (RFC 9110 clause 5.3) and
    // silently taking the first would let a second value decide nothing while
    // looking like it should.
    if headers.get_all("NGSILD-Tenant").iter().count() > 1 {
        return Err(NgsiError::BadRequestData("repeated NGSILD-Tenant".into()).into());
    }
    match headers.get("NGSILD-Tenant") {
        None => Ok(TenantId::default()),
        Some(v) => {
            let raw = v
                .to_str()
                .map_err(|_| NgsiError::BadRequestData("non-ASCII NGSILD-Tenant".into()))?;
            Ok(TenantId::new(raw)?)
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Accept {
    Json,
    LdJson,
    GeoJson,
}

/// One pass of RFC 9110 clause 5.3.2 over the representations 6.3.4 offers
/// for this operation. A media type takes its weight from the MOST SPECIFIC
/// range that matches it, so `application/json;q=0, */*` refuses json and
/// still offers the rest; `q=0` removes a representation from the offered set
/// rather than merely ranking it last.
fn negotiate(
    headers: &HeaderMap,
    offers: &[(&str, Accept)],
    available: &'static [&'static str],
) -> ApiResult<Accept> {
    if !headers.contains_key(header::ACCEPT) {
        return Ok(Accept::Json);
    }
    // Accept is a list-type field, so its members may arrive split over any
    // number of field lines (RFC 9110 clause 5.3) — reading only the first
    // one turned a legal request into a 406.
    let ranges: Vec<(String, f32)> = headers
        .get_all(header::ACCEPT)
        .iter()
        .filter_map(|v| v.to_str().ok())
        .flat_map(|v| v.split(','))
        .filter_map(|part| {
            let mut segs = part.split(';');
            let mt = segs.next()?.trim().to_ascii_lowercase();
            if mt.is_empty() {
                return None;
            }
            let mut q = 1.0f32;
            for p in segs {
                if let Some(v) = p.trim().strip_prefix("q=") {
                    // A malformed or non-finite weight is not one of the HTTP
                    // Accept processing rules, so it must not decide the
                    // outcome — the range keeps the default weight.
                    q = v
                        .parse()
                        .ok()
                        .filter(|f: &f32| f.is_finite())
                        .unwrap_or(1.0);
                }
            }
            Some((mt, q))
        })
        .collect();
    // 6.3.4: "the order of the list above is significant … the first one of
    // the list shall be selected, unless amended by the HTTP Accept header
    // processing rules, e.g. the presence of a q parameter". The weight
    // decides first; the offer order is the tie-break, never the order the
    // client happened to write its tokens in.
    let mut best: Option<(f32, Accept)> = None;
    for (mt, kind) in offers {
        let mut matched: Option<(u8, f32)> = None;
        for (range, q) in &ranges {
            let spec = match range.as_str() {
                r if r == *mt => 2u8,
                "application/*" => 1,
                "*/*" => 0,
                _ => continue,
            };
            let better = match matched {
                None => true,
                Some((s, mq)) => spec > s || (spec == s && *q > mq),
            };
            if better {
                matched = Some((spec, *q));
            }
        }
        let Some((_, q)) = matched.filter(|(_, q)| *q > 0.0) else {
            continue;
        };
        if match best {
            None => true,
            Some((bq, _)) => q > bq,
        } {
            best = Some((q, *kind));
        }
    }
    match best {
        Some((_, kind)) => Ok(kind),
        None => Err(ApiError::NotAcceptable(available)),
    }
}

/// Accept negotiation (6.3.4): json, ld+json, geo+json, */*; 406 otherwise.
/// Absent Accept ⇒ application/json. geo+json is only valid on
/// Retrieve/Query Entities (6.3.15) — everywhere else it is a 406.
pub fn parse_accept_geo(headers: &HeaderMap) -> ApiResult<Accept> {
    negotiate(
        headers,
        &[
            ("application/json", Accept::Json),
            ("application/ld+json", Accept::LdJson),
            ("application/geo+json", Accept::GeoJson),
        ],
        &[
            "application/json",
            "application/ld+json",
            "application/geo+json",
        ],
    )
}

/// Accept negotiation for every operation that is NOT Retrieve/Query
/// Entities: geo+json is not among the representations offered (6.3.15). It
/// is left out of the offered set rather than negotiated and then refused, so
/// a client that weights geo+json highest but also accepts ld+json is served
/// ld+json instead of a 406.
pub fn parse_accept(headers: &HeaderMap) -> ApiResult<Accept> {
    negotiate(
        headers,
        &[
            ("application/json", Accept::Json),
            ("application/ld+json", Accept::LdJson),
        ],
        &["application/json", "application/ld+json"],
    )
}

/// 6.3.6: "Prefer: body=json" on a GeoJSON response — the @context is
/// conveyed only by the Link header and omitted from the payload body.
pub fn prefer_body_json(headers: &HeaderMap) -> bool {
    // RFC 9110 clause 5.3: repeated field lines carry the same meaning as one
    // comma-separated list, so every Prefer line is searched.
    headers
        .get_all("Prefer")
        .iter()
        .filter_map(|v| v.to_str().ok())
        .flat_map(|p| p.split(','))
        .any(|t| t.trim().eq_ignore_ascii_case("body=json"))
}

/// 6.3.6: build a payload-carrying response honouring Prefer on GeoJSON —
/// body=json keeps the @context out of the body (Link header only);
/// omitted / body=ld+json embeds it (the respond() default).
pub fn respond_prefer(
    status: StatusCode,
    payload: Value,
    ctx: &Context,
    accept: Accept,
    tenant: &TenantId,
    headers: &HeaderMap,
) -> Response {
    if accept == Accept::GeoJson && prefer_body_json(headers) {
        let mut resp = (
            status,
            [
                (header::CONTENT_TYPE, "application/geo+json".to_owned()),
                (header::LINK, link_header_value(ctx)),
            ],
            ordered_vec(&payload),
        )
            .into_response();
        echo_tenant(tenant, &mut resp);
        return resp;
    }
    respond(status, payload, ctx, accept, tenant)
}

/// Content-Type of the request (media type only, parameters dropped).
pub fn content_type(headers: &HeaderMap) -> String {
    headers
        .get(header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("")
        .split(';')
        .next()
        .unwrap_or("")
        .trim()
        .to_ascii_lowercase()
}

/// Extract the JSON-LD context URL from Link headers (6.3.5).
pub fn link_context(headers: &HeaderMap) -> Option<String> {
    for link in headers.get_all(header::LINK) {
        let Ok(s) = link.to_str() else { continue };
        for part in s.split(',') {
            let part = part.trim();
            if !part.contains(JSONLD_CONTEXT_REL) {
                continue;
            }
            if let Some(url) = part.split(';').next() {
                let url = url.trim();
                if url.starts_with('<') && url.ends_with('>') {
                    return Some(url[1..url.len() - 1].to_owned());
                }
            }
        }
    }
    None
}

/// The media types a request body may carry per endpoint class.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BodyKind {
    /// POST/PUT and non-merge PATCH: json | ld+json
    Standard,
    /// PATCH accepting RFC 7396: json | ld+json | merge-patch+json
    MergePatch,
}

pub struct ParsedBody {
    pub value: Value,
    pub ctx: Arc<Context>,
}

/// Parse a request body per the 6.3.5 @context rules.
pub async fn parse_body(
    loader: &Loader,
    headers: &HeaderMap,
    bytes: &[u8],
    kind: BodyKind,
) -> ApiResult<ParsedBody> {
    let ct = content_type(headers);
    let ld = match ct.as_str() {
        "application/json" => false,
        "application/ld+json" => true,
        "application/merge-patch+json" if kind == BodyKind::MergePatch => false,
        // absent Content-Type: parse as JSON — a malformed body then reports
        // InvalidRequest 400 rather than a bare 415 (039_05)
        "" if !headers.contains_key(header::CONTENT_TYPE) => false,
        _ => return Err(ApiError::Bare(StatusCode::UNSUPPORTED_MEDIA_TYPE)),
    };
    if bytes.is_empty() {
        return Err(NgsiError::InvalidRequest("empty request body".into()).into());
    }
    // 4.6.1 Supported text encodings: JSON content is UTF-8; serde_json
    // rejects any non-UTF-8 byte sequence here, so a non-UTF-8 body fails
    // as InvalidRequest 400 (and all broker output is serde-emitted UTF-8).
    // 4.6.4 Supported Content: values pass through this parse and every
    // later stage verbatim — no sanitization or escaping of < > " ' = ; ( ),
    // "implementations shall preserve the representation of the content".
    let value: Value = serde_json::from_slice(bytes)
        .map_err(|e| NgsiError::InvalidRequest(format!("request body is not valid JSON: {e}")))?;
    // Every parse_body consumer takes a single JSON object (entities, fragments,
    // subscriptions, …; batch arrays go through parse_batch) — a non-object here
    // is a malformed request, not bad data (001_02_02).
    if !value.is_object() {
        return Err(NgsiError::InvalidRequest("request body must be a JSON object".into()).into());
    }

    let link = link_context(headers);
    let ctx = if ld {
        if link.is_some() {
            return Err(NgsiError::BadRequestData(
                "application/ld+json request must not also carry a Link @context (6.3.5)".into(),
            )
            .into());
        }
        let user_ctx = body_context_member(&value).ok_or_else(|| {
            NgsiError::BadRequestData(
                "application/ld+json request must carry an @context member (6.3.5)".into(),
            )
        })?;
        loader
            .resolve_for(&tenant_from(headers)?, &user_ctx)
            .await?
    } else {
        if body_has_context(&value) {
            return Err(NgsiError::BadRequestData(
                "application/json request must not carry an @context member (6.3.5)".into(),
            )
            .into());
        }
        match link {
            Some(url) => {
                loader
                    .resolve_for(&tenant_from(headers)?, &Value::String(url))
                    .await?
            }
            // 5.5.5 Default @context assignment: input with no @context gets
            // at minimum the Core @context (no default user @context is
            // configured; core terms always take precedence).
            None => loader.core(),
        }
    };
    Ok(ParsedBody { value, ctx })
}

fn body_has_context(v: &Value) -> bool {
    match v {
        Value::Object(o) => o.contains_key("@context"),
        Value::Array(a) => a.iter().any(body_has_context),
        _ => false,
    }
}

/// The @context member for a single-document body.
fn body_context_member(v: &Value) -> Option<Value> {
    v.as_object().and_then(|o| o.get("@context")).cloned()
}

/// Context for GET/DELETE requests: Link header or core (6.3.5; the
/// no-@context fallback to the Core @context is 5.5.5).
pub async fn request_context(loader: &Loader, headers: &HeaderMap) -> ApiResult<Arc<Context>> {
    match link_context(headers) {
        // 5.5.10: the Tenant bounds what the operation may see, and a locally
        // stored @context (5.13.1) is information related to the Tenant that
        // stored it — so the URL resolves only for that Tenant.
        Some(url) => Ok(loader
            .resolve_for(&tenant_from(headers)?, &Value::String(url))
            .await?),
        None => Ok(loader.core()),
    }
}

/// Reject unknown query parameters with 400 InvalidRequest (6.3.20).
pub fn check_params(
    params: &std::collections::HashMap<String, String>,
    allowed: &[&str],
) -> ApiResult<()> {
    for k in params.keys() {
        if !allowed.contains(&k.as_str()) {
            return Err(NgsiError::InvalidRequest(format!("unknown query parameter {k:?}")).into());
        }
    }
    Ok(())
}

/// The context URL to advertise in a response Link header.
pub fn context_link_url(ctx: &Context) -> String {
    match &ctx.source {
        Value::String(url) => url.clone(),
        Value::Array(items) => match items.as_slice() {
            [Value::String(url)] => url.clone(),
            _ => CORE_CONTEXT.to_owned(),
        },
        _ => CORE_CONTEXT.to_owned(),
    }
}

pub(crate) fn link_header_value(ctx: &Context) -> String {
    format!(
        "<{}>; rel=\"{JSONLD_CONTEXT_REL}\"; type=\"application/ld+json\"",
        context_link_url(ctx)
    )
}

/// Build a payload-carrying NGSI-LD response (6.3.6). Egress key order is
/// `antares_model::ordered_vec`, shared with the notification bindings.
pub(crate) use antares_model::{ordered_vec, SpecOrder};

pub fn respond(
    status: StatusCode,
    payload: Value,
    ctx: &Context,
    accept: Accept,
    tenant: &TenantId,
) -> Response {
    let mut resp = match accept {
        Accept::Json => {
            let mut r = (
                status,
                [
                    (header::CONTENT_TYPE, "application/json".to_owned()),
                    (header::LINK, link_header_value(ctx)),
                ],
                ordered_vec(&payload),
            )
                .into_response();
            r.headers_mut().remove(header::CONTENT_LENGTH);
            r
        }
        Accept::LdJson => {
            let with_ctx = inject_context(payload, ctx);
            (
                status,
                [(header::CONTENT_TYPE, "application/ld+json".to_owned())],
                ordered_vec(&with_ctx),
            )
                .into_response()
        }
        Accept::GeoJson => {
            // 6.3.15: GeoJSON bodies carry the @context at top level
            let with_ctx = match payload {
                Value::Object(mut o) => {
                    o.insert("@context".into(), served_context(ctx));
                    Value::Object(o)
                }
                other => other,
            };
            (
                status,
                [
                    (header::CONTENT_TYPE, "application/geo+json".to_owned()),
                    (header::LINK, link_header_value(ctx)),
                ],
                ordered_vec(&with_ctx),
            )
                .into_response()
        }
    };
    echo_tenant(tenant, &mut resp);
    resp
}

/// 6.3.13 `NGSILD-Results-Count` + the 6.3.9 pagination `Link` headers, on
/// every paged list the API serves. A header that will not parse is dropped
/// rather than failing the response: the page itself is still the answer.
pub(crate) fn attach_paging(resp: &mut Response, count_hdr: Option<usize>, links: &[String]) {
    if let Some(total) = count_hdr {
        if let Ok(v) = total.to_string().parse() {
            resp.headers_mut().insert("NGSILD-Results-Count", v);
        }
    }
    for l in links {
        if let Ok(v) = l.parse() {
            resp.headers_mut().append(axum::http::header::LINK, v);
        }
    }
}

/// 6.3.11 `options=sysAttrs`: does this request ask for the system-generated
/// Temporal Properties (4.8) to be shown? Read the same way wherever the
/// answer is not already carried by a `Repr`.
pub(crate) fn sys_attrs_asked(params: &std::collections::HashMap<String, String>) -> bool {
    params
        .get("options")
        .is_some_and(|o| o.split(',').any(|s| s.trim() == "sysAttrs"))
}

/// Build a list response that STREAMS entity-by-entity (J5; the J3/J11c
/// lesson: the serialized page must never exist as one contiguous buffer).
/// Json and LdJson only — GeoJSON wraps a FeatureCollection object and takes
/// the buffered `respond` path.
pub fn respond_list(
    status: StatusCode,
    docs: Vec<Value>,
    ctx: &Context,
    accept: Accept,
    tenant: &TenantId,
) -> Response {
    if accept == Accept::GeoJson {
        return respond(status, Value::Array(docs), ctx, accept, tenant);
    }
    let ld_ctx = (accept == Accept::LdJson).then(|| served_context(ctx));
    let content_type = match accept {
        Accept::LdJson => "application/ld+json",
        _ => "application/json",
    };
    let chunks = std::iter::once(axum::body::Bytes::from_static(b"["))
        .chain(docs.into_iter().enumerate().map(move |(i, doc)| {
            let doc = match (&ld_ctx, doc) {
                (Some(ctx_val), Value::Object(mut o)) => {
                    o.insert("@context".into(), ctx_val.clone());
                    Value::Object(o)
                }
                (_, other) => other,
            };
            let mut buf = if i == 0 { Vec::new() } else { vec![b','] };
            // serializing a Value into a Vec cannot fail
            let _ = serde_json::to_writer(&mut buf, &SpecOrder(&doc));
            axum::body::Bytes::from(buf)
        }))
        .chain(std::iter::once(axum::body::Bytes::from_static(b"]")));
    let body = axum::body::Body::from_stream(futures_util::stream::iter(
        chunks.map(Ok::<_, std::convert::Infallible>),
    ));
    let mut resp = (
        status,
        [
            (header::CONTENT_TYPE, content_type.to_owned()),
            (header::LINK, link_header_value(ctx)),
        ],
        body,
    )
        .into_response();
    if accept == Accept::LdJson {
        resp.headers_mut().remove(header::LINK);
    }
    echo_tenant(tenant, &mut resp);
    resp
}

/// 5.2.3: the @context member served on pure JSON-LD bodies. The sentence
/// "containing a user @context where present, and the core @context shall be
/// included" reads as [user, core] — but the ENTIRE ETSI validation
/// ecosystem (68 official expectation files, strict-compared; the suite is
/// validated against Scorpio/Stellio) pins the user context ALONE, treating
/// the core as implicit per 4.4. Antares follows the ecosystem reading;
/// the clause wording itself is ambiguous.
pub(crate) fn served_context(ctx: &Context) -> Value {
    if ctx.source.is_null() {
        Value::String(CORE_CONTEXT.to_owned())
    } else {
        ctx.source.clone()
    }
}

pub(crate) fn inject_context(payload: Value, ctx: &Context) -> Value {
    let ctx_val = served_context(ctx);
    match payload {
        Value::Object(mut o) => {
            o.insert("@context".into(), ctx_val);
            Value::Object(o)
        }
        Value::Array(items) => Value::Array(
            items
                .into_iter()
                .map(|i| match i {
                    Value::Object(mut o) => {
                        o.insert("@context".into(), ctx_val.clone());
                        Value::Object(o)
                    }
                    other => other,
                })
                .collect(),
        ),
        other => other,
    }
}

/// 6.3.14: echo NGSILD-Tenant on responses when non-default.
pub fn echo_tenant(tenant: &TenantId, resp: &mut Response) {
    if tenant.as_str() != TenantId::DEFAULT {
        if let Ok(v) = tenant.as_str().parse() {
            resp.headers_mut().insert("NGSILD-Tenant", v);
        }
    }
}

/// 201 Created with Location header.
pub fn created(location: String, tenant: &TenantId) -> Response {
    let mut resp = (StatusCode::CREATED, [(header::LOCATION, location)]).into_response();
    echo_tenant(tenant, &mut resp);
    resp
}

pub fn no_content(tenant: &TenantId) -> Response {
    let mut resp = StatusCode::NO_CONTENT.into_response();
    echo_tenant(tenant, &mut resp);
    resp
}

/// Multi-status (batch ops) — always application/json.
pub fn multi_status(payload: Value, tenant: &TenantId) -> Response {
    let mut resp = (
        StatusCode::MULTI_STATUS,
        [(header::CONTENT_TYPE, "application/json")],
        axum::Json(payload),
    )
        .into_response();
    echo_tenant(tenant, &mut resp);
    resp
}

/// ProblemDetails value for batch error entries.
pub fn problem_value(e: &NgsiError) -> Value {
    let pd = e.to_problem_details();
    serde_json::json!({
        "type": pd.r#type,
        "title": pd.title,
        "status": pd.status,
        "detail": pd.detail,
    })
}

/// Strip a possibly-present @context member (consumed during parsing).
pub fn without_context(v: &Value) -> Map<String, Value> {
    let mut o = v.as_object().cloned().unwrap_or_default();
    o.remove("@context");
    o
}

#[cfg(test)]
mod clause_5_5_3 {
    use super::*;

    /// 5.5.5 Default @context assignment: "If the input provided by an API
    /// client does not include any @context, then the implementation shall
    /// at minimum assign the Core @context" — core terms map, non-core
    /// terms fall to the default vocab, and no user context is invented.
    #[tokio::test]
    async fn clause_5_5_5_no_context_input_gets_the_core_context() {
        let loader = antares_jsonld::Loader::new();
        let mut h = HeaderMap::new();
        h.insert(
            header::CONTENT_TYPE,
            axum::http::HeaderValue::from_static("application/json"),
        );
        let parsed = parse_body(
            &loader,
            &h,
            br#"{"id":"urn:x","type":"T"}"#,
            BodyKind::Standard,
        )
        .await
        .expect("no-context body parses under the core context");
        assert_eq!(
            parsed.ctx.expand_key("location"),
            "https://uri.etsi.org/ngsi-ld/location",
            "core term mapped by the assigned Core @context"
        );
        assert_eq!(
            parsed.ctx.expand_key("speed"),
            "https://uri.etsi.org/ngsi-ld/default-context/speed",
            "non-core term falls to the default vocabulary"
        );
        assert_eq!(
            parsed.ctx.source,
            Value::String(antares_jsonld::CORE_CONTEXT.to_owned()),
            "the assigned context is exactly the Core @context — no user \
             context is invented"
        );
        // GET/DELETE requests take the same fallback
        let ctx = request_context(&loader, &HeaderMap::new())
            .await
            .expect("no Link header");
        assert_eq!(
            ctx.expand_key("observedAt"),
            "https://uri.etsi.org/ngsi-ld/observedAt"
        );
    }

    /// 6.3.6: geo+json + "Prefer: body=json" → Link header only, @context
    /// omitted from the body; without the preference the body embeds it.
    #[tokio::test]
    async fn clause_6_3_6_prefer_body_json_omits_geojson_context() {
        let loader = antares_jsonld::Loader::new();
        let ctx = loader.core();
        let tenant = TenantId::default();
        let payload = serde_json::json!({"type": "FeatureCollection", "features": []});

        let mut h = HeaderMap::new();
        h.insert("Prefer", axum::http::HeaderValue::from_static("body=json"));
        let resp = respond_prefer(
            StatusCode::OK,
            payload.clone(),
            &ctx,
            Accept::GeoJson,
            &tenant,
            &h,
        );
        assert!(resp.headers().get(header::LINK).is_some());
        let bytes = axum::body::to_bytes(resp.into_body(), usize::MAX)
            .await
            .expect("body");
        let doc: Value = serde_json::from_slice(&bytes).expect("json");
        assert!(doc.get("@context").is_none(), "{doc}");
        assert_eq!(doc["type"], "FeatureCollection");

        // no preference → the body embeds the @context (6.3.15)
        let resp = respond_prefer(
            StatusCode::OK,
            payload,
            &ctx,
            Accept::GeoJson,
            &tenant,
            &HeaderMap::new(),
        );
        let bytes = axum::body::to_bytes(resp.into_body(), usize::MAX)
            .await
            .expect("body");
        let doc: Value = serde_json::from_slice(&bytes).expect("json");
        assert!(doc.get("@context").is_some(), "{doc}");
    }

    /// 6.3.5: "No mixes are allowed" — application/json takes its @context
    /// from the Link header only (a body @context is 400), application/ld+json
    /// from the body only (a missing body @context is 400, a Link header is
    /// 400).
    #[tokio::test]
    async fn clause_6_3_5_context_source_mixing_rules() {
        let loader = antares_jsonld::Loader::new();
        let core_link = format!(
            "<{}>; rel=\"{JSONLD_CONTEXT_REL}\"; type=\"application/ld+json\"",
            antares_jsonld::CORE_CONTEXT
        );
        let ct = |v: &'static str| axum::http::HeaderValue::from_static(v);

        // json + body @context → BadRequestData
        let mut h = HeaderMap::new();
        h.insert(header::CONTENT_TYPE, ct("application/json"));
        let err = parse_body(
            &loader,
            &h,
            br#"{"id":"urn:x","type":"T","@context":{}}"#,
            BodyKind::Standard,
        )
        .await
        .map(|_| ())
        .expect_err("json body with @context");
        assert!(matches!(err, ApiError::Ngsi(NgsiError::BadRequestData(_))));

        // ld+json without a body @context → BadRequestData
        let mut h = HeaderMap::new();
        h.insert(header::CONTENT_TYPE, ct("application/ld+json"));
        let err = parse_body(
            &loader,
            &h,
            br#"{"id":"urn:x","type":"T"}"#,
            BodyKind::Standard,
        )
        .await
        .map(|_| ())
        .expect_err("ld+json without @context");
        assert!(matches!(err, ApiError::Ngsi(NgsiError::BadRequestData(_))));

        // ld+json + Link header → BadRequestData
        let mut h = HeaderMap::new();
        h.insert(header::CONTENT_TYPE, ct("application/ld+json"));
        h.insert(header::LINK, core_link.parse().expect("link"));
        let err = parse_body(
            &loader,
            &h,
            br#"{"id":"urn:x","type":"T","@context":{}}"#,
            BodyKind::Standard,
        )
        .await
        .map(|_| ())
        .expect_err("ld+json with Link header");
        assert!(matches!(err, ApiError::Ngsi(NgsiError::BadRequestData(_))));

        // the legal combinations still parse: json + Link, ld+json + body
        let mut h = HeaderMap::new();
        h.insert(header::CONTENT_TYPE, ct("application/json"));
        h.insert(header::LINK, core_link.parse().expect("link"));
        let ok = parse_body(
            &loader,
            &h,
            br#"{"id":"urn:x","type":"T"}"#,
            BodyKind::Standard,
        )
        .await
        .expect("json + Link is the sanctioned pair");
        assert!(ok.value.get("@context").is_none());
    }

    /// RFC 9110 clause 5.3: a list-type field may be split over any number of
    /// field lines, and a single-value field may not be repeated at all.
    #[test]
    fn list_headers_are_read_across_field_lines_and_tenant_is_not() {
        let mut h = HeaderMap::new();
        h.append(
            header::ACCEPT,
            axum::http::HeaderValue::from_static("application/json;q=0.1"),
        );
        h.append(
            header::ACCEPT,
            axum::http::HeaderValue::from_static("application/ld+json;q=0.9"),
        );
        assert_eq!(
            parse_accept(&h).expect("both field lines are one list"),
            Accept::LdJson,
            "a weight on a later field line must still decide"
        );

        let mut h = HeaderMap::new();
        h.append(
            "NGSILD-Tenant",
            axum::http::HeaderValue::from_static("alpha"),
        );
        h.append(
            "NGSILD-Tenant",
            axum::http::HeaderValue::from_static("beta"),
        );
        let err = tenant_from(&h).expect_err("a repeated tenant names two tenants");
        assert_eq!(err.into_response().status(), StatusCode::BAD_REQUEST);
        // the single-valued case is untouched
        let mut h = HeaderMap::new();
        h.insert(
            "NGSILD-Tenant",
            axum::http::HeaderValue::from_static("alpha"),
        );
        assert_eq!(tenant_from(&h).expect("one tenant").as_str(), "alpha");
    }

    /// 6.3.4: "Not Acceptable Media Type … shall result in a 406 HTTP status
    /// code and the body of the message shall contain the list of the
    /// available representations of the resources."
    #[tokio::test]
    async fn clause_6_3_4_not_acceptable_body_lists_representations() {
        let mut h = HeaderMap::new();
        h.insert(
            header::ACCEPT,
            axum::http::HeaderValue::from_static("text/html"),
        );
        let err = parse_accept(&h).expect_err("text/html is not acceptable");
        let resp = err.into_response();
        assert_eq!(resp.status(), StatusCode::NOT_ACCEPTABLE);
        let bytes = axum::body::to_bytes(resp.into_body(), usize::MAX)
            .await
            .expect("body");
        let body = String::from_utf8_lossy(&bytes);
        assert!(body.contains("application/json"), "{body}");
        assert!(body.contains("application/ld+json"), "{body}");

        // geo+json on a non-consumption operation: 406 with the two
        // non-geo representations listed
        let mut h = HeaderMap::new();
        h.insert(
            header::ACCEPT,
            axum::http::HeaderValue::from_static("application/geo+json"),
        );
        let err = parse_accept(&h).expect_err("geo is not acceptable here");
        let resp = err.into_response();
        assert_eq!(resp.status(), StatusCode::NOT_ACCEPTABLE);
        let bytes = axum::body::to_bytes(resp.into_body(), usize::MAX)
            .await
            .expect("body");
        let body = String::from_utf8_lossy(&bytes);
        assert!(body.contains("application/ld+json"), "{body}");
        assert!(!body.contains("application/geo+json"), "{body}");
    }

    /// 5.5.3: error bodies are RFC 7807 objects with at least type (5.5.2
    /// URI), title (short summary) and detail — served as application/json,
    /// NOT application/problem+json.
    #[tokio::test]
    async fn error_body_shape_and_mime() {
        let resp =
            ApiError::from(NgsiError::ResourceNotFound("urn:x not found".into())).into_response();
        assert_eq!(resp.status(), axum::http::StatusCode::NOT_FOUND);
        assert_eq!(
            resp.headers()
                .get(header::CONTENT_TYPE)
                .and_then(|v| v.to_str().ok()),
            Some("application/json"),
            "5.5.3: standard JSON MIME, not problem+json"
        );
        let bytes = axum::body::to_bytes(resp.into_body(), usize::MAX)
            .await
            .expect("body");
        let doc: Value = serde_json::from_slice(&bytes).expect("json");
        assert_eq!(
            doc["type"],
            "https://uri.etsi.org/ngsi-ld/errors/ResourceNotFound"
        );
        assert_eq!(doc["title"], "ResourceNotFound");
        assert!(doc["detail"].as_str().is_some_and(|d| d.contains("urn:x")));
        assert_eq!(doc["status"], 404);
    }
}

/// 6.3.4/6.3.5/6.3.14 negotiation surface: header parsing on hostile input,
/// the shape of what goes back on the wire, and what must NOT be in it.
#[cfg(test)]
mod negotiation {
    use super::*;
    use axum::extract::FromRequestParts;
    use axum::http::HeaderValue;
    use serde_json::json;

    fn hdr(name: &'static str, value: &str) -> HeaderMap {
        let mut h = HeaderMap::new();
        h.insert(name, HeaderValue::from_str(value).expect("header value"));
        h
    }

    fn accept(value: &str) -> HeaderMap {
        hdr("accept", value)
    }

    /// The selected representation on an operation that does not offer
    /// geo+json (6.3.15), and on one that does.
    fn acc(value: &str) -> Accept {
        parse_accept(&accept(value)).expect("acceptable")
    }
    fn acc_geo(value: &str) -> Accept {
        parse_accept_geo(&accept(value)).expect("acceptable")
    }

    /// 6.3.14: an NGSILD-Tenant that is not `[A-Za-z0-9_-]{1,64}` is a
    /// BadRequestData 400 — never a panic, a 500, or a silent fallback to the
    /// default tenant.
    #[test]
    fn tenant_header_is_validated_or_400() {
        assert_eq!(
            tenant_from(&HeaderMap::new())
                .expect("absent header")
                .as_str(),
            TenantId::DEFAULT
        );
        assert_eq!(
            tenant_from(&hdr("NGSILD-Tenant", "city-01_A"))
                .expect("valid tenant")
                .as_str(),
            "city-01_A"
        );
        let long = "x".repeat(65);
        for bad in ["", "a b", "a/b", "../etc", "a.b", "tenant;drop", &long] {
            let err = tenant_from(&hdr("NGSILD-Tenant", bad))
                .map(|t| t.as_str().to_owned())
                .expect_err("hostile tenant must be rejected");
            assert!(
                matches!(err, ApiError::Ngsi(NgsiError::BadRequestData(_))),
                "{bad:?} → {err:?}"
            );
        }
        // bytes that are valid in a header field but not in a Rust str
        let mut h = HeaderMap::new();
        h.insert(
            "NGSILD-Tenant",
            HeaderValue::from_bytes(&[0xff, 0xfe]).expect("opaque header bytes"),
        );
        let err = tenant_from(&h)
            .map(|t| t.as_str().to_owned())
            .expect_err("non-ASCII tenant");
        assert!(matches!(err, ApiError::Ngsi(NgsiError::BadRequestData(_))));
    }

    /// The 400 for a hostile tenant carries the ProblemDetails shape and no
    /// broker internals.
    #[tokio::test]
    async fn rejected_tenant_body_leaks_nothing() {
        let err = tenant_from(&hdr("NGSILD-Tenant", "a b"))
            .map(|_| ())
            .expect_err("invalid tenant");
        let resp = err.into_response();
        assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
        let bytes = axum::body::to_bytes(resp.into_body(), usize::MAX)
            .await
            .expect("body");
        let doc: Value = serde_json::from_slice(&bytes).expect("json");
        assert_eq!(doc["title"], "BadRequestData");
        let body = String::from_utf8_lossy(&bytes);
        for internal in ["/workspace", ".rs", "panicked", "TenantId("] {
            assert!(!body.contains(internal), "{internal} leaked: {body}");
        }
    }

    /// 6.3.4: absent Accept ⇒ application/json; the wildcards expand to the
    /// first option of the list; an Accept naming nothing of the list — or
    /// weighting everything to zero — is a 406.
    #[test]
    fn accept_wildcards_and_unacceptable_headers() {
        assert_eq!(
            parse_accept(&HeaderMap::new()).expect("absent Accept"),
            Accept::Json
        );
        assert_eq!(acc("*/*"), Accept::Json);
        assert_eq!(acc("application/*"), Accept::Json);
        assert_eq!(acc_geo("*/*"), Accept::Json);
        assert_eq!(acc("application/json;charset=utf-8"), Accept::Json);
        assert_eq!(
            acc("APPLICATION/LD+JSON"),
            Accept::LdJson,
            "media types are case-insensitive"
        );
        for bad in [
            "",
            "text/html",
            "text/*",
            "application/xml, text/turtle",
            "*/*;q=0",
        ] {
            assert!(
                matches!(parse_accept(&accept(bad)), Err(ApiError::NotAcceptable(_))),
                "{bad:?} must be 406"
            );
        }
        // header bytes that are not a Rust str: nothing acceptable was named
        let mut h = HeaderMap::new();
        h.insert(
            header::ACCEPT,
            HeaderValue::from_bytes(&[0xff]).expect("opaque header bytes"),
        );
        assert!(matches!(parse_accept(&h), Err(ApiError::NotAcceptable(_))));
    }

    /// 6.3.4: "the first one of the list shall be selected, unless amended by
    /// the HTTP Accept header processing rules". A malformed or non-finite
    /// weight is not one of those rules and must not decide the outcome.
    #[test]
    fn accept_quality_values() {
        assert_eq!(
            acc("application/ld+json, application/json"),
            Accept::Json,
            "list order, not header order"
        );
        assert_eq!(
            acc("application/json;q=0.1, application/ld+json;q=0.9"),
            Accept::LdJson
        );
        assert_eq!(
            acc("application/json;q=0, application/ld+json"),
            Accept::LdJson,
            "q=0 removes json from the offered set"
        );
        assert_eq!(
            acc("*/*;q=0.9, application/ld+json;q=0.8"),
            Accept::Json,
            "RFC 9110 5.3.2: json takes the wildcard's 0.9, which outranks 0.8"
        );
        assert_eq!(
            acc("application/json;q=0, */*"),
            Accept::LdJson,
            "the exact range is the more specific match, so json stays refused"
        );
        for weird in ["q=NaN", "q=inf", "q=", "q=abc", "q=1.0.0"] {
            assert_eq!(
                acc(&format!("application/ld+json;{weird}")),
                Accept::LdJson,
                "{weird} is not a usable weight — the type stays acceptable"
            );
        }
    }

    /// 6.3.15 restricts application/geo+json to Retrieve/Query Entity. On
    /// every other operation it is simply not on offer, so a client that also
    /// named an available representation gets that one; a client that named
    /// only geo+json gets a 406 whose body must NOT advertise geo+json.
    #[tokio::test]
    async fn geojson_is_unavailable_outside_entity_consumption() {
        assert_eq!(acc("application/geo+json, application/json"), Accept::Json);
        assert_eq!(
            acc("application/geo+json;q=0.9, application/ld+json;q=0.1"),
            Accept::LdJson,
            "the only available representation wins even weighted below geo"
        );
        assert_eq!(
            acc_geo("application/geo+json"),
            Accept::GeoJson,
            "on Retrieve/Query Entity it IS available"
        );
        let err = parse_accept(&accept("application/geo+json"))
            .map(|_| ())
            .expect_err("406 on other operations");
        let resp = err.into_response();
        assert_eq!(resp.status(), StatusCode::NOT_ACCEPTABLE);
        let bytes = axum::body::to_bytes(resp.into_body(), usize::MAX)
            .await
            .expect("body");
        let body = String::from_utf8_lossy(&bytes);
        assert!(
            !body.contains("geo+json"),
            "must not offer geo+json: {body}"
        );
        assert!(!body.contains("detail"), "no internals in a 406: {body}");
    }

    /// A pathological Accept header is bounded work and still answers.
    #[test]
    fn huge_accept_header_is_survivable() {
        let raw = ["text/html;q=0.5"; 5000].join(",");
        assert!(matches!(
            parse_accept(&accept(&raw)),
            Err(ApiError::NotAcceptable(_))
        ));
        let raw = format!("{raw},application/ld+json");
        assert_eq!(acc(&raw), Accept::LdJson);
    }

    /// Percent-decoding accepts only `%` + two hex digits; anything else is
    /// literal text, and bytes that are not UTF-8 are replaced rather than
    /// panicked on.
    #[test]
    fn percent_decoding_is_strict_and_total() {
        assert_eq!(percent_decode(b"plain"), "plain");
        assert_eq!(percent_decode(b"%41%42"), "AB");
        assert_eq!(percent_decode(b"%7B%22a%22%7D"), r#"{"a"}"#);
        assert_eq!(percent_decode(b"100%"), "100%");
        assert_eq!(percent_decode(b"%4"), "%4");
        assert_eq!(percent_decode(b"%zz"), "%zz");
        assert_eq!(percent_decode(b"%+1"), "%+1", "a sign is not a hex digit");
        assert_eq!(percent_decode(b"% 1"), "% 1");
        assert_eq!(percent_decode(b"%ff"), "\u{fffd}", "lone continuation byte");
        assert_eq!(percent_decode(b"%25%34%31"), "%41", "decoded exactly once");
    }

    /// Query parsing (5.7.2 parameter conventions): `+` is a space, values
    /// are percent-decoded, empty-valued parameters are dropped, and a
    /// hostile query string never panics.
    #[tokio::test]
    async fn clean_params_drops_empties_and_decodes() {
        let (mut parts, ()) = axum::http::Request::builder()
            .uri("/x?q=a%3D%3D1&name=a+b&datasetId=&flag&&raw=%ff&half=%4")
            .body(())
            .expect("request")
            .into_parts();
        let CleanParams(m) = CleanParams::from_request_parts(&mut parts, &())
            .await
            .expect("infallible");
        assert_eq!(m.get("q").map(String::as_str), Some("a==1"));
        assert_eq!(m.get("name").map(String::as_str), Some("a b"));
        assert!(!m.contains_key("datasetId"), "empty value means absent");
        assert!(!m.contains_key("flag"), "valueless key means absent");
        assert_eq!(m.get("raw").map(String::as_str), Some("\u{fffd}"));
        assert_eq!(m.get("half").map(String::as_str), Some("%4"));
    }

    /// 6.3.20: a query parameter the operation does not define is an
    /// InvalidRequest 400.
    #[test]
    fn unknown_query_parameters_are_rejected() {
        let mut p = std::collections::HashMap::new();
        p.insert("type".to_owned(), "T".to_owned());
        assert!(check_params(&p, &["type", "q"]).is_ok());
        p.insert("bogus".to_owned(), "1".to_owned());
        let err = check_params(&p, &["type", "q"]).expect_err("unknown parameter");
        assert!(matches!(err, ApiError::Ngsi(NgsiError::InvalidRequest(_))));
    }

    /// 6.3.5: the @context Link header is the one with the JSON-LD context
    /// relation, and only a properly bracketed URI-reference counts.
    #[test]
    fn link_header_context_extraction() {
        let link = |v: &str| hdr("link", v);
        assert_eq!(
            link_context(&link(&format!(
                "<https://example.org/c.jsonld>; rel=\"{JSONLD_CONTEXT_REL}\"; type=\"application/ld+json\""
            ))),
            Some("https://example.org/c.jsonld".to_owned())
        );
        assert_eq!(link_context(&HeaderMap::new()), None);
        assert_eq!(
            link_context(&link("<https://example.org/c.jsonld>; rel=\"alternate\"")),
            None,
            "another relation is not the @context"
        );
        assert_eq!(
            link_context(&link(&format!(
                "https://example.org/c.jsonld; rel=\"{JSONLD_CONTEXT_REL}\""
            ))),
            None,
            "an unbracketed target is not a Link value"
        );
        // several Link field lines: the JSON-LD one is picked out
        let mut h = HeaderMap::new();
        h.append(
            header::LINK,
            HeaderValue::from_static("<https://a/x>; rel=\"self\""),
        );
        h.append(
            header::LINK,
            HeaderValue::from_str(&format!(
                "<https://example.org/c.jsonld>; rel=\"{JSONLD_CONTEXT_REL}\""
            ))
            .expect("link"),
        );
        assert_eq!(
            link_context(&h),
            Some("https://example.org/c.jsonld".to_owned())
        );
    }

    /// Request Content-Type is compared as a bare media type.
    #[test]
    fn content_type_strips_parameters_and_case() {
        assert_eq!(content_type(&HeaderMap::new()), "");
        assert_eq!(
            content_type(&hdr("content-type", "Application/LD+JSON; charset=UTF-8")),
            "application/ld+json"
        );
        assert_eq!(
            content_type(&hdr("content-type", " application/json ")),
            "application/json"
        );
    }

    /// 6.3.5 request bodies: an unsupported media type is a bare 415, an
    /// empty or non-object body is an InvalidRequest 400, and the 400 detail
    /// carries no broker internals.
    #[tokio::test]
    async fn body_parsing_error_paths() {
        let loader = antares_jsonld::Loader::new();
        let ct = |v: &'static str| hdr("content-type", v);

        for (mime, kind) in [
            ("text/plain", BodyKind::Standard),
            ("application/xml", BodyKind::Standard),
            ("application/merge-patch+json", BodyKind::Standard),
        ] {
            let err = parse_body(&loader, &ct(mime), b"{}", kind)
                .await
                .map(|_| ())
                .expect_err("unsupported media type");
            assert!(
                matches!(err, ApiError::Bare(StatusCode::UNSUPPORTED_MEDIA_TYPE)),
                "{mime} → {err:?}"
            );
        }
        assert!(parse_body(
            &loader,
            &ct("application/merge-patch+json"),
            br#"{"a":1}"#,
            BodyKind::MergePatch
        )
        .await
        .is_ok());

        for (bytes, what) in [
            (&b""[..], "empty"),
            (&b"[{\"id\":\"urn:x\"}]"[..], "array"),
            (&b"\"scalar\""[..], "scalar"),
            (&b"{oops"[..], "malformed"),
            (&[0x7b, 0xff, 0x7d][..], "non-UTF-8"),
        ] {
            let err = parse_body(&loader, &ct("application/json"), bytes, BodyKind::Standard)
                .await
                .map(|_| ())
                .expect_err(what);
            assert!(
                matches!(err, ApiError::Ngsi(NgsiError::InvalidRequest(_))),
                "{what} body → {err:?}"
            );
            // the 400 detail may quote the parser, never the broker's insides
            let detail = format!("{err:?}");
            for internal in ["/workspace", ".rs:", "antares_"] {
                assert!(!detail.contains(internal), "{internal} leaked: {detail}");
            }
        }
    }

    /// 6.3.6 response building: application/json carries the @context in the
    /// Link header and NOT in the body; application/ld+json the other way
    /// round.
    #[tokio::test]
    async fn respond_places_the_context_per_media_type() {
        let loader = antares_jsonld::Loader::new();
        let ctx = loader.core();
        let t = TenantId::default();
        let payload = json!({"id": "urn:a", "type": "T"});

        let resp = respond(StatusCode::OK, payload.clone(), &ctx, Accept::Json, &t);
        assert!(resp.headers().get(header::LINK).is_some());
        let bytes = axum::body::to_bytes(resp.into_body(), usize::MAX)
            .await
            .expect("body");
        let doc: Value = serde_json::from_slice(&bytes).expect("json");
        assert!(doc.get("@context").is_none(), "json body: Link only");

        let resp = respond(StatusCode::OK, payload, &ctx, Accept::LdJson, &t);
        assert!(
            resp.headers().get(header::LINK).is_none(),
            "ld+json body carries the @context itself — no Link header"
        );
        let bytes = axum::body::to_bytes(resp.into_body(), usize::MAX)
            .await
            .expect("body");
        let doc: Value = serde_json::from_slice(&bytes).expect("json");
        assert_eq!(doc["@context"], json!(CORE_CONTEXT));
    }

    /// The streamed list response is a JSON array in both media types, with
    /// the same @context placement rule as `respond`.
    #[tokio::test]
    async fn respond_list_shapes() {
        let loader = antares_jsonld::Loader::new();
        let ctx = loader.core();
        let t = TenantId::default();
        let docs = vec![
            json!({"id": "urn:a", "type": "T"}),
            json!({"id": "urn:b", "type": "T"}),
        ];

        let resp = respond_list(StatusCode::OK, docs.clone(), &ctx, Accept::Json, &t);
        assert!(resp.headers().get(header::LINK).is_some());
        let bytes = axum::body::to_bytes(resp.into_body(), usize::MAX)
            .await
            .expect("body");
        assert_eq!(
            String::from_utf8_lossy(&bytes),
            r#"[{"id":"urn:a","type":"T"},{"id":"urn:b","type":"T"}]"#
        );

        let resp = respond_list(StatusCode::OK, docs, &ctx, Accept::LdJson, &t);
        assert!(resp.headers().get(header::LINK).is_none());
        let bytes = axum::body::to_bytes(resp.into_body(), usize::MAX)
            .await
            .expect("body");
        let arr: Value = serde_json::from_slice(&bytes).expect("json");
        assert_eq!(arr[0]["@context"], json!(CORE_CONTEXT));
        assert_eq!(arr[1]["@context"], json!(CORE_CONTEXT));

        let resp = respond_list(StatusCode::OK, vec![], &ctx, Accept::Json, &t);
        let bytes = axum::body::to_bytes(resp.into_body(), usize::MAX)
            .await
            .expect("body");
        assert_eq!(String::from_utf8_lossy(&bytes), "[]");
    }

    /// 6.3.14: the tenant is echoed only when it is not the default one.
    #[test]
    fn tenant_echo_is_conditional() {
        let mut resp = StatusCode::OK.into_response();
        echo_tenant(&TenantId::default(), &mut resp);
        assert!(
            resp.headers().get("NGSILD-Tenant").is_none(),
            "the default tenant is not echoed"
        );
        let mut resp = StatusCode::OK.into_response();
        echo_tenant(&TenantId::new("city-01").expect("valid"), &mut resp);
        assert_eq!(
            resp.headers()
                .get("NGSILD-Tenant")
                .and_then(|v| v.to_str().ok()),
            Some("city-01")
        );
    }

    /// 6.3.6 `Prefer: body=json` is one preference among the comma-separated
    /// list, on whichever field line it arrives.
    #[test]
    fn prefer_body_json_detection() {
        assert!(!prefer_body_json(&HeaderMap::new()));
        assert!(prefer_body_json(&hdr("prefer", "body=json")));
        assert!(prefer_body_json(&hdr("prefer", "ngsi-ld=1.5, Body=JSON")));
        assert!(!prefer_body_json(&hdr("prefer", "body=ld+json")));
        assert!(!prefer_body_json(&hdr("prefer", "ngsi-ld=1.5")));
        let mut h = HeaderMap::new();
        h.append("prefer", HeaderValue::from_static("ngsi-ld=1.5"));
        h.append("prefer", HeaderValue::from_static("body=json"));
        assert!(prefer_body_json(&h), "a second Prefer line counts too");
    }

    /// The advertised context URL is a single URL or the core context.
    #[test]
    fn context_link_url_selection() {
        let ctx_with = |source: Value| {
            let mut c = Context::default();
            c.source = source;
            c
        };
        assert_eq!(
            context_link_url(&ctx_with(json!("https://example.org/c.jsonld"))),
            "https://example.org/c.jsonld"
        );
        assert_eq!(
            context_link_url(&ctx_with(json!(["https://example.org/c.jsonld"]))),
            "https://example.org/c.jsonld"
        );
        assert_eq!(
            context_link_url(&ctx_with(json!(["https://a/x", "https://b/y"]))),
            CORE_CONTEXT,
            "an inline list cannot be advertised by reference"
        );
        assert_eq!(context_link_url(&ctx_with(json!({"a": "b"}))), CORE_CONTEXT);
        assert_eq!(context_link_url(&ctx_with(Value::Null)), CORE_CONTEXT);
    }

    /// The small response builders: 201 carries Location and no body, 204
    /// carries neither, 207 is application/json.
    #[tokio::test]
    async fn status_only_responses() {
        let t = TenantId::new("city-01").expect("valid");
        let resp = created("/ngsi-ld/v1/entities/urn:a".to_owned(), &t);
        assert_eq!(resp.status(), StatusCode::CREATED);
        assert_eq!(
            resp.headers()
                .get(header::LOCATION)
                .and_then(|v| v.to_str().ok()),
            Some("/ngsi-ld/v1/entities/urn:a")
        );
        assert_eq!(
            resp.headers()
                .get("NGSILD-Tenant")
                .and_then(|v| v.to_str().ok()),
            Some("city-01")
        );
        let bytes = axum::body::to_bytes(resp.into_body(), usize::MAX)
            .await
            .expect("body");
        assert!(bytes.is_empty(), "201 carries no payload");

        assert_eq!(no_content(&t).status(), StatusCode::NO_CONTENT);
        let resp = multi_status(json!({"success": [], "errors": []}), &t);
        assert_eq!(resp.status(), StatusCode::MULTI_STATUS);
        assert_eq!(
            resp.headers()
                .get(header::CONTENT_TYPE)
                .and_then(|v| v.to_str().ok()),
            Some("application/json")
        );
    }

    /// The @context member is consumed during parsing and never re-served
    /// from the stored document.
    #[test]
    fn without_context_strips_the_member() {
        let o = without_context(&json!({"id": "urn:a", "@context": "https://x"}));
        assert!(!o.contains_key("@context"));
        assert!(o.contains_key("id"));
        assert!(without_context(&json!("not an object")).is_empty());
    }
}

#[cfg(test)]
mod clause_5_2_3 {
    use super::*;
    use serde_json::json;

    fn ctx_with(source: Value) -> Context {
        let mut c = Context::default();
        c.source = source;
        c
    }

    /// 5.2.3 as read by the ETSI validation ecosystem: the served @context
    /// echoes the user context where present (core implicit per 4.4) and
    /// falls back to the core context alone otherwise. The literal-wording
    /// alternative ([user, core]) fails 68 strict-compared official
    /// expectations.
    #[test]
    fn served_context_echoes_user_or_core() {
        let out = inject_context(json!({"id": "urn:x"}), &ctx_with(Value::Null));
        assert_eq!(out["@context"], json!(CORE_CONTEXT));
        let out = inject_context(
            json!({"id": "urn:x"}),
            &ctx_with(json!("https://example.org/user.jsonld")),
        );
        assert_eq!(out["@context"], json!("https://example.org/user.jsonld"));
        let out = inject_context(
            json!({"id": "urn:x"}),
            &ctx_with(json!(["https://example.org/a.jsonld", CORE_CONTEXT])),
        );
        assert_eq!(
            out["@context"],
            json!(["https://example.org/a.jsonld", CORE_CONTEXT]),
            "a user context already listing the core is echoed verbatim"
        );
    }
}
