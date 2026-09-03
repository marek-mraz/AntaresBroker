// The request shapes other brokers publish, so the numbers compare:
//   query   GET /entities?type=Vehicle&limit=20&local=true   over 100 five-attribute entities
//           (local=true: the store answers alone, no registration fan-out)
//   retrieve GET /entities/{id}
//   update  PATCH /entities/{id}/attrs  (5.6.3, one Property per request),
//           spread over the same 100 entities — the write shape, which is
//           what takes the store's writer lock
//   facade  GET /x/example/things?kind=Vehicle — the reference façade, which
//           answers by driving the NGSI-LD router in process
//   facade-twin  GET /entities?type=Vehicle&options=keyValues — the request
//           that façade makes, so the pair prices the seam's round trip.
//           Both need a binary built with the plugin-example feature.
// Closed model (constant VUs) on purpose here: this measures what the
// broker sustains at a fixed concurrency (c50, c200), which is what the
// published tables of other brokers report. k6-baseline.js keeps the open
// model for tail latency.
//
//   k6 run -e SHAPE=query -e VUS=50 -e DURATION=5s dev/perf/k6-shapes.js
//
// Env: BROKER_URL, SHAPE (query|retrieve|update|facade|facade-twin), VUS (50), DURATION (5s),
//      TENANT (unset = default tenant), SEED (entities are urn:ngsi-ld:Vehicle:shape:<n>).

import http from "k6/http";
import { check } from "k6";

const ROOT = __ENV.BROKER_URL || "http://localhost:9090";
const BASE = `${ROOT}/ngsi-ld/v1`;
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
  if (SHAPE === "facade") {
    r = http.get(`${ROOT}/x/example/things?kind=Vehicle`, { headers });
  } else if (SHAPE === "facade-twin") {
    // exactly what the façade asks the broker, so the difference is the seam
    r = http.get(`${BASE}/entities?type=Vehicle&options=keyValues`, { headers });
  } else if (SHAPE === "retrieve") {
    r = http.get(`${BASE}/entities/urn:ngsi-ld:Vehicle:shape:${__ITER % N}`, { headers });
  } else if (SHAPE === "update") {
    // 5.6.3 Update Attributes: 204 on success. One Property, so the request
    // costs the store a write and almost nothing else — what is being
    // measured is the writer lock, not the payload.
    const body = JSON.stringify({ speed: { type: "Property", value: __ITER % 130 } });
    r = http.patch(`${BASE}/entities/urn:ngsi-ld:Vehicle:shape:${__ITER % N}/attrs`, body, {
      headers: { "Content-Type": "application/json", ...headers },
    });
    check(r, { "204": (res) => res.status === 204 });
    return;
  } else {
    r = http.get(`${BASE}/entities?type=Vehicle&limit=20&local=true`, { headers });
  }
  check(r, { "200": (res) => res.status === 200 });
}

export function teardown() {
  for (let n = 0; n < N; n++) http.del(`${BASE}/entities/urn:ngsi-ld:Vehicle:shape:${n}`, null, { headers });
}
