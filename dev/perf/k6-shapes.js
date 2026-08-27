// The request shapes other brokers publish, so the numbers compare:
//   query   GET /entities?type=Vehicle&limit=20   over 100 five-attribute entities
//   retrieve GET /entities/{id}
// Closed model (constant VUs) on purpose here: this measures what the
// broker sustains at a fixed concurrency (c50, c200), which is what the
// published tables of other brokers report. k6-baseline.js keeps the open
// model for tail latency.
//
//   k6 run -e SHAPE=query -e VUS=50 -e DURATION=5s dev/perf/k6-shapes.js
//
// Env: BROKER_URL, SHAPE (query|retrieve), VUS (50), DURATION (5s),
//      TENANT (unset = default tenant), SEED (entities are urn:ngsi-ld:Vehicle:shape:<n>).

import http from "k6/http";
import { check } from "k6";

const BASE = `${__ENV.BROKER_URL || "http://localhost:9090"}/ngsi-ld/v1`;
const SHAPE = __ENV.SHAPE || "query";
const N = 100;
const headers = __ENV.TENANT ? { "NGSILD-Tenant": __ENV.TENANT } : {};

export const options = {
  scenarios: {
    shape: {
      executor: "constant-vus",
      vus: Number(__ENV.VUS || 50),
      duration: __ENV.DURATION || "5s",
    },
  },
  summaryTrendStats: ["med", "p(99)", "max"],
};

export function setup() {
  const brands = ["Mercedes", "Skoda", "Volvo", "Toyota", "Tesla"];
  for (let n = 0; n < N; n++) {
    const body = JSON.stringify({
      id: `urn:ngsi-ld:Vehicle:shape:${n}`,
      type: "Vehicle",
      brand: { type: "Property", value: brands[n % brands.length] },
      speed: { type: "Property", value: n % 130 },
      mileage: { type: "Property", value: n * 1000, unitCode: "KMT" },
      colour: { type: "Property", value: "red" },
      location: { type: "GeoProperty", value: { type: "Point", coordinates: [19.1 + n / 1000, 48.7] } },
    });
    const r = http.post(`${BASE}/entities`, body, { headers: { "Content-Type": "application/json", ...headers } });
    if (r.status !== 201 && r.status !== 409) throw new Error(`seed ${n}: ${r.status} ${r.body}`);
  }
}

export default function () {
  let r;
  if (SHAPE === "retrieve") {
    r = http.get(`${BASE}/entities/urn:ngsi-ld:Vehicle:shape:${__ITER % N}`, { headers });
  } else {
    r = http.get(`${BASE}/entities?type=Vehicle&limit=20`, { headers });
  }
  check(r, { "200": (res) => res.status === 200 });
}

export function teardown() {
  for (let n = 0; n < N; n++) http.del(`${BASE}/entities/urn:ngsi-ld:Vehicle:shape:${n}`, null, { headers });
}
