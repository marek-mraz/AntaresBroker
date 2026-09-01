// The transport ladder: OPFS worker (persistent) → in-page wasm broker.
// Also owns the request log: EVERY broker call is recorded (tenant, method,
// path, status) — the 🛰 feature. React components subscribe via `transport`.

import { load, save } from "../persist.js";

const listeners = new Set();
export const transport = {
  mode: "booting", // booting | opfs-worker | in-page | failed
  persistent: false,
  requests: [], // newest first, capped
  reqlogOn: load("antares.reqlog", true),
  bootError: null,
  subscribe(fn) {
    listeners.add(fn);
    return () => listeners.delete(fn);
  },
};
let version = 0;
export const getVersion = () => version;
export function emit() {
  version++;
  for (const fn of listeners) fn();
}

export function setReqlog(on) {
  transport.reqlogOn = on;
  save("antares.reqlog", on);
  emit();
}

const noteHandlers = new Set();
export function onNotification(fn) {
  noteHandlers.add(fn);
  return () => noteHandlers.delete(fn);
}
function dispatchNotification(body) {
  let doc;
  try {
    doc = typeof body === "string" ? JSON.parse(body) : body;
  } catch {
    return;
  }
  for (const fn of noteHandlers) fn(doc);
}

// ?allowPrivateEgress=1 — harness knob: the ETSI mocks live on
// private/loopback addresses the broker's egress policy denies by default.
const ALLOW_PRIVATE_EGRESS = new URLSearchParams(location.search).get(
  "allowPrivateEgress",
) === "1";

// ?ANTARES_SWEEP_SECS=2 — 4.22 GC cadence, the SAME variable as the native
// env knob; the wasm broker reads it off globalThis at construction
// (default 60 s when unset).
const SWEEP_SECS =
  Number(new URLSearchParams(location.search).get("ANTARES_SWEEP_SECS")) || 0;

let worker = null;
let workerDead = null;
let seq = 0;
const waiters = new Map();
let pageBroker = null;

function callWorker(msg, transfer = []) {
  return new Promise((resolve, reject) => {
    if (workerDead) return reject(workerDead);
    const id = ++seq;
    waiters.set(id, { resolve, reject });
    worker.postMessage({ id, ...msg }, transfer);
  });
}

async function bootWorker() {
  for (let attempt = 0; attempt < 2; attempt++) {
    worker = new Worker(new URL("worker.js", document.baseURI), { type: "module" });
    worker.onmessage = (e) => {
      const m = e.data;
      if (m.kind === "notification") return dispatchNotification(m.body);
      const w = waiters.get(m.id);
      if (!w) return;
      waiters.delete(m.id);
      m.ok ? w.resolve(m) : w.reject(new Error(m.error));
    };
    worker.onerror = (e) => {
      // A dead worker must fail every pending and future call loudly.
      workerDead = new Error(`opfs worker error: ${e.message ?? e}`);
      for (const [id, w] of waiters) {
        waiters.delete(id);
        w.reject(workerDead);
      }
      emit();
    };
    try {
      await callWorker({
        kind: "init",
        file: "antares.redb",
        allowPrivateEgress: ALLOW_PRIVATE_EGRESS,
        sweepSecs: SWEEP_SECS,
      });
      return true;
    } catch (err) {
      worker.terminate();
      worker = null;
      transport.bootError = String(err.message ?? err);
      // Only the exclusive-owner case is worth one retry (a just-closed
      // tab's handle releases asynchronously).
      if (!String(err.message).includes("another tab")) return false;
      await new Promise((r) => setTimeout(r, 400));
    }
  }
  return false;
}

async function bootInPage() {
  const pkgUrl = new URL("pkg/antares_wasm.js", document.baseURI).href;
  const mod = await import(/* @vite-ignore */ pkgUrl);
  await mod.default();
  if (SWEEP_SECS > 0) globalThis.ANTARES_SWEEP_SECS = SWEEP_SECS;
  pageBroker = new mod.AntaresBroker(ALLOW_PRIVATE_EGRESS);
  const lbUrl = new URL("loopback.js", document.baseURI).href;
  const { installLoopback } = await import(/* @vite-ignore */ lbUrl);
  installLoopback(() => pageBroker);
  pageBroker.onNotification("http://page.local/", (url, body) => {
    dispatchNotification(body);
    return true;
  });
}

export async function bootTransport() {
  if (await bootWorker()) {
    transport.mode = "opfs-worker";
    transport.persistent = true;
  } else {
    await bootInPage();
    transport.mode = "in-page";
    transport.persistent = false;
  }
  emit();
}

async function rawFetch(path, opts = {}) {
  if (transport.mode === "opfs-worker") {
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
    const noBody = [101, 103, 204, 205, 304].includes(r.status);
    return new Response(noBody ? null : r.body, { status: r.status, headers: r.headers });
  }
  const req = new Request(new URL(path, location.origin), opts);
  return pageBroker.fetch(req);
}

export async function brokerFetch(path, opts = {}) {
  const r = await rawFetch(path, opts);
  if (transport.reqlogOn) {
    const entry = {
      tenant:
        opts.headers?.["NGSILD-Tenant"] ?? opts.headers?.["ngsild-tenant"] ?? "default",
      method: (opts.method ?? "GET").toUpperCase(),
      path,
      status: r.status,
      at: Date.now(),
    };
    transport.requests.unshift(entry);
    if (transport.requests.length > 300) transport.requests.length = 300;
    console.debug("[ngsi-ld]", `[${entry.tenant}] ${entry.method} ${path} → ${r.status}`);
    emit();
  }
  return r;
}
