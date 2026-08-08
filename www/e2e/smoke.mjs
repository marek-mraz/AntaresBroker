// E2E smoke against the BUILT app: boot (wasm broker in a real browser),
// auto-demo, React Flow board populated, tenant sheet + filters, history
// panel, request log. Run: npm run e2e
import { spawn } from "node:child_process";
import { chromium } from "playwright-core";

// --host 127.0.0.1: vite's default "localhost" can bind ::1 only, and the
// test (and CI runners) dial IPv4.
const preview = spawn("npx", ["vite", "preview", "--port", "42096", "--strictPort", "--host", "127.0.0.1"], {
  cwd: new URL("..", import.meta.url).pathname,
  stdio: "pipe",
});
await new Promise((resolve, reject) => {
  preview.stdout.on("data", (d) => d.toString().includes("42096") && resolve());
  preview.stderr.on("data", (d) => process.stderr.write(d));
  preview.on("exit", (c) => reject(new Error(`vite preview exited ${c}`)));
  setTimeout(() => reject(new Error("preview server timeout")), 20_000);
});

const browser = await chromium.launch();
const page = await (await browser.newContext({ viewport: { width: 1500, height: 950 } })).newPage();
page.on("pageerror", (e) => console.log("[pageerror]", e.message));

const fail = async (msg) => {
  console.error(`FAIL: ${msg}`);
  await browser.close();
  preview.kill();
  process.exit(1);
};

try {
  await page.goto("http://127.0.0.1:42096/", { waitUntil: "load" });

  // boot: mode pill leaves "booting"; auto-demo builds the city
  await page.waitForFunction(
    () => document.querySelector("[data-mode]")?.dataset.mode !== "booting",
    { timeout: 60_000 },
  );
  const mode = await page.evaluate(() => document.querySelector("[data-mode]").dataset.mode);
  console.log("mode:", mode);
  if (!["opfs-worker", "in-page"].includes(mode)) await fail(`unexpected mode ${mode}`);

  // board: 9 space nodes + 13 device nodes from the demo
  await page.waitForFunction(
    () => document.querySelectorAll(".react-flow__node").length >= 20,
    { timeout: 60_000 },
  );
  const nodes = await page.evaluate(() => document.querySelectorAll(".react-flow__node").length);
  const edges = await page.evaluate(() => document.querySelectorAll(".react-flow__edge").length);
  console.log(`board: ${nodes} nodes, ${edges} edges`);
  if (edges < 10) await fail(`expected ≥10 edges (7 CSR + pipes), got ${edges}`);

  // click the hub → sheet shows federated rows with origin chips
  await page.click('[data-id="s:smart-city"]');
  await page.waitForFunction(
    () => document.querySelectorAll('[data-testid="sheet-row"]').length >= 3,
    { timeout: 30_000 },
  );
  const before = await page.evaluate(
    () => document.querySelectorAll('[data-testid="sheet-row"]').length);
  console.log("sheet rows (smart-city):", before);

  // origin filter: pick a federated peer → only its rows remain
  const fedOrigin = await page.evaluate(() => {
    const sel = document.querySelector('[data-testid="filter-origin"]');
    const opt = [...sel.options].find((o) => o.value !== "all" && o.value !== "local");
    if (!opt) return null;
    sel.value = opt.value;
    sel.dispatchEvent(new Event("change", { bubbles: true }));
    return opt.value;
  });
  if (!fedOrigin) await fail("no federated origin option in the sheet");
  await page.waitForFunction(
    (n) => document.querySelectorAll('[data-testid="sheet-row"]').length < n,
    before,
    { timeout: 10_000 },
  );
  console.log(`origin filter "${fedOrigin}" narrows the sheet`);
  await page.evaluate(() => {
    const sel = document.querySelector('[data-testid="filter-origin"]');
    sel.value = "all";
    sel.dispatchEvent(new Event("change", { bubbles: true }));
  });

  // row click → temporal history panel appears (chart or "no history yet")
  await page.click('[data-testid="sheet-row"]');
  await page.waitForSelector('[data-testid="history"]', { timeout: 10_000 });
  console.log("history panel opens on row click");

  // request log carries real traffic
  await page.click('[data-testid="reqlog-toggle"]');
  await page.waitForFunction(
    () => document.querySelectorAll('[data-testid="reqlog-entries"] div').length >= 5,
    { timeout: 10_000 },
  );
  console.log("request log populated");

  await page.screenshot({ path: "e2e/last-run.png" });
  console.log("PASS: react app e2e — boot, demo board, sheet + filters, history, request log");
  await browser.close();
  preview.kill();
} catch (e) {
  await page.screenshot({ path: "e2e/last-fail.png" }).catch(() => {});
  await fail(String(e));
}
