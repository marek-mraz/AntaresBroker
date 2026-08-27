// Federated query stream: entity queries on random tenants that the
// broker fans out to the matching Context Source Registrations (the sink
// answers every /csr/<k> with an empty array and counts the calls).
//
//   k6 run -e RATE=200 -e DURATION=30s -e TENANTS=100 dev/perf/k6-fed.js
//
// Five query shapes rotate: by type, type + q, type + geoQ (western
// half), scopeQ, and an idPattern. Registration k of tenant t = k % TENANTS
// has class (k / TENANTS) % 8 (api-load.py CSR_CLASSES) — the registry
// index decides which of them a query reaches, and fed.sh reads how many
// source calls the sink saw per query.
//
// Env: BROKER_URL, RATE (200), DURATION (30s), TENANTS.

import http from "k6/http";
import exec from "k6/execution";
import { Counter } from "k6/metrics";

const BASE = `${__ENV.BROKER_URL || "http://localhost:9090"}/ngsi-ld/v1`;
const RATE = Number(__ENV.RATE || 200);
const TENANTS = Number(__ENV.TENANTS || 10);

export const options = {
  scenarios: {
    fed: {
      executor: "constant-arrival-rate",
      rate: RATE, timeUnit: "1s", duration: __ENV.DURATION || "30s",
      preAllocatedVUs: Math.min(4000, Math.max(50, RATE)), maxVUs: 8000,
    },
  },
  summaryTrendStats: ["med", "p(99)"],
};

const ok = new Counter("queries_ok");
const errors = new Counter("op_errors");
const errConn = new Counter("op_errors_conn");
const err4xx = new Counter("op_errors_4xx");
const err5xx = new Counter("op_errors_5xx");
const warned = new Counter("queries_with_warning"); // 6.3.17: a source failed

const SHAPES = [
  () => "type=Vehicle&limit=10",
  () => "type=Vehicle&q=speed%3E100&limit=10",
  () => "type=Vehicle&georel=within&geometry=Polygon&coordinates=%5B%5B%5B16.7%2C47.6%5D%2C%5B19.7%2C47.6%5D%2C%5B19.7%2C49.7%5D%2C%5B16.7%2C49.7%5D%2C%5B16.7%2C47.6%5D%5D%5D&limit=10",
  () => "type=Building&scopeQ=%2Fregion%2Fnorth%2F%23&limit=10",
  (t) => `type=Sensor&idPattern=urn:ngsi-ld:Sensor:t${t}:${t}.*&limit=10`,
];
const byShape = SHAPES.map((_, i) => new Counter(`queries_shape${i}`));

export default function () {
  const i = exec.scenario.iterationInTest;
  const t = i % TENANTS;
  const shape = Math.floor(i / TENANTS) % SHAPES.length;
  const r = http.get(`${BASE}/entities?${SHAPES[shape](t)}`, { headers: { "NGSILD-Tenant": `t${t}` } });
  if (r.status === 200) {
    ok.add(1); byShape[shape].add(1);
    if (r.headers["Ngsild-Warning"] || r.headers["NGSILD-Warning"]) warned.add(1);
  } else {
    errors.add(1);
    if (r.status === 0) errConn.add(1); else if (r.status >= 500) err5xx.add(1); else err4xx.add(1);
  }
}
