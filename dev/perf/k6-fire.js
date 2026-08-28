// Update and delete stream over the loaded dataset, at one arrival rate,
// with the notification count the subscriptions must produce computed
// alongside: fire.sh compares it with what the sink received.
//
//   k6 run -e RATE=1000 -e DURATION=60s -e TENANTS=10000 -e SUBS=100000 -e ENTITIES=100000000 dev/perf/k6-fire.js
//
// Entity n is a Vehicle / Building / Sensor by n % 3 (gen.py); an update
// sets speed / temperature / value to a fresh number (the same value again
// is no change and does not notify). Subscription k of tenant t = k % TENANTS
// has p = k / TENANTS, filter class p % 8 with p as its parameter
// (api-load.py SUB_CLASSES, every subscription of a tenant unique);
// CLASS_FIRES below is the same rule, so the notifications due are known.
// Deletes take ids from the top tenth of each tenant (a repeat is a 404,
// not a failure); a subscription without entityDeleted in its triggers is
// silent on them. MQTT=1 marks every tenth subscription as delivered
// elsewhere than the HTTP sink.
//
// Env: BROKER_URL, RATE (1000), DURATION (60s), TENANTS, SUBS, ENTITIES,
//      DELETE_PCT (10), MQTT (0|1).

import http from "k6/http";
import exec from "k6/execution";
import { Counter } from "k6/metrics";

const BASE = `${__ENV.BROKER_URL || "http://localhost:9090"}/ngsi-ld/v1`;
const RATE = Number(__ENV.RATE || 1000);
const TENANTS = Number(__ENV.TENANTS || 10);
const SUBS = Number(__ENV.SUBS || 10);
const ENTITIES = Number(__ENV.ENTITIES || 100);
const DELETE_PCT = Number(__ENV.DELETE_PCT || 10);
const MQTT = __ENV.MQTT === "1";
// dashboards poll while devices write: READ_PCT % of RATE as GET /entities/{id}
const READ_PCT = Number(__ENV.READ_PCT || 20);
const PER_TENANT = Math.floor(ENTITIES / TENANTS);
const UPDATE_IDS = Math.max(1, Math.floor(PER_TENANT * 0.9));

const READ_RATE = Math.round(RATE * READ_PCT / 100);
export const options = {
  scenarios: {
    fire: {
      executor: "constant-arrival-rate",
      rate: RATE, timeUnit: "1s", duration: __ENV.DURATION || "60s",
      preAllocatedVUs: Math.min(4000, Math.max(50, RATE)), maxVUs: 8000,
    },
    ...(READ_RATE > 0 ? { reads: {
      executor: "constant-arrival-rate", exec: "read",
      rate: READ_RATE, timeUnit: "1s", duration: __ENV.DURATION || "60s",
      preAllocatedVUs: Math.min(1000, Math.max(20, READ_RATE)), maxVUs: 4000,
    } } : {}),
  },
  // never-failing thresholds: they make the per-scenario latency sub-metrics
  // appear in the summary export
  thresholds: {
    "http_req_duration{scenario:fire}": ["p(99)<600000"],
    "http_req_duration{scenario:reads}": ["p(99)<600000"],
  },
  summaryTrendStats: ["med", "p(99)"],
};

const reads = new Counter("reads_ok");
export function read() {
  const i = exec.scenario.iterationInTest;
  const t = i % TENANTS;
  const n = t + Math.floor(Math.random() * PER_TENANT) * TENANTS;
  const r = http.get(`${BASE}/entities/urn:ngsi-ld:${TYPES[n % 3]}:t${t}:${n}`, { headers: { "NGSILD-Tenant": `t${t}` } });
  if (r.status === 200 || r.status === 404) reads.add(1); else fail(r);
}

const updates = new Counter("updates_ok");
const deletes = new Counter("deletes_ok");
const errors = new Counter("op_errors");
// failed ops by class: status 0 = no HTTP answer (refused/reset/timeout)
const errConn = new Counter("op_errors_conn");
const err4xx = new Counter("op_errors_4xx");
const err5xx = new Counter("op_errors_5xx");
function fail(r) {
  errors.add(1);
  if (r.status === 0) errConn.add(1); else if (r.status >= 500) err5xx.add(1); else err4xx.add(1);
}
const expected = new Counter("notifications_expected");
const expectedHttp = new Counter("notifications_expected_http");
const NCLASS = 8;
// api-load.py SUB_CLASSES, in order: does an update of entity n (type
// kind, new value v) fire a subscription of this class?
const CLASS_FIRES = [
  (kind, n, v, p) => kind === "Vehicle" && v > 100 + p,                    // vehicle-any q=speed>100+p
  () => false,                                                             // vehicle-cold-attr watches brand
  (kind, n, v, p) => kind === "Vehicle" && v > 500000000 + p * 1000000,    // vehicle-high-speed
  (kind, n, v, p) => kind === "Vehicle" && n % 10 === p % 10,              // vehicle-id-tail idPattern .*{p%10}$
  (kind, n, v, p) => kind === "Building" && v > 20 + p,                    // building-any q=temperature>20+p
  (kind, n, v, p) => kind === "Sensor" && v > p,                           // sensor-any q=value>p
  (kind, n, v, p) => kind === "Vehicle" && n % 1000 < 250 + 5 * p,         // vehicle-geo-west, edge at 250+5p
  (kind, n, v, p) => SCOPE_FIRES[p % 4](n % 4),                            // any-scope SCOPE_Q[p%4]
];
// api-load.py SCOPE_Q: /region/north/#, /region/south/#, /region/north/urban,
// /region/south/rural against gen.py scope_of (n % 4: north/urban,
// north/rural, south/urban, south/rural)
const SCOPE_FIRES = [(s) => s < 2, (s) => s >= 2, (s) => s === 0, (s) => s === 3];
const expectedByClass = Array.from({ length: NCLASS }, (_, c) => new Counter(`notifications_expected_class${c}`));
const TYPES = ["Vehicle", "Building", "Sensor"];
const ATTR = { Vehicle: "speed", Building: "temperature", Sensor: "value" };

// subscriptions of tenant t: k = t, t+TENANTS, … < SUBS (api-load.py's
// layout), class (k / TENANTS) % 8 — count the ones this update fires
function fired(t, kind, n, v) {
  let all = 0, viaHttp = 0;
  const perClass = new Array(NCLASS).fill(0);
  for (let k = t; k < SUBS; k += TENANTS) {
    const p = Math.floor(k / TENANTS);
    const c = p % NCLASS;
    if (!CLASS_FIRES[c](kind, n, v, p)) continue;
    all++;
    if (MQTT && k % 10 === 0) continue; // delivered elsewhere than the HTTP sink
    viaHttp++; perClass[c]++;
  }
  return [all, viaHttp, perClass];
}

export default function () {
  // one global sequence across VUs: tenants rotate, slots never repeat
  // until the pool wraps (VU*1e6+ITER made every VU hit the same slots)
  const i = exec.scenario.iterationInTest;
  const t = i % TENANTS;
  const tenant = `t${t}`;
  const headers = { "Content-Type": "application/json", "NGSILD-Tenant": tenant };
  const slot = Math.floor(i / TENANTS);
  // decided per slot, not per i: with i the deletes all land on tenant 0
  // whenever 100/DELETE_PCT divides TENANTS
  const period = Math.round(100 / DELETE_PCT);
  const del = (slot % period) === 0;
  if (del && UPDATE_IDS < PER_TENANT) {
    // gen.py numbers entities globally: tenant t owns t, t+TENANTS, t+2·TENANTS, …
    // pool index walks slot/period so consecutive deletes hit fresh ids
    const n = t + (UPDATE_IDS + (Math.floor(slot / period) % (PER_TENANT - UPDATE_IDS))) * TENANTS;
    const r = http.del(`${BASE}/entities/urn:ngsi-ld:${TYPES[n % 3]}:${tenant}:${n}`, null, { headers });
    if (r.status === 204) deletes.add(1); else if (r.status !== 404) fail(r);
    return;
  }
  const n = t + (slot % UPDATE_IDS) * TENANTS;
  const kind = TYPES[n % 3];
  // above every class threshold (speed>100, temperature>20) and fresh
  const v = 101 + Math.floor(Math.random() * 1e9);
  const body = {}; body[ATTR[kind]] = { type: "Property", value: v };
  const r = http.patch(`${BASE}/entities/urn:ngsi-ld:${kind}:${tenant}:${n}/attrs`, JSON.stringify(body), { headers });
  if (r.status === 204) {
    updates.add(1);
    const [all, viaHttp, perClass] = fired(t, kind, n, v);
    expected.add(all);
    expectedHttp.add(viaHttp);
    for (let c = 0; c < NCLASS; c++) if (perClass[c]) expectedByClass[c].add(perClass[c]);
  } else {
    fail(r);
  }
}
