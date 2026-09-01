// The Node tier — the SAME .wasm the browser loads, fronted by a thin
// http.createServer shim so the ETSI Robot suite can talk to it over real
// TCP. No CORS, unrestricted outbound fetch (undici), so every serial suite
// is in scope here; the browser tier's structural limits don't apply.
//
//   node www/node-shim.mjs [port]       (default 9090)
//
// Build www/pkg first: dev/wasm-build.sh
import { readFile } from "node:fs/promises";
import {
  closeSync,
  constants as fsConstants,
  fstatSync,
  fsyncSync,
  ftruncateSync,
  openSync,
  readSync,
  writeSync,
} from "node:fs";
import { createServer, request as httpRequest } from "node:http";
import { request as httpsRequest } from "node:https";
import dns from "node:dns";
import init, { AntaresBroker } from "./pkg/antares_wasm.js";

// The ETSI mocks (HttpCtrl receivers, context servers) bind IPv4-only;
// Node's default resolution order tries ::1 first and the connect refusal
// surfaces as a 502 on every federation forward. Prefer IPv4 like the
// suite's own python-requests does.
dns.setDefaultResultOrder("ipv4first");

// Transport fidelity: the JS Headers class lowercases every header name,
// but the broker emits RFC-cased names natively ("Location", "Via",
// "X-Additional-Key") and several ETSI keywords assert them
// CASE-SENSITIVELY (the same class of problem haproxy's h1-case-adjust
// solves). node:http sends names verbatim, so this shim restores
// the conventional case on both directions instead of shipping whatever
// the Headers class kept.
const CASE = new Map([
  ["ngsild-tenant", "NGSILD-Tenant"],
  ["ngsild-results-count", "NGSILD-Results-Count"],
  ["ngsild-entitymap", "NGSILD-EntityMap"],
  ["etag", "ETag"],
]);
const recase = (k) => CASE.get(k) ?? k.replace(/(^|-)[a-z]/g, (m) => m.toUpperCase());

// The broker's own egress (notifications, forwards, @context fetches) runs
// through global fetch; replace it with a node:http-backed equivalent that
// keeps header case. GET/HEAD follow up to 5 redirects (fetch semantics for
// the @context path); bodies are buffered — every payload here is small.
globalThis.fetch = async (input, init) => {
  const req = new Request(input, init);
  const body =
    req.method === "GET" || req.method === "HEAD"
      ? null
      : Buffer.from(await req.arrayBuffer());
  const headers = {};
  req.headers.forEach((v, k) => {
    headers[recase(k)] = v;
  });
  if (body?.length) headers["Content-Length"] = String(body.length);
  let url = new URL(req.url);
  for (let hop = 0; ; hop++) {
    const res = await new Promise((resolve, reject) => {
      const mk = url.protocol === "https:" ? httpsRequest : httpRequest;
      const r = mk(url, { method: req.method, headers }, (res) => {
        const chunks = [];
        res.on("data", (c) => chunks.push(c));
        res.on("end", () => resolve({ res, buf: Buffer.concat(chunks) }));
      });
      r.on("error", reject);
      if (body?.length) r.write(body);
      r.end();
    });
    const status = res.res.statusCode;
    const loc = res.res.headers.location;
    if ([301, 302, 303, 307, 308].includes(status) && loc && hop < 5
        && (req.method === "GET" || req.method === "HEAD")) {
      url = new URL(loc, url);
      continue;
    }
    const respHeaders = new Headers();
    for (let i = 0; i < res.res.rawHeaders.length; i += 2) {
      respHeaders.append(res.res.rawHeaders[i], res.res.rawHeaders[i + 1]);
    }
    const noBody = [101, 103, 204, 205, 304].includes(status);
    const out = new Response(noBody ? null : res.buf, {
      status,
      headers: respHeaders,
    });
    // A constructed Response has url:"" — reqwest's wasm backend does
    // Url::parse(resp.url()).expect("url parse") and aborts on it.
    Object.defineProperty(out, "url", { value: url.href });
    return out;
  }
};

const port = Number(process.argv[2] ?? 9090);

await init({
  module_or_path: await readFile(new URL("./pkg/antares_wasm_bg.wasm", import.meta.url)),
});
// Persistence outside the browser: ANTARES_STORE=file runs the SAME redb
// write-through shadow the OPFS worker uses, over node:fs sync calls —
// the six methods FileSystemSyncAccessHandle exposes, fs-backed. O_RDWR
// (never "a+": append mode ignores the positional writes redb depends on).
class FsSyncAccessHandle {
  constructor(path) {
    this.fd = openSync(path, fsConstants.O_RDWR | fsConstants.O_CREAT);
  }
  getSize() {
    return fstatSync(this.fd).size;
  }
  read(buf, opts) {
    return readSync(this.fd, buf, 0, buf.length, opts?.at ?? 0);
  }
  write(buf, opts) {
    return writeSync(this.fd, buf, 0, buf.length, opts?.at ?? 0);
  }
  truncate(len) {
    ftruncateSync(this.fd, len);
  }
  flush() {
    fsyncSync(this.fd);
  }
  close() {
    closeSync(this.fd);
  }
}

// allowPrivateEgress: the suite's mocks (notification receivers, context
// servers) live on loopback/private nets — same knob the container sets.
// The per-port hostAlias keeps Via loop detection honest across the five
// shims of the federation tier (five instances named alike = 508 storm).
// 4.22 GC parity with the native harness: the wasm sweep loop reads
// globalThis.ANTARES_SWEEP_SECS (wasm32 has no process env) — forward the
// env var BEFORE construction, where the loop samples it.
if (process.env.ANTARES_SWEEP_SECS) {
  globalThis.ANTARES_SWEEP_SECS = Number(process.env.ANTARES_SWEEP_SECS);
}
// 5.8.1.4: the callback URL distributed subscriptions hand to remote
// brokers — same globalThis route (wasm32 has no process env). Without it
// the portless host-alias default is undialable between local processes.
if (process.env.ANTARES_PUBLIC_URL) {
  globalThis.ANTARES_PUBLIC_URL = process.env.ANTARES_PUBLIC_URL;
}
const hostAlias = process.env.ANTARES_HOST_ALIAS ?? `antares-wasm-${port}`;

const storeMode = process.env.ANTARES_STORE ?? "memory";
let broker;
if (storeMode === "file") {
  const file = process.env.ANTARES_FILE ?? `antares-${port}.redb`;
  broker = AntaresBroker.persistentWithHandle(
    new FsSyncAccessHandle(file),
    `fs:${file}`,
    true,
    hostAlias,
  );
} else if (storeMode === "memory") {
  broker = new AntaresBroker(true, hostAlias);
} else {
  console.error(`ANTARES_STORE=${storeMode}: the wasm artifact has memory and file only`);
  process.exit(2);
}

const server = createServer(async (req, res) => {
  try {
    // 6.3.2/6.3.4: POST, PATCH or PUT without Content-Length → bare 411, no
    // exemption for chunked. Natively hyper hands bounds.rs the wire headers
    // untouched; the wasm seam (browser.rs fetch) must stamp content-length
    // from the buffered body because a browser cannot carry the header — so
    // the header's wire-absence is only observable here, and this shim
    // enforces the precondition as part of the serialization layer it
    // replaces (same posture as the 6.3.6 strip below). node:http is
    // HTTP/1.x only, matching the bounds.rs version gate.
    if (
      ["POST", "PATCH", "PUT"].includes(req.method) &&
      req.headers["content-length"] === undefined
    ) {
      req.resume();
      res.writeHead(411);
      res.end();
      return;
    }
    const chunks = [];
    for await (const c of req) chunks.push(c);
    const body = Buffer.concat(chunks);
    const rawUrl = `http://localhost:${port}${req.url}`;
    const request = new Request(rawUrl, {
      method: req.method,
      headers: req.headers,
      body: body.length ? body : undefined,
      duplex: "half",
    });
    // WHATWG URL parsing inside Request normalizes percent-encoded dot
    // segments (/attrs/%2e%2e re-targets the parent resource). Natively
    // hyper hands the router the wire path untouched; restore it the same
    // way the egress fetch restores Response.url above.
    Object.defineProperty(request, "url", { value: rawUrl });
    const resp = await broker.fetch(request);
    const headers = {};
    resp.headers.forEach((v, k) => {
      headers[recase(k)] = v;
    });
    // 6.3.6: null-body statuses (204 above all) carry NO Content-Length.
    // Natively hyper strips it at serialization; this shim IS the
    // serialization layer here, so it does the same (the axum parts carry
    // "content-length: 0" through the wasm Response).
    const noBody = [101, 103, 204, 205, 304].includes(resp.status);
    if (noBody) delete headers["Content-Length"];
    res.writeHead(resp.status, headers);
    res.end(noBody ? undefined : Buffer.from(await resp.arrayBuffer()));
  } catch (e) {
    // The console gets the exception; the response does not. An egress
    // failure's text carries the URL it dialled, userinfo and all.
    console.error("shim error:", e);
    res.writeHead(500, { "content-type": "application/json" });
    res.end(JSON.stringify({ title: "shim error" }));
  }
});
// All interfaces on purpose: the Robot suite drives this shim from another
// container, and a loopback bind would not be reachable from it.
server.listen(port, "0.0.0.0", () =>
  console.log(`antares-wasm (Node tier) listening on :${port}`),
);
