import { describe, expect, it, vi } from "vitest";
import { installVirtualHost } from "../src/broker/virtualhost.js";

// The virtual host must route ONLY same-origin /ngsi-ld/* to the broker
// handler — everything else (other origins, this origin's real assets)
// stays on the native fetch.
describe("virtualhost", () => {
  const ok = () => new Response("[]", { status: 200 });

  function fresh() {
    const native = vi.fn(async () => new Response("native"));
    self.fetch = native;
    const handler = vi.fn(async () => ok());
    installVirtualHost(handler);
    return { native, handler };
  }

  it("routes same-origin /ngsi-ld/* to the handler, path+query preserved", async () => {
    const { native, handler } = fresh();
    const r = await fetch(`${location.origin}/ngsi-ld/v1/entities?type=Vehicle&limit=2`);
    expect(r.status).toBe(200);
    expect(native).not.toHaveBeenCalled();
    const [path, opts] = handler.mock.calls[0];
    expect(path).toBe("/ngsi-ld/v1/entities?type=Vehicle&limit=2");
    expect(opts.method).toBe("GET");
    expect(opts.body).toBeUndefined();
  });

  it("relative /ngsi-ld/ URLs route too, with method, headers and body", async () => {
    const { handler } = fresh();
    await fetch("/ngsi-ld/v1/entities", {
      method: "POST",
      headers: { "NGSILD-Tenant": "city", "Content-Type": "application/ld+json" },
      body: '{"id":"urn:ngsi-ld:V:1","type":"Vehicle"}',
    });
    const [path, opts] = handler.mock.calls[0];
    expect(path).toBe("/ngsi-ld/v1/entities");
    expect(opts.method).toBe("POST");
    // Headers round-trip lowercases names — transport's log lookup covers both.
    expect(opts.headers["ngsild-tenant"]).toBe("city");
    expect(opts.body).toBe('{"id":"urn:ngsi-ld:V:1","type":"Vehicle"}');
  });

  it("leaves other origins and non-ngsi paths on native fetch", async () => {
    const { native, handler } = fresh();
    await fetch("https://example.com/ngsi-ld/v1/entities");
    await fetch("/openapi/ngsi-ld-api.yaml");
    expect(handler).not.toHaveBeenCalled();
    expect(native).toHaveBeenCalledTimes(2);
  });
});
