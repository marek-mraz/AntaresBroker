import { createServer } from "node:http";
import { readFile } from "node:fs/promises";
import { extname, join, normalize } from "node:path";
import { chromium } from "playwright-core";
const ROOT = "/workspace/www/dist";
const MIME = { ".html": "text/html", ".js": "text/javascript", ".css": "text/css", ".wasm": "application/wasm", ".json": "application/json" };
const s = createServer(async (req, res) => {
  try {
    const p = normalize(new URL(req.url, "http://x").pathname).replace(/^\/+/, "");
    const f = join(ROOT, p === "" ? "index.html" : p);
    const body = await readFile(f);
    res.writeHead(200, { "content-type": MIME[extname(f)] ?? "application/octet-stream" });
    res.end(body);
  } catch { res.writeHead(404); res.end(); }
});
await new Promise((r) => s.listen(0, "127.0.0.1", r));
const browser = await chromium.launch();
const page = await (await browser.newContext()).newPage();
await page.goto(`http://127.0.0.1:${s.address().port}`, { waitUntil: "load" });
await page.waitForFunction(() => document.querySelectorAll(".react-flow__node").length >= 22, { timeout: 60_000 });
console.log("t(min)  opfs(KB)  temporalInstances");
for (let m = 0; m <= 14; m++) {
  const st = await page.evaluate(async () => {
    const est = await navigator.storage.estimate();
    const spaces = ["smart-city","old-town","old-town-market","old-town-castle","harbor","airport","university","energy-grid","transit","decidim","default"];
    let inst = 0;
    for (const sp of spaces) {
      const r = await window.brokerFetch("/ngsi-ld/v1/entities?limit=100&local=true", { headers: { "NGSILD-Tenant": sp } });
      for (const e of await r.json()) {
        const t = await window.brokerFetch(`/ngsi-ld/v1/temporal/entities/${encodeURIComponent(e.id)}`, { headers: { "NGSILD-Tenant": sp } });
        if (!t.ok) continue;
        const doc = await t.json();
        for (const [k, v] of Object.entries(doc)) {
          if (Array.isArray(v) && k !== "type") inst += v.length;
        }
      }
    }
    return { usage: Math.round((est.usage ?? 0) / 1024), inst };
  });
  console.log(`${m}\t${st.usage}\t${st.inst}`);
  if (m < 14) await page.waitForTimeout(60_000);
}
await browser.close(); s.close();
