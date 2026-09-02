// SPDX-License-Identifier: EUPL-1.2
//! Batch operations /entityOperations/* (5.6.7–5.6.10, 5.6.20, 5.7.2-POST;
//! resources 6.14–6.17, 6.23, 6.31).

use crate::entities::{merge_into, paginate, stamp_new};
use crate::negotiate::*;
use crate::repr::{apply, parse_repr};
use crate::state::{now_iso, AppState};
use antares_jsonld::{expand_entity, ExpandOpts};
use antares_model::NgsiError;
use antares_store::CurrentStateDriverExt;
use axum::body::Bytes;
use axum::extract::State;
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use serde_json::{json, Map, Value};
use std::collections::HashMap;

use crate::negotiate::CleanParams;

/// Parse a batch body: JSON array of entity documents; per-document context
/// resolution (ld+json ⇒ each doc's own @context; json ⇒ Link header).
/// Batch bodies are the ingest hot path — sonic-rs (3–4× parse) behind
/// the `sonic` feature, serde_json always compiled as the fallback.
#[cfg(feature = "sonic")]
fn parse_batch_body(body: &[u8]) -> Result<Value, String> {
    sonic_rs::from_slice(body).map_err(|e| e.to_string())
}
#[cfg(not(feature = "sonic"))]
fn parse_batch_body(body: &[u8]) -> Result<Value, String> {
    serde_json::from_slice(body).map_err(|e| e.to_string())
}

/// One batch body into (item, its resolved @context) pairs. Each item's
/// `@context` — or the Link header standing in for it — resolves within the
/// requesting Tenant (5.5.10): a Hosted @context belongs to the Tenant that
/// stored it (5.13.1), so for any other Tenant the URL is unresolvable rather
/// than a set of term mappings the Entity would be expanded through.
async fn parse_batch(
    st: &AppState,
    tenant: &antares_model::TenantId,
    headers: &HeaderMap,
    body: &[u8],
) -> ApiResult<Vec<(Value, ApiResult<std::sync::Arc<antares_jsonld::Context>>)>> {
    let ct = content_type(headers)?;
    let ld = match ct.as_str() {
        "application/json" => false,
        "application/ld+json" => true,
        _ => return Err(ApiError::Bare(StatusCode::UNSUPPORTED_MEDIA_TYPE)),
    };
    let value: Value = parse_batch_body(body)
        .map_err(|e| NgsiError::InvalidRequest(format!("body is not valid JSON: {e}")))?;
    let items = value.as_array().filter(|a| !a.is_empty()).ok_or_else(|| {
        NgsiError::BadRequestData("batch body must be a non-empty JSON array".into())
    })?;
    // 5.6.7.4 (and siblings): a null value in ANY item fails the whole
    // request with BadRequestData — not a per-item 207 error.
    if items.iter().any(Value::is_null) {
        return Err(
            NgsiError::BadRequestData("batch array must not contain null items".into()).into(),
        );
    }
    // Batch entity count cap
    if items.len() > *crate::bounds::MAX_BATCH_ITEMS {
        return Err(NgsiError::BadRequestData(format!(
            "batch of {} exceeds the {}-entity limit",
            items.len(),
            *crate::bounds::MAX_BATCH_ITEMS
        ))
        .into());
    }
    let link = link_context(headers)?;
    if ld && link.is_some() {
        return Err(NgsiError::BadRequestData(
            "application/ld+json batch must not also carry a Link @context (6.3.5)".into(),
        )
        .into());
    }
    let mut out = Vec::new();
    // The loader caps ONE @context resolution at MAX_CONTEXT_FETCHES fetched
    // documents. A batch resolves once per item, so without a ceiling here
    // the item count multiplies that cap: a body inside the 4 MiB limit can
    // name a thousand different @contexts and buy a thousand crawls of a
    // chosen host from one request. Distinct values are what cost a crawl —
    // repeats hit the loader's merged cache, and an inline object fetches
    // nothing — so only those are counted.
    let mut named_contexts: std::collections::HashSet<String> = std::collections::HashSet::new();
    let mut count_context = |c: &Value| -> Result<(), ApiError> {
        if matches!(c, Value::Object(_) | Value::Null) {
            return Ok(());
        }
        let key = c.to_string();
        if named_contexts.contains(&key) {
            return Ok(());
        }
        // At the ceiling nothing more is remembered either: the set is keyed
        // by client-supplied text, so it may not grow past the cap it guards.
        if named_contexts.len() >= crate::bounds::MAX_CONTEXT_FETCHES {
            return Err(NgsiError::BadRequestData(format!(
                "batch names more than {} distinct @contexts",
                crate::bounds::MAX_CONTEXT_FETCHES
            ))
            .into());
        }
        named_contexts.insert(key);
        Ok(())
    };
    for item in items {
        let ctx = if ld {
            match item.get("@context") {
                Some(c) => match count_context(c) {
                    Err(e) => Err(e),
                    Ok(()) => st
                        .loader
                        .resolve_for(tenant, c)
                        .await
                        .map_err(ApiError::from),
                },
                None => Err(NgsiError::BadRequestData(
                    "ld+json batch entity without @context".into(),
                )
                .into()),
            }
        } else if item.get("@context").is_some() {
            Err(NgsiError::BadRequestData("application/json entity carries @context".into()).into())
        } else {
            match &link {
                Some(url) => st
                    .loader
                    .resolve_for(tenant, &Value::String(url.clone()))
                    .await
                    .map_err(ApiError::from),
                None => Ok(st.loader.core()),
            }
        };
        out.push((item.clone(), ctx));
    }
    Ok(out)
}

struct BatchOutcome {
    success: Vec<Value>,
    errors: Vec<Value>,
}

impl BatchOutcome {
    /// Batch output data (5.6.7.5 / 5.6.8.5 / 5.6.9.5 / 5.6.10.5): if every
    /// entity succeeded, the operation's all-ok status (201 + id array for
    /// create, else 204 with no body); otherwise 207 with the S array
    /// ("success") and the E array of BatchEntityError 5.2.17 ("errors").
    fn respond(
        self,
        tenant: &antares_model::TenantId,
        all_ok_status: StatusCode,
        body_on_ok: bool,
    ) -> Response {
        if self.errors.is_empty() {
            if body_on_ok {
                let mut resp = (
                    all_ok_status,
                    [(axum::http::header::CONTENT_TYPE, "application/json")],
                    axum::Json(Value::Array(self.success)),
                )
                    .into_response();
                echo_tenant(tenant, &mut resp);
                resp
            } else {
                let mut resp = all_ok_status.into_response();
                echo_tenant(tenant, &mut resp);
                resp
            }
        } else {
            multi_status(
                json!({"success": self.success, "errors": self.errors}),
                tenant,
            )
        }
    }
}

fn err_entry(id: Option<&str>, e: &NgsiError) -> Value {
    json!({
        "entityId": id.unwrap_or("unknown"),
        "error": problem_value(e),
    })
}

/// 5.2.17 BatchEntityError for a failed forwarded part — the remote status
/// and detail travel inside the ProblemDetails (5.6.7.4/5.6.10.4: remote
/// error results merge into E).
fn err_remote(id: Option<&str>, status: u16, detail: &str) -> Value {
    let etype = match status {
        400 => "BadRequestData",
        404 => "ResourceNotFound",
        409 => "AlreadyExists",
        422 => "OperationNotSupported",
        _ => "InternalError",
    };
    json!({
        "entityId": id.unwrap_or("unknown"),
        "error": {
            "type": format!("https://uri.etsi.org/ngsi-ld/errors/{etype}"),
            "title": "distributed operation failed",
            "status": status,
            "detail": detail,
        }
    })
}

/// Merge one forwarded BATCH operation's outcome into the remote S/E sets
/// (5.6.7.4: "Merge the returned list of Entities successfully created with
/// S. Merge the returned list of Entities in Error with E.").
fn merge_remote_batch(
    status: u16,
    body: &Value,
    sent_ids: &[String],
    created: bool,
    ok: &mut Vec<(String, bool)>,
    err: &mut Vec<Value>,
) {
    // Only the Entities this broker forwarded may appear in the client's
    // S/E arrays: an id the Context Source names but we never sent is not
    // part of this request's outcome, and the source's own error text is
    // rebuilt from its status rather than relayed verbatim.
    let mine = |id: &str| sent_ids.iter().any(|s| s == id);
    match (status, body) {
        (200..=206, Value::Array(a)) => {
            for id in a.iter().filter_map(Value::as_str).filter(|i| mine(i)) {
                ok.push((id.to_owned(), created));
            }
        }
        (200..=206, _) => {
            for id in sent_ids {
                ok.push((id.clone(), created));
            }
        }
        (207, Value::Object(o)) => {
            if let Some(Value::Array(a)) = o.get("success") {
                for id in a.iter().filter_map(Value::as_str).filter(|i| mine(i)) {
                    ok.push((id.to_owned(), created));
                }
            }
            if let Some(Value::Array(a)) = o.get("errors") {
                for e in a {
                    let Some(id) = e
                        .get("entityId")
                        .and_then(Value::as_str)
                        .filter(|i| mine(i))
                    else {
                        continue;
                    };
                    let remote = e
                        .get("error")
                        .and_then(|p| p.get("status"))
                        .and_then(Value::as_u64)
                        .unwrap_or(u64::from(status)) as u16;
                    err.push(err_remote(
                        Some(id),
                        remote,
                        "forwarded batch operation reported an error for this entity",
                    ));
                }
            }
        }
        _ => {
            for id in sent_ids {
                err.push(err_remote(
                    Some(id),
                    status,
                    &format!("forwarded batch operation returned {status}"),
                ));
            }
        }
    }
}

fn ngsi_of(e: ApiError) -> NgsiError {
    match e {
        ApiError::Ngsi(n) => n,
        ApiError::Bare(code) => NgsiError::BadRequestData(format!("HTTP {code}")),
        ApiError::NotAcceptable(_) => NgsiError::BadRequestData("HTTP 406".into()),
    }
}

// ---------- POST /entityOperations/create (5.6.7) ----------

pub async fn batch_create(
    State(st): State<AppState>,
    CleanParams(params): CleanParams,
    headers: HeaderMap,
    body: Bytes,
) -> Response {
    match batch_write(&st, &params, &headers, &body, BatchMode::Create).await {
        Ok(r) => r,
        Err(e) => e.into_response(),
    }
}

pub async fn batch_upsert(
    State(st): State<AppState>,
    CleanParams(params): CleanParams,
    headers: HeaderMap,
    body: Bytes,
) -> Response {
    match batch_write(&st, &params, &headers, &body, BatchMode::Upsert).await {
        Ok(r) => r,
        Err(e) => e.into_response(),
    }
}

pub async fn batch_update(
    State(st): State<AppState>,
    CleanParams(params): CleanParams,
    headers: HeaderMap,
    body: Bytes,
) -> Response {
    match batch_write(&st, &params, &headers, &body, BatchMode::Update).await {
        Ok(r) => r,
        Err(e) => e.into_response(),
    }
}

/// 5.6.20 Batch Entity Merge: each entity merged per 5.6.17 locally;
/// 204 when all succeed, 207 with S/E arrays otherwise (5.6.20.5).
pub async fn batch_merge(
    State(st): State<AppState>,
    CleanParams(params): CleanParams,
    headers: HeaderMap,
    body: Bytes,
) -> Response {
    match batch_write(&st, &params, &headers, &body, BatchMode::Merge).await {
        Ok(r) => r,
        Err(e) => e.into_response(),
    }
}

#[derive(Clone, Copy, PartialEq)]
enum BatchMode {
    Create,
    Upsert,
    Update,
    Merge,
}

async fn batch_write(
    st: &AppState,
    params: &HashMap<String, String>,
    headers: &HeaderMap,
    body: &[u8],
    mode: BatchMode,
) -> ApiResult<Response> {
    let tenant = tenant_from(headers)?;
    check_params(params, &["options", "local"])?;
    // 6.3.7: `options` is a comma separated list of strings, so a mode is
    // selected when it appears as ONE MEMBER of the list — never by
    // whole-string equality. 6.15.3.1 replace (default) | update for
    // upsert; 6.16.3.1 noOverwrite for update.
    let has_option = |name: &str| {
        params
            .get("options")
            .is_some_and(|o| o.split(',').any(|s| s.trim() == name))
    };
    let update_mode = has_option("update");
    let no_overwrite = has_option("noOverwrite");
    let items = parse_batch(st, &tenant, headers, body).await?;
    // distributed batch (4.3.6): one forwarded request per matching source
    let mut fwd_items: Vec<(
        serde_json::Map<String, Value>,
        std::sync::Arc<antares_jsonld::Context>,
    )> = Vec::new();
    let mut spec = crate::csource::CsrSpec::default();
    let mut spec_types = Vec::new();
    let mut spec_ids = Vec::new();
    let mut spec_attrs = Vec::new();
    for (item, ctx) in &items {
        let (Some(o), Ok(c)) = (item.as_object(), ctx.as_ref()) else {
            continue;
        };
        if let Some(id) = o.get("id").and_then(Value::as_str) {
            spec_ids.push(id.to_owned());
        }
        match o.get("type") {
            Some(Value::String(t)) => spec_types.push(c.expand_key(t)),
            Some(Value::Array(a)) => {
                spec_types.extend(a.iter().filter_map(Value::as_str).map(|t| c.expand_key(t)))
            }
            _ => {}
        }
        for k in o.keys() {
            if !matches!(k.as_str(), "id" | "type" | "scope" | "@context") {
                spec_attrs.push(c.expand_key(k));
            }
        }
        fwd_items.push((o.clone(), c.clone()));
    }
    if !spec_types.is_empty() {
        spec.types = Some(spec_types);
    }
    if !spec_ids.is_empty() {
        spec.ids = Some(spec_ids);
    }
    if !spec_attrs.is_empty() {
        spec.attrs = Some(spec_attrs);
    }
    let mut fed_regs =
        crate::federation::write_regs(st, &tenant, &spec, &st.loader.core(), params, headers)?;
    if let Some(r) = crate::federation::handle_via_loop(
        headers,
        &crate::federation::alias_for(&st.host_alias, &tenant),
        &tenant,
        &mut fed_regs,
    ) {
        return Ok(r);
    }
    let mut out = BatchOutcome {
        success: vec![],
        errors: vec![],
    };
    let mut created_ids: Vec<String> = vec![];
    let mut any_created = false;
    let proxies: Vec<&crate::federation::FedReg> =
        fed_regs.iter().filter(|r| r.is_proxy()).collect();
    // Creates are collected and written as ONE multi-row statement;
    // upsert/update/merge run batched per round below.
    // 5.5.11.0: "All Entities and Attributes in the batch will get the same
    // modifiedAt timestamp" — the clock is read once for the whole array, not
    // per document. Stamping per document spreads a large create over several
    // milliseconds, and a Context Consumer filtering or paging on modifiedAt
    // then sees one batch as two. The per-round stamp below is the separate
    // case: repeated instances of one id are sequential operations (5.5.11.2,
    // 5.5.11.5) and do carry their own instants.
    let create_ts = now_iso();
    let mut pending_creates: Vec<(String, Value)> = Vec::new();
    let mut prepped: Vec<(String, Value)> = Vec::new();
    for (item, ctx) in items {
        let id_hint = item.get("id").and_then(Value::as_str).map(str::to_owned);
        let ctx = match ctx {
            Ok(c) => c,
            Err(e) => {
                out.errors.push(err_entry(id_hint.as_deref(), &ngsi_of(e)));
                continue;
            }
        };
        // proxied (exclusive/redirect) attributes are never stored locally
        let item = if proxies.is_empty() {
            item
        } else if let Some(o) = item.as_object() {
            let (rest, has_attrs) = crate::federation::strip_proxied(o, &proxies, &ctx);
            if !has_attrs {
                continue; // wholly proxied: no local part for this item
            }
            Value::Object(rest)
        } else {
            item
        };
        if mode == BatchMode::Create {
            let prep = || -> Result<(String, Value), NgsiError> {
                let obj = item
                    .as_object()
                    .ok_or_else(|| NgsiError::BadRequestData("entity must be an object".into()))?;
                let mut expanded = expand_entity(obj, &ctx, ExpandOpts::default())?;
                let id = antares_jsonld::expanded_id(&expanded)?.to_owned();
                stamp_new(&mut expanded, &create_ts);
                Ok((id, expanded))
            };
            match prep() {
                Ok(pair) => pending_creates.push(pair),
                Err(e) => out.errors.push(err_entry(id_hint.as_deref(), &e)),
            }
            continue;
        }
        // Expansion/validation per item only;
        // the store operations run BATCHED below — one transaction per round
        // instead of one per item.
        let prep = || -> Result<(String, Value), NgsiError> {
            let obj = item
                .as_object()
                .ok_or_else(|| NgsiError::BadRequestData("entity must be an object".into()))?;
            let fragment_ok = mode == BatchMode::Merge;
            let expanded = expand_entity(
                obj,
                &ctx,
                ExpandOpts {
                    fragment: false,
                    allow_null: fragment_ok,
                    merge: fragment_ok,
                    temporal: false,
                    ..Default::default()
                },
            )?;
            let id = antares_jsonld::expanded_id(&expanded)?.to_owned();
            Ok((id, expanded))
        };
        match prep() {
            Ok(pair) => prepped.push(pair),
            Err(e) => out.errors.push(err_entry(id_hint.as_deref(), &e)),
        }
    }
    // 4.6.6: duplicate instances of one Entity in a batch array "shall come
    // in chronological order" — first oldest. Sequential semantics are kept
    // by splitting into rounds: the Nth occurrence of an id lands in round
    // N, rounds execute in order, and within a round every id is unique.
    let mut rounds: Vec<Vec<(String, Value)>> = Vec::new();
    {
        let mut occurrence: HashMap<String, usize> = HashMap::new();
        for (id, doc) in prepped {
            let n = occurrence.entry(id.clone()).or_insert(0);
            if rounds.len() <= *n {
                rounds.push(Vec::new());
            }
            rounds[*n].push((id, doc));
            *n += 1;
        }
    }
    for round in rounds {
        let ts = now_iso();
        match mode {
            // Creates are collected into pending_creates above and never
            // reach a round; a request handler answers, it does not panic.
            BatchMode::Create => {
                for (id, _) in round {
                    out.errors.push(err_entry(
                        Some(&id),
                        &NgsiError::InternalError(
                            "batch create is handled before the rounds".into(),
                        ),
                    ));
                }
            }
            BatchMode::Upsert => {
                // options=update: merge into existing rows first; ids that
                // turn out absent (or vanish mid-flight) fall through to the
                // replace batch — never a silent success (TOCTOU fix).
                let mut replaces: Vec<(String, Value)> = Vec::new();
                if update_mode {
                    let ids: Vec<String> = round.iter().map(|(id, _)| id.clone()).collect();
                    let docs: HashMap<&str, &Value> =
                        round.iter().map(|(id, d)| (id.as_str(), d)).collect();
                    let res = st.store.batch_mutate(&tenant, &ids, |id, doc| {
                        merge_into(doc, docs[id], &ts);
                        Ok::<(), NgsiError>(())
                    })?;
                    for ((id, expanded), r) in round.iter().zip(res) {
                        match r {
                            Some(Err(e)) => out.errors.push(err_entry(Some(id), &e)),
                            Some(Ok(())) => {
                                if !out.success.contains(&Value::String(id.clone())) {
                                    out.success.push(Value::String(id.clone()));
                                }
                            }
                            None => replaces.push((id.clone(), expanded.clone())),
                        }
                    }
                } else {
                    replaces = round;
                }
                if !replaces.is_empty() {
                    for (_, doc) in replaces.iter_mut() {
                        stamp_new(doc, &ts);
                    }
                    // ids first, then MOVE the documents into the store —
                    // the loop below only needs the ids, so the batch payload
                    // is never deep-cloned on the ingest hot path.
                    let ids: Vec<String> = replaces.iter().map(|(id, _)| id.clone()).collect();
                    let flags = st.store.batch_upsert(&tenant, replaces)?;
                    for (id, created) in ids.iter().zip(flags) {
                        if created {
                            any_created = true;
                            if !created_ids.contains(id) {
                                created_ids.push(id.clone());
                            }
                        }
                        if !out.success.contains(&Value::String(id.clone())) {
                            out.success.push(Value::String(id.clone()));
                        }
                    }
                }
            }
            BatchMode::Update | BatchMode::Merge => {
                let ids: Vec<String> = round.iter().map(|(id, _)| id.clone()).collect();
                let docs: HashMap<&str, &Value> =
                    round.iter().map(|(id, d)| (id.as_str(), d)).collect();
                // batch update with noOverwrite: existing attribute instances
                // are left alone; if any existed, the entity is a partial
                // failure (005_02 ⇒ 207)
                let mut skipped: HashMap<String, bool> = HashMap::new();
                let res = st.store.batch_mutate(&tenant, &ids, |id, doc| {
                    let expanded = docs[id];
                    if mode == BatchMode::Update && no_overwrite {
                        // noOverwrite is instance-level: only instances
                        // whose datasetId already exists are skipped
                        let target = antares_store::stored_object(doc)?;
                        for (k, v) in antares_jsonld::expanded_object(expanded)? {
                            if matches!(
                                k.as_str(),
                                "id" | "type" | "scope" | "createdAt" | "modifiedAt"
                            ) {
                                continue;
                            }
                            let incoming: Vec<Value> = v.as_array().cloned().unwrap_or_default();
                            match target.get_mut(k).and_then(Value::as_array_mut) {
                                None => {
                                    target.insert(k.clone(), Value::Array(incoming));
                                }
                                Some(cur) => {
                                    for ni in incoming {
                                        let ds = ni.get("datasetId").and_then(Value::as_str);
                                        if cur.iter().any(|ci| {
                                            ci.get("datasetId").and_then(Value::as_str) == ds
                                        }) {
                                            skipped.insert(id.to_owned(), true);
                                        } else {
                                            cur.push(ni);
                                        }
                                    }
                                }
                            }
                        }
                        target.insert("modifiedAt".into(), Value::String(ts.clone()));
                    } else {
                        merge_into(doc, expanded, &ts);
                    }
                    Ok::<(), NgsiError>(())
                })?;
                for ((id, _), r) in round.iter().zip(res) {
                    match r {
                        None => out.errors.push(err_entry(
                            Some(id),
                            &NgsiError::ResourceNotFound(format!("entity {id} not found")),
                        )),
                        Some(Err(e)) => out.errors.push(err_entry(Some(id), &e)),
                        Some(Ok(())) => {
                            if skipped.get(id).copied().unwrap_or(false) {
                                out.errors.push(err_entry(
                                    Some(id),
                                    &NgsiError::BadRequestData(
                                        "some attributes already existed (noOverwrite)".into(),
                                    ),
                                ));
                            } else if !out.success.contains(&Value::String(id.clone())) {
                                out.success.push(Value::String(id.clone()));
                            }
                        }
                    }
                }
            }
        }
    }
    // The collected creates, one multi-row statement, one transaction.
    if !pending_creates.is_empty() {
        let ids: Vec<String> = pending_creates.iter().map(|(id, _)| id.clone()).collect();
        let flags = st.store.batch_create(&tenant, pending_creates)?;
        for (id, created) in ids.iter().zip(flags) {
            if created {
                any_created = true;
                if !created_ids.contains(id) {
                    created_ids.push(id.clone());
                }
                if !out.success.contains(&Value::String(id.clone())) {
                    out.success.push(Value::String(id.clone()));
                }
            } else {
                out.errors.push(err_entry(
                    Some(id),
                    &NgsiError::AlreadyExists(format!("entity {id} already exists")),
                ));
            }
        }
    }
    // Distributed arm (5.6.7.4/5.6.8.4/5.6.9.4/5.6.20.4): remote outcomes
    // merge into the same S/E arrays as local ones — the response body
    // carries Entity IDs and BatchEntityErrors (5.2.16/5.2.17), never
    // opaque part descriptors.
    if !fed_regs.is_empty() {
        fn one_outcome(
            id: &str,
            status: u16,
            created: bool,
            ok: &mut Vec<(String, bool)>,
            err: &mut Vec<Value>,
        ) {
            if (200..300).contains(&status) && status != 207 {
                ok.push((id.to_owned(), created && status == 201));
            } else {
                err.push(err_remote(
                    Some(id),
                    status,
                    &format!("forwarded operation returned {status}"),
                ));
            }
        }
        let (op, res_path) = match mode {
            BatchMode::Create => ("createBatch", "create"),
            BatchMode::Upsert => ("upsertBatch", "upsert"),
            BatchMode::Update => ("updateBatch", "update"),
            BatchMode::Merge => ("mergeBatch", "merge"),
        };
        let src = fwd_items
            .first()
            .map(|(_, c)| c.source.clone())
            .unwrap_or(Value::Null);
        let ctx_url = crate::federation::ctx_link_url(headers, &src);
        let mut query: Vec<(String, String)> = Vec::new();
        if let Some(o) = params.get("options") {
            query.push(("options".into(), o.clone()));
        }
        let replace_mode = !update_mode;
        let mut remote_ok: Vec<(String, bool)> = Vec::new();
        let mut remote_err: Vec<Value> = Vec::new();
        for reg in &fed_regs {
            let arr: Vec<Value> = fwd_items
                .iter()
                .filter_map(|(o, c)| crate::federation::reduce_to_scope(o, reg, c))
                .collect();
            if arr.is_empty() {
                continue;
            }
            let sent_ids: Vec<String> = arr
                .iter()
                .filter_map(|e| e.get("id").and_then(Value::as_str).map(str::to_owned))
                .collect();
            if reg.supports(op) {
                let (status, body, _) = crate::federation::forward(
                    st,
                    reqwest::Method::POST,
                    format!("{}/ngsi-ld/v1/entityOperations/{res_path}", reg.endpoint),
                    &query,
                    headers,
                    &tenant,
                    reg,
                    &ctx_url,
                    Some(Value::Array(arr)),
                )
                .await;
                // 5.6.8.5: an upsert forward only CREATED entities when the
                // remote said so — 201 (body lists the created ids); a 204
                // means every forwarded entity was updated.
                merge_remote_batch(
                    status,
                    &body,
                    &sent_ids,
                    mode == BatchMode::Create || (mode == BatchMode::Upsert && status == 201),
                    &mut remote_ok,
                    &mut remote_err,
                );
                continue;
            }
            // 5.6.7.4/5.6.8.4/5.6.9.4/5.6.20.4 single-op fallbacks. These
            // never inherit the batch `options` parameter — e.g.
            // options=update is no Create Entity parameter (5.6.1.3) and
            // would 400 the forward; the append fallback re-adds
            // noOverwrite explicitly (5.6.9.4).
            let mut handled = true;
            match mode {
                BatchMode::Create if reg.supports("createEntity") => {
                    let call = BatchFwd::create();
                    for ent in arr.clone() {
                        let (id, status) =
                            forward_one(st, reg, headers, &tenant, &ctx_url, ent, &call).await;
                        one_outcome(&id, status, call.created, &mut remote_ok, &mut remote_err);
                    }
                }
                BatchMode::Upsert if reg.supports("createEntity") => {
                    let create = BatchFwd::create();
                    for ent in arr.clone() {
                        let (id, status) =
                            forward_one(st, reg, headers, &tenant, &ctx_url, ent.clone(), &create)
                                .await;
                        // 5.6.8.4: an Entity the peer already holds is not a
                        // failed upsert — it falls through to the operation
                        // that updates the one that is there.
                        let fallback = match () {
                            _ if status != 409 => None,
                            _ if replace_mode && reg.supports("replaceEntity") => {
                                Some(BatchFwd::replace())
                            }
                            _ if !replace_mode && reg.supports("updateEntity") => {
                                Some(BatchFwd::update())
                            }
                            _ => {
                                // 5.6.8.4: neither replace nor update available
                                remote_err.push(err_remote(
                                    Some(&id),
                                    422,
                                    &format!("OperationNotSupported: no upsert path for {id}"),
                                ));
                                continue;
                            }
                        };
                        match fallback {
                            None => one_outcome(&id, status, true, &mut remote_ok, &mut remote_err),
                            Some(call) => {
                                let (id, status) =
                                    forward_one(st, reg, headers, &tenant, &ctx_url, ent, &call)
                                        .await;
                                one_outcome(
                                    &id,
                                    status,
                                    call.created,
                                    &mut remote_ok,
                                    &mut remote_err,
                                );
                            }
                        }
                    }
                }
                BatchMode::Upsert if replace_mode && reg.supports("replaceEntity") => {
                    let call = BatchFwd::replace();
                    for ent in arr.clone() {
                        let (id, status) =
                            forward_one(st, reg, headers, &tenant, &ctx_url, ent, &call).await;
                        one_outcome(&id, status, call.created, &mut remote_ok, &mut remote_err);
                    }
                }
                BatchMode::Upsert if !replace_mode && reg.supports("updateEntity") => {
                    let call = BatchFwd::update();
                    for ent in arr.clone() {
                        let (id, status) =
                            forward_one(st, reg, headers, &tenant, &ctx_url, ent, &call).await;
                        one_outcome(&id, status, call.created, &mut remote_ok, &mut remote_err);
                    }
                }
                BatchMode::Update if !no_overwrite && reg.supports("updateEntity") => {
                    let call = BatchFwd::update();
                    for ent in arr.clone() {
                        let (id, status) =
                            forward_one(st, reg, headers, &tenant, &ctx_url, ent, &call).await;
                        one_outcome(&id, status, call.created, &mut remote_ok, &mut remote_err);
                    }
                }
                BatchMode::Merge if reg.supports("mergeEntity") => {
                    // 5.6.20.4 support ladder: no mergeBatch -> per-entity
                    // Merge Entity (5.6.17) forwards.
                    let call = BatchFwd::merge();
                    for ent in arr.clone() {
                        let (id, status) =
                            forward_one(st, reg, headers, &tenant, &ctx_url, ent, &call).await;
                        one_outcome(&id, status, call.created, &mut remote_ok, &mut remote_err);
                    }
                }
                BatchMode::Update if no_overwrite && reg.supports("appendAttrs") => {
                    // 5.6.9.4: append with Attribute overwrite disabled.
                    let call = BatchFwd::append_no_overwrite();
                    for ent in arr.clone() {
                        let (id, status) =
                            forward_one(st, reg, headers, &tenant, &ctx_url, ent, &call).await;
                        one_outcome(&id, status, call.created, &mut remote_ok, &mut remote_err);
                    }
                }
                _ => handled = false,
            }
            if !handled && reg.is_proxy() {
                // 5.6.7.4/5.6.8.4/5.6.9.4/5.6.20.4 last rung: "In case CSR is
                // an exclusive or redirect Context Source Registration, add an
                // Error of type Conflict for each Entity in IN to E."
                for id in &sent_ids {
                    remote_err.push(err_entry(
                        Some(id),
                        &NgsiError::Conflict(format!(
                            "registration does not accept the operation {op}"
                        )),
                    ));
                }
            }
        }
        for (id, was_created) in remote_ok {
            if was_created {
                any_created = true;
                created_ids.push(id.clone());
            }
            if !out.success.iter().any(|v| v.as_str() == Some(id.as_str())) {
                out.success.push(Value::String(id));
            }
        }
        out.errors.extend(remote_err);
    }
    // 5.6.8.5: the created-only S array is the ALL-SUCCEEDED reading ("if all
    // Entities not existing prior to this request have been successfully
    // created and the others have been successfully updated"). Once E is
    // non-empty the third bullet applies and S is "the list of Entities
    // successfully created or updated".
    if mode == BatchMode::Upsert && any_created && out.errors.is_empty() {
        out.success = created_ids.into_iter().map(Value::String).collect();
    }
    let (status, body_on_ok) = match mode {
        BatchMode::Create => (StatusCode::CREATED, true),
        BatchMode::Upsert => {
            if any_created {
                (StatusCode::CREATED, true)
            } else {
                (StatusCode::NO_CONTENT, false)
            }
        }
        BatchMode::Update | BatchMode::Merge => (StatusCode::NO_CONTENT, false),
    };
    Ok(out.respond(&tenant, status, body_on_ok))
}

/// Where a forwarded batch item lands on the peer.
#[derive(Clone, Copy)]
enum FwdPath {
    Collection,
    Entity,
    Attrs,
}

/// The single-Entity request a batch operation forwards when the peer does
/// not support the batch operation itself — the 5.6.7.4 / 5.6.8.4 / 5.6.9.4 /
/// 5.6.20.4 fallback ladders. 5.6.7 posts to the collection, 5.6.8 replaces
/// the Entity or updates its Attributes, 5.6.9 updates them or appends with
/// overwrite disabled, 5.6.20 merges the Entity; the loop around them is the
/// same. None of them inherits the batch `options` parameter, which is no
/// parameter of the single-Entity operation being carried — 5.6.9.4's
/// noOverwrite is re-added here, explicitly, because that one is.
struct BatchFwd {
    path: FwdPath,
    method: reqwest::Method,
    query: Vec<(String, String)>,
    /// whether a 201 from the peer counts as a creation in the 5.2.16 S array
    created: bool,
}

impl BatchFwd {
    /// 5.6.1 Create Entity.
    fn create() -> Self {
        Self {
            path: FwdPath::Collection,
            method: reqwest::Method::POST,
            query: Vec::new(),
            created: true,
        }
    }
    /// 5.6.4 Replace Entity.
    fn replace() -> Self {
        Self {
            path: FwdPath::Entity,
            method: reqwest::Method::PUT,
            query: Vec::new(),
            created: false,
        }
    }
    /// 5.6.2 Update Entity Attributes.
    fn update() -> Self {
        Self {
            path: FwdPath::Attrs,
            method: reqwest::Method::PATCH,
            query: Vec::new(),
            created: false,
        }
    }
    /// 5.6.17 Merge Entity.
    fn merge() -> Self {
        Self {
            path: FwdPath::Entity,
            method: reqwest::Method::PATCH,
            query: Vec::new(),
            created: false,
        }
    }
    /// 5.6.3 Append Entity Attributes with overwrite disabled (5.6.9.4).
    fn append_no_overwrite() -> Self {
        Self {
            path: FwdPath::Attrs,
            method: reqwest::Method::POST,
            query: vec![("options".into(), "noOverwrite".into())],
            created: false,
        }
    }
}

/// Forward one Entity of the batch to one registration (4.3.6.3) and report
/// the id it was sent under with the status the peer answered. The id is
/// reported back to the client verbatim, so it is kept raw here and
/// percent-encoded per RFC 3986 clause 3.3 only where it becomes a path
/// segment of the forwarded URL.
async fn forward_one(
    st: &AppState,
    reg: &crate::federation::FedReg,
    headers: &HeaderMap,
    tenant: &antares_model::TenantId,
    ctx_url: &str,
    ent: Value,
    call: &BatchFwd,
) -> (String, u16) {
    let id = ent
        .get("id")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_owned();
    let seg = crate::federation::path_segment(&id);
    let base = &reg.endpoint;
    let url = match call.path {
        FwdPath::Collection => format!("{base}/ngsi-ld/v1/entities"),
        FwdPath::Entity => format!("{base}/ngsi-ld/v1/entities/{seg}"),
        FwdPath::Attrs => format!("{base}/ngsi-ld/v1/entities/{seg}/attrs"),
    };
    let (status, _, _) = crate::federation::forward(
        st,
        call.method.clone(),
        url,
        &call.query,
        headers,
        tenant,
        reg,
        ctx_url,
        Some(ent),
    )
    .await;
    (id, status)
}

// ---------- POST /entityOperations/delete (5.6.10) ----------

pub async fn batch_delete(
    State(st): State<AppState>,
    CleanParams(params): CleanParams,
    headers: HeaderMap,
    body: Bytes,
) -> Response {
    let go = async {
        let tenant = tenant_from(&headers)?;
        check_params(&params, &["local"])?;
        let ct = content_type(&headers)?;
        if ct != "application/json" && ct != "application/ld+json" {
            return Err(ApiError::Bare(StatusCode::UNSUPPORTED_MEDIA_TYPE));
        }
        let value: Value = serde_json::from_slice(&body)
            .map_err(|e| NgsiError::InvalidRequest(format!("body is not valid JSON: {e}")))?;
        let ids = value.as_array().filter(|a| !a.is_empty()).ok_or_else(|| {
            NgsiError::BadRequestData("batch delete body must be a non-empty array".into())
        })?;
        // 5.6.10.4: a null item fails the whole request
        if ids.iter().any(Value::is_null) {
            return Err(NgsiError::BadRequestData(
                "batch array must not contain null items".into(),
            )
            .into());
        }
        // Batch entity count cap — the same ceiling the write batches carry,
        // and the one that bounds the per-id forwarded DELETE fan-out below.
        if ids.len() > *crate::bounds::MAX_BATCH_ITEMS {
            return Err(NgsiError::BadRequestData(format!(
                "batch of {} exceeds the {}-entity limit",
                ids.len(),
                *crate::bounds::MAX_BATCH_ITEMS
            ))
            .into());
        }
        let spec = crate::csource::CsrSpec {
            ids: Some(
                ids.iter()
                    .filter_map(Value::as_str)
                    .map(str::to_owned)
                    .collect(),
            ),
            ..Default::default()
        };
        let mut regs = crate::federation::write_regs(
            &st,
            &tenant,
            &spec,
            &st.loader.core(),
            &params,
            &headers,
        )?;
        if let Some(r) = crate::federation::handle_via_loop(
            &headers,
            &crate::federation::alias_for(&st.host_alias, &tenant),
            &tenant,
            &mut regs,
        ) {
            return Ok(r);
        }
        let mut out = BatchOutcome {
            success: vec![],
            errors: vec![],
        };
        // One multi-row DELETE for the whole batch; flags in input order.
        let id_strs: Vec<String> = ids
            .iter()
            .filter_map(Value::as_str)
            .map(str::to_owned)
            .collect();
        let mut flags = st.store.batch_delete(&tenant, &id_strs)?.into_iter();
        let mut local_ok: std::collections::HashSet<String> = Default::default();
        let mut local_miss: Vec<String> = Vec::new();
        for id in ids {
            let Some(id) = id.as_str() else {
                out.errors.push(err_entry(
                    None,
                    &NgsiError::BadRequestData("entity id must be a string".into()),
                ));
                continue;
            };
            if flags.next().unwrap_or(false) {
                // 5.6.10 deletes carry the same temporal-deletion semantics
                // as 5.6.6 — without this, batch-deleted entities live on in
                // the temporal store (the reset's batch delete leaked
                // every prior suite's Buildings into the orderBy queries).
                crate::entities::mirror_delete_entity(&st, &tenant, id);
                local_ok.insert(id.to_owned());
            } else {
                // proxied entities may live remotely only (4.3.6.3) — a
                // local miss becomes an error only if no forward covers it.
                local_miss.push(id.to_owned());
            }
        }
        // 5.6.10.4 support ladder: deleteBatch -> one batch forward; else
        // per-entity Delete Entity forwards; else proxy modes get Conflict
        // per entity. Remote outcomes merge into S/E (never opaque parts).
        let mut remote_ok: Vec<(String, bool)> = Vec::new();
        let mut remote_err: Vec<Value> = Vec::new();
        if !regs.is_empty() {
            let ctx_url = crate::federation::ctx_link_url(&headers, &st.loader.core().source);
            for reg in &regs {
                // "Remove from IN all Entities not matched by CSR" — an
                // id-scoped registration (exact ids OR idPattern, 5.12)
                // only receives its own ids (4.3.6.1).
                let sent_ids: Vec<String> = id_strs
                    .iter()
                    .filter(|i| reg.can_match_id(i))
                    .cloned()
                    .collect();
                if sent_ids.is_empty() {
                    continue;
                }
                if reg.supports("deleteBatch") {
                    let sent_vals: Vec<Value> =
                        sent_ids.iter().cloned().map(Value::String).collect();
                    let (status, body, _) = crate::federation::forward(
                        &st,
                        reqwest::Method::POST,
                        format!("{}/ngsi-ld/v1/entityOperations/delete", reg.endpoint),
                        &[],
                        &headers,
                        &tenant,
                        reg,
                        &ctx_url,
                        Some(Value::Array(sent_vals)),
                    )
                    .await;
                    merge_remote_batch(
                        status,
                        &body,
                        &sent_ids,
                        false,
                        &mut remote_ok,
                        &mut remote_err,
                    );
                } else if reg.supports("deleteEntity") {
                    for id in &sent_ids {
                        let (status, _, _) = crate::federation::forward(
                            &st,
                            reqwest::Method::DELETE,
                            format!(
                                "{}/ngsi-ld/v1/entities/{}",
                                reg.endpoint,
                                crate::federation::path_segment(id.as_str())
                            ),
                            &[],
                            &headers,
                            &tenant,
                            reg,
                            &ctx_url,
                            None,
                        )
                        .await;
                        if (200..300).contains(&status) && status != 207 {
                            remote_ok.push((id.clone(), false));
                        } else {
                            remote_err.push(err_remote(
                                Some(id),
                                status,
                                &format!("forwarded delete returned {status}"),
                            ));
                        }
                    }
                } else if reg.is_proxy() {
                    // 5.6.10.4 last rung: an exclusive or redirect CSR that
                    // supports neither delete operation contributes an Error
                    // of type Conflict for each Entity in IN.
                    for id in &sent_ids {
                        remote_err.push(err_entry(
                            Some(id),
                            &NgsiError::Conflict(
                                "registration does not accept the operation deleteBatch".into(),
                            ),
                        ));
                    }
                }
            }
        }
        let remote_success: std::collections::HashSet<String> =
            remote_ok.iter().map(|(id, _)| id.clone()).collect();
        // success in input order (local first occurrence), then remote-only
        for id in &id_strs {
            if local_ok.contains(id) && !out.success.iter().any(|v| v.as_str() == Some(id.as_str()))
            {
                out.success.push(Value::String(id.clone()));
            }
        }
        for (id, _) in remote_ok {
            if !out.success.iter().any(|v| v.as_str() == Some(id.as_str())) {
                out.success.push(Value::String(id));
            }
        }
        let erred: Vec<String> = remote_err
            .iter()
            .filter_map(|e| e.get("entityId").and_then(Value::as_str).map(str::to_owned))
            .collect();
        out.errors.extend(remote_err);
        // 5.6.10.4 local step (5.6.6 limited to local): a missed occurrence
        // is ResourceNotFound unless a FORWARD resolved that id — a local
        // success does not excuse it (5.5.11.4 duplicate-id semantics: the
        // second occurrence of the same id errors).
        for id in local_miss {
            if !remote_success.contains(&id) && !erred.contains(&id) {
                out.errors.push(err_entry(
                    Some(&id),
                    &NgsiError::ResourceNotFound(format!("entity {id} not found")),
                ));
            }
        }
        Ok::<_, ApiError>(out.respond(&tenant, StatusCode::NO_CONTENT, false))
    };
    go.await.unwrap_or_else(|e| e.into_response())
}

// ---------- POST /entityOperations/query (6.23) ----------

/// 5.2.23 Query: flatten the JSON members into query-param form with the
/// Table 5.2.23-1 value spaces enforced — entities is a non-empty
/// EntitySelector[], string members must be strings, string-array members
/// are non-empty arrays of strings, joinLevel is a positive integer,
/// entityMap/splitEntities are booleans, geoQ/ordering are objects.
/// `temporal` selects the "Query Temporal Evolution of Entities" reading:
/// temporalQ/aggrParams are only allowed there, containedBy only outside it.
pub(crate) fn query_doc_params(
    q: &Map<String, Value>,
    temporal: bool,
    vp: &mut HashMap<String, String>,
) -> Result<(), NgsiError> {
    let bad = NgsiError::BadRequestData;
    // A member lifted out of the body becomes the same parameter the GET twin
    // carries in the URI, where it is capped at MAX_URI_BYTES (6.3.4 bare
    // 414). Without the same cap here the POST form is the cheap way to hand
    // the query and projection parsers a multi-megabyte string. That includes
    // the three parameters assembled from the `entities` selectors below:
    // there is no cap on the selector array, so a body inside MAX_BODY_BYTES
    // holds hundreds of thousands of them, each costing one expanded IRI and
    // one store bind.
    let capped = |k: &str, s: String| -> Result<String, NgsiError> {
        if s.len() > crate::bounds::MAX_URI_BYTES {
            return Err(bad(format!(
                "Query {k} exceeds the {} byte limit",
                crate::bounds::MAX_URI_BYTES
            )));
        }
        Ok(s)
    };
    match q.get("entities") {
        None => {}
        Some(Value::Array(es)) if !es.is_empty() => {
            let (mut types, mut ids, mut pats) = (Vec::new(), Vec::new(), Vec::new());
            let (mut with_id, mut with_pat) = (0usize, 0usize);
            for e in es {
                if !e.is_object() {
                    return Err(bad(
                        "entities entries must be EntitySelector objects (5.2.33)".into(),
                    ));
                }
                // Table 5.2.33-1: type is the mandatory selector member (a
                // 4.17 type selection, "*" allowed)
                match e.get("type") {
                    Some(Value::String(s)) if !s.is_empty() => types.push(s.clone()),
                    _ => return Err(bad("EntitySelector requires type (5.2.33)".into())),
                }
                // id: "String or String[]", valid URI(s)
                match e.get("id") {
                    None => {}
                    Some(Value::String(s)) => {
                        antares_model::EntityId::new(s)?;
                        ids.push(s.clone());
                        with_id += 1;
                    }
                    Some(Value::Array(a)) => {
                        for i in a {
                            let s = i.as_str().ok_or_else(|| {
                                bad("EntitySelector id entries must be URIs (5.2.33)".into())
                            })?;
                            antares_model::EntityId::new(s)?;
                            ids.push(s.to_owned());
                        }
                        with_id += 1;
                    }
                    Some(_) => {
                        return Err(bad(
                            "EntitySelector id must be a URI string or array (5.2.33)".into(),
                        ))
                    }
                }
                match e.get("idPattern") {
                    None => {}
                    Some(Value::String(s)) => {
                        pats.push(s.clone());
                        with_pat += 1;
                    }
                    Some(_) => {
                        return Err(bad(
                            "EntitySelector idPattern must be a string (5.2.33)".into()
                        ))
                    }
                }
            }
            if !types.is_empty() {
                vp.insert("type".into(), capped("entities type", types.join(","))?);
            }
            // 5.2.33: the selectors are a union, and "id takes precedence over
            // idPattern" holds PER selector. These flat params carry a single
            // id/idPattern pair applied to the whole result, so a member is
            // only emitted when every selector agrees on it — otherwise one
            // selector's id would filter away the Entities another selector
            // selects on its own. Where they disagree the type predicate alone
            // stands, which over-matches rather than losing Entities.
            if with_id == es.len() && !ids.is_empty() {
                vp.insert("id".into(), capped("entities id", ids.join(","))?);
            }
            if with_id == 0 && with_pat == es.len() && !pats.is_empty() {
                vp.insert(
                    "idPattern".into(),
                    capped("entities idPattern", pats.join("|"))?,
                );
            }
        }
        Some(_) => {
            return Err(bad(
                "entities must be a non-empty EntitySelector array (5.2.23)".into(),
            ))
        }
    }
    for k in [
        "q",
        "scopeQ",
        "csf",
        "lang",
        "join",
        "expandValues",
        "jsonKeys",
    ] {
        match q.get(k) {
            None => {}
            Some(Value::String(s)) => {
                vp.insert(k.into(), capped(k, s.clone())?);
            }
            Some(_) => return Err(bad(format!("Query {k} must be a string (5.2.23)"))),
        }
    }
    // string-array members; "Empty array (0 length) is not allowed"
    for k in ["attrs", "pick", "omit", "containedBy", "datasetId"] {
        match q.get(k) {
            None => {}
            Some(Value::Array(a)) if !a.is_empty() => {
                if k == "containedBy" && temporal {
                    return Err(bad(
                        "containedBy is only applicable to Retrieve Entity and Query Entities (5.2.23)"
                            .into(),
                    ));
                }
                let mut parts = Vec::with_capacity(a.len());
                for m in a {
                    parts.push(m.as_str().ok_or_else(|| {
                        bad(format!("Query {k} entries must be strings (5.2.23)"))
                    })?);
                }
                vp.insert(k.into(), capped(k, parts.join(","))?);
            }
            Some(_) => {
                return Err(bad(format!(
                    "Query {k} must be a non-empty array of strings (5.2.23)"
                )))
            }
        }
    }
    if let Some(n) = q.get("joinLevel") {
        let v = n
            .as_u64()
            .filter(|v| *v >= 1)
            .ok_or_else(|| bad("Query joinLevel must be a positive integer (5.2.23)".into()))?;
        vp.insert("joinLevel".into(), v.to_string());
    }
    for k in ["entityMap", "splitEntities"] {
        match q.get(k) {
            None => {}
            Some(Value::Bool(b)) => {
                vp.insert(k.into(), b.to_string());
            }
            Some(_) => return Err(bad(format!("Query {k} must be a boolean (5.2.23)"))),
        }
    }
    if let Some(l) = q.get("entityMapLifetime") {
        // ISO 8601 duration; EntityMap lifetimes are the broker's call
        // ("possibly overriding the requested duration") — 5.14.x surface.
        if !l.is_string() {
            return Err(bad(
                "Query entityMapLifetime must be a string (5.2.23)".into()
            ));
        }
    }
    match q.get("geoQ") {
        None => {}
        Some(Value::Object(g)) => {
            for k in ["georel", "geometry", "geoproperty"] {
                match g.get(k) {
                    None => {}
                    Some(Value::String(s)) => {
                        vp.insert(k.into(), capped(k, s.clone())?);
                    }
                    Some(_) => return Err(bad(format!("geoQ {k} must be a string (5.2.13)"))),
                }
            }
            // `coordinates` is the one lifted member NOT capped in bytes: its
            // ceiling is MAX_GEO_VERTICES (1024), which a legal polygon can
            // spend more than MAX_URI_BYTES on. One cap per parameter, the
            // one that governs it.
            if let Some(c) = g.get("coordinates") {
                vp.insert(
                    "coordinates".into(),
                    match c {
                        Value::String(s) => s.clone(),
                        other => other.to_string(),
                    },
                );
            }
        }
        Some(_) => return Err(bad("geoQ must be a GeoQuery object (5.2.13)".into())),
    }
    match q.get("temporalQ") {
        None => {}
        Some(Value::Object(tq)) if temporal => crate::temporal::temporal_q_params(tq, vp)?,
        Some(Value::Object(_)) => {
            return Err(bad(
                "temporalQ is only allowed for Query Temporal Evolution of Entities (5.2.23)"
                    .into(),
            ))
        }
        Some(_) => {
            return Err(bad(
                "temporalQ must be a TemporalQuery object (5.2.21)".into()
            ))
        }
    }
    match q.get("aggrParams") {
        None => {}
        Some(Value::Object(ap)) if temporal => {
            // 5.2.44 AggregationParams: aggrMethods + aggrPeriodDuration
            match ap.get("aggrMethods") {
                None => {}
                Some(Value::String(s)) => {
                    vp.insert("aggrMethods".into(), capped("aggrMethods", s.clone())?);
                }
                Some(Value::Array(a)) => {
                    let mut parts = Vec::with_capacity(a.len());
                    for m in a {
                        parts.push(m.as_str().ok_or_else(|| {
                            bad("aggrParams aggrMethods entries must be strings (5.2.44)".into())
                        })?);
                    }
                    vp.insert(
                        "aggrMethods".into(),
                        capped("aggrMethods", parts.join(","))?,
                    );
                }
                Some(_) => {
                    return Err(bad(
                        "aggrParams aggrMethods must be a comma separated list of strings (5.2.44)"
                            .into(),
                    ))
                }
            }
            match ap.get("aggrPeriodDuration") {
                None => {}
                Some(Value::String(s)) => {
                    vp.insert(
                        "aggrPeriodDuration".into(),
                        capped("aggrPeriodDuration", s.clone())?,
                    );
                }
                Some(_) => {
                    return Err(bad(
                        "aggrParams aggrPeriodDuration must be a string (5.2.44)".into(),
                    ))
                }
            }
        }
        Some(Value::Object(_)) => {
            return Err(bad(
                "aggrParams is only allowed for Query Temporal Evolution of Entities (5.2.23)"
                    .into(),
            ))
        }
        Some(_) => {
            return Err(bad(
                "aggrParams must be an AggregationParams object (5.2.44)".into(),
            ))
        }
    }
    match q.get("ordering") {
        None => {}
        Some(Value::Object(o)) => {
            // Table 5.2.43-1 (OrderingParams): orderBy String[] -> the 4.23
            // keys; coordinates (JSON array) + geometry (default "Point")
            // -> the dist-ordering reference (orderFrom/orderGeometry).
            // Every member lifted here takes the same MAX_URI_BYTES cap as
            // the rest of the body, for the same reason: it becomes the
            // parameter the GET twin carries in its URI. `coordinates` is
            // again the exception — its ceiling is MAX_GEO_VERTICES, the one
            // that governs a reference geometry.
            if let Some(ob) = o.get("orderBy") {
                let a = ob.as_array().ok_or_else(|| {
                    bad("ordering orderBy must be an array of strings (5.2.43)".into())
                })?;
                let mut parts = Vec::with_capacity(a.len());
                for m in a {
                    parts.push(m.as_str().ok_or_else(|| {
                        bad("ordering orderBy entries must be strings (5.2.43)".into())
                    })?);
                }
                vp.insert("orderBy".into(), capped("orderBy", parts.join(","))?);
            }
            match o.get("coordinates") {
                None => {}
                Some(Value::Array(c)) => {
                    vp.insert("orderFrom".into(), Value::Array(c.clone()).to_string());
                }
                Some(_) => {
                    return Err(bad(
                        "ordering coordinates must be a JSON array (5.2.43)".into()
                    ))
                }
            }
            match o.get("geometry") {
                None => {}
                Some(Value::String(g)) => {
                    vp.insert("orderGeometry".into(), capped("orderGeometry", g.clone())?);
                }
                Some(_) => return Err(bad("ordering geometry must be a string (5.2.43)".into())),
            }
            match o.get("collation") {
                None => {}
                Some(Value::String(c)) => {
                    vp.insert("collation".into(), capped("collation", c.clone())?);
                }
                Some(_) => return Err(bad("ordering collation must be a string (5.2.43)".into())),
            }
        }
        Some(_) => {
            return Err(bad(
                "ordering must be an OrderingParams object (5.2.43)".into()
            ))
        }
    }
    Ok(())
}

pub async fn batch_query(
    State(st): State<AppState>,
    CleanParams(params): CleanParams,
    headers: HeaderMap,
    body: Bytes,
) -> Response {
    match batch_query_inner(&st, &params, &headers, &body).await {
        Ok(r) => r,
        Err(e) => e.into_response(),
    }
}

async fn batch_query_inner(
    st: &AppState,
    params: &HashMap<String, String>,
    headers: &HeaderMap,
    body: &[u8],
) -> ApiResult<Response> {
    let tenant = tenant_from(headers)?;
    check_params(
        params,
        &["limit", "offset", "count", "options", "format", "local"],
    )?;
    // POST query IS Query Entities: geo+json is a valid Accept here (6.3.15)
    let accept = parse_accept_geo(headers)?;
    let parsed = parse_body(&st.loader, headers, body, BodyKind::Standard).await?;
    let q = parsed.object(NgsiError::BadRequestData(
        "query body must be an object".into(),
    ))?;
    if q.get("type").and_then(Value::as_str) != Some("Query") {
        return Err(NgsiError::BadRequestData("body type must be Query (5.2.23)".into()).into());
    }
    // Convert Query members into virtual params reusing the GET filter path.
    let mut vp: HashMap<String, String> = HashMap::new();
    query_doc_params(q, false, &mut vp)?;
    if let Some(l) = params.get("local") {
        vp.insert("local".into(), l.clone());
    }
    // 5.7.2.4 (p. 201): "At least one of the following input data shall be
    // provided: a) selector of Entity Types; b) list of Attribute names,
    // including at least one non-system Attribute; c) NGSI-LD Query,
    // including at least one non-system Attribute; d) NGSI-LD GeoQuery;
    // e) local scope. If none of the above is provided, then an error of type
    // BadRequestData shall be raised (too wide query)." Query Entities is ONE
    // operation: the resource that carries the Query in a body answers to the
    // same behaviour clause as the one that carries it in the URI, and
    // without this a bare `{"type":"Query"}` reads the whole tenant and fans
    // the filterless query out to every matching registration.
    let q_ast = vp.get("q").map(|q| antares_ql::parse_q(q)).transpose()?;
    if !crate::entities::qualifies_non_wide(&vp, q_ast.as_ref()) {
        return Err(NgsiError::BadRequestData(
            "query needs at least one of type, attrs, q, georel (5.7.2.4)".into(),
        )
        .into());
    }
    let fed = if crate::federation::active(&vp)
        && !crate::federation::via_loop(
            headers,
            &crate::federation::alias_for(&st.host_alias, &tenant),
        ) {
        // 6.3.17 scopes NGSILD-Warning to GET /entities(/{id}) — collected
        // here for the log only, never emitted on entityOperations/query
        let mut warnings = Vec::new();
        let fed =
            crate::federation::fed_query(st, &tenant, headers, &parsed.ctx, &vp, &mut warnings)
                .await?;
        for w in &warnings {
            tracing::debug!("distributed query warning (batch query): {w}");
        }
        fed
    } else {
        Vec::new()
    };
    let mut matches = crate::entities::filter_entities_fed(st, &tenant, &vp, &parsed.ctx, fed)?;
    let mut page_params = params.clone();
    page_params.extend(vp.clone());
    // 5.2.43 ordering: same 4.23 keys as the GET twin, applied pre-pagination
    if let Some(spec) = page_params.get("orderBy") {
        crate::entities::order_entities(&mut matches, spec, &page_params, &parsed.ctx)?;
    }
    let (page, count_hdr, _links) = paginate(
        st,
        &page_params,
        matches,
        "/ngsi-ld/v1/entityOperations/query",
    )?;
    // body members (pick/omit/attrs/lang/datasetId) shape the representation
    // exactly like their 6.3.7 query-parameter twins
    let repr = parse_repr(&page_params, &parsed.ctx)?;
    let join = crate::entities::parse_join(&vp)?;
    crate::entities::check_linked_projection(&repr, &join)?;
    let mut payload: Vec<Value> = page
        .iter()
        .filter_map(|doc| {
            let shaped = apply(doc, &repr);
            if repr.pick.is_some() && shaped.as_object().is_some_and(|o| o.is_empty()) {
                return None;
            }
            Some(crate::entities::compact_for(&repr, &shaped, &parsed.ctx))
        })
        .collect();
    if let Some((mode, level)) = &join {
        // 4.5.23.1 bounds the WIDTH of the retrieval per REQUEST, so one
        // allowance is spent across the whole page, exactly as the GET twin
        // spends it. Minting a fresh allowance per payload Entity multiplies
        // the ceiling by the page size, which is the request the ceiling
        // exists to bound: a page of densely linked Entities.
        let held = crate::entities::contained_by(&page_params);
        let mut budget = crate::entities::MAX_JOIN_LOOKUPS;
        match mode.as_str() {
            "inline" => {
                for p in &mut payload {
                    crate::entities::inline_join_beyond(
                        st,
                        &tenant,
                        &parsed.ctx,
                        &repr,
                        p,
                        *level,
                        &held,
                        &mut budget,
                    );
                }
            }
            "flat" => {
                let mut linked = std::collections::BTreeMap::new();
                for doc in &page {
                    crate::entities::collect_flat_beyond(
                        st,
                        &tenant,
                        &repr,
                        doc,
                        *level,
                        &mut linked,
                        &held,
                        &mut budget,
                    );
                }
                let page_ids: Vec<&str> = page.iter().filter_map(|d| d["id"].as_str()).collect();
                for (id, (ldoc, lrepr)) in linked {
                    if !page_ids.contains(&id.as_str()) {
                        payload.push(crate::entities::compact_for(
                            &lrepr,
                            &apply(&ldoc, &lrepr),
                            &parsed.ctx,
                        ));
                    }
                }
            }
            _ => {}
        }
    }
    let out = if accept == Accept::GeoJson {
        crate::entities::to_geojson_collection(payload, None)
    } else {
        Value::Array(payload)
    };
    let mut resp = respond_prefer(StatusCode::OK, out, &parsed.ctx, accept, &tenant, headers);
    if let Some(total) = count_hdr {
        if let Ok(v) = total.to_string().parse() {
            resp.headers_mut().insert("NGSILD-Results-Count", v);
        }
    }
    Ok(resp)
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::body::Body;
    use axum::http::Request;
    use axum::Router;
    use http_body_util::BodyExt;
    use tower::ServiceExt;

    fn app() -> Router {
        crate::router(AppState::new("antares-test".into()))
    }

    async fn post(app: &Router, uri: &str, body: Value) -> Response {
        let s = body.to_string();
        app.clone()
            .oneshot(
                Request::post(uri)
                    .header("Content-Type", "application/json")
                    .header("Content-Length", s.len())
                    .body(Body::from(s))
                    .expect("req"),
            )
            .await
            .expect("resp")
    }

    async fn get_status(app: &Router, uri: &str) -> StatusCode {
        app.clone()
            .oneshot(Request::get(uri).body(Body::empty()).expect("req"))
            .await
            .expect("resp")
            .status()
    }

    async fn body_json(resp: Response) -> Value {
        let bytes = resp.into_body().collect().await.expect("body").to_bytes();
        serde_json::from_slice(&bytes).expect("json body")
    }

    /// 5.7.2.4 (p. 201): "At least one of the following input data shall be
    /// provided: a) selector of Entity Types; b) list of Attribute names,
    /// including at least one non-system Attribute; c) NGSI-LD Query,
    /// including at least one non-system Attribute; d) NGSI-LD GeoQuery;
    /// e) local scope. If none of the above is provided, then an error of
    /// type BadRequestData shall be raised (too wide query)." Query Entities
    /// is one operation, so the resource that carries the Query in a body is
    /// bound by it exactly as the one that carries it in the URI.
    #[tokio::test]
    async fn a_too_wide_query_body_is_refused_like_its_uri_twin() {
        let app = app();
        for wide in [
            json!({"type": "Query"}),
            // ids and an id pattern alone are the case the clause names as
            // insufficient ("it is not possible to retrieve a set of entities
            // by only specifying desired Entity identifiers")
            json!({"type": "Query", "entities": [{"id": "urn:ngsi-ld:Vehicle:1"}]}),
            json!({"type": "Query", "entities": [{"idPattern": ".*"}]}),
            // a system Attribute qualifies neither as an attrs list nor as q
            json!({"type": "Query", "attrs": ["createdAt"]}),
            json!({"type": "Query", "q": "createdAt>\"2020-01-01T00:00:00Z\""}),
        ] {
            let resp = post(&app, "/ngsi-ld/v1/entityOperations/query", wide.clone()).await;
            assert_eq!(
                resp.status(),
                StatusCode::BAD_REQUEST,
                "too wide query accepted: {wide}"
            );
            let body = body_json(resp).await;
            assert_eq!(
                body["type"], "https://uri.etsi.org/ngsi-ld/errors/BadRequestData",
                "{body}"
            );
        }

        // Each of the five qualifying inputs on its own is still served.
        for ok in [
            json!({"type": "Query", "entities": [{"type": "Vehicle"}]}),
            json!({"type": "Query", "attrs": ["speed"]}),
            json!({"type": "Query", "q": "speed>100"}),
            json!({"type": "Query", "geoQ": {"georel": "near;maxDistance==2000",
                   "geometry": "Point", "coordinates": "[1,2]"}}),
        ] {
            let resp = post(&app, "/ngsi-ld/v1/entityOperations/query", ok.clone()).await;
            assert_eq!(
                resp.status(),
                StatusCode::OK,
                "qualifying query refused: {ok}"
            );
        }
    }

    /// A Query member lifted out of the body becomes the parameter its GET
    /// twin carries in the URI, where 6.3.4 caps it — the POST form must not
    /// be the cheap way past that cap.
    #[tokio::test]
    async fn query_body_members_are_capped_like_the_uri() {
        let app = app();
        let huge = "a".repeat(crate::bounds::MAX_URI_BYTES + 1);
        let resp = post(
            &app,
            "/ngsi-ld/v1/entityOperations/query",
            json!({"type": "Query", "entities": [{"type": "Vehicle"}], "q": format!("name==\"{huge}\"")}),
        )
        .await;
        assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
        let body = body_json(resp).await;
        assert!(
            body["title"].as_str().is_some_and(|t| t.contains("Bad")),
            "{body}"
        );

        // the same member just inside the cap is still served
        let ok = "a".repeat(64);
        let resp = post(
            &app,
            "/ngsi-ld/v1/entityOperations/query",
            json!({"type": "Query", "entities": [{"type": "Vehicle"}], "q": format!("name==\"{ok}\"")}),
        )
        .await;
        assert_eq!(resp.status(), StatusCode::OK, "a normal query still works");

        // and the array members are assembled under the same cap
        let many: Vec<Value> = (0..600)
            .map(|i| Value::String(format!("attribute-with-a-long-name-{i:04}")))
            .collect();
        let resp = post(
            &app,
            "/ngsi-ld/v1/entityOperations/query",
            json!({"type": "Query", "entities": [{"type": "Vehicle"}], "attrs": many}),
        )
        .await;
        assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
    }

    /// The same cap again, for the members `ordering` and `aggrParams` are
    /// lifted into. 5.2.43 makes `orderBy` a String[] and 5.2.44 makes
    /// `aggrMethods` one, and both are joined into the flat parameter the GET
    /// twin carries in its URI — where 6.3.4's bare 414 caps them. They went
    /// into the map uncapped, so the POST form handed the 4.23 ordering and
    /// the 4.5.19 aggregation parsers a string the GET form cannot express:
    /// one body inside `MAX_BODY_BYTES` holds ~150 000 order keys, each
    /// expanded once per Entity comparison.
    #[tokio::test]
    async fn ordering_and_aggregation_members_are_capped_like_the_uri() {
        let app = app();
        let many: Vec<Value> = (0..600)
            .map(|i| Value::String(format!("attributeWithALongOrderKeyName{i:04}")))
            .collect();
        let resp = post(
            &app,
            "/ngsi-ld/v1/entityOperations/query",
            json!({"type": "Query", "entities": [{"type": "Vehicle"}],
                   "ordering": {"orderBy": many}}),
        )
        .await;
        assert_eq!(
            resp.status(),
            StatusCode::BAD_REQUEST,
            "an over-cap orderBy is refused"
        );

        // the temporal twin carries aggrParams; its aggrMethods is joined the
        // same way and takes the same cap
        let methods: Vec<Value> = (0..600)
            .map(|i| Value::String(format!("totallyUnknownAggregationMethod{i:04}")))
            .collect();
        let resp = post(
            &app,
            "/ngsi-ld/v1/temporal/entityOperations/query",
            json!({"type": "Query", "entities": [{"type": "Vehicle"}],
                   "temporalQ": {"timerel": "before", "timeAt": "2026-01-01T00:00:00Z"},
                   "aggrParams": {"aggrMethods": methods}}),
        )
        .await;
        assert_eq!(
            resp.status(),
            StatusCode::BAD_REQUEST,
            "an over-cap aggrMethods is refused"
        );

        // an ordinary ordering is untouched
        let resp = post(
            &app,
            "/ngsi-ld/v1/entityOperations/query",
            json!({"type": "Query", "entities": [{"type": "Vehicle"}],
                   "ordering": {"orderBy": ["name", "!speed"], "collation": "sk"}}),
        )
        .await;
        assert_eq!(
            resp.status(),
            StatusCode::OK,
            "a normal ordering still works"
        );
    }

    /// The same cap, for the three parameters the `entities` selectors are
    /// lifted into. 5.2.33 makes `entities` an array of EntitySelectors, and
    /// `query_doc_params` folds their `type`, `id` and `idPattern` members
    /// into the flat parameters the GET twin carries in its URI — where
    /// 6.3.4's bare 414 caps them at `MAX_URI_BYTES`. They went into the map
    /// before the cap existed in the function, so the POST form handed the
    /// expansion and store layers a string the GET form cannot express:
    /// ~320 000 selectors fit inside `MAX_BODY_BYTES`, one expanded IRI and
    /// one bind parameter each, past PostgreSQL's 65 535-parameter limit.
    #[tokio::test]
    async fn entity_selector_members_are_capped_like_the_uri() {
        let app = app();
        let many_types: Vec<Value> = (0..600)
            .map(|i| json!({"type": format!("VeryLongTypeNameForTheCap{i:04}")}))
            .collect();
        let resp = post(
            &app,
            "/ngsi-ld/v1/entityOperations/query",
            json!({"type": "Query", "entities": many_types}),
        )
        .await;
        assert_eq!(
            resp.status(),
            StatusCode::BAD_REQUEST,
            "the joined type parameter is capped like its GET twin"
        );

        let many_ids: Vec<Value> = (0..400)
            .map(|i| json!({"id": format!("urn:ngsi-ld:Vehicle:a-fairly-long-identifier-{i:06}")}))
            .collect();
        let resp = post(
            &app,
            "/ngsi-ld/v1/entityOperations/query",
            json!({"type": "Query", "entities": many_ids}),
        )
        .await;
        assert_eq!(
            resp.status(),
            StatusCode::BAD_REQUEST,
            "the joined id parameter is capped like its GET twin"
        );

        let many_pats: Vec<Value> = (0..400)
            .map(|i| json!({"idPattern": format!("^urn:ngsi-ld:Vehicle:pattern-{i:06}.*$")}))
            .collect();
        let resp = post(
            &app,
            "/ngsi-ld/v1/entityOperations/query",
            json!({"type": "Query", "entities": many_pats}),
        )
        .await;
        assert_eq!(
            resp.status(),
            StatusCode::BAD_REQUEST,
            "the joined idPattern parameter is capped like its GET twin"
        );

        // The GeoQuery strings are lifted the same way (5.2.13).
        let resp = post(
            &app,
            "/ngsi-ld/v1/entityOperations/query",
            json!({"type": "Query", "entities": [{"type": "Vehicle"}],
                   "geoQ": {"georel": format!("near;maxDistance=={}", "9".repeat(
                       crate::bounds::MAX_URI_BYTES + 1)),
                            "geometry": "Point", "coordinates": [0, 0]}}),
        )
        .await;
        assert_eq!(
            resp.status(),
            StatusCode::BAD_REQUEST,
            "geoQ georel is capped like its GET twin"
        );

        // …but `coordinates` keeps its own ceiling: MAX_GEO_VERTICES, which a
        // legal polygon can spend more than MAX_URI_BYTES on. Capping it in
        // bytes too would refuse a geometry the broker advertises support for.
        let ring: Vec<Value> = (0..600)
            .map(|i| json!([f64::from(i) / 10_000.0, f64::from(i) / 10_000.0]))
            .chain(std::iter::once(json!([0.0, 0.0])))
            .collect();
        let coords = json!([ring]);
        assert!(
            coords.to_string().len() > crate::bounds::MAX_URI_BYTES,
            "the polygon has to exceed the byte cap for this to prove anything"
        );
        let resp = post(
            &app,
            "/ngsi-ld/v1/entityOperations/query",
            json!({"type": "Query", "entities": [{"type": "Vehicle"}],
                   "geoQ": {"georel": "within", "geometry": "Polygon",
                            "coordinates": coords}}),
        )
        .await;
        assert_eq!(
            resp.status(),
            StatusCode::OK,
            "a polygon under MAX_GEO_VERTICES is served whatever it weighs"
        );

        // The cap may not cost an ordinary multi-selector query: 5.2.33's
        // union of a handful of selectors stays well inside it.
        let resp = post(
            &app,
            "/ngsi-ld/v1/entityOperations/query",
            json!({"type": "Query", "entities": [
                {"type": "Vehicle", "id": "urn:ngsi-ld:Vehicle:1"},
                {"type": "Vehicle", "id": "urn:ngsi-ld:Vehicle:2"}
            ]}),
        )
        .await;
        assert_eq!(resp.status(), StatusCode::OK, "a normal union still works");
    }

    async fn get_entity(app: &Router, id: &str) -> Value {
        let resp = app
            .clone()
            .oneshot(
                Request::get(format!("/ngsi-ld/v1/entities/{id}"))
                    .body(Body::empty())
                    .expect("req"),
            )
            .await
            .expect("resp");
        assert_eq!(resp.status(), StatusCode::OK, "entity {id} readable");
        let bytes = resp.into_body().collect().await.expect("body").to_bytes();
        serde_json::from_slice(&bytes).expect("json body")
    }

    /// 6.3.7: `options` is a comma separated list of strings, so the
    /// 6.15.3.1 "update" upsert mode applies whenever it is one member of
    /// the list — existing Entity content is updated, not replaced.
    #[tokio::test]
    async fn upsert_update_mode_in_option_list_merges() {
        let app = app();
        let id = "urn:ngsi-ld:Building:optlist-upsert";
        let resp = post(
            &app,
            "/ngsi-ld/v1/entityOperations/create",
            json!([{"id": id, "type": "Building",
                    "speed": {"type": "Property", "value": 1},
                    "brand": {"type": "Property", "value": "acme"}}]),
        )
        .await;
        assert_eq!(resp.status(), StatusCode::CREATED);
        let resp = post(
            &app,
            "/ngsi-ld/v1/entityOperations/upsert?options=update,sysAttrs",
            json!([{"id": id, "type": "Building",
                    "speed": {"type": "Property", "value": 2}}]),
        )
        .await;
        assert_eq!(resp.status(), StatusCode::NO_CONTENT);
        let doc = get_entity(&app, id).await;
        assert_eq!(doc["speed"]["value"], 2, "the payload attribute is applied");
        // update mode must NOT destroy attributes absent from the payload
        assert_eq!(
            doc["brand"]["value"], "acme",
            "update mode keeps attributes not in the payload: {doc}"
        );
    }

    /// 4.22 + 5.6.9.4 end to end. 5.6.9.4 (PDF p.175): "For each of the
    /// NGSI-LD Entities included in the input Array execute the behaviour
    /// defined by clause 5.6.3, but limited to a local operation… If the
    /// Entity update failed, then a new BatchEntityError shall be added to E
    /// containing the failed Entity ID and the ProblemDetails associated."
    /// 4.22 makes an entity past its `expiresAt` invalid, so 5.6.3 answers
    /// ResourceNotFound and the batch has to report it in E — not in S, and
    /// not by writing to it.
    #[tokio::test]
    async fn batch_update_of_an_expired_entity_is_an_error_not_a_success() {
        let app = app();
        let id = "urn:ngsi-ld:Building:expired-batch-update";
        let resp = post(
            &app,
            "/ngsi-ld/v1/entityOperations/create",
            json!([{"id": id, "type": "Building", "expiresAt": "2020-01-01T00:00:00Z",
                    "speed": {"type": "Property", "value": 1}}]),
        )
        .await;
        assert_eq!(resp.status(), StatusCode::CREATED);

        // The premise the client can see: it is already absent to a read.
        assert_eq!(
            get_status(&app, &format!("/ngsi-ld/v1/entities/{id}")).await,
            StatusCode::NOT_FOUND,
            "4.22: an expired entity does not exist"
        );

        let resp = post(
            &app,
            "/ngsi-ld/v1/entityOperations/update",
            json!([{"id": id, "type": "Building",
                    "speed": {"type": "Property", "value": 2}}]),
        )
        .await;
        assert_eq!(
            resp.status(),
            StatusCode::MULTI_STATUS,
            "5.6.9.5: none updated, so S and E are reported"
        );
        let outcome = body_json(resp).await;
        assert!(
            outcome["success"].as_array().is_none_or(|a| a.is_empty()),
            "an entity every read refuses may not be reported as updated: {outcome}"
        );
        let errors = outcome["errors"].as_array().expect("E array");
        assert_eq!(errors.len(), 1, "{outcome}");
        assert_eq!(errors[0]["entityId"], id, "{outcome}");
        assert!(
            errors[0]["error"]["type"]
                .as_str()
                .is_some_and(|t| t.ends_with("ResourceNotFound")),
            "5.6.3 on an absent entity is ResourceNotFound: {outcome}"
        );

        // and the write did not happen behind the error
        assert_eq!(
            get_status(&app, &format!("/ngsi-ld/v1/entities/{id}")).await,
            StatusCode::NOT_FOUND,
            "still absent"
        );
    }

    /// 6.3.7 + 6.16.3.1: "noOverwrite" as one member of the `options` list
    /// disables Attribute overwrite for Batch Entity Update (5.6.9).
    #[tokio::test]
    async fn batch_update_no_overwrite_in_option_list_is_honoured() {
        let app = app();
        let id = "urn:ngsi-ld:Building:optlist-update";
        let resp = post(
            &app,
            "/ngsi-ld/v1/entityOperations/create",
            json!([{"id": id, "type": "Building",
                    "speed": {"type": "Property", "value": 1}}]),
        )
        .await;
        assert_eq!(resp.status(), StatusCode::CREATED);
        let resp = post(
            &app,
            "/ngsi-ld/v1/entityOperations/update?options=noOverwrite,sysAttrs",
            json!([{"id": id, "type": "Building",
                    "speed": {"type": "Property", "value": 2},
                    "brand": {"type": "Property", "value": "acme"}}]),
        )
        .await;
        // `speed` already existed, so the skip is a partial failure: 5.6.9.5
        // makes 207 with the S and E arrays the only correct answer.
        assert_eq!(
            resp.status(),
            StatusCode::MULTI_STATUS,
            "the skipped instance must be reported"
        );
        let outcome = body_json(resp).await;
        let errs = outcome["errors"].as_array().expect("E array");
        assert_eq!(errs.len(), 1, "{outcome}");
        assert_eq!(errs[0]["entityId"], id, "E names the entity: {outcome}");
        assert!(
            !outcome["success"]
                .as_array()
                .expect("S array")
                .iter()
                .any(|v| v.as_str() == Some(id)),
            "a partially skipped entity must not also be in S: {outcome}"
        );
        let doc = get_entity(&app, id).await;
        assert_eq!(
            doc["speed"]["value"], 1,
            "noOverwrite leaves the existing instance alone: {doc}"
        );
        assert_ne!(doc["speed"]["value"], 2, "the payload must not overwrite");
        assert_eq!(doc["brand"]["value"], "acme", "new attributes are appended");
    }

    /// 5.6.7.4: what merges into the client's S and E arrays is the outcome
    /// of the Entities this broker forwarded. Ids a Context Source invents
    /// are dropped, and its error text is never relayed verbatim.
    #[test]
    fn remote_batch_results_are_confined_to_forwarded_ids() {
        let sent = vec!["urn:ngsi-ld:Building:mine".to_owned()];
        let (mut ok, mut err) = (Vec::new(), Vec::new());
        merge_remote_batch(
            207,
            &json!({
                "success": ["urn:ngsi-ld:Building:mine", "urn:ngsi-ld:Secret:peer"],
                "errors": [
                    {"entityId": "urn:ngsi-ld:Building:mine",
                     "error": {"status": 404, "detail": "row 42 of table peer_secrets"}},
                    {"entityId": "urn:ngsi-ld:Secret:other",
                     "error": {"status": 409, "detail": "peer internals"}}
                ]
            }),
            &sent,
            false,
            &mut ok,
            &mut err,
        );
        assert_eq!(
            ok,
            vec![("urn:ngsi-ld:Building:mine".to_owned(), false)],
            "only forwarded ids reach S"
        );
        assert_eq!(err.len(), 1, "only forwarded ids reach E: {err:?}");
        assert_eq!(err[0]["entityId"], "urn:ngsi-ld:Building:mine");
        let dump = Value::Array(err.clone()).to_string();
        assert!(
            !dump.contains("Secret") && !dump.contains("peer"),
            "peer ids and error text must not be relayed: {dump}"
        );
        assert_eq!(err[0]["error"]["status"], 404, "the remote status travels");
        // a 2xx id list is confined the same way
        let (mut ok, mut err) = (Vec::new(), Vec::new());
        merge_remote_batch(
            201,
            &json!(["urn:ngsi-ld:Building:mine", "urn:ngsi-ld:Secret:peer"]),
            &sent,
            true,
            &mut ok,
            &mut err,
        );
        assert_eq!(ok, vec![("urn:ngsi-ld:Building:mine".to_owned(), true)]);
        assert!(err.is_empty());
    }

    /// 5.6.10.3: the Batch Entity Delete input is "an array of Entity IDs"
    /// with no per-operation exemption from this broker's batch ceiling —
    /// an over-cap array is rejected whole and deletes nothing.
    #[tokio::test]
    async fn batch_delete_over_the_item_cap_deletes_nothing() {
        let app = app();
        let id = "urn:ngsi-ld:Building:delete-cap";
        let resp = post(
            &app,
            "/ngsi-ld/v1/entityOperations/create",
            json!([{"id": id, "type": "Building"}]),
        )
        .await;
        assert_eq!(resp.status(), StatusCode::CREATED);
        let mut ids: Vec<Value> = (0..*crate::bounds::MAX_BATCH_ITEMS)
            .map(|i| Value::String(format!("urn:ngsi-ld:Building:cap-{i}")))
            .collect();
        ids.push(Value::String(id.to_owned()));
        let resp = post(
            &app,
            "/ngsi-ld/v1/entityOperations/delete",
            Value::Array(ids),
        )
        .await;
        assert_eq!(resp.status(), StatusCode::BAD_REQUEST, "over-cap delete");
        let doc = body_json(resp).await;
        assert!(
            doc["type"]
                .as_str()
                .expect("problem type")
                .ends_with("BadRequestData"),
            "{doc}"
        );
        // rejected whole: the in-range entity of the same array survives
        get_entity(&app, id).await;
    }

    /// 5.6.8.5 third bullet: when only some Entities succeeded, S is "the
    /// list of Entities successfully created **or updated**" — the
    /// created-only list of the second bullet is the all-succeeded case.
    #[tokio::test]
    async fn upsert_207_success_carries_updated_entities_too() {
        let app = app();
        let existing = "urn:ngsi-ld:Building:upsert-207-old";
        let fresh = "urn:ngsi-ld:Building:upsert-207-new";
        let resp = post(
            &app,
            "/ngsi-ld/v1/entityOperations/create",
            json!([{"id": existing, "type": "Building"}]),
        )
        .await;
        assert_eq!(resp.status(), StatusCode::CREATED);
        let resp = post(
            &app,
            "/ngsi-ld/v1/entityOperations/upsert",
            json!([
                {"id": existing, "type": "Building",
                 "speed": {"type": "Property", "value": 1}},
                {"id": fresh, "type": "Building"},
                "not an entity"
            ]),
        )
        .await;
        assert_eq!(resp.status(), StatusCode::MULTI_STATUS);
        let doc = body_json(resp).await;
        let s: Vec<&str> = doc["success"]
            .as_array()
            .expect("S array")
            .iter()
            .filter_map(Value::as_str)
            .collect();
        assert!(s.contains(&existing), "the updated Entity is in S: {doc}");
        assert!(s.contains(&fresh), "the created Entity is in S: {doc}");
        assert_eq!(doc["errors"].as_array().expect("E array").len(), 1, "{doc}");
    }

    /// Table 5.2.23-1 splitEntities: "If true it is assumed that single
    /// Entities are distributed between different Context Brokers and/or
    /// Context Sources and this has to be taken into account when applying
    /// any kind of filters" — the body member drives the same read as the
    /// query-parameter twin, so it has to reach the filter path.
    #[test]
    fn query_body_split_entities_reaches_the_filter_params() {
        let q = json!({"type": "Query", "entityMap": true, "splitEntities": true});
        let mut vp = HashMap::new();
        query_doc_params(q.as_object().expect("object"), false, &mut vp).expect("valid Query");
        assert_eq!(
            vp.get("splitEntities").map(String::as_str),
            Some("true"),
            "splitEntities must not be dropped: {vp:?}"
        );
        assert_eq!(vp.get("entityMap").map(String::as_str), Some("true"));
        // the false reading is carried through unchanged, never as "true"
        let q = json!({"type": "Query", "splitEntities": false});
        let mut vp = HashMap::new();
        query_doc_params(q.as_object().expect("object"), false, &mut vp).expect("valid Query");
        assert_eq!(vp.get("splitEntities").map(String::as_str), Some("false"));
    }

    /// 5.2.33: the `entities` EntitySelectors are a union and "id takes
    /// precedence over idPattern" PER selector. A flat filter carries one
    /// id/idPattern pair, so a member may only be emitted when it holds for
    /// every selector — otherwise the flat filter excludes Entities that a
    /// selector on its own selects.
    #[test]
    fn entity_selectors_are_a_union_not_a_flat_id_filter() {
        let mixed = json!({"type": "Query", "entities": [
            {"type": "Vehicle", "id": "urn:ngsi-ld:Vehicle:1"},
            {"type": "Building", "idPattern": "^urn:ngsi-ld:Building:"}
        ]});
        let mut vp = HashMap::new();
        query_doc_params(mixed.as_object().expect("object"), false, &mut vp).expect("valid Query");
        assert_eq!(vp.get("type").map(String::as_str), Some("Vehicle,Building"));
        assert!(
            !vp.contains_key("id"),
            "an id from one selector must not filter the other selector out: {vp:?}"
        );
        assert!(
            !vp.contains_key("idPattern"),
            "a pattern from one selector must not filter the other out: {vp:?}"
        );
        // every selector carries id → the union of ids is exact
        let all_ids = json!({"type": "Query", "entities": [
            {"type": "Vehicle", "id": "urn:ngsi-ld:Vehicle:1"},
            {"type": "Building", "id": ["urn:ngsi-ld:Building:1"]}
        ]});
        let mut vp = HashMap::new();
        query_doc_params(all_ids.as_object().expect("object"), false, &mut vp)
            .expect("valid Query");
        assert_eq!(
            vp.get("id").map(String::as_str),
            Some("urn:ngsi-ld:Vehicle:1,urn:ngsi-ld:Building:1")
        );
        assert!(!vp.contains_key("idPattern"));
        // no selector carries id and every one carries idPattern → union
        let all_pats = json!({"type": "Query", "entities": [
            {"type": "Vehicle", "idPattern": "^urn:ngsi-ld:Vehicle:"},
            {"type": "Building", "idPattern": "^urn:ngsi-ld:Building:"}
        ]});
        let mut vp = HashMap::new();
        query_doc_params(all_pats.as_object().expect("object"), false, &mut vp)
            .expect("valid Query");
        assert_eq!(
            vp.get("idPattern").map(String::as_str),
            Some("^urn:ngsi-ld:Vehicle:|^urn:ngsi-ld:Building:")
        );
        assert!(!vp.contains_key("id"));
        // a single selector keeps the 5.2.33 id-over-idPattern precedence
        let both = json!({"type": "Query", "entities": [
            {"type": "Vehicle", "id": "urn:ngsi-ld:Vehicle:1", "idPattern": "^urn:"}
        ]});
        let mut vp = HashMap::new();
        query_doc_params(both.as_object().expect("object"), false, &mut vp).expect("valid Query");
        assert_eq!(
            vp.get("id").map(String::as_str),
            Some("urn:ngsi-ld:Vehicle:1")
        );
        assert!(!vp.contains_key("idPattern"), "id wins in one selector");
    }
}
