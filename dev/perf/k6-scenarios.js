// Scenario-specific load test driver for dev/perf/scenarios.sh (MODE=load).
//
//   k6 run -e SCENARIO=hot-entity -e BROKER_URL=http://localhost:9101 dev/perf/k6-scenarios.js

import http from "k6/http";
import exec from "k6/execution";
import { Counter, Trend } from "k6/metrics";

const SCENARIO = __ENV.SCENARIO || "hot-entity";
const BROKER = __ENV.BROKER_URL || "http://127.0.0.1:9101";
const SOURCE = __ENV.SOURCE_URL || "http://127.0.0.1:9102";
const RATE = Number(__ENV.RATE || 100);
const DURATION = __ENV.DURATION || "30s";
const TENANT = __ENV.TENANT || SCENARIO.replace(/-/g, "_");

export const options = {
  scenarios: {
    stage: {
      executor: "constant-arrival-rate",
      rate: RATE,
      timeUnit: "1s",
      duration: DURATION,
      preAllocatedVUs: Math.min(2000, Math.max(20, RATE)),
      maxVUs: 4000,
    },
  },
  thresholds: {
    http_req_duration: ["p(99)<60000"],
  },
  summaryTrendStats: ["med", "p(99)"],
};

const opOk = new Counter("ops_ok");
const opFail = new Counter("ops_failed");
const warnings = new Counter("ops_warning");
const loops508 = new Counter("ops_508");
const quietGetMs = new Trend("quiet_get_ms");
const loudPatchMs = new Trend("loud_patch_ms");

const HEADERS = {
  "Content-Type": "application/json",
  "NGSILD-Tenant": TENANT,
};

export default function () {
  const iter = exec.scenario.iterationInTest;
  let res;

  if (SCENARIO === "hot-entity") {
    const isSpread = __ENV.SPREAD === "1";
    const entityId = isSpread
      ? `urn:ngsi-ld:Vehicle:${TENANT}:${iter % 1000}`
      : `urn:ngsi-ld:Vehicle:${TENANT}:hot0`;
    const body = JSON.stringify({ speed: { type: "Property", value: iter % 100000 } });
    res = http.patch(`${BROKER}/ngsi-ld/v1/entities/${entityId}/attrs`, body, { headers: HEADERS });
    if (res.status === 204) opOk.add(1); else opFail.add(1);
  } else if (SCENARIO === "noisy-tenant") {
    const isLoudZero = __ENV.LOUD === "0";
    if (isLoudZero) {
      res = http.get(`${BROKER}/ngsi-ld/v1/entities/urn:ngsi-ld:Vehicle:quiet:${iter % 100}`, {
        headers: { "NGSILD-Tenant": "quiet" },
      });
      quietGetMs.add(res.timings.duration);
      if (res.status === 200) opOk.add(1); else opFail.add(1);
    } else if (iter % 5 === 0) {
      res = http.get(`${BROKER}/ngsi-ld/v1/entities/urn:ngsi-ld:Vehicle:quiet:${iter % 100}`, {
        headers: { "NGSILD-Tenant": "quiet" },
      });
      quietGetMs.add(res.timings.duration);
      if (res.status === 200) opOk.add(1); else opFail.add(1);
    } else {
      const body = JSON.stringify({ speed: { type: "Property", value: iter % 50000 } });
      res = http.patch(`${BROKER}/ngsi-ld/v1/entities/urn:ngsi-ld:Vehicle:loud:${iter % 100}/attrs`, body, {
        headers: { "NGSILD-Tenant": "loud", "Content-Type": "application/json" },
      });
      loudPatchMs.add(res.timings.duration);
      if (res.status === 204) opOk.add(1); else opFail.add(1);
    }
  } else if (SCENARIO === "slow-subscriber" || SCENARIO === "fan-in") {
    const body = JSON.stringify({ speed: { type: "Property", value: iter % 100000 } });
    res = http.patch(`${BROKER}/ngsi-ld/v1/entities/urn:ngsi-ld:Vehicle:${TENANT}:0/attrs`, body, { headers: HEADERS });
    if (res.status === 204) opOk.add(1); else opFail.add(1);
  } else if (SCENARIO === "hub-sources") {
    res = http.get(`${BROKER}/ngsi-ld/v1/entities?type=Vehicle&limit=20`, { headers: HEADERS });
    if (res.status === 200) {
      opOk.add(1);
      if (res.headers["Ngsild-Warning"] || res.headers["NGSILD-Warning"]) warnings.add(1);
    } else opFail.add(1);
  } else if (SCENARIO === "collision") {
    res = http.get(`${BROKER}/ngsi-ld/v1/entities/urn:ngsi-ld:Vehicle:${TENANT}:both`, { headers: HEADERS });
    if (res.status === 200) opOk.add(1); else opFail.add(1);
  } else if (SCENARIO === "loop") {
    res = http.get(`${BROKER}/ngsi-ld/v1/entities?type=Vehicle&limit=20`, { headers: HEADERS });
    if (res.status === 200) opOk.add(1);
    else if (res.status === 508) { loops508.add(1); opFail.add(1); }
    else opFail.add(1);
  } else if (SCENARIO === "distributed-subscription") {
    // Stamp send time in observedAt for end-to-end latency measurement
    const nowIso = new Date().toISOString();
    const body = JSON.stringify({
      speed: { type: "Property", value: iter % 100000, observedAt: nowIso },
    });
    const target = iter % 2 === 0 ? BROKER : SOURCE;
    res = http.patch(`${target}/ngsi-ld/v1/entities/urn:ngsi-ld:Vehicle:${TENANT}:srca0/attrs`, body, { headers: HEADERS });
    if (res.status === 204) opOk.add(1); else opFail.add(1);
  } else if (SCENARIO === "ha-pair") {
    const target = iter % 2 === 0 ? BROKER : SOURCE;
    const body = JSON.stringify({ speed: { type: "Property", value: iter % 100000 } });
    res = http.patch(`${target}/ngsi-ld/v1/entities/urn:ngsi-ld:Vehicle:${TENANT}:0/attrs`, body, { headers: HEADERS });
    if (res.status === 204) opOk.add(1); else opFail.add(1);
  }
}
