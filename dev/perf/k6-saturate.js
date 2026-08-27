// Saturation sweep: open model, arrival rate stepped up until the broker
// stops holding it. Each stage adds STEP rps for STAGE seconds; the
// per-stage p99 and error rate are exported as tagged trends so
// saturate.sh can find the knee (the last stage that held).
//
//   k6 run -e SHAPE=query -e STEP=500 -e STAGES=20 dev/perf/k6-saturate.js
//
// Env: BROKER_URL, SHAPE (query|write), STEP (500), STAGES (20), STAGE ("30s"),
//      TENANT.

import http from "k6/http";
import { Trend, Rate } from "k6/metrics";

const BASE = `${__ENV.BROKER_URL || "http://localhost:9090"}/ngsi-ld/v1`;
const SHAPE = __ENV.SHAPE || "query";
const STEP = Number(__ENV.STEP || 500);
const STAGES = Number(__ENV.STAGES || 20);
const STAGE = __ENV.STAGE || "30s";
const N = 100;
const headers = __ENV.TENANT ? { "NGSILD-Tenant": __ENV.TENANT } : {};

const stages = [];
for (let i = 1; i <= STAGES; i++) stages.push({ target: i * STEP, duration: STAGE });

export const options = {
  scenarios: {
    sweep: {
      executor: "ramping-arrival-rate",
      startRate: STEP,
      timeUnit: "1s",
      preAllocatedVUs: 200,
      maxVUs: 4000,
      stages,
    },
  },
  summaryTrendStats: ["med", "p(99)"],
};

const latency = new Trend("stage_latency", true);
const errors = new Rate("stage_errors");
const t0 = Date.now();
const stageMs = parseInt(STAGE) * (STAGE.endsWith("m") ? 60000 : 1000);

export function setup() {
  for (let n = 0; n < N; n++) {
    const body = JSON.stringify({
      id: `urn:ngsi-ld:Vehicle:sat:${n}`, type: "Vehicle",
      brand: { type: "Property", value: "Skoda" }, speed: { type: "Property", value: n % 130 },
      mileage: { type: "Property", value: n * 1000 }, colour: { type: "Property", value: "red" },
      location: { type: "GeoProperty", value: { type: "Point", coordinates: [19.1, 48.7] } },
    });
    const r = http.post(`${BASE}/entities`, body, { headers: { "Content-Type": "application/json", ...headers } });
    if (r.status !== 201 && r.status !== 409) throw new Error(`seed ${n}: ${r.status}`);
  }
}

export default function () {
  const stage = Math.min(STAGES, 1 + Math.floor((Date.now() - t0) / stageMs));
  const tags = { stage: String(stage), rate: String(stage * STEP) };
  let r;
  if (SHAPE === "write") {
    const body = JSON.stringify({
      id: `urn:ngsi-ld:Vehicle:satw:${__VU}:${__ITER}`, type: "Vehicle",
      speed: { type: "Property", value: __ITER % 130 },
    });
    r = http.post(`${BASE}/entities`, body, { headers: { "Content-Type": "application/json", ...headers }, tags });
    latency.add(r.timings.duration, tags);
    errors.add(r.status !== 201, tags);
  } else {
    r = http.get(`${BASE}/entities?type=Vehicle&limit=20`, { headers, tags });
    latency.add(r.timings.duration, tags);
    errors.add(r.status !== 200, tags);
  }
}
