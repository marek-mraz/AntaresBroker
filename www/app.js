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

// ---- transport: SW when possible, in-page module otherwise ----------------
let pageBroker = null;
let mode = "in-page";

async function brokerFetch(path, opts) {
  if (mode === "service-worker") return fetch(path, opts);
  const req = new Request(new URL(path, location.origin), opts);
  return pageBroker.fetch(req);
}

async function boot() {
  if ("serviceWorker" in navigator && location.protocol !== "file:") {
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
  if (mode !== "service-worker") {
    await init();
    pageBroker = new AntaresBroker();
    pageBroker.onNotification(NOTIFY_ENDPOINT, (url, body) => {
      showNotification(JSON.parse(body));
      return true;
    });
  } else {
    new BroadcastChannel("antares-notifications").onmessage = (e) =>
      showNotification(e.data.body);
  }
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
boot().catch((e) => log(`boot failed: ${e}`, "err"));
