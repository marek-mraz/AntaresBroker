// Baseline load scenario for the weekly performance run.
//
// Open model on purpose: constant-arrival-rate keeps issuing requests at
// the intended rate regardless of how slowly the broker answers, so tail
// latency is not silently flattened by coordinated omission (a closed
// constant-vus model would wait for each response before sending the next
// request, hiding exactly the latencies this run exists to see).
//
// One scenario per concern; grow this file one scenario at a time — a
// changed scenario invalidates every number recorded against it.
//
//   k6 run --summary-export summary.json dev/perf/k6-baseline.js
//
// Env: BROKER_URL (default http://localhost:9090), RATE (rps, default 50),
//      DURATION (default 2m).

import http from "k6/http";
import { check } from "k6";

const BASE = `${__ENV.BROKER_URL || "http://localhost:9090"}/ngsi-ld/v1`;
const RATE = Number(__ENV.RATE || 50);
const DURATION = __ENV.DURATION || "2m";

export const options = {
  scenarios: {
    create_entities: {
      executor: "constant-arrival-rate",
      exec: "createEntity",
      rate: RATE,
      timeUnit: "1s",
      duration: DURATION,
      preAllocatedVUs: 50,
      maxVUs: 500,
    },
    query_entities: {
      executor: "constant-arrival-rate",
      exec: "queryEntities",
      rate: RATE,
      timeUnit: "1s",
      duration: DURATION,
      preAllocatedVUs: 50,
      maxVUs: 500,
    },
  },
  // Report-only: no thresholds until the noise profile from repeated
  // same-commit runs says what a real regression looks like.
  summaryTrendStats: ["min", "med", "p(90)", "p(95)", "p(99)", "max"],
};

const HEADERS = { "Content-Type": "application/json" };

export function createEntity() {
  const id = `urn:ngsi-ld:PerfEntity:${__VU}-${__ITER}`;
  const body = JSON.stringify({
    id,
    type: "PerfEntity",
    speed: { type: "Property", value: (__ITER * 7) % 130 },
    location: {
      type: "GeoProperty",
      value: { type: "Point", coordinates: [17.1, 48.1] },
    },
  });
  const res = http.post(`${BASE}/entities`, body, { headers: HEADERS });
  check(res, { "create 201": (r) => r.status === 201 });
}

export function queryEntities() {
  const res = http.get(
    `${BASE}/entities?type=PerfEntity&q=speed%3E50&limit=20`,
    { headers: { Accept: "application/json" } },
  );
  check(res, { "query 200": (r) => r.status === 200 });
}
