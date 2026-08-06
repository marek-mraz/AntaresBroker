// N6: demo page logic. Prefers the Service Worker broker (real URLs, shared
// across tabs); falls back to an in-page broker when module SWs are
// unavailable (Firefox) or the page is opened from file://.
import init, { AntaresBroker } from "./pkg/antares_wasm.js";

const CORE_CTX =
  "https://uri.etsi.org/ngsi-ld/v1/ngsi-ld-core-context-v1.8.jsonld";
const NOTIFY_ENDPOINT = "http://page.local/demo";

const $ = (id) => document.getElementById(id);
const log = (line, cls = "") => {
  const el = document.createElement("div");
  el.textContent = line;
  if (cls) el.className = cls;
  $("log").prepend(el);
};

// ---- transport ladder: OPFS worker (persistent) → SW → in-page ------------
// ?allowPrivateEgress=1 — harness knob (N7b): the ETSI mocks live on
// loopback, which the §16.4 egress policy denies by default. The demo keeps
// the deny; the ETSI proxy opens the page with the override.
const ALLOW_PRIVATE_EGRESS = new URLSearchParams(location.search).has(
  "allowPrivateEgress",
);
let pageBroker = null;
let worker = null;
let workerDead = null; // Error once the worker has crashed (fail fast)
let mode = "in-page";
let workerSeq = 0;
const workerWaiters = new Map();

function callWorker(msg, transfer = []) {
  return new Promise((resolve, reject) => {
    if (workerDead) {
      reject(workerDead);
      return;
    }
    const id = ++workerSeq;
    workerWaiters.set(id, { resolve, reject });
    worker.postMessage({ id, ...msg }, transfer);
  });
}

async function brokerFetch(path, opts = {}) {
  if (mode === "opfs-worker") {
    const body = opts.body ? new TextEncoder().encode(opts.body) : null;
    const r = await callWorker(
      {
        kind: "fetch",
        req: {
          method: opts.method ?? "GET",
          url: new URL(path, location.origin).href,
          headers: opts.headers ?? {},
          body,
        },
      },
      body ? [body.buffer] : [],
    );
    // 204-class statuses reject a body in the Response constructor.
    const noBody = [101, 103, 204, 205, 304].includes(r.status);
    return new Response(noBody ? null : r.body, {
      status: r.status,
      headers: r.headers,
    });
  }
  if (mode === "service-worker") return fetch(path, opts);
  const req = new Request(new URL(path, location.origin), opts);
  return pageBroker.fetch(req);
}

// N4: try to own the OPFS store from a dedicated worker. Retried once — a
// just-closed tab's handle releases asynchronously on navigation.
async function bootPersistent() {
  for (let attempt = 0; attempt < 2; attempt++) {
    worker = new Worker("./worker.js", { type: "module" });
    worker.onmessage = (e) => {
      const m = e.data;
      if (m.kind === "notification") {
        showNotification(JSON.parse(m.body));
        return;
      }
      const w = workerWaiters.get(m.id);
      if (!w) return;
      workerWaiters.delete(m.id);
      m.ok ? w.resolve(m) : w.reject(new Error(m.error));
    };
    // A dead worker must FAIL every pending and future call, loudly — a
    // silent crash otherwise leaves callers pending forever (N7b: the ETSI
    // run froze for an hour on a worker that had died without a trace).
    worker.onerror = (e) => {
      const msg = `opfs worker error: ${e.message ?? e}`;
      console.error(msg);
      log(msg, "err");
      workerDead = new Error(msg);
      for (const [id, w] of workerWaiters) {
        workerWaiters.delete(id);
        w.reject(workerDead);
      }
    };
    try {
      await callWorker({
        kind: "init",
        file: "antares.redb",
        allowPrivateEgress: ALLOW_PRIVATE_EGRESS,
      });
      mode = "opfs-worker";
      return true;
    } catch (e) {
      worker.terminate();
      worker = null;
      log(`persistence unavailable: ${e.message}`, "err");
      // Only the exclusive-owner case is worth one retry.
      if (!String(e.message).includes("another tab")) return false;
      await new Promise((r) => setTimeout(r, 400));
    }
  }
  return false;
}

async function boot() {
  if (await bootPersistent()) {
    // fallthrough to shared boot tail below
  } else if ("serviceWorker" in navigator && location.protocol !== "file:") {
    try {
      await navigator.serviceWorker.register("./sw.js", { type: "module" });
      await navigator.serviceWorker.ready;
      // First visit: the SW activates but the page is not yet CONTROLLED, so
      // fetches would bypass it. clients.claim() flips control moments later
      // — wait for it (bounded) instead of racing it.
      if (!navigator.serviceWorker.controller) {
        await new Promise((resolve) => {
          const t = setTimeout(resolve, 3000);
          navigator.serviceWorker.addEventListener(
            "controllerchange",
            () => {
              clearTimeout(t);
              resolve();
            },
            { once: true },
          );
        });
      }
      if (navigator.serviceWorker.controller) {
        mode = "service-worker";
      }
    } catch (e) {
      console.warn("module service worker unavailable, using in-page broker", e);
    }
  }
  if (mode === "service-worker") {
    new BroadcastChannel("antares-notifications").onmessage = (e) =>
      showNotification(e.data.body);
  } else if (mode !== "opfs-worker") {
    await init();
    pageBroker = new AntaresBroker(ALLOW_PRIVATE_EGRESS);
    pageBroker.onNotification(NOTIFY_ENDPOINT, (url, body) => {
      showNotification(JSON.parse(body));
      return true;
    });
  } // opfs-worker: notifications arrive on the worker port (bootPersistent)
  $("mode").textContent = mode;
  const health = await (await brokerFetch("/q/health")).json();
  $("health").textContent = JSON.stringify(health);
  log(`broker up (${mode}), store=${health.store}`, "ok");
  await refresh();
}

// ---- demo actions ---------------------------------------------------------
function showNotification(n) {
  const ids = (n.data || []).map((e) => e.id).join(", ");
  log(`NOTIFICATION ${n.id ?? ""} → ${ids}`, "notif");
}

async function createEntity() {
  const id = `urn:ngsi-ld:Room:${crypto.randomUUID().slice(0, 8)}`;
  const body = {
    id,
    type: "Room",
    temperature: { type: "Property", value: Math.round(15 + Math.random() * 15) },
    "@context": CORE_CTX,
  };
  const r = await brokerFetch("/ngsi-ld/v1/entities", {
    method: "POST",
    headers: { "Content-Type": "application/ld+json" },
    body: JSON.stringify(body),
  });
  log(`POST /entities ${id} → ${r.status}`, r.status === 201 ? "ok" : "err");
  await refresh();
}

async function bumpTemperature(id) {
  const r = await brokerFetch(
    `/ngsi-ld/v1/entities/${encodeURIComponent(id)}/attrs/temperature`,
    {
      method: "PATCH",
      headers: { "Content-Type": "application/ld+json" },
      body: JSON.stringify({
        type: "Property",
        value: Math.round(15 + Math.random() * 15),
        "@context": CORE_CTX,
      }),
    },
  );
  log(`PATCH ${id} temperature → ${r.status}`, r.status === 204 ? "ok" : "err");
  await refresh();
}

async function subscribe() {
  const body = {
    id: `urn:ngsi-ld:Subscription:demo-${crypto.randomUUID().slice(0, 8)}`,
    type: "Subscription",
    entities: [{ type: "Room" }],
    notification: { endpoint: { uri: NOTIFY_ENDPOINT, accept: "application/json" } },
    "@context": CORE_CTX,
  };
  const r = await brokerFetch("/ngsi-ld/v1/subscriptions", {
    method: "POST",
    headers: { "Content-Type": "application/ld+json" },
    body: JSON.stringify(body),
  });
  log(`POST /subscriptions → ${r.status}`, r.status === 201 ? "ok" : "err");
}

async function refresh() {
  const r = await brokerFetch("/ngsi-ld/v1/entities?type=Room&limit=100");
  const rooms = r.ok ? await r.json() : [];
  const list = $("entities");
  list.replaceChildren();
  for (const e of rooms) {
    const li = document.createElement("li");
    const btn = document.createElement("button");
    btn.textContent = "bump";
    btn.onclick = () => bumpTemperature(e.id);
    li.textContent = `${e.id} — temperature ${e.temperature?.value ?? "?"} `;
    li.appendChild(btn);
    list.appendChild(li);
  }
}

$("create").onclick = createEntity;
$("subscribe").onclick = subscribe;
$("refresh").onclick = refresh;
// N7b: the ETSI browser-tier proxy drives the broker through this hook —
// the same transport ladder the buttons use, whatever mode won.
window.brokerFetch = brokerFetch;
boot().catch((e) => log(`boot failed: ${e}`, "err"));
