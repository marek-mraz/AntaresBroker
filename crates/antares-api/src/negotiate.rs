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

pub(crate) fn percent_decode(input: &[u8]) -> String {
    let mut out = Vec::with_capacity(input.len());
    let mut i = 0;
    while i < input.len() {
        if input[i] == b'%' && i + 2 < input.len() {
            let hex = std::str::from_utf8(&input[i + 1..i + 3]).ok();
            if let Some(b) = hex.and_then(|h| u8::from_str_radix(h, 16).ok()) {
                out.push(b);
                i += 3;
                continue;
            }
        }
        out.push(input[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// Handler-level error: an NGSI-LD ProblemDetails or a bare status (6.3.4).
#[derive(Debug)]
pub enum ApiError {
    Ngsi(NgsiError),
    Bare(StatusCode),
}

impl From<NgsiError> for ApiError {
    fn from(e: NgsiError) -> Self {
        Self::Ngsi(e)
    }
}

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
        }
    }
}

pub type ApiResult<T> = Result<T, ApiError>;

/// Tenant from the NGSILD-Tenant header (6.3.14).
pub fn tenant_from(headers: &HeaderMap) -> ApiResult<TenantId> {
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

/// Accept negotiation (6.3.4): json, ld+json, geo+json, */*; 406 otherwise.
/// Absent Accept ⇒ application/json. geo+json is only valid on
/// Retrieve/Query Entities (6.3.15) — everywhere else it is a 406.
pub fn parse_accept_geo(headers: &HeaderMap) -> ApiResult<Accept> {
    let Some(raw) = headers.get(header::ACCEPT) else {
        return Ok(Accept::Json);
    };
    let raw = raw.to_str().unwrap_or("");
    // (q, specificity, 6.3.4 list rank, kind)
    let mut best: Option<(f32, u8, u8, Accept)> = None;
    for part in raw.split(',') {
        let mut segs = part.split(';');
        let mt = segs.next().unwrap_or("").trim().to_ascii_lowercase();
        let mut q = 1.0f32;
        for p in segs {
            if let Some(v) = p.trim().strip_prefix("q=") {
                q = v.parse().unwrap_or(1.0);
            }
        }
        // 6.3.4: the option list is json > ld+json > geo+json and "the order of
        // the list above is significant … the first one of the list shall be
        // selected, unless amended by the HTTP Accept header processing rules,
        // e.g. the presence of a q parameter". So q wins first, then
        // specificity, and list rank is the final tie-break — never the order
        // the client happened to write the tokens in.
        let cand = match mt.as_str() {
            "application/json" => Some((2, 0, Accept::Json)),
            "application/ld+json" => Some((2, 1, Accept::LdJson)),
            "application/geo+json" => Some((2, 2, Accept::GeoJson)),
            "application/*" => Some((1, 0, Accept::Json)),
            "*/*" => Some((0, 0, Accept::Json)),
            _ => None,
        };
        if let Some((spec, rank, kind)) = cand {
            let better = match &best {
                None => true,
                Some((bq, bspec, brank, _)) => {
                    q > *bq || (q == *bq && (spec > *bspec || (spec == *bspec && rank < *brank)))
                }
            };
            if better {
                best = Some((q, spec, rank, kind));
            }
        }
    }
    match best {
        Some((q, _, _, kind)) if q > 0.0 => Ok(kind),
        _ => Err(ApiError::Bare(StatusCode::NOT_ACCEPTABLE)),
    }
}

/// Accept negotiation for every operation that is NOT Retrieve/Query
/// Entities: application/geo+json ⇒ 406 (6.3.15).
pub fn parse_accept(headers: &HeaderMap) -> ApiResult<Accept> {
    match parse_accept_geo(headers)? {
        Accept::GeoJson => Err(ApiError::Bare(StatusCode::NOT_ACCEPTABLE)),
        other => Ok(other),
    }
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
        loader.resolve(&user_ctx).await?
    } else {
        if body_has_context(&value) {
            return Err(NgsiError::BadRequestData(
                "application/json request must not carry an @context member (6.3.5)".into(),
            )
            .into());
        }
        match link {
            Some(url) => loader.resolve(&Value::String(url)).await?,
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

/// Context for GET/DELETE requests: Link header or core (6.3.5).
pub async fn request_context(loader: &Loader, headers: &HeaderMap) -> ApiResult<Arc<Context>> {
    match link_context(headers) {
        Some(url) => Ok(loader.resolve(&Value::String(url)).await?),
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

/// Build a payload-carrying NGSI-LD response (6.3.6).
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
                axum::Json(payload),
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
                axum::Json(with_ctx),
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
                axum::Json(with_ctx),
            )
                .into_response()
        }
    };
    echo_tenant(tenant, &mut resp);
    resp
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
            let _ = serde_json::to_writer(&mut buf, &doc);
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
/// the core as implicit per 4.4. Antares follows the ecosystem reading; the
/// wording doubt is logged in testsuite-doubts.md (2026-08-11).
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
    /// expectations — see testsuite-doubts.md 2026-08-11.
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
