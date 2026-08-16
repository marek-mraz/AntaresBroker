// The loopback host. The broker's own outbound HTTP (federation
// forwards, notifications outside the page sink) leaves through the global
// fetch of whatever JS context hosts it — and a fetch made INSIDE a Service
// Worker or dedicated worker never re-enters that worker's own handler. So
// requests to this virtual host are routed straight back into the same
// broker instance. That is what lets one tenant hold Context Source
// Registrations pointing at ANOTHER tenant of the same in-browser broker
// (CSR endpoint = LOOPBACK, CSR tenant = the peer tenant, CIM 009 5.2.9).
export const LOOPBACK = "http://self.antares.internal";

// `getBroker` returns the AntaresBroker (or a promise of it). Everything
// else falls through to the native fetch untouched.
export function installLoopback(getBroker) {
  const native = self.fetch.bind(self);
  self.fetch = (input, init) => {
    const url = typeof input === "string" ? input : input.url;
    if (typeof url === "string" && url.startsWith(LOOPBACK)) {
      const req = input instanceof Request ? input : new Request(input, init);
      return Promise.resolve(getBroker())
        .then((b) => b.fetch(req))
        .then((resp) => {
          // A constructed Response has url:"" — reqwest's wasm backend does
          // Url::parse(resp.url()) and aborts on it (node-shim lesson).
          Object.defineProperty(resp, "url", { value: req.url });
          return resp;
        });
    }
    return native(input, init);
  };
}
