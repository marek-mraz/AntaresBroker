// The browser tier — the ETSI suite talks real HTTP to THIS process,
// and every request is forwarded INTO a headless-Chromium page hosting the
// React playground (www/dist → transport ladder: OPFS worker / in-page).
// The response the suite sees is byte-for-byte what window.brokerFetch
// returned inside the page. So the suite exercises the same .wasm a user's
// browser runs, on the engine's own Request/Response/fetch/OPFS plumbing.
//
//   node www/test/etsi-proxy.mjs        binds 9090..9094 (all → the one page;
//                                       the suite set only queries 9090,
//                                       the pipeline health-waits all five)
//
// Chromium runs with --disable-web-security: a HARNESS-ONLY CORS bypass so
// the broker's own egress (suite @context servers, notification receivers —
// none of which send CORS headers) works from inside the page. The REAL
// browser limits stay documented elsewhere; this tier proves
// engine-correctness, not CORS policy.
import { createServer } from "node:http";
import { readFile } from "node:fs/promises";
import { extname, join, normalize } from "node:path";
import { chromium } from "playwright-core";

const ROOT = new URL("../dist", import.meta.url).pathname;
const MIME = {
  ".html": "text/html",
  ".js": "text/javascript",
  ".mjs": "text/javascript",
  ".css": "text/css",
  ".wasm": "application/wasm",
  ".json": "application/json",
  ".svg": "image/svg+xml",
};

// ---- static host for the page itself --------------------------------------
const statics = createServer(async (req, res) => {
  try {
    const path = normalize(new URL(req.url, "http://x").pathname).replace(/^\/+/, "");
    const file = join(ROOT, path === "" ? "index.html" : path);
    if (!file.startsWith(ROOT)) throw new Error("traversal");
    const body = await readFile(file);
    res.writeHead(200, { "content-type": MIME[extname(file)] ?? "application/octet-stream" });
    res.end(body);
  } catch {
    res.writeHead(404);
    res.end();
  }
});
await new Promise((r) => statics.listen(0, "127.0.0.1", r));
const base = `http://127.0.0.1:${statics.address().port}`;

// ---- the page -------------------------------------------------------------
const browser = await chromium.launch({
  args: ["--disable-web-security"], // harness-only, see header
});
const ctx = await browser.newContext();
const page = await ctx.newPage();
page.on("console", (m) => console.log("[page]", m.text()));
page.on("pageerror", (e) => console.log("[pageerror]", e.message));
// The suite needs a pristine broker — suppress the first-visit auto-demo.
await page.addInitScript(() => localStorage.setItem("antares.demoed", "1"));
await page.goto(`${base}/?allowPrivateEgress=1`, { waitUntil: "load" });
await page.waitForFunction(
  () => document.querySelector("[data-mode]")?.dataset.mode !== "booting",
  { timeout: 30_000 },
);
await page.waitForFunction(
  async () => (await (await window.brokerFetch("/q/health")).json()).store !== undefined,
  { timeout: 15_000 },
);
console.log(
  "browser tier up, mode:",
  await page.evaluate(() => document.querySelector("[data-mode]").dataset.mode),
);

// ---- forward one suite request through the page ---------------------------
// Hop-by-hop / transport headers the page's fetch must not (or cannot) carry.
const DROP_REQ = new Set([
  "host", "connection", "content-length", "transfer-encoding",
  "keep-alive", "accept-encoding", "upgrade", "proxy-connection",
]);
const DROP_RES = new Set(["content-length", "content-encoding", "transfer-encoding", "connection"]);
const CASE = new Map([
  ["ngsild-tenant", "NGSILD-Tenant"],
  ["ngsild-results-count", "NGSILD-Results-Count"],
  ["ngsild-entitymap", "NGSILD-EntityMap"],
  ["etag", "ETag"],
]);
const recase = (k) => CASE.get(k) ?? k.replace(/(^|-)[a-z]/g, (m) => m.toUpperCase());

async function forward(req, res) {
  const chunks = [];
  for await (const c of req) chunks.push(c);
  const body = Buffer.concat(chunks).toString("utf8");
  const headers = {};
  for (const [k, v] of Object.entries(req.headers)) {
    if (!DROP_REQ.has(k.toLowerCase())) headers[k] = v;
  }
  try {
    // A request that dies inside the page must not hang the suite forever
    // (python-requests has no timeout): 30 s, then 504.
    const out = await Promise.race([
      forwardEval(req, headers, body),
      new Promise((_, rej) => setTimeout(() => rej(new Error("page deadline (30s)")), 30_000)),
    ]);
    for (const [k, v] of out.headers) {
      if (!DROP_RES.has(k)) res.setHeader(recase(k), v);
    }
    res.writeHead(out.status);
    res.end(Buffer.from(out.body, "utf8"));
  } catch (e) {
    res.writeHead(502, { "content-type": "text/plain" });
    res.end(`browser-tier proxy: ${e.message}`);
  }
}

function forwardEval(req, headers, body) {
  return page.evaluate(
      async ({ path, method, headers, body }) => {
        const r = await window.brokerFetch(path, {
          method,
          headers,
          body: body === "" || method === "GET" || method === "HEAD" ? undefined : body,
        });
        const text = await r.text();
        const hdrs = [];
        r.headers.forEach((v, k) => hdrs.push([k, v]));
        return { status: r.status, headers: hdrs, body: text };
      },
      { path: req.url, method: req.method, headers, body },
    );
}

// ---- suite-facing listeners ------------------------------------------------
const PORTS = [9090, 9091, 9092, 9093, 9094];
for (const port of PORTS) {
  const s = createServer(forward);
  await new Promise((r) => s.listen(port, "0.0.0.0", r));
}
console.log(`READY — proxying :${PORTS.join(" :")} into the page`);

// The pipeline teardown kills this pid — take Chromium down with us.
for (const sig of ["SIGTERM", "SIGINT"]) {
  process.on(sig, async () => {
    await browser.close().catch(() => {});
    process.exit(0);
  });
}
