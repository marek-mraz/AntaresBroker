// N6: the React playground (www-react/dist), driven headless — the CI proof
// that the browser build actually runs IN A BROWSER: broker up (OPFS worker
// preferred), the auto-demo populates the board through the real NGSI-LD API,
// a subscription delivers an in-page notification, N9 cross-tenant federation
// over the loopback host, and the N4 persistence contract: state survives a
// page reload, and a second tab gets the exclusive-owner fallback instead of
// a torn store. Run:
//
//   node www/test/browser-test.mjs        (needs `npx playwright-core install chromium`
//                                          and a built www-react/dist — npm run build)
import { createServer } from "node:http";
import { readFile, access } from "node:fs/promises";
import { extname, join, normalize } from "node:path";
import { chromium } from "playwright-core";

const ROOT = new URL("../../www-react/dist", import.meta.url).pathname;
await access(join(ROOT, "index.html")).catch(() => {
  console.error("FAIL: www-react/dist missing — run `npm run build` in www-react first");
  process.exit(1);
});
const MIME = {
  ".html": "text/html",
  ".js": "text/javascript",
  ".mjs": "text/javascript",
  ".css": "text/css",
  ".wasm": "application/wasm",
  ".json": "application/json",
  ".svg": "image/svg+xml",
};

const server = createServer(async (req, res) => {
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
await new Promise((r) => server.listen(0, "127.0.0.1", r));
const base = `http://127.0.0.1:${server.address().port}`;

const browser = await chromium.launch();
// ONE context: browser.newPage() would mint a fresh context per call, and a
// separate context means a separate origin partition — a different broker.
const ctx = await browser.newContext({ viewport: { width: 1500, height: 950 } });
const page = await ctx.newPage();
page.on("pageerror", (e) => console.log("[pageerror]", e.message));

const fail = async (msg) => {
  console.error(`FAIL: ${msg}`);
  await browser.close();
  server.close();
  process.exit(1);
};

const bootMode = async (p) => {
  await p.waitForFunction(
    () => document.querySelector("[data-mode]")?.dataset.mode !== "booting",
    { timeout: 60_000 },
  );
  return p.evaluate(() => document.querySelector("[data-mode]").dataset.mode);
};

try {
  await page.goto(base, { waitUntil: "load" });
  const mode = await bootMode(page);
  console.log("broker mode:", mode);
  if (!["opfs-worker", "in-page"].includes(mode)) await fail(`unexpected mode ${mode}`);
  const store = await page.evaluate(async () => {
    const h = await (await window.brokerFetch("/q/health")).json();
    return h.store;
  });
  console.log("store:", store);
  if (mode === "opfs-worker" && store !== "file") {
    await fail(`opfs-worker must report store=file, got ${store}`);
  }

  // Auto-demo (first visit) builds the city through the real API.
  await page.waitForFunction(
    () => document.querySelectorAll(".react-flow__node").length >= 20,
    { timeout: 60_000 },
  );
  console.log(
    "board:",
    await page.evaluate(() => document.querySelectorAll(".react-flow__node").length),
    "nodes",
  );

  // Subscribe FIRST, then create — the create must produce an in-page
  // notification (endpoint http://page.local/ fans out to the page).
  const notif = await page.evaluate(async () => {
    const CTX = "https://uri.etsi.org/ngsi-ld/v1/ngsi-ld-core-context-v1.8.jsonld";
    const post = (path, body) =>
      window.brokerFetch(path, {
        method: "POST",
        headers: { "Content-Type": "application/ld+json" },
        body: JSON.stringify({ ...body, "@context": CTX }),
      });
    let r = await post("/ngsi-ld/v1/subscriptions", {
      id: "urn:ngsi-ld:Subscription:browser-smoke",
      type: "Subscription",
      entities: [{ type: "BrowserSmoke" }],
      notification: {
        endpoint: { uri: "http://page.local/", accept: "application/json" },
      },
    });
    if (r.status !== 201) return `subscribe → ${r.status}`;
    r = await post("/ngsi-ld/v1/entities", {
      id: "urn:ngsi-ld:BrowserSmoke:1",
      type: "BrowserSmoke",
      v: { type: "Property", value: 1 },
    });
    if (r.status !== 201) return `create → ${r.status}`;
    return "OK";
  });
  if (notif !== "OK") await fail(`notification setup: ${notif}`);
  await page.waitForFunction(
    () =>
      window.__antares.notifications.some((n) =>
        (n.data ?? []).some((e) => e.id === "urn:ngsi-ld:BrowserSmoke:1"),
      ),
    { timeout: 15_000 },
  );
  console.log("notification observed in-page");

  // N9: cross-tenant federation inside the ONE in-browser broker — space-a
  // holds a CSR whose endpoint is the loopback host and whose `tenant`
  // member (5.2.9) names space-b; a federated query in space-a must return
  // space-b's entity, and local=true must not.
  const fed = await page.evaluate(async () => {
    const CTX =
      "https://uri.etsi.org/ngsi-ld/v1/ngsi-ld-core-context-v1.8.jsonld";
    const api = (tenant, path, opts = {}) =>
      window.brokerFetch(path, {
        ...opts,
        headers: { ...(opts.headers ?? {}), "NGSILD-Tenant": tenant },
      });
    const post = (tenant, path, body) =>
      api(tenant, path, {
        method: "POST",
        headers: { "Content-Type": "application/ld+json" },
        body: JSON.stringify({ ...body, "@context": CTX }),
      });
    let r = await post("space-b", "/ngsi-ld/v1/entities", {
      id: "urn:ngsi-ld:FedCar:bt-1",
      type: "FedCar",
      speed: { type: "Property", value: 1 },
    });
    if (r.status !== 201) return `space-b create → ${r.status}`;
    r = await post("space-a", "/ngsi-ld/v1/csourceRegistrations", {
      id: "urn:ngsi-ld:ContextSourceRegistration:browser-a-to-b",
      type: "ContextSourceRegistration",
      information: [{ entities: [{ type: "FedCar" }] }],
      endpoint: "http://self.antares.internal",
      mode: "inclusive",
      tenant: "space-b",
    });
    if (r.status !== 201) return `space-a CSR → ${r.status}`;
    r = await api("space-a", "/ngsi-ld/v1/entities?type=FedCar");
    const docs = await r.json();
    if (!docs.some((e) => e.id === "urn:ngsi-ld:FedCar:bt-1")) {
      return `federated query missed the entity: ${JSON.stringify(docs)}`;
    }
    r = await api("space-a", "/ngsi-ld/v1/entities?type=FedCar&local=true");
    if ((await r.json()).length !== 0) return "local=true leaked the peer";
    return "OK";
  });
  if (fed !== "OK") await fail(`cross-tenant federation: ${fed}`);
  console.log("cross-tenant federation over the loopback host holds");

  // Temporal chart: a simulated device ticks observedAt-stamped readings, so
  // clicking its sheet row must render the value line AND the Δ-bars chart
  // fed by /temporal/entities. Use transit: its local rows are direct source
  // devices (3 s ticks), so history accrues fast and deterministically.
  await page.click('[data-id="s:transit"]');
  await page.waitForSelector('[data-testid="sheet-row"]', { timeout: 30_000 });
  await page.click('[data-testid="sheet-row"]');
  await page.waitForSelector('[data-testid="history"]', { timeout: 10_000 });
  await page.waitForSelector('[data-testid="values-chart"]', { timeout: 60_000 });
  await page.waitForSelector('[data-testid="changes-chart"]', { timeout: 60_000 });
  const bars = await page.evaluate(
    () => document.querySelectorAll('[data-testid="delta-bar"]').length,
  );
  console.log(`temporal chart renders (${bars} Δ bars)`);

  if (mode === "opfs-worker") {
    // N4b: a second tab must get the exclusive-owner refusal and fall back —
    // never a second writer on the same file.
    const page2 = await ctx.newPage();
    page2.on("pageerror", (e) => console.log("[page2 pageerror]", e.message));
    await page2.goto(base, { waitUntil: "load" });
    const mode2 = await bootMode(page2);
    console.log("second tab mode:", mode2);
    if (mode2 === "opfs-worker") await fail("two tabs both claim the OPFS store");
    const bootError = await page2.evaluate(() => window.__antares.transport.bootError);
    if (!String(bootError).includes("another tab")) {
      await fail(`second tab should surface the owner error, got: ${bootError}`);
    }
    await page2.close();

    // N4: persistence — reload releases the worker's handle; the fresh
    // worker must rebuild the SAME store from OPFS (boot rebuild, B4).
    // The auto-demo runs only once (localStorage), so surviving state is
    // proof of OPFS, not of a re-seed.
    await page.reload({ waitUntil: "load" });
    const modeAfter = await bootMode(page);
    if (modeAfter !== "opfs-worker") {
      await fail(`after reload expected opfs-worker again, got ${modeAfter}`);
    }
    const survived = await page.evaluate(async () => {
      const r = await window.brokerFetch("/ngsi-ld/v1/entities?type=FedCar", {
        headers: { "NGSILD-Tenant": "space-b" },
      });
      return (await r.json()).some((e) => e.id === "urn:ngsi-ld:FedCar:bt-1");
    });
    if (!survived) await fail("space-b FedCar entity lost across reload");
    await page.waitForFunction(
      () => document.querySelectorAll(".react-flow__node").length >= 20,
      { timeout: 60_000 },
    );
    console.log("state survived the reload — OPFS persistence holds");
  }

  console.log("PASS: browser tier (react app) — boot, demo, notification, federation, persistence");
  await browser.close();
  server.close();
} catch (e) {
  await fail(String(e));
}
