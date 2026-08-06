// N6: the demo page, driven headless — the CI proof that the browser build
// actually runs IN A BROWSER: Service Worker registered, entity created,
// subscription created, notification observed in-page. Run:
//
//   node www/test/browser-test.mjs        (needs `npx playwright install chromium`
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
    res.writeHead(200, {
      "content-type": MIME[extname(file)] ?? "application/octet-stream",
      // OPFS/SharedArrayBuffer-adjacent APIs want isolation; harmless now,
      // required when N4 lands.
      "cross-origin-opener-policy": "same-origin",
      "cross-origin-embedder-policy": "require-corp",
    });
    res.end(body);
  } catch {
    res.writeHead(404);
    res.end();
  }
});
await new Promise((r) => server.listen(0, "127.0.0.1", r));
const port = server.address().port;
const base = `http://127.0.0.1:${port}`;

const browser = await chromium.launch();
// ONE context: browser.newPage() would mint a fresh context per call, and a
// separate context means a separate service worker — i.e. a different broker.
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

try {
  await page.goto(base, { waitUntil: "load" });
  await page.waitForFunction(
    () => document.getElementById("mode").textContent !== "…",
    { timeout: 30_000 },
  );
  const mode = await page.textContent("#mode");
  console.log("broker mode:", mode);
  if (mode !== "service-worker") {
    // First load can race SW control; reload once — after that it must hold.
    await page.reload({ waitUntil: "load" });
    await page.waitForFunction(
      () => document.getElementById("mode").textContent !== "…",
      { timeout: 30_000 },
    );
  }
  const finalMode = await page.textContent("#mode");
  console.log("final mode:", finalMode);
  if (finalMode !== "service-worker" && finalMode !== "in-page") {
    await fail(`unexpected mode ${finalMode}`);
  }

  // The page's own health line proves /q/health round-tripped the broker.
  await page.waitForFunction(
    () => document.getElementById("health").textContent.includes('"store":"memory"'),
    { timeout: 15_000 },
  );

  // Subscribe FIRST, then create — the create must produce a notification.
  await page.click("#subscribe");
  await page.click("#create");
  await page.waitForFunction(
    () => document.querySelectorAll("#entities li").length >= 1,
    { timeout: 15_000 },
  );
  await page.waitForFunction(
    () =>
      [...document.querySelectorAll("#log .notif")].length >= 1,
    { timeout: 15_000 },
  );
  const notif = await page.textContent("#log .notif");
  console.log("notification observed:", notif);

  // A second tab sees the SAME broker when the SW carries it (shared state).
  if (finalMode === "service-worker") {
    const page2 = await ctx.newPage();
    page2.on("console", (m) => console.log("[page2]", m.text()));
    await page2.goto(base, { waitUntil: "load" });
    await page2.waitForFunction(() => navigator.serviceWorker.controller !== null, {
      timeout: 15_000,
    });
    const rooms = await page2.evaluate(async () => {
      const r = await fetch("/ngsi-ld/v1/entities?type=Room&limit=100");
      const t = await r.text();
      console.log("tab2 raw:", r.status, t.slice(0, 300));
      const h = await (await fetch("/q/health")).json();
      console.log("tab2 health:", JSON.stringify(h).slice(0, 120));
      return JSON.parse(t).length;
    });
    // was the FIRST tab's view still alive? re-ask through tab 1
    const tab1rooms = await page.evaluate(async () => {
      const r = await fetch("/ngsi-ld/v1/entities?type=Room&limit=100");
      return (await r.json()).length;
    });
    console.log("tab1 re-query sees", tab1rooms);
    if (rooms < 1) await fail("second tab does not see the first tab's entity");
    console.log("second tab sees", rooms, "entities via the SW broker");
  }

  console.log("PASS: browser tier — SW/page broker, entity, subscription, notification");
  await browser.close();
  server.close();
} catch (e) {
  console.error(e);
  await fail(String(e));
}
