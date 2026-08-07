// N4: the persistence host — a DEDICATED worker, because OPFS sync access
// handles exist nowhere else (not the main thread, not a Service Worker).
// It owns the exclusive handle on antares.redb (N4b: second opener gets a
// clear error) and runs the same broker over the same redb shadow as native
// `file` mode: commit-before-ack, format check, boot rebuild.
import init, { AntaresBroker } from "./pkg/antares_wasm.js";
import { installLoopback } from "./loopback.js";

let broker = null;
installLoopback(() => broker);

self.onmessage = async (e) => {
  const msg = e.data;
  try {
    if (msg.kind === "init") {
      await init();
      broker = await AntaresBroker.persistent(
        msg.file ?? "antares.redb",
        msg.allowPrivateEgress === true,
        msg.hostAlias,
      );
      broker.onNotification("http://page.local/", (url, body) => {
        self.postMessage({ kind: "notification", url, body });
        return true;
      });
      self.postMessage({ id: msg.id, ok: true });
      return;
    }
    if (msg.kind === "fetch") {
      const { method, url, headers, body } = msg.req;
      const r = await broker.fetch(
        new Request(url, {
          method,
          headers,
          body: body && body.byteLength ? body : undefined,
        }),
      );
      const out = new Uint8Array(await r.arrayBuffer());
      const hdrs = [];
      r.headers.forEach((v, k) => hdrs.push([k, v]));
      self.postMessage(
        { id: msg.id, ok: true, status: r.status, headers: hdrs, body: out },
        [out.buffer],
      );
    }
  } catch (err) {
    self.postMessage({ id: msg.id, ok: false, error: String(err) });
  }
};
