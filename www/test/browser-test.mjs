// N6: the demo page, driven headless — the CI proof that the browser build
// actually runs IN A BROWSER: broker up (OPFS worker preferred), entity
// created, subscription created, notification observed in-page, and the N4
// persistence contract: state survives a page reload, and a second tab gets
// the clear exclusive-owner error instead of a torn store. Run:
//
//   node www/test/browser-test.mjs        (needs `npx playwright-core install chromium`
//                                          and a built www/pkg — dev/wasm-build.sh)
import { createServer } from "node:http";
import { readFile } from "node:fs/promises";
import { extname, join, normalize } from "node:path";
import { chromium } from "playwright-core";

const ROOT = new URL("..", import.meta.url).pathname; // www/
const MIME = {
  ".html": "text/html",
  ".js": "text/javascript",
  ".mjs": "text/javascript",
  ".wasm": "application/wasm",
  ".json": "application/json",
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
const ctx = await browser.newContext();
const page = await ctx.newPage();
page.on("console", (m) => console.log("[page]", m.text()));
page.on("pageerror", (e) => console.log("[pageerror]", e.message));

const fail = async (msg) => {
  console.error(`FAIL: ${msg}`);
  await browser.close();
  server.close();
  process.exit(1);
};

const bootAndMode = async (p) => {
  await p.waitForFunction(() => document.getElementById("mode").textContent !== "…", {
    timeout: 30_000,
  });
  await p.waitForFunction(
    () => document.getElementById("health").textContent.includes('"store":'),
    { timeout: 15_000 },
  );
  return p.textContent("#mode");
};

try {
  await page.goto(base, { waitUntil: "load" });
  const mode = await bootAndMode(page);
  console.log("broker mode:", mode);
  const health = await page.textContent("#health");
  const store = (health.match(/"store":"([a-z]+)"/) ?? [])[1];
  console.log("store:", store);
  if (mode === "opfs-worker" && store !== "file") {
    await fail(`opfs-worker must report store=file, got ${store}`);
  }
  if (!["opfs-worker", "service-worker", "in-page"].includes(mode)) {
    await fail(`unexpected mode ${mode}`);
  }

  // Subscribe FIRST, then create — the create must produce a notification.
  await page.click("#subscribe");
  await page.click("#create");
  await page.waitForFunction(
    () => document.querySelectorAll("#entities li").length >= 1,
    { timeout: 15_000 },
  );
  await page.waitForFunction(
    () => [...document.querySelectorAll("#log .notif")].length >= 1,
    { timeout: 15_000 },
  );
  console.log("notification observed:", await page.textContent("#log .notif"));

  if (mode === "opfs-worker") {
    // N4b: a second tab must get the exclusive-owner refusal and fall back —
    // never a second writer on the same file.
    const page2 = await ctx.newPage();
    page2.on("console", (m) => console.log("[page2]", m.text()));
    await page2.goto(base, { waitUntil: "load" });
    const mode2 = await bootAndMode(page2);
    console.log("second tab mode:", mode2);
    if (mode2 === "opfs-worker") await fail("two tabs both claim the OPFS store");
    const log2 = await page2.textContent("#log");
    if (!log2.includes("another tab")) {
      await fail(`second tab should surface the owner error, log: ${log2.slice(0, 200)}`);
    }
    await page2.close();

    // N4: persistence — reload releases the worker's handle; the fresh
    // worker must rebuild the SAME store from OPFS (boot rebuild, B4).
    await page.reload({ waitUntil: "load" });
    const modeAfter = await bootAndMode(page);
    if (modeAfter !== "opfs-worker") {
      await fail(`after reload expected opfs-worker again, got ${modeAfter}`);
    }
    await page.waitForFunction(
      () => document.querySelectorAll("#entities li").length >= 1,
      { timeout: 15_000 },
    );
    console.log("entity survived the reload — OPFS persistence holds");
  } else if (mode === "service-worker") {
    // No OPFS in this engine: fall back to the SW cross-tab assertion.
    const page2 = await ctx.newPage();
    await page2.goto(base, { waitUntil: "load" });
    await page2.waitForFunction(() => navigator.serviceWorker.controller !== null, {
      timeout: 15_000,
    });
    const rooms = await page2.evaluate(async () => {
      const r = await fetch(
        "/ngsi-ld/v1/entities?type=Room,TemperatureSensor,ParkingSpot,Streetlight&limit=100",
      );
      return (await r.json()).length;
    });
    if (rooms < 1) await fail("second tab does not see the first tab's entity");
    await page2.close();
  }

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

  // Board overview: ▶ create demo must materialize spaces + CSRs + pipelines
  // (idempotently), 🧨 remove everything must wipe them all and reseed.
  page.on("dialog", (d) => d.accept()); // the reset confirm()
  await page.click("#overview");
  await page.waitForSelector("#dlg-overview[open]", { timeout: 10_000 });
  await page.click("#ov-demo");
  await page.waitForFunction(
    () => {
      const t = document.getElementById("ov-body").textContent;
      const csrs = document.querySelectorAll("#ov-body .tag").length;
      return t.includes("smart-city") && t.includes("old-town") &&
        t.includes("harbor") && csrs >= 2 && t.includes("simulated device") &&
        t.includes("copy · ParkingSpot");
    },
    { timeout: 30_000 },
  );
  console.log("demo built: spaces + 2 CSRs + device & copy pipelines visible on the board");
  // Second click must not duplicate anything (idempotence).
  await page.click("#ov-demo");
  await page.waitForFunction(
    () => !document.getElementById("ov-demo").disabled, { timeout: 30_000 });
  const csrCount = await page.evaluate(
    () => document.querySelectorAll("#ov-body .tag").length);
  if (csrCount !== 2) await fail(`demo is not idempotent: ${csrCount} CSRs after 2nd run`);
  await page.click("#ov-reset");
  await page.waitForFunction(
    () => {
      const t = document.getElementById("ov-body").textContent;
      return t.includes("pipelines (0)") && t.includes("none — 🔗") &&
        ![...document.querySelectorAll("#ov-body .item")].some((i) =>
          /🏠 [1-9]/.test(i.textContent));
    },
    { timeout: 30_000 },
  );
  console.log("remove everything: board wiped and reseeded, all spaces at 0 local");

  console.log(`PASS: browser tier (${mode}) — broker, entity, subscription, notification, federation, board demo/reset`);
  await browser.close();
  server.close();
} catch (e) {
  console.error(e);
  await fail(String(e));
}
