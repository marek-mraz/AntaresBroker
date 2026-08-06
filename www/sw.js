// N3: the Service Worker glue — an NGSI-LD context broker living entirely
// inside this worker. Requests to the virtual origin path /ngsi-ld/v1/* (and
// /q/*) never touch the network: they are fed straight into the same axum
// router the native binary runs, compiled to WebAssembly.
//
// This is a MODULE service worker (registered with {type: "module"}), which
// Chromium-family browsers support; on browsers without module SWs the demo
// page falls back to the in-page API automatically (see app.js). One broker
// instance per SW lifetime — memory store, so state survives page reloads
// but not a SW restart (persistence is the OPFS story, tasks.md N4).
import init, { AntaresBroker } from "./pkg/antares_wasm.js";

const ready = (async () => {
  await init();
  const broker = new AntaresBroker();
  // Notifications to the reserved page endpoint fan out to every open tab.
  const channel = new BroadcastChannel("antares-notifications");
  broker.onNotification("http://page.local/", (url, body) => {
    channel.postMessage({ url, body: JSON.parse(body), at: Date.now() });
    return true;
  });
  return broker;
})();

self.addEventListener("install", () => self.skipWaiting());
self.addEventListener("activate", (e) => e.waitUntil(self.clients.claim()));

self.addEventListener("fetch", (event) => {
  const url = new URL(event.request.url);
  const ours =
    url.origin === self.location.origin &&
    (url.pathname.startsWith("/ngsi-ld/") || url.pathname.startsWith("/q/"));
  if (!ours) return; // static assets etc. go to the network as usual
  event.respondWith(ready.then((broker) => broker.fetch(event.request)));
});
