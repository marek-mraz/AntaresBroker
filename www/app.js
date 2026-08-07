// N6/N9: playground logic. Transport ladder unchanged (OPFS worker →
// Service Worker → in-page); on top of it, the "context spaces" board: each
// space is a tenant of this one in-browser broker, spaces federate through
// Context Source Registrations whose endpoint is the loopback host and whose
// `tenant` member names the peer space (CIM 009 5.2.9 / 4.3.6), and
// pipelines stream or copy data between spaces.
import init, { AntaresBroker } from "./pkg/antares_wasm.js";
import { LOOPBACK, installLoopback } from "./loopback.js";

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
    installLoopback(() => pageBroker);
    pageBroker.onNotification(NOTIFY_ENDPOINT, (url, body) => {
      showNotification(JSON.parse(body));
      return true;
    });
  } // opfs-worker: notifications arrive on the worker port (bootPersistent)
  $("mode").textContent = mode;
  const health = await (await brokerFetch("/q/health")).json();
  // Compact — but keep the '"store":' shape browser-test.mjs keys on.
  $("health").textContent = JSON.stringify({ store: health.store, status: health.status });
  log(`broker up (${mode}), store=${health.store}`, "ok");
  renderBoard();
  await refreshAll();
  for (const p of pipes) if (p.running) startPipe(p);
  setInterval(refreshAll, 3000);
}

// ---- context spaces --------------------------------------------------------
// A space IS a tenant. "default" always exists (the spec's default tenant).
const PALETTE = ["#6d5ef1", "#19a974", "#e8850c", "#d9534f", "#3b82f6", "#b45fd9"];
const EMOJI = ["🏙", "🏔", "🛰", "🏭", "🌊", "🌳", "🎡", "🚉"];
const TYPES = {
  Room: { emoji: "🚪", attr: "temperature", gen: () => Math.round(15 + Math.random() * 15) },
  TemperatureSensor: { emoji: "🌡", attr: "temperature", gen: (t) => Math.round(180 + 60 * Math.sin(t / 20e3) + 20 * Math.random()) / 10 },
  ParkingSpot: { emoji: "🚗", attr: "occupied", gen: () => (Math.random() < 0.5 ? 0 : 1) },
  Streetlight: { emoji: "💡", attr: "powerDraw", gen: () => Math.round(Math.random() * 60) },
};
const ALL_TYPES = Object.keys(TYPES).join(",");

const load = (k, d) => JSON.parse(localStorage.getItem(k) ?? "null") ?? d;
const save = (k, v) => localStorage.setItem(k, JSON.stringify(v));
let spaces = load("antares.spaces", [{ name: "default" }, { name: "smart-city" }]);
if (!spaces.some((s) => s.name === "default")) spaces.unshift({ name: "default" });
let pipes = load("antares.pipes", []);
const fedView = new Set(load("antares.fedview", []));
const links = new Map(); // space -> [{id, to, type}]

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

// ---- board rendering -------------------------------------------------------
function renderBoard() {
  document.querySelectorAll("#board .space").forEach((el) => el.remove());
  const anchor = $("newspace");
  for (const s of spaces) {
    const card = document.createElement("div");
    card.className = "space";
    card.dataset.space = s.name;
    card.style.setProperty("--space-color", color(s.name));
    const isDefault = s.name === "default";
    // The default card carries the test-contract ids (#entities, #create,
    // #subscribe, #refresh) — browser-test.mjs and the ETSI proxy use them.
    card.innerHTML = `
      <h2>${avatar(s.name)} ${s.name}
        ${isDefault ? "" : `<button class="x" title="remove space">✕</button>`}
      </h2>
      <div class="sub">tenant <code>${s.name}</code> · <span class="links"></span></div>
      <ul ${isDefault ? 'id="entities"' : ""} class="ents"></ul>
      <div class="row">
        <button class="add" ${isDefault ? 'id="create"' : ""}>＋ entity</button>
        <button class="watch" ${isDefault ? 'id="subscribe"' : ""}>🔔 watch</button>
        <button class="link">🔗 federate</button>
        <button class="fed ghost" ${isDefault ? 'id="refresh"' : ""}>🌐 fed view</button>
      </div>`;
    card.querySelector(".add").onclick = () => createEntity(s.name);
    card.querySelector(".watch").onclick = () => subscribe(s.name);
    card.querySelector(".link").onclick = () => openLinkDialog(s.name);
    card.querySelector(".fed").onclick = () => toggleFed(s.name);
    card.querySelector(".x")?.addEventListener("click", () => removeSpace(s.name));
    styleFedButton(card, s.name);
    $("board").insertBefore(card, anchor);
  }
  renderPipes();
}

function styleFedButton(card, name) {
  const b = card.querySelector(".fed");
  b.classList.toggle("primary", fedView.has(name));
  b.classList.toggle("ghost", !fedView.has(name));
}

function toggleFed(name) {
  fedView.has(name) ? fedView.delete(name) : fedView.add(name);
  save("antares.fedview", [...fedView]);
  const card = document.querySelector(`.space[data-space="${name}"]`);
  if (card) styleFedButton(card, name);
  refreshSpace(name);
}

function pulse(name) {
  const card = document.querySelector(`.space[data-space="${name}"]`);
  if (!card) return;
  card.classList.add("pulse");
  setTimeout(() => card.classList.remove("pulse"), 600);
}

// ---- entities --------------------------------------------------------------
async function createEntity(space, type) {
  type ??= Object.keys(TYPES)[Math.floor(Math.random() * 4)];
  const t = TYPES[type];
  const id = `urn:ngsi-ld:${type}:${crypto.randomUUID().slice(0, 8)}`;
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
    id: `urn:ngsi-ld:Subscription:${space}-${crypto.randomUUID().slice(0, 8)}`,
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

function entLabel(e) {
  const type = Array.isArray(e.type) ? e.type[0] : e.type;
  const t = TYPES[type] ?? { emoji: "▪", attr: null };
  const val = t.attr ? e[t.attr]?.value : undefined;
  return { type, emoji: t.emoji, val };
}

async function fetchEnts(space, { local }) {
  const q = `/ngsi-ld/v1/entities?type=${ALL_TYPES}&limit=100${local ? "&local=true" : ""}`;
  const r = await api(space, q);
  return r.ok ? await r.json() : [];
}

async function refreshSpace(space) {
  const card = document.querySelector(`.space[data-space="${space}"]`);
  if (!card) return;
  const local = await fetchEnts(space, { local: true });
  let remote = [];
  if (fedView.has(space)) {
    const fed = await fetchEnts(space, { local: false });
    const have = new Set(local.map((e) => e.id));
    remote = fed.filter((e) => !have.has(e.id));
  }
  const ul = card.querySelector(".ents");
  const prev = new Map(
    [...ul.children].map((li) => [li.dataset.id, li.dataset.val]),
  );
  ul.replaceChildren();
  for (const e of local) {
    const { type, emoji, val } = entLabel(e);
    const li = document.createElement("li");
    li.dataset.id = e.id;
    li.dataset.val = String(val);
    li.title = `${e.id} — click to bump`;
    li.innerHTML = `<span>${emoji}</span><span>${e.id.split(":").pop()}</span>
      <span class="val">${val ?? "·"}</span>`;
    li.onclick = () => bump(space, e);
    if (prev.has(e.id) && prev.get(e.id) !== String(val)) li.classList.add("flash");
    ul.appendChild(li);
  }
  for (const e of remote) {
    const { emoji, val } = entLabel(e);
    const li = document.createElement("li");
    li.className = "remote";
    li.title = `${e.id} — via federation`;
    li.innerHTML = `<span>${emoji}</span><span>${e.id.split(":").pop()}</span>
      <span class="tag">fed</span><span class="val">${val ?? "·"}</span>`;
    ul.appendChild(li);
  }
  await refreshLinks(space, card);
}

async function refreshAll() {
  await Promise.all(spaces.map((s) => refreshSpace(s.name)));
  drawWires();
}

// ---- federation links (CSRs with the 5.2.9 `tenant` member) ---------------
async function refreshLinks(space, card) {
  // 5.10.2.4: registration discovery needs a discriminating input.
  const r = await api(space, `/ngsi-ld/v1/csourceRegistrations?type=${ALL_TYPES}&limit=100`);
  const regs = r.ok ? await r.json() : [];
  const ours = regs
    .filter((reg) => (reg.endpoint ?? "").startsWith(LOOPBACK))
    .map((reg) => {
      const ents = reg.information?.[0]?.entities ?? [];
      return {
        id: reg.id,
        to: reg.tenant ?? "default",
        type: ents.length === 1 ? ents[0].type : ents.length > 1 ? `${ents.length} types` : undefined,
      };
    });
  links.set(space, ours);
  const span = card.querySelector(".links");
  span.replaceChildren();
  if (!ours.length) {
    span.textContent = "no federation links";
    return;
  }
  for (const l of ours) {
    const tag = document.createElement("span");
    tag.className = "tag";
    tag.textContent = `→ ${l.to}${l.type ? ` (${l.type})` : ""} ✕`;
    tag.style.cursor = "pointer";
    tag.title = "click to delete this registration";
    tag.onclick = async () => {
      await api(space, `/ngsi-ld/v1/csourceRegistrations/${encodeURIComponent(l.id)}`, {
        method: "DELETE",
      });
      log(`[${space}] unlinked ${l.to}`, "fed");
      refreshSpace(space).then(drawWires);
    };
    span.appendChild(tag);
    span.append(" ");
  }
}

function openLinkDialog(from) {
  const dlg = $("dlg-link");
  const opt = (sel, values, chosen) => {
    sel.replaceChildren();
    for (const v of values) {
      const o = document.createElement("option");
      o.value = o.textContent = v;
      if (v === chosen) o.selected = true;
      sel.appendChild(o);
    }
  };
  opt($("link-from"), spaces.map((s) => s.name), from);
  opt($("link-to"), spaces.map((s) => s.name).filter((n) => n !== from));
  opt($("link-type"), ["(all playground types)", ...Object.keys(TYPES)]);
  dlg.showModal();
}

$("link-create").onclick = async () => {
  const from = $("link-from").value;
  const to = $("link-to").value;
  const type = $("link-type").value;
  const entities =
    type === "(all playground types)"
      ? Object.keys(TYPES).map((t) => ({ type: t }))
      : [{ type }];
  // The spec-clean cross-tenant link: endpoint = this same broker via the
  // loopback host, `tenant` = the peer space (5.2.9). Queries in `from`
  // now transparently include `to`'s matching entities (4.3.6.2 inclusive).
  const body = {
    id: `urn:ngsi-ld:ContextSourceRegistration:${from}-to-${to}-${crypto.randomUUID().slice(0, 6)}`,
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
  $("dlg-link").close();
  if (r.status === 201 && !fedView.has(from)) toggleFed(from);
  await refreshSpace(from);
  drawWires();
};

// ---- the wires (SVG arrows between cards) ---------------------------------
function drawWires() {
  const svg = $("wires");
  const board = $("board").getBoundingClientRect();
  svg.setAttribute("width", board.width);
  svg.setAttribute("height", board.height);
  svg.replaceChildren();
  const at = (name) => {
    const el = document.querySelector(`.space[data-space="${name}"]`);
    if (!el) return null;
    const r = el.getBoundingClientRect();
    return { x: r.x - board.x + r.width / 2, y: r.y - board.y + r.height / 2 };
  };
  const wire = (from, to, color, dash) => {
    const a = at(from), b = at(to);
    if (!a || !b) return;
    const p = document.createElementNS("http://www.w3.org/2000/svg", "path");
    const mx = (a.x + b.x) / 2, my = (a.y + b.y) / 2 - 40;
    p.setAttribute("d", `M ${a.x} ${a.y} Q ${mx} ${my} ${b.x} ${b.y}`);
    p.setAttribute("fill", "none");
    p.setAttribute("stroke", color);
    p.setAttribute("stroke-width", "2");
    p.setAttribute("stroke-dasharray", dash);
    p.setAttribute("opacity", "0.55");
    p.innerHTML = `<animate attributeName="stroke-dashoffset" from="24" to="0" dur="1.2s" repeatCount="indefinite"/>`;
    svg.appendChild(p);
  };
  for (const [from, ls] of links) {
    for (const l of ls) wire(l.to, from, "var(--fed)", "6 6"); // data flows to → from
  }
  for (const p of pipes) {
    if (p.kind === "sync" && p.running) wire(p.from, p.into, "var(--ok)", "2 8");
  }
}
addEventListener("resize", drawWires);

// ---- spaces CRUD -----------------------------------------------------------
$("newspace").onclick = () => {
  $("space-name").value = "";
  $("dlg-space").showModal();
};
$("space-create").onclick = () => {
  const name = $("space-name").value.trim();
  if (!/^[a-zA-Z0-9_-]+$/.test(name)) {
    log(`invalid space name ${name || "(empty)"} — letters, digits, - _`, "err");
    return;
  }
  if (!spaces.some((s) => s.name === name)) {
    spaces.push({ name });
    save("antares.spaces", spaces);
    renderBoard();
    refreshAll();
    log(`context space ${name} ready (tenants are created on first write)`, "ok");
  }
  $("dlg-space").close();
};

async function removeSpace(name) {
  // Full cleanup in the broker, then forget the card: pipelines touching the
  // space stop, its subs + CSRs are deleted, its entities batch-deleted.
  for (const p of [...pipes]) {
    if (p.into === name || p.from === name) deletePipe(p.id);
  }
  const subs = await (await api(name, "/ngsi-ld/v1/subscriptions?limit=100")).json().catch(() => []);
  for (const s of subs ?? []) {
    await api(name, `/ngsi-ld/v1/subscriptions/${encodeURIComponent(s.id)}`, { method: "DELETE" });
  }
  const regs = await (await api(name, `/ngsi-ld/v1/csourceRegistrations?type=${ALL_TYPES}&limit=100`)).json().catch(() => []);
  for (const reg of regs ?? []) {
    await api(name, `/ngsi-ld/v1/csourceRegistrations/${encodeURIComponent(reg.id)}`, { method: "DELETE" });
  }
  const ents = await fetchEnts(name, { local: true });
  if (ents.length) {
    await api(name, "/ngsi-ld/v1/entityOperations/delete", {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify(ents.map((e) => e.id)),
    });
  }
  spaces = spaces.filter((s) => s.name !== name);
  save("antares.spaces", spaces);
  links.delete(name);
  renderBoard();
  refreshAll();
  log(`space ${name} removed (entities, subscriptions, links cleaned)`, "ok");
}

// ---- pipelines -------------------------------------------------------------
const GENERATORS = {
  "🌡 city temperature (sine)": { type: "TemperatureSensor" },
  "🚗 parking occupancy (random)": { type: "ParkingSpot" },
  "💡 streetlight power (random)": { type: "Streetlight" },
};
const timers = new Map(); // pipe.id -> interval handle

function renderPipes() {
  const box = $("pipes");
  box.replaceChildren();
  for (const p of pipes) {
    const el = document.createElement("div");
    el.className = `pipe${p.running ? " running" : ""}`;
    const label =
      p.kind === "source"
        ? `${p.gen} → <strong>${p.into}</strong>`
        : `🔁 ${p.from} → <strong>${p.into}</strong> (${p.type})`;
    el.innerHTML = `<span class="dot"></span><span>${label}</span>
      <span class="meta">every ${p.secs}s</span>
      <span class="count">${p.ticks ?? 0} ticks</span>
      <button class="toggle">${p.running ? "⏸" : "▶"}</button>
      <button class="x">✕</button>`;
    el.querySelector(".toggle").onclick = () => togglePipe(p.id);
    el.querySelector(".x").onclick = () => deletePipe(p.id);
    box.appendChild(el);
  }
  drawWires();
}

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
  renderPipes();
}

function deletePipe(id) {
  stopTimer(id);
  pipes = pipes.filter((x) => x.id !== id);
  save("antares.pipes", pipes);
  renderPipes();
}

async function tickPipe(p) {
  if (p.kind === "source") {
    // A simulated device: one stable entity per pipeline, fresh reading per
    // tick — "convert a data source and insert it into a space".
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
      await api(p.into, "/ngsi-ld/v1/entities", {
        method: "POST",
        headers: { "Content-Type": "application/ld+json" },
        body: JSON.stringify({
          id,
          type: p.type,
          [t.attr]: { type: "Property", value: t.gen(Date.now()) },
          "@context": CORE_CTX,
        }),
      });
    }
  } else {
    // Periodic copy: batch-upsert the source space's entities into the
    // target space — same broker, two tenants, one HTTP call each way.
    const r = await api(p.from, `/ngsi-ld/v1/entities?type=${p.type}&limit=100&local=true`);
    const ents = r.ok ? await r.json() : [];
    if (!ents.length) return;
    const body = ents.map((e) => ({ ...e, "@context": CORE_CTX }));
    await api(p.into, "/ngsi-ld/v1/entityOperations/upsert", {
      method: "POST",
      headers: { "Content-Type": "application/ld+json" },
      body: JSON.stringify(body),
    });
  }
  p.ticks = (p.ticks ?? 0) + 1;
  save("antares.pipes", pipes);
  pulse(p.into);
  renderPipes();
  refreshSpace(p.into);
}

$("addpipe").onclick = () => {
  const opt = (sel, values) => {
    sel.replaceChildren();
    for (const v of values) {
      const o = document.createElement("option");
      o.value = o.textContent = v;
      sel.appendChild(o);
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
    id: crypto.randomUUID().slice(0, 8),
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
  renderPipes();
  log(
    p.kind === "source"
      ? `pipeline: ${p.gen} → ${p.into} every ${p.secs}s`
      : `pipeline: ${p.from} → ${p.into} (${p.type}) every ${p.secs}s`,
    "ok",
  );
  $("dlg-pipe").close();
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
