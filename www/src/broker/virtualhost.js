// The API-console virtual host — loopback.js's pattern one prefix further in:
// same-origin fetches to /ngsi-ld/* are routed into the in-tab broker
// (whatever transport mode is live) instead of the network. This is what lets
// RapiDoc's "Execute" — or anything pasted into the devtools console — talk
// to the wasm broker as if a server were listening on this origin.
// The handler is injected (brokerFetch) so tests can install a spy.
export function installVirtualHost(handler) {
  const native = self.fetch.bind(self);
  self.fetch = async (input, init) => {
    const raw = typeof input === "string" ? input : input.url;
    const url = new URL(raw, location.origin);
    if (url.origin !== location.origin || !url.pathname.startsWith("/ngsi-ld/")) {
      return native(input, init);
    }
    const req =
      input instanceof Request
        ? init ? new Request(input, init) : input
        : new Request(url.href, init);
    const headers = {};
    req.headers.forEach((v, k) => (headers[k] = v));
    const body =
      req.method === "GET" || req.method === "HEAD" ? null : await req.text();
    return handler(url.pathname + url.search, {
      method: req.method,
      headers,
      body: body || undefined,
    });
  };
}
