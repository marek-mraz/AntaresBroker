// The write shapes together, with reads, over a loaded tenant that carries
// its subscriptions: what the broker sustains when nothing runs alone.
//
// Per iteration one operation is drawn from MIX (weights):
//   update    PATCH /entities/{id}/attrs        a loaded entity
//   replace   PUT   /entities/{id}              a loaded entity
//   get       GET   /entities/{id}              a loaded entity
//   upsert20  POST  /entityOperations/upsert    the client's own churn block
//   delete20  POST  /entityOperations/delete    the same block
//
// The churn block sits above the loaded id range, so a delete never removes
// a loaded entity and the phases that follow still see the dataset they
// were given. It carries a loaded type, so the tenant's subscriptions match
// it exactly as they match the rest.
//
//   k6 run -e TENANT=t7 -e ENTITIES=1000000 -e VUS=50 dev/perf/k6-mix.js
//
// Env: BROKER_URL, TENANT, ENTITIES (the loader's per-run entity count),
//      VUS (50), DURATION (60s), MIX ("update:4,replace:2,get:2,upsert20:1,delete20:1").

import http from "k6/http";
import { Counter, Trend } from "k6/metrics";

const BASE = `${__ENV.BROKER_URL || "http://127.0.0.1:9090"}/ngsi-ld/v1`;
const TENANT = __ENV.TENANT || "t0";
const ENTITIES = Number(__ENV.ENTITIES || 1000);
const BLOCK = 20;
const H = { "Content-Type": "application/json", "NGSILD-Tenant": TENANT };
const TYPES = ["Vehicle", "Building", "Sensor"];

const ops = {};
for (const part of (__ENV.MIX || "update:4,replace:2,get:2,upsert20:1,delete20:1").split(",")) {
  const [name, w] = part.split(":");
  ops[name.trim()] = Number(w);
}
const wheel = [];
for (const [name, w] of Object.entries(ops)) for (let i = 0; i < w; i++) wheel.push(name);

const done = {};
const took = {};
for (const name of Object.keys(ops)) {
  done[name] = new Counter(`op_${name}`);
  took[name] = new Trend(`dur_${name}`, true);
}
const errors = new Counter("op_errors");

export const options = {
  scenarios: {
    mix: {
      executor: "constant-vus",
      vus: Number(__ENV.VUS || 50),
      duration: __ENV.DURATION || "60s",
    },
  },
  summaryTrendStats: ["med", "p(99)", "max"],
};

// The loader's own id rule: three types round-robin by entity number.
const loadedId = (n) => `urn:ngsi-ld:${TYPES[n % 3]}:${TENANT}:${n}`;
const churnId = (n) => `urn:ngsi-ld:Vehicle:${TENANT}:${n}`;

function churnEntity(n) {
  return {
    id: churnId(n),
    type: "Vehicle",
    brand: { type: "Property", value: "Skoda" },
    speed: { type: "Property", value: n % 130 },
    location: { type: "GeoProperty", value: { type: "Point", coordinates: [19.1, 48.7] } },
  };
}

export default function () {
  const op = wheel[Math.floor(Math.random() * wheel.length)];
  const n = Math.floor(Math.random() * ENTITIES);
  const id = loadedId(n);
  const base = ENTITIES + (__VU - 1) * BLOCK;
  const v = (__ITER % 130) + 1;
  let r, ok;

  switch (op) {
    case "replace": {
      const e = { id, type: TYPES[n % 3], speed: { type: "Property", value: v } };
      r = http.put(`${BASE}/entities/${id}`, JSON.stringify(e), { headers: H, tags: { op } });
      ok = r.status === 204;
      break;
    }
    case "get":
      r = http.get(`${BASE}/entities/${id}`, { headers: H, tags: { op } });
      ok = r.status === 200 || r.status === 404;
      break;
    case "upsert20": {
      const batch = [];
      for (let i = 0; i < BLOCK; i++) batch.push(churnEntity(base + i));
      r = http.post(`${BASE}/entityOperations/upsert`, JSON.stringify(batch), { headers: H, tags: { op } });
      ok = r.status === 201 || r.status === 204;
      break;
    }
    case "delete20": {
      const ids = [];
      for (let i = 0; i < BLOCK; i++) ids.push(churnId(base + i));
      r = http.post(`${BASE}/entityOperations/delete`, JSON.stringify(ids), { headers: H, tags: { op } });
      // 207 when the block was already gone: the draw is random, a delete
      // can precede its upsert.
      ok = r.status === 204 || r.status === 207;
      break;
    }
    default:
      r = http.patch(`${BASE}/entities/${id}/attrs`, JSON.stringify({ speed: { type: "Property", value: v } }), {
        headers: H,
        tags: { op: "update" },
      });
      ok = r.status === 204;
  }
  took[op].add(r.timings.duration);
  if (ok) done[op].add(1);
  else errors.add(1);
}
