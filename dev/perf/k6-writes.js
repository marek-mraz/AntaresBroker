// One write shape at a time, no subscription anywhere: what the store
// sustains for each way of changing an entity.
//
//   update    PATCH /entities/{id}/attrs        one attribute of five (5.6.2)
//   partial   PATCH /entities/{id}/attrs/speed  one named attribute (5.6.4)
//   merge     PATCH /entities/{id}              merge fragment (5.6.16)
//   replace   PUT   /entities/{id}              the whole five-attribute entity
//   append    POST  /entities/{id}/attrs        add an attribute (5.6.3)
//   create    POST  /entities                   a fresh id every iteration (5.6.1)
//   upsert20  POST  /entityOperations/upsert    twenty entities per request
//
// The set carries its own entity type, so no subscription of a loaded
// tenant matches it and the numbers are the write path alone. What a write
// costs once subscriptions fire is the mixed workload (k6-mix.js).
// Writes spread over the set: every client on one id measures row
// contention, not throughput.
//
//   k6 run -e SHAPE=update -e VUS=50 -e DURATION=5s dev/perf/k6-writes.js
//
// Env: BROKER_URL, SHAPE, VUS (50), DURATION (5s), TENANT (unset = default).

import http from "k6/http";
import { check } from "k6";

const BASE = `${__ENV.BROKER_URL || "http://127.0.0.1:9471"}/ngsi-ld/v1`;
const SHAPE = __ENV.SHAPE || "update";
const N = 200;
const KIND = "WriteProbe";
const H = { "Content-Type": "application/json", ...(__ENV.TENANT ? { "NGSILD-Tenant": __ENV.TENANT } : {}) };

export const options = {
  scenarios: {
    write: {
      executor: "constant-vus",
      vus: Number(__ENV.VUS || 50),
      duration: __ENV.DURATION || "5s",
    },
  },
  summaryTrendStats: ["med", "p(99)", "max"],
};

function entity(n) {
  return {
    id: `urn:ngsi-ld:${KIND}:${n}`,
    type: KIND,
    brand: { type: "Property", value: "Skoda" },
    speed: { type: "Property", value: n % 130 },
    mileage: { type: "Property", value: n * 1000, unitCode: "KMT" },
    colour: { type: "Property", value: "red" },
    location: { type: "GeoProperty", value: { type: "Point", coordinates: [19.1 + n / 1000, 48.7] } },
  };
}

export function setup() {
  for (let n = 0; n < N; n++) {
    const r = http.post(`${BASE}/entities`, JSON.stringify(entity(n)), { headers: H });
    if (r.status !== 201 && r.status !== 409) throw new Error(`seed ${n}: ${r.status} ${r.body}`);
  }
}

export default function () {
  const n = (__VU * 7 + __ITER) % N;
  const id = `urn:ngsi-ld:${KIND}:${n}`;
  const v = (__ITER % 130) + 1;
  let r;
  let ok = (s) => s === 204;
  switch (SHAPE) {
    case "partial":
      r = http.patch(`${BASE}/entities/${id}/attrs/speed`, JSON.stringify({ type: "Property", value: v }), { headers: H });
      break;
    case "merge":
      r = http.patch(`${BASE}/entities/${id}`, JSON.stringify({ speed: { type: "Property", value: v } }), { headers: H });
      break;
    case "replace":
      r = http.put(`${BASE}/entities/${id}`, JSON.stringify(entity(n)), { headers: H });
      break;
    case "append":
      r = http.post(`${BASE}/entities/${id}/attrs`, JSON.stringify({ tyre: { type: "Property", value: v } }), { headers: H });
      break;
    case "create": {
      const e = entity(n);
      e.id = `urn:ngsi-ld:${KIND}:new:${__VU}-${__ITER}`;
      r = http.post(`${BASE}/entities`, JSON.stringify(e), { headers: H });
      ok = (s) => s === 201;
      break;
    }
    case "upsert20": {
      const batch = [];
      for (let i = 0; i < 20; i++) batch.push(entity((n + i) % N));
      r = http.post(`${BASE}/entityOperations/upsert`, JSON.stringify(batch), { headers: H });
      // 201 when the batch created, 204 when every entity already existed
      ok = (s) => s === 201 || s === 204;
      break;
    }
    default:
      r = http.patch(`${BASE}/entities/${id}/attrs`, JSON.stringify({ speed: { type: "Property", value: v } }), { headers: H });
  }
  check(r, { accepted: (res) => ok(res.status) });
}

export function teardown() {
  for (let n = 0; n < N; n++) http.del(`${BASE}/entities/urn:ngsi-ld:${KIND}:${n}`, null, { headers: H });
}
