// Pure data + pure logic. NO I/O in this module — everything here is
// unit-testable without a broker or a DOM.

export const CORE_CTX =
  "https://uri.etsi.org/ngsi-ld/v1/ngsi-ld-core-context-v1.8.jsonld";
// Mirror of public/loopback.js — the virtual host that routes a CSR's
// endpoint back into the same in-browser broker (CIM 009 5.2.9 pattern).
export const LOOPBACK = "http://self.antares.internal";

// Tenant charset: A-Za-z0-9 and dash, max 64 — a strict subset of the
// broker's TenantId rule so UI and API can never disagree.
export const TENANT_RE = /^[A-Za-z0-9-]{1,64}$/;

export const TYPES = {
  Room: { emoji: "🚪", attr: "temperature", gen: () => Math.round(15 + Math.random() * 15) },
  TemperatureSensor: { emoji: "🌡", attr: "temperature", gen: (t) => Math.round(180 + 60 * Math.sin(t / 20e3) + 20 * Math.random()) / 10 },
  ParkingSpot: { emoji: "🚗", attr: "occupied", gen: () => (Math.random() < 0.5 ? 0 : 1) },
  Streetlight: { emoji: "💡", attr: "powerDraw", gen: () => Math.round(Math.random() * 60) },
  AirQualitySensor: { emoji: "🌫", attr: "pm25", gen: (t) => Math.round(250 + 180 * Math.sin(t / 45e3) + 70 * Math.random()) / 10 },
  NoiseSensor: { emoji: "🔊", attr: "decibels", gen: () => Math.round(35 + Math.random() * 50) },
  WaterLevelSensor: { emoji: "🌊", attr: "level", gen: (t) => Math.round(220 + 160 * Math.sin(t / 60e3) + 20 * Math.random()) / 100 },
  EnergyMeter: { emoji: "⚡", attr: "consumption", gen: (t) => Math.round(300 + 220 * Math.sin(t / 30e3) + 90 * Math.random()) / 10 },
  TrafficCounter: { emoji: "🚦", attr: "vehiclesPerMin", gen: () => Math.round(Math.random() * 45) },
  BikeStation: { emoji: "🚲", attr: "availableBikes", gen: () => Math.round(Math.random() * 20) },
  WasteContainer: { emoji: "🗑", attr: "fillLevel", gen: () => Math.round(Math.random() * 100) },
  CitizenProposal: { emoji: "🗳", attr: "supports", gen: () => Math.round(Math.random() * 500) },
};
export const ALL_TYPES = Object.keys(TYPES).join(",");
export const GENERATORS = Object.fromEntries(
  Object.keys(TYPES).map((t) => [`${TYPES[t].emoji} ${t}`, { type: t }]),
);

export const SEED_SPACES = ["default", "smart-city", "old-town", "harbor",
  "airport", "university", "energy-grid", "transit"];

// The demo city. Invariant (unit-tested): EVERY device's data reaches the
// smart-city hub — through an all-type CSR, a type-scoped CSR, or a copy
// pipeline chain that ends at a space the hub federates.
export const DEMO = {
  hub: "smart-city",
  spaces: ["default", "smart-city", "old-town", "harbor", "airport",
    "university", "energy-grid", "transit", "decidim",
    "old-town-market", "old-town-castle"],
  // spaces with fed view ON from the start: the hub, plus old-town — a NESTED
  // federator that reads its own sub-spaces through its own CSRs
  fedView: ["smart-city", "old-town"],
  layout: {
    "smart-city": [0.5, 0.45], "old-town": [0.3, 0.2], harbor: [0.72, 0.18],
    airport: [0.88, 0.45], university: [0.12, 0.48], "energy-grid": [0.28, 0.78],
    transit: [0.72, 0.8], decidim: [0.1, 0.16], default: [0.52, 0.08],
    "old-town-market": [0.22, 0.02], "old-town-castle": [0.44, 0.02],
  },
  devices: [
    ["old-town", "TemperatureSensor", 3], ["old-town", "NoiseSensor", 4],
    ["harbor", "WaterLevelSensor", 4], ["harbor", "ParkingSpot", 3],
    ["airport", "AirQualitySensor", 3], ["airport", "TrafficCounter", 2],
    ["university", "Room", 5], ["university", "EnergyMeter", 3],
    ["energy-grid", "EnergyMeter", 2], ["energy-grid", "Streetlight", 4],
    ["transit", "BikeStation", 3], ["transit", "TrafficCounter", 3],
    ["decidim", "CitizenProposal", 5],
    ["old-town-market", "WasteContainer", 4], ["old-town-castle", "Streetlight", 5],
  ],
  csrs: [
    ["smart-city", "old-town", null],
    // nested federation: old-town itself federates its sub-spaces —
    // type-scoped, so the nested CSRs only carry the sub-space's own data
    // and never attract unrelated write forwarding down the chain
    ["old-town", "old-town-market", "WasteContainer"],
    ["old-town", "old-town-castle", "Streetlight"],
    ["smart-city", "harbor", null],
    ["smart-city", "university", null],
    ["smart-city", "energy-grid", null],
    ["smart-city", "decidim", null],
    ["smart-city", "airport", "AirQualitySensor"],
    ["smart-city", "transit", "BikeStation"],
  ],
  copies: [
    ["airport", "smart-city", "TrafficCounter", 6],
    ["transit", "smart-city", "TrafficCounter", 7],
    ["university", "energy-grid", "EnergyMeter", 8],
  ],
};

const PALETTE = ["#6d5ef1", "#19a974", "#e8850c", "#d9534f", "#3b82f6",
  "#b45fd9", "#0ea5a3", "#b8860b"];
const EMOJI = ["🏙", "🏔", "🛰", "🏭", "🌊", "🌳", "🎡", "🚉", "🗳"];
const hash = (name) => [...name].reduce((a, c) => a + c.charCodeAt(0), 0);
export const colorOf = (name) => PALETTE[hash(name) % PALETTE.length];
export const avatarOf = (name) =>
  name === "default" ? "⭐" : name === "decidim" ? "🗳" : EMOJI[hash(name) % EMOJI.length];

export function entLabel(doc) {
  const type = Array.isArray(doc.type) ? doc.type[0] : doc.type;
  const t = TYPES[type] ?? { emoji: "▪", attr: null };
  return { type, emoji: t.emoji, attr: t.attr, value: t.attr ? doc[t.attr]?.value : undefined };
}

// ---- deterministic board layout ---------------------------------------------
// Hub-and-spoke, collision-free by construction (unit-tested): the hub sits
// center, every other space on an ellipse around it, and each space's
// devices fan OUTWARD on the far side from the hub so device edges never
// cross the middle. Positions are React-Flow top-left anchored.
export function arrangeBoard(spaceNames, pipes, { hub = "smart-city", satellites = new Map(), W = 1400, H = 880 } = {}) {
  const pos = {};
  const cx = W / 2, cy = H / 2 + 20;
  const SP = 55, DVX = 48, DVY = 22; // half sizes: space 110px circle, device 96×44 chip
  // satellites (sub-spaces federated by a NON-hub space) attach to their
  // parent instead of taking a ring slot
  const others = spaceNames.filter((n) => n !== hub && !satellites.has(n));
  const center = new Map(); // space -> center point
  if (spaceNames.includes(hub)) center.set(hub, { x: cx, y: cy });
  const n = others.length || 1;
  others.forEach((name, i) => {
    const ang = -Math.PI / 2 + (i * 2 * Math.PI) / n;
    center.set(name, { x: cx + 460 * Math.cos(ang), y: cy + 310 * Math.sin(ang) });
  });
  const byParent = new Map();
  for (const [child, parent] of satellites) {
    if (!spaceNames.includes(child)) continue;
    if (!byParent.has(parent)) byParent.set(parent, []);
    byParent.get(parent).push(child);
  }
  for (const [parent, kids] of byParent) {
    const c = center.get(parent);
    if (!c) continue;
    let ux = c.x - cx, uy = c.y - cy;
    const d = Math.hypot(ux, uy) || 1;
    ux /= d; uy /= d;
    const px = -uy, py = ux;
    kids.forEach((k, i) => {
      const off = (i - (kids.length - 1) / 2) * 185;
      center.set(k, { x: c.x + ux * 250 + px * off, y: c.y + uy * 250 + py * off });
    });
  }
  for (const [name, c] of center) pos[`s:${name}`] = { x: c.x - SP, y: c.y - SP };

  const bySpace = new Map();
  for (const p of pipes) {
    if (p.kind !== "source") continue;
    if (!bySpace.has(p.into)) bySpace.set(p.into, []);
    bySpace.get(p.into).push(p);
  }
  for (const [space, list] of bySpace) {
    const c = center.get(space);
    if (!c) continue;
    let ux = c.x - cx, uy = c.y - cy;
    const d = Math.hypot(ux, uy);
    if (d < 1) { ux = 0; uy = 1; } else { ux /= d; uy /= d; }
    let px = -uy, py = ux;
    // a satellite-parent's outward corridor is occupied by its sub-space
    // edges — its devices swing 90° to the perpendicular side instead
    if (byParent.has(space)) {
      ux = px; uy = py;
      px = -uy; py = ux;
    }
    list.forEach((p, i) => {
      const off = (i - (list.length - 1) / 2) * 122;
      pos[`p:${p.id}`] = {
        x: c.x + ux * 175 + px * off - DVX,
        y: c.y + uy * 175 + py * off - DVY,
      };
    });
  }
  return pos;
}

// ---- the board template: structure as JSON ---------------------------------
export function buildTemplate({ mode, spaces, fedView, links, pipes, ents, selected }) {
  return {
    app: "antares-playground-board",
    version: 1,
    mode,
    contextSpaces: spaces.map((s) => ({
      name: s.name,
      icon: avatarOf(s.name),
      local: ents.get(s.name)?.local.length ?? 0,
      ...(fedView.has(s.name)
        ? { fedView: true, federated: ents.get(s.name)?.remote.length ?? 0 }
        : {}),
    })),
    csrs: [...links].flatMap(([from, ls]) =>
      ls.map((l) => ({
        kind: "federation", protocol: "CSR", from, peer: l.to,
        type: l.type && TYPES[l.type] ? l.type : "all",
      }))),
    pipelines: pipes.map((p) =>
      p.kind === "source"
        ? { kind: "device", gen: p.gen, type: p.type, into: p.into,
            secs: p.secs, running: p.running, ticks: p.ticks ?? 0 }
        : { kind: "copy", from: p.from, into: p.into, type: p.type,
            secs: p.secs, running: p.running, ticks: p.ticks ?? 0 }),
    selected,
  };
}

// The pure half of "apply template": validation + normalization. The caller
// executes the result against the broker. Malformed rows drop silently —
// a template is a wish list, not a schema fight.
export function normalizeTemplate(tpl) {
  const spaces = [];
  const fedView = [];
  for (const s of tpl?.contextSpaces ?? []) {
    const name = s.name ?? s.id;
    if (!TENANT_RE.test(name ?? "")) continue;
    spaces.push(name);
    if (s.fedView) fedView.push(name);
  }
  const known = new Set(spaces);
  const csrs = [];
  for (const c of tpl?.csrs ?? []) {
    const to = c.peer ?? c.to;
    if (!known.has(c.from) || !known.has(to) || c.from === to) continue;
    csrs.push({ from: c.from, to, type: c.type && TYPES[c.type] ? c.type : null });
  }
  const pipes = [];
  for (const t of tpl?.pipelines ?? []) {
    const kind = t.kind === "device" || t.kind === "source" ? "source" : "sync";
    if (!TYPES[t.type] || !known.has(t.into)) continue;
    if (kind === "sync" && (!known.has(t.from) || t.from === t.into)) continue;
    pipes.push({
      kind, into: t.into, type: t.type,
      secs: Math.max(1, Number(t.secs) || 3),
      running: t.running !== false,
      ...(kind === "source"
        ? { gen: t.gen ?? `${TYPES[t.type].emoji} ${t.type}` }
        : { from: t.from }),
    });
  }
  return { spaces, fedView, csrs, pipes };
}
