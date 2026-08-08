import { describe, expect, it } from "vitest";
import {
  ALL_TYPES, DEMO, GENERATORS, TENANT_RE, TYPES,
  arrangeBoard, buildTemplate, normalizeTemplate,
} from "../src/model.js";

describe("sensor catalog", () => {
  it("every type carries emoji, attribute and a generator", () => {
    for (const [name, t] of Object.entries(TYPES)) {
      expect(t.emoji, name).toBeTruthy();
      expect(t.attr, name).toBeTruthy();
      expect(typeof t.gen(Date.now()), name).toBe("number");
    }
  });
  it("one generator per type, derived", () => {
    expect(Object.keys(GENERATORS).length).toBe(Object.keys(TYPES).length);
    for (const g of Object.values(GENERATORS)) expect(TYPES[g.type]).toBeTruthy();
  });
  it("ALL_TYPES joins the catalog", () => {
    expect(ALL_TYPES.split(",").length).toBe(Object.keys(TYPES).length);
  });
});

describe("tenant name rule", () => {
  it("rejects spaces and junk", () => {
    expect(TENANT_RE.test("fsdafda dfa df dfa")).toBe(false);
    expect(TENANT_RE.test("")).toBe(false);
    expect(TENANT_RE.test("a".repeat(65))).toBe(false);
    expect(TENANT_RE.test("under_score")).toBe(false);
  });
  it("accepts A-Za-z0-9-", () => {
    expect(TENANT_RE.test("smart-city")).toBe(true);
    expect(TENANT_RE.test("Decidim-2")).toBe(true);
  });
});

describe("DEMO topology", () => {
  it("meets the ordered floor: ≥8 spaces, ≥10 devices, ≥5 CSRs, 3 copies", () => {
    expect(DEMO.spaces.length).toBeGreaterThanOrEqual(8);
    expect(DEMO.devices.length).toBeGreaterThanOrEqual(10);
    expect(DEMO.csrs.length).toBeGreaterThanOrEqual(5);
    expect(DEMO.copies.length).toBe(3);
    expect(DEMO.spaces).toContain("decidim");
  });

  it("every device's data reaches the smart-city hub", () => {
    // (space, type) reaches the hub if a CSR covers it there, or a copy
    // moves it to a space from which it reaches the hub.
    const reaches = (space, type, hops = 0) => {
      if (hops > 4) return false;
      if (space === DEMO.hub) return true;
      if (DEMO.csrs.some(([hub, peer, t]) =>
        hub === DEMO.hub && peer === space && (t === null || t === type))) return true;
      return DEMO.copies.some(([from, into, t]) =>
        from === space && t === type && reaches(into, type, hops + 1));
    };
    for (const [space, type] of DEMO.devices) {
      expect(reaches(space, type), `${space}/${type} must reach ${DEMO.hub}`).toBe(true);
    }
  });

  it("all demo names pass the tenant rule and layout covers every space", () => {
    for (const s of DEMO.spaces) {
      expect(TENANT_RE.test(s), s).toBe(true);
      expect(DEMO.layout[s], `layout for ${s}`).toBeTruthy();
    }
  });
});

describe("arrangeBoard — the demo layout is collision-free by construction", () => {
  const pipes = [
    ...DEMO.devices.map(([into, type, secs], i) => ({ id: `d${i}`, kind: "source", type, into, secs })),
    ...DEMO.copies.map(([from, into, type, secs], i) => ({ id: `c${i}`, kind: "sync", from, into, type, secs })),
  ];
  const pos = arrangeBoard(DEMO.spaces, pipes, { hub: DEMO.hub });
  // space: 110px circle; device: 96×44 chip (bounding-circle radius)
  const GEOM = { s: { w: 110, h: 110, r: 55 }, p: { w: 96, h: 44, r: 48 } };
  const centers = Object.entries(pos).map(([key, p]) => ({
    key, r: GEOM[key[0]].r,
    x: p.x + GEOM[key[0]].w / 2,
    y: p.y + GEOM[key[0]].h / 2,
  }));

  it("positions every space and every device, nothing else", () => {
    expect(Object.keys(pos).filter((k) => k.startsWith("s:"))).toHaveLength(DEMO.spaces.length);
    expect(Object.keys(pos).filter((k) => k.startsWith("p:"))).toHaveLength(DEMO.devices.length);
  });

  it("no two nodes overlap (min gap 12px between borders)", () => {
    for (let a = 0; a < centers.length; a++) {
      for (let b = a + 1; b < centers.length; b++) {
        const d = Math.hypot(centers[a].x - centers[b].x, centers[a].y - centers[b].y);
        expect(d, `${centers[a].key} vs ${centers[b].key}`)
          .toBeGreaterThanOrEqual(centers[a].r + centers[b].r + 12);
      }
    }
  });

  it("the hub sits in the middle of its districts", () => {
    const hub = centers.find((c) => c.key === `s:${DEMO.hub}`);
    const spaces = centers.filter((c) => c.key.startsWith("s:") && c.key !== hub.key);
    const mx = spaces.reduce((a, c) => a + c.x, 0) / spaces.length;
    const my = spaces.reduce((a, c) => a + c.y, 0) / spaces.length;
    expect(Math.hypot(hub.x - mx, hub.y - my)).toBeLessThan(80);
  });

  it("devices sit farther from the hub than the space they feed (fanned outward)", () => {
    const hub = centers.find((c) => c.key === `s:${DEMO.hub}`);
    for (const p of pipes.filter((p) => p.kind === "source" && p.into !== DEMO.hub)) {
      const dev = centers.find((c) => c.key === `p:${p.id}`);
      const sp = centers.find((c) => c.key === `s:${p.into}`);
      const dDev = Math.hypot(dev.x - hub.x, dev.y - hub.y);
      const dSp = Math.hypot(sp.x - hub.x, sp.y - hub.y);
      expect(dDev, `${p.id} beyond ${p.into}`).toBeGreaterThan(dSp);
    }
  });
});

describe("template round-trip", () => {
  const state = {
    mode: "test",
    spaces: [{ name: "default" }, { name: "smart-city" }, { name: "harbor" }],
    fedView: new Set(["smart-city"]),
    links: new Map([["smart-city", [{ id: "urn:r:1", to: "harbor", type: "ParkingSpot" }]]]),
    pipes: [
      { id: "a1", kind: "source", gen: "🌡 TemperatureSensor", type: "TemperatureSensor", into: "harbor", secs: 3, running: true, ticks: 9 },
      { id: "a2", kind: "sync", from: "harbor", into: "smart-city", type: "ParkingSpot", secs: 5, running: false, ticks: 2 },
    ],
    ents: new Map([["harbor", { local: [{ id: "x" }], remote: [] }]]),
    selected: "smart-city",
  };

  it("build → normalize preserves the structure", () => {
    const tpl = buildTemplate(state);
    expect(tpl.csrs).toEqual([
      { kind: "federation", protocol: "CSR", from: "smart-city", peer: "harbor", type: "ParkingSpot" },
    ]);
    const norm = normalizeTemplate(tpl);
    expect(norm.spaces).toEqual(["default", "smart-city", "harbor"]);
    expect(norm.fedView).toEqual(["smart-city"]);
    expect(norm.csrs).toEqual([{ from: "smart-city", to: "harbor", type: "ParkingSpot" }]);
    expect(norm.pipes).toHaveLength(2);
    expect(norm.pipes[0]).toMatchObject({ kind: "source", type: "TemperatureSensor", into: "harbor", secs: 3, running: true });
    expect(norm.pipes[1]).toMatchObject({ kind: "sync", from: "harbor", into: "smart-city", running: false });
  });

  it("normalize drops malformed rows instead of failing", () => {
    const norm = normalizeTemplate({
      contextSpaces: [{ name: "ok-space" }, { name: "bad name!" }, {}],
      csrs: [
        { from: "ok-space", peer: "ghost" }, // unknown peer → dropped
        { from: "ok-space", peer: "ok-space" }, // self-link → dropped
      ],
      pipelines: [
        { kind: "device", type: "NotAType", into: "ok-space" }, // unknown type
        { kind: "copy", from: "ghost", into: "ok-space", type: "Room" }, // unknown from
        { kind: "device", type: "Room", into: "ok-space", secs: 0 }, // secs clamped
      ],
    });
    expect(norm.spaces).toEqual(["ok-space"]);
    expect(norm.csrs).toEqual([]);
    expect(norm.pipes).toHaveLength(1);
    expect(norm.pipes[0].secs).toBeGreaterThanOrEqual(1);
    expect(norm.pipes[0].gen).toContain("Room");
  });

  it("the full DEMO survives a template round-trip", () => {
    const links = new Map([[DEMO.hub, DEMO.csrs.map(([, to, type], i) => ({ id: `urn:r:${i}`, to, type: type ?? undefined }))]]);
    const pipes = [
      ...DEMO.devices.map(([into, type, secs], i) => ({ id: `d${i}`, kind: "source", gen: `x ${type}`, type, into, secs, running: true })),
      ...DEMO.copies.map(([from, into, type, secs], i) => ({ id: `c${i}`, kind: "sync", from, into, type, secs, running: true })),
    ];
    const tpl = buildTemplate({
      mode: "t", spaces: DEMO.spaces.map((name) => ({ name })),
      fedView: new Set([DEMO.hub]), links, pipes, ents: new Map(), selected: DEMO.hub,
    });
    const norm = normalizeTemplate(tpl);
    expect(norm.csrs).toHaveLength(DEMO.csrs.length);
    expect(norm.pipes).toHaveLength(DEMO.devices.length + DEMO.copies.length);
  });
});
