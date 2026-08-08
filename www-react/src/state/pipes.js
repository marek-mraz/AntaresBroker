// Pipeline timers. A tick counts ONLY when the broker accepted the write —
// failures land in the request log (red rows), never in the counter.
import { batchUpsert, createEntity, listEntities, patchAttr } from "../broker/api.js";
import { TYPES } from "../model.js";
import { board, burst, emit, refreshSpace, save } from "./board.js";

const timers = new Map();

export function startPipe(p) {
  stopTimer(p.id);
  timers.set(p.id, setInterval(() => tickPipe(p).catch(() => {}), p.secs * 1000));
}
export const stopTimer = (id) => {
  clearInterval(timers.get(id));
  timers.delete(id);
};

export function startAllPipes() {
  for (const p of board.pipes) if (p.running) startPipe(p);
}

export function togglePipe(id) {
  const p = board.pipes.find((x) => x.id === id);
  if (!p) return;
  p.running = !p.running;
  p.running ? startPipe(p) : stopTimer(id);
  save("antares.pipes", board.pipes);
  emit();
}

export function deletePipe(id) {
  stopTimer(id);
  board.pipes = board.pipes.filter((x) => x.id !== id);
  delete board.positions[`p:${id}`];
  save("antares.pipes", board.pipes);
  save("antares.pos", board.positions);
  emit();
}

export function addPipe(spec) {
  const p = { id: crypto.randomUUID().slice(0, 8), running: true, ticks: 0, ...spec };
  board.pipes.push(p);
  save("antares.pipes", board.pipes);
  startPipe(p);
  emit();
  return p;
}

async function tickPipe(p) {
  let moved = 1;
  if (p.kind === "source") {
    // One stable entity per simulated device, fresh reading per tick.
    const t = TYPES[p.type];
    const id = `urn:ngsi-ld:${p.type}:pipe-${p.id}`;
    const patch = await patchAttr(p.into, id, t.attr, {
      type: "Property",
      value: t.gen(Date.now()),
    });
    if (patch.status === 404) {
      const post = await createEntity(p.into, {
        id,
        type: p.type,
        [t.attr]: { type: "Property", value: t.gen(Date.now()) },
      });
      if (post.status !== 201) return; // rejected — no tick, log has the row
    } else if (patch.status !== 204) {
      return;
    }
  } else {
    const list = await listEntities(p.from, { local: true, type: p.type });
    if (!list.length) return; // nothing moved — no burst, no tick
    const up = await batchUpsert(p.into, list);
    if (!up.ok && up.status !== 207) return;
    moved = list.length;
  }
  p.ticks = (p.ticks ?? 0) + 1;
  save("antares.pipes", board.pipes);
  burst(`pipe:${p.id}`, moved);
  await refreshSpace(p.into);
  emit();
}
