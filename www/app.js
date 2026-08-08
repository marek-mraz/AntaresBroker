// N6/N9: playground logic. Transport ladder unchanged (OPFS worker →
// Service Worker → in-page); on top of it, the "context space" GRAPH: each
// space is a tenant of this one in-browser broker rendered as a draggable
// bubble, data sources are small bubbles, and the edges are the actual
// wiring — Context Source Registrations (CIM 009 5.2.9 / 4.3.6, dashed) and
// pipelines (dotted). Parallel edges between the same pair fan out with
// distinct curvatures so they never overlap. The inspector panel carries the
// per-space controls, the entity list and a raw NGSI-LD payload editor.
import init, { AntaresBroker } from "./pkg/antares_wasm.js";
import { LOOPBACK, installLoopback } from "./loopback.js";

// uuid() is secure-context-AND-modern-browser only — it throws on
// plain HTTP and on older browsers/webviews. Fall back to getRandomValues (no
// secure-context requirement), then Math.random, so the playground never
// hard-fails on `crypto.randomUUID is not a function`.
function uuid() {
  const c = globalThis.crypto;
  if (c && typeof c.randomUUID === "function") return c.randomUUID();
  const b = new Uint8Array(16);
  if (c && typeof c.getRandomValues === "function") c.getRandomValues(b);
  else for (let i = 0; i < 16; i++) b[i] = Math.floor(Math.random() * 256);
  b[6] = (b[6] & 0x0f) | 0x40; // version 4
  b[8] = (b[8] & 0x3f) | 0x80; // variant
  const h = [...b].map((x) => x.toString(16).padStart(2, "0")).join("");
  return `${h.slice(0, 8)}-${h.slice(8, 12)}-${h.slice(12, 16)}-${h.slice(16, 20)}-${h.slice(20)}`;
}

const CORE_CTX =
  "https://uri.etsi.org/ngsi-ld/v1/ngsi-ld-core-context-v1.8.jsonld";
const NOTIFY_ENDPOINT = "http://page.local/demo";

const $ = (id) => document.getElementById(id);
const log = (line, cls = "") => {
  const el = document.createElement("div");
  el.textContent = line;
  if (cls) el.className = cls;
  $("log").prepend(el);
  while ($("log").childElementCount > 200) $("log").lastChild.remove();
};
$("logbtn").onclick = () => $("logwrap").classList.toggle("min");
$("reqlog").onclick = () => {
  REQLOG = !REQLOG;
  save("antares.reqlog", REQLOG);
  $("reqlog").textContent = `🛰 requests: ${REQLOG ? "on" : "off"}`;
  if (REQLOG) $("logwrap").classList.remove("min");
  log(`request logging ${REQLOG ? "ON — every NGSI-LD call shows here and in the console" : "off"}`, "ok");
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

// Request log: EVERY NGSI-LD/broker call through brokerFetch lands in the 📜
// log and the browser console (method, tenant, path, status). Toggleable via
// the 🛰 header button; defaults ON (off under the ETSI harness — thousands
// of requests would just churn the DOM).
let REQLOG =
  JSON.parse(localStorage.getItem("antares.reqlog") ?? "null") ?? !ALLOW_PRIVATE_EGRESS;

async function brokerFetch(path, opts = {}) {
  const r = await rawBrokerFetch(path, opts);
  if (REQLOG) {
    const line = `[${opts.headers?.["NGSILD-Tenant"] ?? "default"}] ${
      (opts.method ?? "GET").toUpperCase()} ${path} → ${r.status}`;
    console.debug("[ngsi-ld]", line);
    log(line, r.ok || r.status === 207 ? "req" : "err");
  }
  return r;
}

async function rawBrokerFetch(path, opts = {}) {
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
    // silent crash otherwise leaves callers pending forever (N7b lesson).
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
        // Trust but verify: a controlling SW whose in-worker broker failed to
        // boot (stale cache / glue-wasm skew) intercepts nothing — API calls
        // would fall through to the real server as 404s. Probe before
        // committing; on failure the in-page broker takes over.
        try {
          const probe = await fetch("/q/health");
          if (!probe.ok) throw new Error(`health ${probe.status}`);
          mode = "service-worker";
        } catch (e) {
          log(`service worker controls the page but its broker is dead (${e.message}) — using the in-page broker`, "err");
        }
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
    installLoopback(() => pageBroker);
    pageBroker.onNotification(NOTIFY_ENDPOINT, (url, body) => {
      showNotification(JSON.parse(body));
      return true;
    });
  } // opfs-worker: notifications arrive on the worker port (bootPersistent)
  // First visit: the board demos itself — a small federated city instead of
  // an empty canvas. Once per browser (flag), only on a pristine board,
  // never under the ETSI harness. Runs before the health pill shows so
  // automation (browser-test) sees a settled board.
  if (!localStorage.getItem("antares.demoed") && !ALLOW_PRIVATE_EGRESS) {
    await refreshAll();
    const pristine =
      !pipes.length &&
      ![...links.values()].some((ls) => ls.length) &&
      ![...ents.values()].some((c) => c.local.length);
    if (pristine) await createDemo();
    localStorage.setItem("antares.demoed", "1");
  }
  $("mode").textContent = mode;
  $("reqlog").textContent = `🛰 requests: ${REQLOG ? "on" : "off"}`;
  if (REQLOG) $("logwrap").classList.remove("min");
  if (mode !== "opfs-worker") {
    // Non-OPFS modes have NO durable store: a service worker is torn down by
    // the browser when idle and takes the in-memory entities with it.
    $("mode").textContent = `${mode} ⚠ ephemeral`;
    $("mode").parentElement.title =
      "no persistence — this mode's store resets whenever its worker restarts; close the tab holding antares.redb and reload to get opfs-worker";
    log("⚠ ephemeral store: entities vanish when this worker restarts (close the other tab owning antares.redb, then reload)", "err");
  }
  const health = await (await brokerFetch("/q/health")).json();
  // Compact — but keep the '"store":' shape browser-test.mjs keys on.
  $("health").textContent = JSON.stringify({ store: health.store, status: health.status });
  log(`broker up (${mode}), store=${health.store}`, "ok");
  select("s:default");
  await refreshAll();
  for (const p of pipes) if (p.running) startPipe(p);
  setInterval(refreshAll, 3000);
}

// ---- context spaces --------------------------------------------------------
// A space IS a tenant. "default" always exists (the spec's default tenant).
const PALETTE = ["#6d5ef1", "#19a974", "#e8850c", "#d9534f", "#3b82f6", "#b45fd9",
  "#0ea5a3", "#b8860b"];
const EMOJI = ["🏙", "🏔", "🛰", "🏭", "🌊", "🌳", "🎡", "🚉"];
const TYPES = {
  Room: { emoji: "🚪", attr: "temperature", gen: () => Math.round(15 + Math.random() * 15) },
  TemperatureSensor: { emoji: "🌡", attr: "temperature", gen: (t) => Math.round(180 + 60 * Math.sin(t / 20e3) + 20 * Math.random()) / 10 },
  ParkingSpot: { emoji: "🚗", attr: "occupied", gen: () => (Math.random() < 0.5 ? 0 : 1) },
  Streetlight: { emoji: "💡", attr: "powerDraw", gen: () => Math.round(Math.random() * 60) },
  AirQualitySensor: { emoji: "🌫", attr: "pm25", gen: (t) => Math.round(250 + 180 * Math.sin(t / 45e3) + 70 * Math.random()) / 10 },
  NoiseSensor: { emoji: "🔊", attr: "decibels", gen: () => Math.round(35 + Math.random() * 50) },
  WaterLevelSensor: { emoji: "🌊", attr: "level", gen: (t) => Math.round(220 + 160 * Math.sin(t / 60e3) + 20 * Math.random()) / 100 },
  EnergyMeter: { emoji: "⚡", attr: "consumption", gen: (t) => Math.round(300 + 220 * Math.sin(t / 30e3) + 90 * Math.random()) / 10 },
  TrafficCounter: { emoji: "🚦", attr: "vehiclesPerMin", gen: () => Math.round(Math.random() * 45) },
  BikeStation: { emoji: "🚲", attr: "availableBikes", gen: () => Math.round(Math.random() * 20) },
  WasteContainer: { emoji: "🗑", attr: "fillLevel", gen: () => Math.round(Math.random() * 100) },
  // Decidim (participatory democracy platform): proposal support counts as a
  // live signal — participation is city data too.
  CitizenProposal: { emoji: "🗳", attr: "supports", gen: () => Math.round(Math.random() * 500) },
};
const ALL_TYPES = Object.keys(TYPES).join(",");

const load = (k, d) => JSON.parse(localStorage.getItem(k) ?? "null") ?? d;
const save = (k, v) => localStorage.setItem(k, JSON.stringify(v));
// The default board shows a small federated city: at least 7 tenants.
const SEED_SPACES = ["default", "smart-city", "old-town", "harbor", "airport",
  "university", "energy-grid", "transit"];
let spaces = load("antares.spaces", SEED_SPACES.map((name) => ({ name })));
if (!spaces.some((s) => s.name === "default")) spaces.unshift({ name: "default" });
// Existing saved boards top up to the 7-tenant minimum from the seed list.
for (const name of SEED_SPACES) {
  if (spaces.length >= 7) break;
  if (!spaces.some((s) => s.name === name)) spaces.push({ name });
}
save("antares.spaces", spaces);
let pipes = load("antares.pipes", []);
const fedView = new Set(load("antares.fedview", []));
const links = new Map(); // space -> [{id, to, type}]
const ents = new Map(); // space -> {local: [], remote: []}

const color = (name) =>
  PALETTE[[...name].reduce((a, c) => a + c.charCodeAt(0), 0) % PALETTE.length];
const avatar = (name) =>
  name === "default"
    ? "⭐"
    : EMOJI[[...name].reduce((a, c) => a + c.charCodeAt(0), 0) % EMOJI.length];

// Tenant-scoped broker call. The default tenant sends no header (6.3.14).
function api(space, path, opts = {}) {
  const headers = { ...(opts.headers ?? {}) };
  if (space !== "default") headers["NGSILD-Tenant"] = space;
  return brokerFetch(path, { ...opts, headers });
}

// ---- the 2D graph ----------------------------------------------------------
// Node keys: "s:<space>" (bubble = tenant) and "p:<pipeId>" (small bubble =
// data source). Positions persist; new nodes land on a golden-angle spiral.
const positions = load("antares.pos", {});
let sel = "s:default";
let dragging = null; // {key, dx, dy}

const svgNS = "http://www.w3.org/2000/svg";
const graph = $("graph");

function nodeList() {
  const out = spaces.map((s) => ({
    key: `s:${s.name}`,
    kind: "space",
    name: s.name,
    r: 44 + Math.min(26, (ents.get(s.name)?.local.length ?? 0) * 2),
  }));
  for (const p of pipes) {
    if (p.kind === "source") {
      out.push({ key: `p:${p.id}`, kind: "source", pipe: p, r: 26 });
    }
  }
  return out;
}

// Zoom = a viewBox scale. bounds() reports the WORLD rect (screen ÷ zoom), so
// zooming out genuinely gives bubbles more room instead of just shrinking
// pixels; all node coordinates live in world space.
let zoom = load("antares.zoom", 1);

function bounds() {
  const r = graph.getBoundingClientRect();
  return { w: Math.max(r.width, 320) / zoom, h: Math.max(r.height, 320) / zoom };
}

function applyZoom() {
  const { w, h } = bounds();
  graph.setAttribute("viewBox", `0 0 ${w} ${h}`);
  $("zoomlvl").textContent = `${Math.round(zoom * 100)}%`;
}

function setZoom(z) {
  zoom = Math.min(1.5, Math.max(0.4, Math.round(z * 100) / 100));
  save("antares.zoom", zoom);
  applyZoom();
  resolveOverlaps(); // zooming IN shrinks the world — clamp bubbles back
  renderGraph();
}
$("zoomout").onclick = () => setZoom(zoom - 0.15);
$("zoomin").onclick = () => setZoom(zoom + 0.15);
applyZoom();

function pos(key, i = 0, n = 1) {
  if (!positions[key]) {
    const { w, h } = bounds();
    const ang = i * 2.399963, rad = 90 + 42 * Math.sqrt(i + (key.startsWith("p:") ? n : 0));
    positions[key] = {
      x: w / 2 + rad * Math.cos(ang),
      y: h / 2 + rad * Math.sin(ang),
    };
  }
  return positions[key];
}

// Push-apart collision pass — bubbles never overlap, everything stays on the
// canvas. Cheap O(n²) per iteration; n is tens at most.
function resolveOverlaps(pinned = null) {
  const nodes = nodeList();
  const { w, h } = bounds();
  for (let it = 0; it < 40; it++) {
    let moved = false;
    for (let a = 0; a < nodes.length; a++) {
      for (let b = a + 1; b < nodes.length; b++) {
        const A = pos(nodes[a].key, a, nodes.length);
        const B = pos(nodes[b].key, b, nodes.length);
        const min = nodes[a].r + nodes[b].r + 18;
        let dx = B.x - A.x, dy = B.y - A.y;
        let d = Math.hypot(dx, dy);
        if (d >= min) continue;
        if (d < 1) { dx = 1; dy = 0; d = 1; }
        const push = (min - d) / 2 + 0.5;
        const ux = dx / d, uy = dy / d;
        if (nodes[a].key !== pinned) { A.x -= ux * push; A.y -= uy * push; }
        if (nodes[b].key !== pinned) { B.x += ux * push; B.y += uy * push; }
        moved = true;
      }
    }
    for (const nd of nodes) {
      const P = pos(nd.key);
      P.x = Math.min(w - nd.r - 6, Math.max(nd.r + 6, P.x));
      P.y = Math.min(h - nd.r - 6, Math.max(nd.r + 24, P.y));
    }
    if (!moved) break;
  }
  save("antares.pos", positions);
}

// Every edge on the board: CSR federation links + pipelines. Grouped per
// unordered node pair; each edge in a group gets its own curvature slot so
// parallel edges fan out instead of overlapping.
function edgeList() {
  const out = [];
  for (const [from, ls] of links) {
    for (const l of ls) {
      // data flows peer → registrant
      out.push({ kind: "fed", key: `fed:${l.id}`, a: `s:${l.to}`, b: `s:${from}`, label: l.type ?? "all types", reg: { space: from, id: l.id, to: l.to } });
    }
  }
  for (const p of pipes) {
    if (p.kind === "sync") out.push({ kind: "pipe", key: `pipe:${p.id}`, a: `s:${p.from}`, b: `s:${p.into}`, label: `${p.type} / ${p.secs}s`, pipe: p });
    if (p.kind === "source") out.push({ kind: "pipe", key: `pipe:${p.id}`, a: `p:${p.id}`, b: `s:${p.into}`, label: `${p.secs}s`, pipe: p });
  }
  const groups = new Map();
  for (const e of out) {
    const gk = [e.a, e.b].sort().join("|");
    if (!groups.has(gk)) groups.set(gk, []);
    groups.get(gk).push(e);
  }
  for (const g of groups.values()) {
    g.forEach((e, i) => {
      e.slot = i - (g.length - 1) / 2;
    });
  }
  return out;
}

function el(tag, attrs = {}, parent = null) {
  const n = document.createElementNS(svgNS, tag);
  for (const [k, v] of Object.entries(attrs)) n.setAttribute(k, v);
  if (parent) parent.appendChild(n);
  return n;
}

function renderGraph() {
  const nodes = nodeList();
  nodes.forEach((n, i) => pos(n.key, i, nodes.length));
  graph.replaceChildren();

  const defs = el("defs", {}, graph);
  for (const [id, c] of [["arrow-fed", "var(--fed)"], ["arrow-pipe", "var(--ok)"]]) {
    const m = el("marker", { id, viewBox: "0 0 10 10", refX: 9, refY: 5,
      markerWidth: 7, markerHeight: 7, orient: "auto-start-reverse" }, defs);
    el("path", { d: "M 0 1 L 9 5 L 0 9 z", fill: c }, m);
  }

  const eg = el("g", {}, graph);
  const byKey = new Map(nodes.map((n) => [n.key, n]));
  for (const e of edgeList()) {
    const A = byKey.get(e.a), B = byKey.get(e.b);
    if (!A || !B) continue;
    const pa = pos(e.a), pb = pos(e.b);
    let dx = pb.x - pa.x, dy = pb.y - pa.y;
    const d = Math.hypot(dx, dy) || 1;
    const ux = dx / d, uy = dy / d, px = -uy, py = ux;
    // curvature slot → perpendicular control-point offset (never overlap)
    const bend = e.slot * 46 + (e.slot === 0 ? 0 : Math.sign(e.slot) * 8);
    const mx = (pa.x + pb.x) / 2 + px * bend;
    const my = (pa.y + pb.y) / 2 + py * bend;
    // trim endpoints to the bubble borders, aimed at the control point
    const aim = (P, r, tx, ty) => {
      const vx = tx - P.x, vy = ty - P.y, vd = Math.hypot(vx, vy) || 1;
      return { x: P.x + (vx / vd) * (r + 4), y: P.y + (vy / vd) * (r + 4) };
    };
    const s = aim(pa, A.r, mx, my);
    const t = aim(pb, B.r, mx, my);
    const dpath = `M ${s.x} ${s.y} Q ${mx} ${my} ${t.x} ${t.y}`;
    const hit = el("path", { d: dpath, class: "hit" }, eg);
    const p = el("path", {
      d: dpath, class: `edge ${e.kind}`,
      stroke: e.kind === "fed" ? "var(--fed)" : "var(--ok)",
      "stroke-dasharray": e.kind === "fed" ? "7 6" : "2 7",
      "marker-end": `url(#arrow-${e.kind === "fed" ? "fed" : "pipe"})`,
    }, eg);
    // Flow animation is EVIDENCE, not decoration: an edge pulses only while
    // a real transfer just happened on it (fed query that returned entities,
    // pipe tick that wrote). Idle edges stay static — dashes mark the kind.
    const b = bursts.get(e.key);
    const bursting = b && b.until > Date.now();
    if (bursting) {
      p.innerHTML = `<animate attributeName="stroke-dashoffset" from="26" to="0" dur="0.45s" repeatCount="indefinite"/>`;
      p.setAttribute("opacity", "1");
      p.setAttribute("stroke-width", "3.5");
    }
    if (e.pipe && !e.pipe.running) p.setAttribute("opacity", "0.25");
    const lab = el("text", { class: `edgelabel ${e.kind}`, x: mx, y: my - 5, "text-anchor": "middle" }, eg);
    lab.textContent = (e.kind === "fed" ? `CSR · ${e.label}` : `⏱ ${e.label}`) +
      (bursting && b.count ? `  ·  ${b.count} ⇢` : "");
    const title = el("title", {}, hit);
    title.textContent = e.kind === "fed"
      ? `Context Source Registration: queries in "${e.reg.space}" include "${e.reg.to}" (${e.label}) — click to delete`
      : `pipeline every ${e.pipe.secs}s — click to pause/resume`;
    hit.onclick = () => (e.kind === "fed" ? unlink(e.reg) : togglePipe(e.pipe.id));
  }

  const ng = el("g", {}, graph);
  for (const nd of nodes) {
    const P = pos(nd.key);
    const g = el("g", { class: `bubble ${nd.kind}${sel === nd.key ? " selected" : ""}`,
      transform: `translate(${P.x} ${P.y})` }, ng);
    g.dataset.key = nd.key;
    if (nd.kind === "space") {
      const c = color(nd.name);
      el("circle", { class: "halo", r: nd.r + 7, fill: "none" }, g);
      el("circle", { class: "body", r: nd.r,
        fill: `color-mix(in srgb, ${c} 22%, var(--card))`, stroke: c }, g);
      const em = el("text", { class: "emoji", y: -4, "text-anchor": "middle" }, g);
      em.textContent = avatar(nd.name);
      const nm = el("text", { class: "name", y: 18, "text-anchor": "middle" }, g);
      nm.textContent = nd.name;
      const cur = ents.get(nd.name) ?? { local: [], remote: [] };
      const ct = el("text", { class: "count", y: 32, "text-anchor": "middle" }, g);
      ct.textContent = fedView.has(nd.name)
        ? `🏠 ${cur.local.length} · 🌐 ${cur.remote.length}`
        : `🏠 ${cur.local.length} local`;
    } else {
      el("circle", { class: "halo", r: nd.r + 6, fill: "none" }, g);
      el("circle", { class: "body", r: nd.r, fill: "color-mix(in srgb, var(--ok) 18%, var(--card))",
        stroke: "var(--ok)", "stroke-dasharray": nd.pipe.running ? "" : "4 4" }, g);
      const em = el("text", { class: "emoji", y: 1, "text-anchor": "middle" }, g);
      em.textContent = (nd.pipe.gen ?? "⚙").slice(0, 2).trim();
      const ct = el("text", { class: "count", y: 16, "text-anchor": "middle" }, g);
      ct.textContent = `${nd.pipe.ticks ?? 0}`;
    }
  }
}

// Dragging lives on the SVG ROOT, never on bubbles: renderGraph replaces all
// bubble nodes, and a pointer capture on a replaced node dies with it — the
// "sometimes I can't move bubbles" bug. The <svg> survives every re-render.
graph.addEventListener("pointerdown", (ev) => {
  const g = ev.target.closest?.(".bubble");
  if (!g) return;
  const P = pos(g.dataset.key);
  // screen deltas ÷ zoom = world deltas
  dragging = { key: g.dataset.key, x0: ev.clientX, y0: ev.clientY,
    px: P.x, py: P.y, moved: false };
  graph.setPointerCapture(ev.pointerId);
});
graph.addEventListener("pointermove", (ev) => {
  if (!dragging) return;
  const P = pos(dragging.key);
  P.x = dragging.px + (ev.clientX - dragging.x0) / zoom;
  P.y = dragging.py + (ev.clientY - dragging.y0) / zoom;
  dragging.moved = true;
  requestAnimationFrame(renderGraph);
});
graph.addEventListener("pointerup", () => {
  if (!dragging) return;
  const { key, moved } = dragging;
  dragging = null;
  if (moved) {
    resolveOverlaps(key);
    save("antares.pos", positions);
  } else {
    select(key);
  }
  renderGraph();
});
addEventListener("resize", () => {
  applyZoom();
  resolveOverlaps();
  renderGraph();
});

// ---- real-flow bursts ------------------------------------------------------
// bursts[edgeKey] marks "data actually crossed this edge just now"; the
// render loop shows motion only while a mark is fresh.
const bursts = new Map(); // edgeKey -> { until, count }
function burst(edgeKey, count = 0) {
  bursts.set(edgeKey, { until: Date.now() + 1400, count });
  renderGraph();
  setTimeout(() => {
    const cur = bursts.get(edgeKey);
    if (cur && cur.until <= Date.now()) {
      bursts.delete(edgeKey);
      renderGraph();
    }
  }, 1500);
}

function pulse(name) {
  const g = graph.querySelector(`.bubble[data-key="s:${CSS.escape(name)}"]`) ??
    [...graph.querySelectorAll(".bubble")].find((b) => b.dataset.key === `s:${name}`);
  if (!g) return;
  g.classList.add("pulse");
  setTimeout(() => g.classList.remove("pulse"), 600);
}

// ---- selection + inspector -------------------------------------------------
function selSpace() {
  if (sel.startsWith("s:")) return sel.slice(2);
  const p = pipes.find((x) => `p:${x.id}` === sel);
  return p?.into ?? "default";
}

function select(key) {
  // selecting a data-source bubble inspects the space it feeds
  if (key.startsWith("p:")) {
    const p = pipes.find((x) => `p:${x.id}` === key);
    key = `s:${p?.into ?? "default"}`;
  }
  sel = key;
  renderInspector();
  renderGraph();
}

function renderInspector() {
  const name = selSpace();
  $("insp-avatar").textContent = avatar(name);
  $("insp-name").textContent = name;
  $("insp-tenant").textContent = name;
  $("insp-remove").hidden = name === "default";
  const fedBtn = $("refresh");
  fedBtn.classList.toggle("primary", fedView.has(name));
  fedBtn.classList.toggle("ghost", !fedView.has(name));
  renderEntities(name);
  renderLinksPanel(name);
  renderPipesPanel(name);
}

function entLabel(e) {
  const type = Array.isArray(e.type) ? e.type[0] : e.type;
  const t = TYPES[type] ?? { emoji: "▪", attr: null };
  const val = t.attr ? e[t.attr]?.value : undefined;
  return { type, emoji: t.emoji, val };
}

function renderEntities(name) {
  const ul = $("entities");
  const cur = ents.get(name) ?? { local: [], remote: [] };
  const prev = new Map([...ul.children].map((li) => [li.dataset.id, li.dataset.val]));
  ul.replaceChildren();
  // The always-on count line: how many live HERE vs arrive via federation.
  const stats = document.createElement("li");
  stats.className = "sep";
  stats.innerHTML = `<span class="sub">🏠 ${cur.local.length} local · 🌐 ${
    fedView.has(name) ? `${cur.remote.length} federated` : "fed view off"}</span>`;
  ul.appendChild(stats);
  for (const e of cur.local) {
    const { emoji, val } = entLabel(e);
    const li = document.createElement("li");
    li.dataset.id = e.id;
    li.dataset.val = String(val);
    li.title = `${e.id} — click to bump`;
    li.innerHTML = `<span>${emoji}</span><span>${e.id.split(":").pop()}</span>
      <button class="edit" title="open in editor">✎</button>
      <span class="val">${val ?? "·"}</span>`;
    li.onclick = () => bump(name, e);
    li.querySelector(".edit").onclick = (ev) => {
      ev.stopPropagation();
      openInEditor(name, e.id);
    };
    if (prev.has(e.id) && prev.get(e.id) !== String(val)) li.classList.add("flash");
    ul.appendChild(li);
  }
  if (cur.remote.length) {
    const sep = document.createElement("li");
    sep.className = "sep";
    sep.innerHTML = `<span class="tag">🌐 federated — served by peers via CSR</span>`;
    ul.appendChild(sep);
  }
  for (const e of cur.remote) {
    const { emoji, val } = entLabel(e);
    const origin = originOf(e.id, name);
    const li = document.createElement("li");
    li.className = "remote";
    if (origin) li.style.borderColor = color(origin);
    li.title = `${e.id} — lives in "${origin ?? "?"}", visible here through federation`;
    li.innerHTML = `<span>${emoji}</span><span>${e.id.split(":").pop()}</span>
      <span class="tag">← ${origin ? `${avatar(origin)} ${origin}` : "fed"}</span>
      <span class="val">${val ?? "·"}</span>`;
    ul.appendChild(li);
  }
}

// Which space actually OWNS a federated entity: search the linked peers'
// local lists (already cached) — the id lives exactly one hop away here.
function originOf(id, viewer) {
  for (const l of links.get(viewer) ?? []) {
    if ((ents.get(l.to)?.local ?? []).some((e) => e.id === id)) return l.to;
  }
  for (const [space, cur] of ents) {
    if (space !== viewer && cur.local.some((e) => e.id === id)) return space;
  }
  return null;
}

function renderLinksPanel(name) {
  const box = $("links");
  box.replaceChildren();
  const ls = links.get(name) ?? [];
  if (!ls.length) {
    box.innerHTML = `<span class="sub">none — 🔗 federate registers a CSR whose
      endpoint is this same broker and whose <code>tenant</code> is the peer space</span>`;
    return;
  }
  for (const l of ls) {
    const item = document.createElement("div");
    item.className = "item";
    item.innerHTML = `<span class="tag">CSR</span>
      <span>queries here include <strong>${l.to}</strong>${l.type ? ` (${l.type})` : ""}</span>
      <button class="x" title="delete registration">✕</button>`;
    item.querySelector(".x").onclick = () => unlink({ space: name, id: l.id, to: l.to });
    box.appendChild(item);
  }
}

function renderPipesPanel(name) {
  const box = $("pipes");
  box.replaceChildren();
  const mine = pipes.filter((p) => p.into === name || p.from === name);
  if (!mine.length) {
    box.innerHTML = `<span class="sub">none — ＋ pipeline streams a data source
      into a space, or copies entities between spaces</span>`;
    return;
  }
  for (const p of mine) {
    const item = document.createElement("div");
    item.className = "item";
    const label = p.kind === "source"
      ? `${p.gen} → <strong>${p.into}</strong>`
      : `🔁 ${p.from} → <strong>${p.into}</strong> (${p.type})`;
    item.innerHTML = `<span>${label}</span>
      <span class="sub">${p.secs}s · ${p.ticks ?? 0} ticks</span>
      <button class="x toggle" title="pause/resume">${p.running ? "⏸" : "▶"}</button>
      <button class="x" title="delete">✕</button>`;
    item.querySelector(".toggle").onclick = () => togglePipe(p.id);
    item.querySelectorAll(".x")[1].onclick = () => deletePipe(p.id);
    box.appendChild(item);
  }
}

// ---- entities --------------------------------------------------------------
async function createEntity(space, type) {
  type ??= Object.keys(TYPES)[Math.floor(Math.random() * 4)];
  const t = TYPES[type];
  const id = `urn:ngsi-ld:${type}:${uuid().slice(0, 8)}`;
  const body = {
    id,
    type,
    [t.attr]: { type: "Property", value: t.gen(Date.now()) },
    "@context": CORE_CTX,
  };
  const r = await api(space, "/ngsi-ld/v1/entities", {
    method: "POST",
    headers: { "Content-Type": "application/ld+json" },
    body: JSON.stringify(body),
  });
  log(`[${space}] POST /entities ${id} → ${r.status}`, r.status === 201 ? "ok" : "err");
  pulse(space);
  await refreshSpace(space);
}

async function bump(space, e) {
  const type = Array.isArray(e.type) ? e.type[0] : e.type;
  const t = TYPES[type];
  if (!t) return;
  const r = await api(
    space,
    `/ngsi-ld/v1/entities/${encodeURIComponent(e.id)}/attrs/${t.attr}`,
    {
      method: "PATCH",
      headers: { "Content-Type": "application/ld+json" },
      body: JSON.stringify({ type: "Property", value: t.gen(Date.now()), "@context": CORE_CTX }),
    },
  );
  log(`[${space}] PATCH ${e.id} → ${r.status}`, r.status === 204 ? "ok" : "err");
  await refreshSpace(space);
}

async function subscribe(space) {
  const body = {
    id: `urn:ngsi-ld:Subscription:${space}-${uuid().slice(0, 8)}`,
    type: "Subscription",
    entities: Object.keys(TYPES).map((type) => ({ type })),
    notification: { endpoint: { uri: NOTIFY_ENDPOINT, accept: "application/json" } },
    "@context": CORE_CTX,
  };
  const r = await api(space, "/ngsi-ld/v1/subscriptions", {
    method: "POST",
    headers: { "Content-Type": "application/ld+json" },
    body: JSON.stringify(body),
  });
  log(`[${space}] POST /subscriptions → ${r.status}`, r.status === 201 ? "ok" : "err");
}

async function fetchEnts(space, { local }) {
  const q = `/ngsi-ld/v1/entities?type=${ALL_TYPES}&limit=100${local ? "&local=true" : ""}`;
  const r = await api(space, q);
  return r.ok ? await r.json() : [];
}

async function refreshSpace(space) {
  const local = await fetchEnts(space, { local: true });
  let remote = [];
  if (fedView.has(space)) {
    const fed = await fetchEnts(space, { local: false });
    const have = new Set(local.map((e) => e.id));
    remote = fed.filter((e) => !have.has(e.id));
  }
  ents.set(space, { local, remote });
  await refreshLinks(space);
  // Real flow only: pulse exactly the CSR edges that just carried entities
  // into this federated query, with the count that crossed.
  if (remote.length) {
    const perOrigin = new Map();
    for (const e of remote) {
      const o = originOf(e.id, space);
      if (o) perOrigin.set(o, (perOrigin.get(o) ?? 0) + 1);
    }
    for (const l of links.get(space) ?? []) {
      const n = perOrigin.get(l.to);
      if (n) burst(`fed:${l.id}`, n);
    }
  }
  if (selSpace() === space) {
    renderEntities(space);
    renderLinksPanel(space);
    renderPipesPanel(space);
  }
}

async function refreshAll() {
  await Promise.all(spaces.map((s) => refreshSpace(s.name)));
  // No overlap pass here: the 3s poll must never shove user-placed bubbles
  // around (it made positions feel "stuck"). Overlaps resolve on structural
  // changes only. And never re-render under an active drag.
  if (!dragging) renderGraph();
}

// ---- the entity editor -----------------------------------------------------
const edStatus = (msg, cls = "") => {
  $("editor-status").textContent = msg;
  $("editor-status").className = cls;
};

function editorTemplate() {
  const type = Object.keys(TYPES)[Math.floor(Math.random() * 4)];
  const t = TYPES[type];
  $("editor").value = JSON.stringify(
    {
      id: `urn:ngsi-ld:${type}:${uuid().slice(0, 8)}`,
      type,
      [t.attr]: { type: "Property", value: t.gen(Date.now()) },
      "@context": CORE_CTX,
    },
    null,
    2,
  );
  edStatus(`template for ${selSpace()} — edit freely, then POST`);
}

async function openInEditor(space, id) {
  const r = await api(space, `/ngsi-ld/v1/entities/${encodeURIComponent(id)}?local=true`);
  if (!r.ok) {
    edStatus(`GET ${id} → ${r.status}`, "err");
    return;
  }
  const doc = await r.json();
  $("editor").value = JSON.stringify(doc, null, 2);
  edStatus(`${id} loaded from ${space} — PUT replace to save changes`);
}

function editorDoc() {
  try {
    const doc = JSON.parse($("editor").value);
    if (!doc["@context"]) doc["@context"] = CORE_CTX;
    return doc;
  } catch (e) {
    edStatus(`not JSON: ${e.message}`, "err");
    return null;
  }
}

$("ed-template").onclick = editorTemplate;
$("ed-create").onclick = async () => {
  const doc = editorDoc();
  if (!doc) return;
  const space = selSpace();
  const r = await api(space, "/ngsi-ld/v1/entities", {
    method: "POST",
    headers: { "Content-Type": "application/ld+json" },
    body: JSON.stringify(doc),
  });
  edStatus(`POST /entities → ${r.status}${r.status === 201 ? "" : ` ${await r.text()}`}`,
    r.status === 201 ? "ok" : "err");
  log(`[${space}] editor POST ${doc.id ?? "?"} → ${r.status}`, r.status === 201 ? "ok" : "err");
  pulse(space);
  await refreshSpace(space);
  renderGraph();
};
$("ed-save").onclick = async () => {
  const doc = editorDoc();
  if (!doc?.id) {
    if (doc) edStatus("the payload needs an id to PUT", "err");
    return;
  }
  const space = selSpace();
  const r = await api(space, `/ngsi-ld/v1/entities/${encodeURIComponent(doc.id)}`, {
    method: "PUT",
    headers: { "Content-Type": "application/ld+json" },
    body: JSON.stringify(doc),
  });
  edStatus(`PUT ${doc.id} → ${r.status}${r.ok || r.status === 204 ? "" : ` ${await r.text()}`}`,
    r.status === 204 ? "ok" : "err");
  await refreshSpace(space);
};
$("ed-delete").onclick = async () => {
  const doc = editorDoc();
  if (!doc?.id) {
    if (doc) edStatus("the payload needs an id to DELETE", "err");
    return;
  }
  const space = selSpace();
  const r = await api(space, `/ngsi-ld/v1/entities/${encodeURIComponent(doc.id)}`, {
    method: "DELETE",
  });
  edStatus(`DELETE ${doc.id} → ${r.status}`, r.status === 204 ? "ok" : "err");
  await refreshSpace(space);
  renderGraph();
};

// ---- federation links (CSRs with the 5.2.9 `tenant` member) ---------------
async function refreshLinks(space) {
  // 5.10.2.4: registration discovery needs a discriminating input.
  const r = await api(space, `/ngsi-ld/v1/csourceRegistrations?type=${ALL_TYPES}&limit=100`);
  const regs = r.ok ? await r.json() : [];
  links.set(
    space,
    regs
      .filter((reg) => (reg.endpoint ?? "").startsWith(LOOPBACK))
      .map((reg) => {
        const es = reg.information?.[0]?.entities ?? [];
        return {
          id: reg.id,
          to: reg.tenant ?? "default",
          type: es.length === 1 ? es[0].type : es.length > 1 ? `${es.length} types` : undefined,
        };
      }),
  );
}

async function unlink(reg) {
  await api(reg.space, `/ngsi-ld/v1/csourceRegistrations/${encodeURIComponent(reg.id)}`, {
    method: "DELETE",
  });
  log(`[${reg.space}] unlinked ${reg.to}`, "fed");
  await refreshSpace(reg.space);
  renderGraph();
}

function openLinkDialog(from) {
  const dlg = $("dlg-link");
  const opt = (selEl, values, chosen) => {
    selEl.replaceChildren();
    for (const v of values) {
      const o = document.createElement("option");
      o.value = o.textContent = v;
      if (v === chosen) o.selected = true;
      selEl.appendChild(o);
    }
  };
  opt($("link-from"), spaces.map((s) => s.name), from);
  opt($("link-to"), spaces.map((s) => s.name).filter((n) => n !== from));
  opt($("link-type"), ["(all playground types)", ...Object.keys(TYPES)]);
  dlg.showModal();
}

// The spec-clean cross-tenant link: endpoint = this same broker via the
// loopback host, `tenant` = the peer space (5.2.9). Queries in `from`
// now transparently include `to`'s matching entities (4.3.6.2 inclusive).
// `type` null ⇒ all playground types.
async function registerLink(from, to, type) {
  const entities = type ? [{ type }] : Object.keys(TYPES).map((t) => ({ type: t }));
  const body = {
    id: `urn:ngsi-ld:ContextSourceRegistration:${from}-to-${to}-${uuid().slice(0, 6)}`,
    type: "ContextSourceRegistration",
    information: [{ entities }],
    endpoint: LOOPBACK,
    mode: "inclusive",
    operations: ["federationOps"],
    ...(to !== "default" ? { tenant: to } : {}),
    "@context": CORE_CTX,
  };
  const r = await api(from, "/ngsi-ld/v1/csourceRegistrations", {
    method: "POST",
    headers: { "Content-Type": "application/ld+json" },
    body: JSON.stringify(body),
  });
  log(`[${from}] 🔗 federated → ${to} (${r.status})`, r.status === 201 ? "fed" : "err");
  if (r.status === 201) fedView.add(from);
  save("antares.fedview", [...fedView]);
  await refreshSpace(from);
  return r.status === 201;
}

$("link-create").onclick = async () => {
  const type = $("link-type").value;
  await registerLink(
    $("link-from").value,
    $("link-to").value,
    type === "(all playground types)" ? null : type,
  );
  $("dlg-link").close();
  renderInspector();
  renderGraph();
};

// ---- inspector buttons (the ids are the browser-test contract) -------------
$("create").onclick = () => createEntity(selSpace());
$("subscribe").onclick = () => subscribe(selSpace());
$("linkbtn").onclick = () => openLinkDialog(selSpace());
$("refresh").onclick = () => {
  const name = selSpace();
  fedView.has(name) ? fedView.delete(name) : fedView.add(name);
  save("antares.fedview", [...fedView]);
  refreshSpace(name).then(() => {
    renderInspector();
    renderGraph();
  });
};
$("insp-remove").onclick = () => removeSpace(selSpace());

// ---- spaces CRUD -----------------------------------------------------------
$("newspace").onclick = () => {
  $("space-name").value = "";
  $("dlg-space").showModal();
};
$("space-create").onclick = () => {
  const name = $("space-name").value.trim();
  // Tenant charset: A-Za-z0-9 and dash only (no spaces, no underscore) —
  // a strict subset of the broker's TenantId rule, so UI and API never argue.
  if (!/^[A-Za-z0-9-]{1,64}$/.test(name)) {
    log(`invalid space name ${name || "(empty)"} — use A-Za-z0-9 and "-" (max 64)`, "err");
    return;
  }
  if (!spaces.some((s) => s.name === name)) {
    spaces.push({ name });
    save("antares.spaces", spaces);
    resolveOverlaps();
    refreshAll();
    log(`context space ${name} ready (tenants are created on first write)`, "ok");
    select(`s:${name}`);
  }
  $("dlg-space").close();
};

// API-level wipe of one space: subscriptions, CSRs, entities. The bubble and
// pipelines are the caller's business.
async function cleanupSpace(name) {
  const subs = await (await api(name, "/ngsi-ld/v1/subscriptions?limit=100")).json().catch(() => []);
  for (const s of subs ?? []) {
    await api(name, `/ngsi-ld/v1/subscriptions/${encodeURIComponent(s.id)}`, { method: "DELETE" });
  }
  const regs = await (await api(name, `/ngsi-ld/v1/csourceRegistrations?type=${ALL_TYPES}&limit=100`)).json().catch(() => []);
  for (const reg of regs ?? []) {
    await api(name, `/ngsi-ld/v1/csourceRegistrations/${encodeURIComponent(reg.id)}`, { method: "DELETE" });
  }
  const list = await fetchEnts(name, { local: true });
  if (list.length) {
    await api(name, "/ngsi-ld/v1/entityOperations/delete", {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify(list.map((e) => e.id)),
    });
  }
}

async function removeSpace(name) {
  // Full cleanup in the broker, then forget the bubble: pipelines touching
  // the space stop, its subs + CSRs are deleted, its entities batch-deleted.
  for (const p of [...pipes]) {
    if (p.into === name || p.from === name) deletePipe(p.id);
  }
  await cleanupSpace(name);
  spaces = spaces.filter((s) => s.name !== name);
  save("antares.spaces", spaces);
  links.delete(name);
  ents.delete(name);
  delete positions[`s:${name}`];
  save("antares.pos", positions);
  select("s:default");
  refreshAll();
  log(`space ${name} removed (entities, subscriptions, links cleaned)`, "ok");
}

// ---- pipelines -------------------------------------------------------------
// One simulated-device generator per sensor type — derived, so a new TYPES
// entry automatically becomes a pipeline option.
const GENERATORS = Object.fromEntries(
  Object.keys(TYPES).map((t) => [`${TYPES[t].emoji} ${t}`, { type: t }]),
);
const timers = new Map(); // pipe.id -> interval handle

function startPipe(p) {
  stopTimer(p.id);
  timers.set(
    p.id,
    setInterval(() => tickPipe(p).catch((e) => log(`pipeline error: ${e}`, "err")), p.secs * 1000),
  );
}
const stopTimer = (id) => {
  clearInterval(timers.get(id));
  timers.delete(id);
};

function togglePipe(id) {
  const p = pipes.find((x) => x.id === id);
  if (!p) return;
  p.running = !p.running;
  p.running ? startPipe(p) : stopTimer(id);
  save("antares.pipes", pipes);
  renderInspector();
  renderGraph();
}

function deletePipe(id) {
  stopTimer(id);
  pipes = pipes.filter((x) => x.id !== id);
  delete positions[`p:${id}`];
  save("antares.pipes", pipes);
  save("antares.pos", positions);
  renderInspector();
  renderGraph();
}

async function tickPipe(p) {
  let synced = 1; // entities that actually moved this tick (source: 1 reading)
  if (p.kind === "source") {
    // A simulated device: one stable entity per pipeline, fresh reading per
    // tick — "convert a data source and insert it into a space".
    // A tick only COUNTS if the broker accepted the write (real flow rule):
    // failures go to the log instead of silently inflating the counter.
    const t = TYPES[p.type];
    const id = `urn:ngsi-ld:${p.type}:pipe-${p.id}`;
    const patch = await api(
      p.into,
      `/ngsi-ld/v1/entities/${encodeURIComponent(id)}/attrs/${t.attr}`,
      {
        method: "PATCH",
        headers: { "Content-Type": "application/ld+json" },
        body: JSON.stringify({ type: "Property", value: t.gen(Date.now()), "@context": CORE_CTX }),
      },
    );
    if (patch.status === 404) {
      const post = await api(p.into, "/ngsi-ld/v1/entities", {
        method: "POST",
        headers: { "Content-Type": "application/ld+json" },
        body: JSON.stringify({
          id,
          type: p.type,
          [t.attr]: { type: "Property", value: t.gen(Date.now()) },
          "@context": CORE_CTX,
        }),
      });
      if (post.status !== 201) {
        log(`pipeline ${p.gen} → ${p.into}: POST ${id} → ${post.status} ${(await post.text()).slice(0, 120)}`, "err");
        return;
      }
    } else if (patch.status !== 204) {
      log(`pipeline ${p.gen} → ${p.into}: PATCH ${t.attr} → ${patch.status} ${(await patch.text()).slice(0, 120)}`, "err");
      return;
    }
  } else {
    // Periodic copy: batch-upsert the source space's entities into the
    // target space — same broker, two tenants, one HTTP call each way.
    const r = await api(p.from, `/ngsi-ld/v1/entities?type=${p.type}&limit=100&local=true`);
    const list = r.ok ? await r.json() : [];
    if (!list.length) return; // nothing moved — no burst, no tick
    synced = list.length;
    const body = list.map((e) => ({ ...e, "@context": CORE_CTX }));
    await api(p.into, "/ngsi-ld/v1/entityOperations/upsert", {
      method: "POST",
      headers: { "Content-Type": "application/ld+json" },
      body: JSON.stringify(body),
    });
  }
  p.ticks = (p.ticks ?? 0) + 1;
  save("antares.pipes", pipes);
  burst(`pipe:${p.id}`, p.kind === "sync" ? synced : 1);
  pulse(p.into);
  await refreshSpace(p.into);
  renderGraph();
}

$("addpipe").onclick = () => {
  const opt = (selEl, values) => {
    selEl.replaceChildren();
    for (const v of values) {
      const o = document.createElement("option");
      o.value = o.textContent = v;
      selEl.appendChild(o);
    }
  };
  opt($("pipe-gen"), Object.keys(GENERATORS));
  opt($("pipe-from"), spaces.map((s) => s.name));
  opt($("pipe-type"), Object.keys(TYPES));
  opt($("pipe-into"), spaces.map((s) => s.name));
  $("dlg-pipe").showModal();
};
$("pipe-kind").onchange = () => {
  const src = $("pipe-kind").value === "source";
  $("pipe-source-opts").hidden = !src;
  $("pipe-sync-opts").hidden = src;
};
$("pipe-create").onclick = () => {
  const kind = $("pipe-kind").value;
  const p = {
    id: uuid().slice(0, 8),
    kind,
    into: $("pipe-into").value,
    secs: Math.max(1, Number($("pipe-secs").value) || 3),
    running: true,
    ticks: 0,
  };
  if (kind === "source") {
    p.gen = $("pipe-gen").value;
    p.type = GENERATORS[p.gen].type;
  } else {
    p.from = $("pipe-from").value;
    p.type = $("pipe-type").value;
    if (p.from === p.into) {
      log("sync pipeline needs two different spaces", "err");
      return;
    }
  }
  pipes.push(p);
  save("antares.pipes", pipes);
  startPipe(p);
  resolveOverlaps();
  renderInspector();
  renderGraph();
  log(
    p.kind === "source"
      ? `pipeline: ${p.gen} → ${p.into} every ${p.secs}s`
      : `pipeline: ${p.from} → ${p.into} (${p.type}) every ${p.secs}s`,
    "ok",
  );
  $("dlg-pipe").close();
};

// ---- board overview: the whole structure in one dialog ----------------------
function renderOverview() {
  const box = $("ov-body");
  box.replaceChildren();
  const h = (t) => {
    const e = document.createElement("h5");
    e.textContent = t;
    box.appendChild(e);
  };
  const item = (html) => {
    const d = document.createElement("div");
    d.className = "item";
    d.innerHTML = html;
    box.appendChild(d);
    return d;
  };

  h(`context spaces (${spaces.length})`);
  for (const s of spaces) {
    const cur = ents.get(s.name) ?? { local: [], remote: [] };
    const it = item(`<span>${avatar(s.name)}</span><strong>${s.name}</strong>
      <span class="sub">🏠 ${cur.local.length} local${
        fedView.has(s.name) ? ` · 🌐 ${cur.remote.length} fed` : ""}</span>
      ${s.name === "default" ? "" : `<button class="x" title="remove this space">✕</button>`}`);
    it.querySelector(".x")?.addEventListener("click", async () => {
      await removeSpace(s.name);
      renderOverview();
    });
  }

  h("federation — CSR edges (data flows peer → registrant)");
  let anyLink = false;
  for (const [from, ls] of links) {
    for (const l of ls) {
      anyLink = true;
      const it = item(`<span class="tag">CSR</span>
        <span>${avatar(l.to)} ${l.to} → ${avatar(from)} ${from}</span>
        <span class="sub">${l.type ?? "all types"}</span>
        <button class="x" title="delete registration">✕</button>`);
      it.querySelector(".x").onclick = async () => {
        await unlink({ space: from, id: l.id, to: l.to });
        renderOverview();
      };
    }
  }
  if (!anyLink) item(`<span class="sub">none — 🔗 federate registers a CSR between two spaces</span>`);

  h(`pipelines (${pipes.length})`);
  if (!pipes.length) {
    item(`<span class="sub">none — ＋ pipeline adds a simulated device or a periodic copy</span>`);
  }
  for (const p of pipes) {
    const label = p.kind === "source"
      ? `${p.gen} → <strong>${p.into}</strong> <span class="sub">simulated device</span>`
      : `🔁 ${p.from} → <strong>${p.into}</strong> <span class="sub">copy · ${p.type}</span>`;
    const it = item(`<span>${label}</span>
      <span class="sub">${p.secs}s · ${p.ticks ?? 0} ticks</span>
      <button class="x t" title="pause/resume">${p.running ? "⏸" : "▶"}</button>
      <button class="x" title="delete">✕</button>`);
    it.querySelector(".t").onclick = () => {
      togglePipe(p.id);
      renderOverview();
    };
    it.querySelectorAll(".x")[1].onclick = () => {
      deletePipe(p.id);
      renderOverview();
    };
  }
}

// One-click demo — a meaningful city: 8 spaces (smart-city is the hub),
// 12 simulated sensors/sources across the districts, 5 CSRs federating into
// the hub, 3 periodic copies. Idempotent: running it twice adds nothing
// twice. Only real API calls; every flow obeys the evidence rules.
const DEMO = {
  spaces: ["default", "smart-city", "old-town", "harbor", "airport",
    "university", "energy-grid", "transit", "decidim"],
  // hub-and-spoke default positions, as fractions of the canvas
  layout: {
    "smart-city": [0.5, 0.45], "old-town": [0.3, 0.2], harbor: [0.72, 0.18],
    airport: [0.88, 0.45], university: [0.12, 0.48], "energy-grid": [0.28, 0.78],
    transit: [0.72, 0.8], decidim: [0.1, 0.16], default: [0.52, 0.08],
  },
  devices: [ // [space, sensor type, seconds] — each is one simulated source
    ["old-town", "TemperatureSensor", 3], ["old-town", "NoiseSensor", 4],
    ["harbor", "WaterLevelSensor", 4], ["harbor", "ParkingSpot", 3],
    ["airport", "AirQualitySensor", 3], ["airport", "TrafficCounter", 2],
    ["university", "Room", 5], ["university", "EnergyMeter", 3],
    ["energy-grid", "EnergyMeter", 2], ["energy-grid", "Streetlight", 4],
    ["transit", "BikeStation", 3], ["transit", "TrafficCounter", 3],
    ["decidim", "CitizenProposal", 5],
  ],
  // EVERY data path terminates in smart-city: five all-type CSRs, two
  // deliberately type-scoped ones (their excluded types arrive as copies).
  csrs: [
    ["smart-city", "old-town", null],
    ["smart-city", "harbor", null],
    ["smart-city", "university", null],
    ["smart-city", "energy-grid", null],
    ["smart-city", "decidim", null],
    ["smart-city", "airport", "AirQualitySensor"],
    ["smart-city", "transit", "BikeStation"],
  ],
  copies: [ // 3 periodic copies — each closes a gap the scoped CSRs leave
    ["airport", "smart-city", "TrafficCounter", 6],
    ["transit", "smart-city", "TrafficCounter", 7],
    ["university", "energy-grid", "EnergyMeter", 8],
  ],
};

async function createDemo() {
  log("▶ demo: a federated city — districts sense, smart-city sees", "ok");
  for (const n of DEMO.spaces) {
    if (!spaces.some((s) => s.name === n)) spaces.push({ name: n });
  }
  save("antares.spaces", spaces);
  // default positions: the hub layout (only touches demo spaces)
  const { w, h } = bounds();
  for (const [name, [fx, fy]] of Object.entries(DEMO.layout)) {
    positions[`s:${name}`] = { x: fx * w, y: fy * h };
  }
  await refreshAll(); // fresh links/ents for the idempotence checks below
  const linked = (from, to) => (links.get(from) ?? []).some((l) => l.to === to);
  for (const [from, to, type] of DEMO.csrs) {
    if (!linked(from, to)) await registerLink(from, to, type);
  }
  const subs = await (await api("smart-city", "/ngsi-ld/v1/subscriptions?limit=100"))
    .json().catch(() => []);
  if (!(subs ?? []).length) await subscribe("smart-city");
  for (const [into, type, secs] of DEMO.devices) {
    if (pipes.some((p) => p.kind === "source" && p.into === into && p.type === type)) continue;
    const p = { id: uuid().slice(0, 8), kind: "source",
      gen: `${TYPES[type].emoji} ${type}`, type, into, secs, running: true, ticks: 0 };
    pipes.push(p);
    startPipe(p);
  }
  for (const [from, into, type, secs] of DEMO.copies) {
    if (pipes.some((p) => p.kind === "sync" && p.from === from && p.into === into && p.type === type)) continue;
    const p = { id: uuid().slice(0, 8), kind: "sync",
      from, into, type, secs, running: true, ticks: 0 };
    pipes.push(p);
    startPipe(p);
  }
  save("antares.pipes", pipes);
  // park each device bubble beside the space it feeds (golden-angle scatter)
  let di = 0;
  for (const p of pipes) {
    if (p.kind !== "source") continue;
    const base = positions[`s:${p.into}`];
    if (base) {
      const ang = di++ * 2.399963;
      positions[`p:${p.id}`] = { x: base.x + 100 * Math.cos(ang), y: base.y + 100 * Math.sin(ang) };
    }
  }
  save("antares.pos", positions);
  select("s:smart-city");
  resolveOverlaps();
  await refreshAll();
  renderGraph();
  log("demo ready — 13 sources → 7 districts (incl. decidim) → 7 CSRs + 3 copies, everything ends in smart-city", "ok");
}

// Remove EVERYTHING: API-level wipe of every space, pipelines stopped and
// dropped, board reseeded to the default spaces.
async function resetBoard() {
  for (const p of [...pipes]) deletePipe(p.id);
  for (const s of [...spaces]) await cleanupSpace(s.name);
  spaces = SEED_SPACES.map((name) => ({ name }));
  save("antares.spaces", spaces);
  links.clear();
  ents.clear();
  fedView.clear();
  save("antares.fedview", []);
  for (const k of Object.keys(positions)) delete positions[k];
  save("antares.pos", positions);
  select("s:default");
  await refreshAll();
  log("board reset — all entities/subscriptions/CSRs/pipelines removed, spaces reseeded", "ok");
}

// ---- board template: the whole structure as JSON, exportable + applyable --
function boardTemplate() {
  return {
    app: "antares-playground-board",
    version: 1,
    mode,
    contextSpaces: spaces.map((s) => ({
      name: s.name,
      icon: avatar(s.name),
      local: ents.get(s.name)?.local.length ?? 0,
      ...(fedView.has(s.name)
        ? { fedView: true, federated: ents.get(s.name)?.remote.length ?? 0 }
        : {}),
    })),
    csrs: [...links].flatMap(([from, ls]) =>
      ls.map((l) => ({
        kind: "federation", protocol: "CSR", from, peer: l.to,
        type: l.type && TYPES[l.type] ? l.type : "all",
      }))),
    pipelines: pipes.map((p) =>
      p.kind === "source"
        ? { kind: "device", gen: p.gen, type: p.type, into: p.into,
            secs: p.secs, running: p.running, ticks: p.ticks ?? 0 }
        : { kind: "copy", from: p.from, into: p.into, type: p.type,
            secs: p.secs, running: p.running, ticks: p.ticks ?? 0 }),
    selected: selSpace(),
  };
}

// Apply is additive + idempotent: missing spaces/CSRs/pipes are created,
// existing ones left alone, malformed rows skipped. Entities never travel in
// a template — structure only.
async function applyBoardTemplate(tpl) {
  for (const s of tpl.contextSpaces ?? []) {
    const name = s.name ?? s.id;
    if (!/^[A-Za-z0-9-]{1,64}$/.test(name ?? "")) continue;
    if (!spaces.some((x) => x.name === name)) spaces.push({ name });
    if (s.fedView) fedView.add(name);
  }
  save("antares.spaces", spaces);
  save("antares.fedview", [...fedView]);
  await refreshAll();
  for (const c of tpl.csrs ?? []) {
    const from = c.from;
    const to = c.peer ?? c.to;
    if (!spaces.some((x) => x.name === from) || !spaces.some((x) => x.name === to)) continue;
    if ((links.get(from) ?? []).some((l) => l.to === to)) continue;
    await registerLink(from, to, c.type && TYPES[c.type] ? c.type : null);
  }
  for (const t of tpl.pipelines ?? []) {
    const kind = t.kind === "device" || t.kind === "source" ? "source" : "sync";
    if (!TYPES[t.type] || !spaces.some((x) => x.name === t.into)) continue;
    const dup = kind === "source"
      ? pipes.some((p) => p.kind === "source" && p.into === t.into && p.type === t.type)
      : pipes.some((p) => p.kind === "sync" && p.from === t.from && p.into === t.into && p.type === t.type);
    if (dup) continue;
    if (kind === "sync" && (!spaces.some((x) => x.name === t.from) || t.from === t.into)) continue;
    const p = {
      id: uuid().slice(0, 8), kind,
      into: t.into, type: t.type,
      secs: Math.max(1, Number(t.secs) || 3),
      running: t.running !== false, ticks: 0,
    };
    if (kind === "source") p.gen = t.gen ?? `${TYPES[t.type].emoji} ${t.type}`;
    else p.from = t.from;
    pipes.push(p);
    if (p.running) startPipe(p);
  }
  save("antares.pipes", pipes);
  resolveOverlaps();
  await refreshAll();
  renderInspector();
  log("template applied — spaces, CSRs and pipelines materialized", "ok");
}

$("ov-template").onclick = async () => {
  await refreshAll();
  $("tpl-text").value = JSON.stringify(boardTemplate(), null, 2);
  $("dlg-template").showModal();
};
$("tpl-copy").onclick = async () => {
  try {
    await navigator.clipboard.writeText($("tpl-text").value);
    log("template JSON copied to clipboard", "ok");
  } catch {
    $("tpl-text").select();
    log("clipboard blocked by the browser — JSON selected, press ⌘C / Ctrl-C", "err");
  }
};
$("tpl-apply").onclick = async () => {
  let tpl;
  try {
    tpl = JSON.parse($("tpl-text").value);
  } catch (e) {
    log(`template is not valid JSON: ${e.message}`, "err");
    return;
  }
  $("tpl-apply").disabled = true;
  try {
    await applyBoardTemplate(tpl);
  } finally {
    $("tpl-apply").disabled = false;
  }
  renderOverview();
};
// Test hooks, same contract as window.brokerFetch (browser-test.mjs).
window.boardTemplate = boardTemplate;
window.applyBoardTemplate = applyBoardTemplate;

$("overview").onclick = async () => {
  await refreshAll();
  renderOverview();
  $("dlg-overview").showModal();
};
$("ov-demo").onclick = async () => {
  $("ov-demo").disabled = true;
  try {
    await createDemo();
  } finally {
    $("ov-demo").disabled = false;
  }
  renderOverview();
};
$("ov-reset").onclick = async () => {
  if (!confirm("Remove EVERYTHING? Entities, subscriptions, CSRs and pipelines in every space are deleted; the board reseeds.")) return;
  $("ov-reset").disabled = true;
  try {
    await resetBoard();
  } finally {
    $("ov-reset").disabled = false;
  }
  renderOverview();
};

// ---- notifications ---------------------------------------------------------
function showNotification(n) {
  const ids = (n.data || []).map((e) => e.id).join(", ");
  // subscription ids are urn:ngsi-ld:Subscription:<space>-<rand>
  const space = (n.subscriptionId ?? "").split(":").pop()?.split("-").slice(0, -1).join("-");
  log(`NOTIFICATION ${n.id ?? ""} → ${ids}`, "notif");
  const toast = document.createElement("div");
  toast.className = "toast";
  toast.textContent = `🔔 ${space || "space"}: ${ids}`;
  $("toasts").appendChild(toast);
  setTimeout(() => toast.remove(), 4000);
}

// N7b: the ETSI browser-tier proxy drives the broker through this hook —
// the same transport ladder the buttons use, whatever mode won.
window.brokerFetch = brokerFetch;
boot().catch((e) => log(`boot failed: ${e}`, "err"));
