//! N3: the browser bindings — the Service Worker glue and the in-page API.
//!
//! Both reduce to `Broker::handle`. The Service Worker intercepts `fetch` on
//! a virtual `/ngsi-ld/v1/*` path so page JS talks to what looks like an
//! ordinary broker URL; the in-page API (`await broker.fetch(...)`) is for
//! pages that skip the worker.
//!
//! Single-threaded by construction: one `Broker` per JS context, driven from
//! the event loop, so no `Send`/`Sync` is required of anything held here.

use wasm_bindgen::prelude::*;

/// The handle page JS (or the Service Worker) holds.
#[wasm_bindgen]
pub struct AntaresBroker {
    inner: crate::Broker,
}

#[wasm_bindgen]
impl AntaresBroker {
    /// `new AntaresBroker(allowPrivateEgress?)` — one broker per JS context.
    ///
    /// `allowPrivateEgress: true` is `ANTARES_EGRESS_ALLOW_PRIVATE` for a
    /// target with NO process environment (`std::env::var` always errs on
    /// wasm32, so the env route cannot work). The Node tier (N7a) needs it:
    /// the ETSI suite's notification receivers live on 127.0.0.1, which the
    /// I4 egress policy denies by default.
    #[wasm_bindgen(constructor)]
    pub fn new(allow_private_egress: Option<bool>) -> Self {
        console_error_panic_hook::set_once();
        antares_jsonld::loader::allow_private_egress(allow_private_egress == Some(true));
        Self {
            inner: crate::Broker::new(),
        }
    }

    /// `await AntaresBroker.persistent(file?, allowPrivateEgress?)` — the
    /// N4 OPFS-backed broker: the same redb write-through shadow as native
    /// `file` mode (commit-before-ack, format check, boot rebuild), storage
    /// supplied by an exclusive OPFS sync access handle. Dedicated workers
    /// only; a second opener gets the N4b "another tab owns this store"
    /// error instead of a torn file.
    #[wasm_bindgen]
    pub async fn persistent(
        file: Option<String>,
        allow_private_egress: Option<bool>,
    ) -> Result<AntaresBroker, JsValue> {
        console_error_panic_hook::set_once();
        antares_jsonld::loader::allow_private_egress(allow_private_egress == Some(true));
        let name = file.unwrap_or_else(|| "antares.redb".to_owned());
        let backend = crate::opfs::OpfsBackend::acquire(&name)
            .await
            .map_err(|e| JsValue::from_str(&e))?;
        let db = redb::Database::builder()
            .create_with_backend(backend)
            .map_err(|e| JsValue::from_str(&format!("opening redb over OPFS: {e}")))?;
        let store = antares_sql::store::Store::from_database(db, &format!("opfs:{name}"))
            .map_err(|e| JsValue::from_str(&e))?;
        Ok(Self {
            inner: crate::Broker::with_store(store, "file"),
        })
    }

    /// `broker.onNotification(prefix, callback)` — subscription endpoints
    /// whose URL starts with `prefix` are delivered to `callback(url,
    /// bodyText)` instead of the network (a page has no inbound socket, N3).
    /// One registration per module instance; endpoints outside the prefix
    /// still leave via fetch.
    #[wasm_bindgen(js_name = onNotification)]
    pub fn on_notification(&self, prefix: String, callback: js_sys::Function) {
        // send_wrapper: js_sys::Function is !Send; the module is
        // single-threaded (same argument as HttpClient, N2).
        let cb = send_wrapper::SendWrapper::new(callback);
        antares_api::page_sink::set(
            prefix,
            Box::new(move |url, body| {
                let body = String::from_utf8_lossy(body).into_owned();
                cb.call2(
                    &wasm_bindgen::JsValue::NULL,
                    &wasm_bindgen::JsValue::from_str(url),
                    &wasm_bindgen::JsValue::from_str(&body),
                )
                .is_ok()
            }),
        );
    }

    /// `await broker.fetch(request)` — takes and returns the browser's own
    /// `Request`/`Response`, so a caller cannot tell this from a network
    /// broker. The Service Worker's `fetch` handler passes its event request
    /// straight through.
    #[wasm_bindgen]
    pub async fn fetch(&mut self, request: web_sys::Request) -> Result<web_sys::Response, JsValue> {
        let method = request.method();
        let url = request.url();
        // Path + query only: the router is mounted at the origin's root, and
        // the virtual origin is whatever page served the worker.
        let path = match url.find("://").and_then(|i| url[i + 3..].find('/')) {
            Some(i) => url[url.find("://").map_or(0, |j| j + 3) + i..].to_owned(),
            None => "/".to_owned(),
        };

        let mut builder = http::Request::builder().method(method.as_str()).uri(path);
        // Headers: NGSILD-Tenant, Content-Type, Link and Accept all matter to
        // the binding, so carry every one rather than a hand-picked subset.
        let headers = js_sys::try_iter(&request.headers())?
            .ok_or_else(|| JsValue::from_str("headers are not iterable"))?;
        for entry in headers {
            let pair = js_sys::Array::from(&entry?);
            let (Some(name), Some(value)) = (pair.get(0).as_string(), pair.get(1).as_string())
            else {
                continue;
            };
            builder = builder.header(name, value);
        }

        let body = wasm_bindgen_futures::JsFuture::from(request.array_buffer()?).await?;
        let body = js_sys::Uint8Array::new(&body).to_vec();
        let req = builder
            .body(body)
            .map_err(|e| JsValue::from_str(&format!("bad request: {e}")))?;

        let resp = self.inner.handle(req).await;
        let (parts, bytes) = resp.into_parts();

        let headers = web_sys::Headers::new()?;
        for (name, value) in parts.headers.iter() {
            if let Ok(v) = value.to_str() {
                headers.append(name.as_str(), v)?;
            }
        }
        let init = web_sys::ResponseInit::new();
        init.set_status(parts.status.as_u16());
        init.set_headers(&headers);
        // The web Response constructor REJECTS a body (even empty) for the
        // null-body statuses — 204 in particular is all over the binding.
        let status = parts.status.as_u16();
        if matches!(status, 101 | 103 | 204 | 205 | 304) {
            web_sys::Response::new_with_opt_buffer_source_and_init(None, &init)
        } else {
            let body = js_sys::Uint8Array::from(bytes.as_slice());
            web_sys::Response::new_with_opt_buffer_source_and_init(Some(&body.into()), &init)
        }
    }
}

impl Default for AntaresBroker {
    fn default() -> Self {
        Self::new(None)
    }
}
