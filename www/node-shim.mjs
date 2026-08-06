// N7a: the Node tier — the SAME .wasm the browser loads, fronted by a thin
// http.createServer shim so the ETSI Robot suite can talk to it over real
// TCP. No CORS, unrestricted outbound fetch (undici), so every serial suite
// is in scope here; the browser tier's structural limits (N7c) don't apply.
//
//   node www/node-shim.mjs [port]       (default 9090)
//
// Build www/pkg first: dev/wasm-build.sh
import { readFile } from "node:fs/promises";
import { createServer } from "node:http";
import init, { AntaresBroker } from "./pkg/antares_wasm.js";

const port = Number(process.argv[2] ?? 9090);

await init({
  module_or_path: await readFile(new URL("./pkg/antares_wasm_bg.wasm", import.meta.url)),
});
// allowPrivateEgress: the suite's mocks (notification receivers, context
// servers) live on loopback/private nets — same knob the container sets.
const broker = new AntaresBroker(true);

const server = createServer(async (req, res) => {
  try {
    const chunks = [];
    for await (const c of req) chunks.push(c);
    const body = Buffer.concat(chunks);
    const request = new Request(`http://localhost:${port}${req.url}`, {
      method: req.method,
      headers: req.headers,
      body: body.length ? body : undefined,
      duplex: "half",
    });
    const resp = await broker.fetch(request);
    const headers = [];
    resp.headers.forEach((v, k) => headers.push([k, v]));
    res.writeHead(resp.status, Object.fromEntries(headers));
    res.end(Buffer.from(await resp.arrayBuffer()));
  } catch (e) {
    console.error("shim error:", e);
    res.writeHead(500, { "content-type": "application/json" });
    res.end(JSON.stringify({ title: "shim error", detail: String(e) }));
  }
});
server.listen(port, "0.0.0.0", () =>
  console.log(`antares-wasm (Node tier) listening on :${port}`),
);
