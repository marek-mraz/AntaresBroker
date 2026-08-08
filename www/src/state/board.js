// The one store. Plain module object + subscribe/emit — components read it
// through useSyncExternalStore. Persistence keys are shared with www/ so
// both UIs show the same board when served from the same origin.
import * as apiMod from "../broker/api.js";
import { uuid } from "../uuid.js";
import { DEMO, SEED_SPACES, TENANT_RE, TYPES, arrangeBoard, buildTemplate, normalizeTemplate } from "../model.js";
import { transport } from "../broker/transport.js";

export const load = (k, d) => JSON.parse(localStorage.getItem(k) ?? "null") ?? d;
export const save = (k, v) => localStorage.setItem(k, JSON.stringify(v));

const listeners = new Set();
let version = 0;
export const board = {
  spaces: load("antares.spaces", SEED_SPACES.map((name) => ({ name }))),
  pipes: load("antares.pipes", []),
  fedView: new Set(load("antares.fedview", [])),
  positions: load("antares.pos", {}),
  links: new Map(), // space -> [{id, to, type}]
  ents: new Map(), // space -> {local: [], remote: []}
  bursts: new Map(), // edgeKey -> {until, count} — evidence of real flow
  toasts: [],
};
if (!board.spaces.some((s) => s.name === "default")) board.spaces.unshift({ name: "default" });

export const subscribe = (fn) => (listeners.add(fn), () => listeners.delete(fn));
export const getVersion = () => version;
export function emit() {
  version++;
  for (const fn of listeners) fn();
}

// ---- evidence bursts --------------------------------------------------------
export function burst(edgeKey, count = 0, reader = null) {
  board.bursts.set(edgeKey, { until: Date.now() + 1600, count, reader });
  emit();
  setTimeout(() => {
    const cur = board.bursts.get(edgeKey);
    if (cur && cur.until <= Date.now()) {
      board.bursts.delete(edgeKey);
      emit();
    }
  }, 1700);
}

// ---- polling ---------------------------------------------------------------
// Which space actually OWNS a federated entity (T0 heuristic — replaced by
// /entityMaps when the broker grows it; see README rule 2).
export function originOf(id, viewer) {
  for (const l of board.links.get(viewer) ?? []) {
    if ((board.ents.get(l.to)?.local ?? []).some((e) => e.id === id)) return l.to;
  }
  for (const [space, cur] of board.ents) {
    if (space !== viewer && cur.local.some((e) => e.id === id)) return space;
  }
  return null;
}

async function refreshLinks(space) {
  const regs = await apiMod.listCsrs(space);
  board.links.set(
    space,
    (regs ?? [])
      .filter((reg) => (reg.endpoint ?? "").startsWith("http://self.antares.internal"))
      .map((reg) => {
        const es = reg.information?.[0]?.entities ?? [];
        return {
          id: reg.id,
          to: reg.tenant ?? "default",
          type: es.length === 1 ? es[0].type : undefined,
        };
      }),
  );
}

export async function refreshSpace(space) {
  const local = await apiMod.listEntities(space, { local: true });
  let remote = [];
  if (board.fedView.has(space)) {
    const fed = await apiMod.listEntities(space, { local: false });
    const have = new Set(local.map((e) => e.id));
    remote = (fed ?? []).filter((e) => !have.has(e.id));
  }
  board.ents.set(space, { local: local ?? [], remote });
  await refreshLinks(space);
  // Real flow only: pulse exactly the CSR edges that carried entities into
  // this federated query — the WHOLE chain for nested hops (smart-city →
  // old-town → old-town-market), and only along links whose type scope
  // covers the data that actually crossed. The reader tag lets the board
  // animate a chain only while ITS reader is the focused space.
  if (remote.length) {
    const perOriginType = new Map(); // `${origin}|${type}` -> count
    for (const e of remote) {
      const o = originOf(e.id, space);
      if (o) {
        const k = `${o}|${e.type}`;
        perOriginType.set(k, (perOriginType.get(k) ?? 0) + 1);
      }
    }
    // BFS over type-covering links: fed edge ids along space→…→origin.
    const pathTo = (from, to, type) => {
      const prev = new Map([[from, null]]);
      const q = [from];
      while (q.length) {
        const cur = q.shift();
        for (const l of board.links.get(cur) ?? []) {
          if (prev.has(l.to) || (l.type && l.type !== type)) continue;
          prev.set(l.to, { space: cur, link: l });
          if (l.to === to) {
            const ids = [];
            let at = to;
            while (prev.get(at)) {
              ids.push(prev.get(at).link.id);
              at = prev.get(at).space;
            }
            return ids;
          }
          q.push(l.to);
        }
      }
      return null;
    };
    for (const [k, n] of perOriginType) {
      const [origin, type] = k.split("|");
      for (const id of pathTo(space, origin, type) ?? []) burst(`fed:${id}`, n, space);
    }
  }
}

let pollTimer = null;
export async function refreshAll() {
  await Promise.all(board.spaces.map((s) => refreshSpace(s.name)));
  emit();
}
export function startPolling(ms = 3000) {
  clearInterval(pollTimer);
  pollTimer = setInterval(() => refreshAll().catch(() => {}), ms);
}

// ---- spaces ----------------------------------------------------------------
export function addSpace(name) {
  if (!TENANT_RE.test(name)) return false;
  if (!board.spaces.some((s) => s.name === name)) {
    board.spaces.push({ name });
    save("antares.spaces", board.spaces);
    emit();
    refreshSpace(name).then(emit);
  }
  return true;
}

export async function removeSpace(name, { deletePipe }) {
  if (name === "default") return;
  for (const p of [...board.pipes]) {
    if (p.into === name || p.from === name) deletePipe(p.id);
  }
  await apiMod.cleanupSpace(name);
  board.spaces = board.spaces.filter((s) => s.name !== name);
  save("antares.spaces", board.spaces);
  board.links.delete(name);
  board.ents.delete(name);
  delete board.positions[`s:${name}`];
  save("antares.pos", board.positions);
  await refreshAll();
}

export function setFedView(space, on) {
  on ? board.fedView.add(space) : board.fedView.delete(space);
  save("antares.fedview", [...board.fedView]);
  refreshSpace(space).then(emit);
  emit();
}

export function toast(text) {
  board.toasts.push({ id: uuid(), text });
  emit();
  setTimeout(() => {
    board.toasts.shift();
    emit();
  }, 4000);
}

// ---- template + demo + reset -------------------------------------------------
export function boardTemplate(selected) {
  return buildTemplate({
    mode: transport.mode,
    spaces: board.spaces,
    fedView: board.fedView,
    links: board.links,
    pipes: board.pipes,
    ents: board.ents,
    selected,
  });
}

export async function applyBoardTemplate(tpl, { startPipe }) {
  const norm = normalizeTemplate(tpl);
  for (const name of norm.spaces) {
    if (!board.spaces.some((s) => s.name === name)) board.spaces.push({ name });
  }
  for (const name of norm.fedView) board.fedView.add(name);
  save("antares.spaces", board.spaces);
  save("antares.fedview", [...board.fedView]);
  await refreshAll();
  for (const c of norm.csrs) {
    if ((board.links.get(c.from) ?? []).some((l) => l.to === c.to)) continue;
    await apiMod.registerLink(c.from, c.to, c.type, TYPES);
  }
  for (const t of norm.pipes) {
    const dup = t.kind === "source"
      ? board.pipes.some((p) => p.kind === "source" && p.into === t.into && p.type === t.type)
      : board.pipes.some((p) => p.kind === "sync" && p.from === t.from && p.into === t.into && p.type === t.type);
    if (dup) continue;
    const p = { id: uuid().slice(0, 8), ticks: 0, ...t };
    board.pipes.push(p);
    if (p.running) startPipe(p);
  }
  save("antares.pipes", board.pipes);
  await refreshAll();
}

export async function createDemo({ startPipe }) {
  for (const n of DEMO.spaces) {
    if (!board.spaces.some((s) => s.name === n)) board.spaces.push({ name: n });
  }
  save("antares.spaces", board.spaces);
  await refreshAll();
  const linked = (from, to) => (board.links.get(from) ?? []).some((l) => l.to === to);
  for (const [from, to, type] of DEMO.csrs) {
    if (!linked(from, to)) await apiMod.registerLink(from, to, type, TYPES);
  }
  for (const n of DEMO.fedView ?? [DEMO.hub]) board.fedView.add(n);
  save("antares.fedview", [...board.fedView]);
  const subs = await apiMod.listSubscriptions(DEMO.hub);
  if (!(subs ?? []).length) {
    await apiMod.subscribeAll(DEMO.hub, "http://page.local/demo", Object.keys(TYPES));
  }
  for (const [into, type, secs] of DEMO.devices) {
    if (board.pipes.some((p) => p.kind === "source" && p.into === into && p.type === type)) continue;
    const p = { id: uuid().slice(0, 8), kind: "source",
      gen: `${TYPES[type].emoji} ${type}`, type, into, secs, running: true, ticks: 0 };
    board.pipes.push(p);
    startPipe(p);
  }
  for (const [from, into, type, secs] of DEMO.copies) {
    if (board.pipes.some((p) => p.kind === "sync" && p.from === from && p.into === into && p.type === type)) continue;
    const p = { id: uuid().slice(0, 8), kind: "sync",
      from, into, type, secs, running: true, ticks: 0 };
    board.pipes.push(p);
    startPipe(p);
  }
  save("antares.pipes", board.pipes);
  await refreshAll(); // links must be fresh so arrange sees the satellites
  arrangeAll(); // the demo always lands in a clean hub-and-spoke layout
}

// Re-layout the whole board deterministically (also the ✳ arrange button).
export function arrangeAll() {
  // a space federated by a NON-hub space is that parent's satellite
  const satellites = new Map();
  for (const [from, ls] of board.links) {
    if (from === DEMO.hub) continue;
    for (const l of ls) satellites.set(l.to, from);
  }
  const arranged = arrangeBoard(board.spaces.map((s) => s.name), board.pipes, {
    hub: DEMO.hub,
    satellites,
  });
  Object.assign(board.positions, arranged);
  save("antares.pos", board.positions);
  board.layoutEpoch = (board.layoutEpoch ?? 0) + 1; // remounts ReactFlow → fitView
  emit();
}

export async function resetBoard({ deletePipe }) {
  for (const p of [...board.pipes]) deletePipe(p.id);
  for (const s of [...board.spaces]) await apiMod.cleanupSpace(s.name);
  board.spaces = SEED_SPACES.map((name) => ({ name }));
  save("antares.spaces", board.spaces);
  board.links.clear();
  board.ents.clear();
  board.fedView.clear();
  save("antares.fedview", []);
  for (const k of Object.keys(board.positions)) delete board.positions[k];
  save("antares.pos", board.positions);
  await refreshAll();
}
