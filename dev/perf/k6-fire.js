// Update and delete stream over the loaded dataset, at one arrival rate,
// with the notification count the subscriptions must produce computed
// alongside: fire.sh compares it with what the sink received.
//
//   k6 run -e RATE=1000 -e DURATION=60s -e TENANTS=10000 -e SUBS=100000 -e ENTITIES=100000000 dev/perf/k6-fire.js
//
// Every update sets speed to a new value above 100 on a loaded entity (the
// same value again is no change and does not notify), so each subscription of
// that tenant (`q=speed>100`, type Vehicle) fires once. Deletes take ids
// from the top tenth of each tenant (a repeat is a 404, not a failure); a subscription
// without entityDeleted in its triggers is silent on them. MQTT=1 marks
// every tenth subscription as delivered elsewhere than the HTTP sink.
//
// Env: BROKER_URL, RATE (1000), DURATION (60s), TENANTS, SUBS, ENTITIES,
//      DELETE_PCT (10), MQTT (0|1).

import http from "k6/http";
import { Counter } from "k6/metrics";

const BASE = `${__ENV.BROKER_URL || "http://localhost:9090"}/ngsi-ld/v1`;
const RATE = Number(__ENV.RATE || 1000);
const TENANTS = Number(__ENV.TENANTS || 10);
const SUBS = Number(__ENV.SUBS || 10);
const ENTITIES = Number(__ENV.ENTITIES || 100);
const DELETE_PCT = Number(__ENV.DELETE_PCT || 10);
const MQTT = __ENV.MQTT === "1";
const PER_TENANT = Math.floor(ENTITIES / TENANTS);
const UPDATE_IDS = Math.max(1, Math.floor(PER_TENANT * 0.9));

export const options = {
  scenarios: {
    fire: {
      executor: "constant-arrival-rate",
      rate: RATE, timeUnit: "1s", duration: __ENV.DURATION || "60s",
      preAllocatedVUs: Math.min(4000, Math.max(50, RATE)), maxVUs: 8000,
    },
  },
  summaryTrendStats: ["med", "p(99)"],
};

const updates = new Counter("updates_ok");
const deletes = new Counter("deletes_ok");
const errors = new Counter("op_errors");
const expected = new Counter("notifications_expected");
const expectedHttp = new Counter("notifications_expected_http");

// subscriptions of tenant t: k = t, t+TENANTS, … < SUBS (api-load.py's layout)
function subsOf(t) {
  let all = 0, viaHttp = 0;
  for (let k = t; k < SUBS; k += TENANTS) { all++; if (!(MQTT && k % 10 === 0)) viaHttp++; }
  return [all, viaHttp];
}

export default function () {
  const i = __VU * 1e6 + __ITER;
  const t = i % TENANTS;
  const tenant = `t${t}`;
  const headers = { "Content-Type": "application/json", "NGSILD-Tenant": tenant };
  const del = (i % Math.round(100 / DELETE_PCT)) === 0;
  const slot = Math.floor(i / TENANTS);
  if (del && UPDATE_IDS < PER_TENANT) {
    const n = UPDATE_IDS + (slot % (PER_TENANT - UPDATE_IDS));
    const r = http.del(`${BASE}/entities/urn:ngsi-ld:Vehicle:${tenant}:${n}`, null, { headers });
    if (r.status === 204) deletes.add(1); else if (r.status !== 404) errors.add(1);
    return;
  }
  const n = slot % UPDATE_IDS;
  const r = http.patch(`${BASE}/entities/urn:ngsi-ld:Vehicle:${tenant}:${n}/attrs`,
    JSON.stringify({ speed: { type: "Property", value: 101 + Math.floor(Math.random() * 1e9) } }), { headers });
  if (r.status === 204) {
    updates.add(1);
    const [all, viaHttp] = subsOf(t);
    expected.add(all);
    expectedHttp.add(viaHttp);
  } else {
    errors.add(1);
  }
}
