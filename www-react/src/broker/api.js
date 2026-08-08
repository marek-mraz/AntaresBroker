// The NGSI-LD client — the ONLY module that builds broker URLs. Origin
// attribution lives here so the /entityMaps upgrade (exact provenance,
// clause 5.14) later changes this file alone.
import { brokerFetch } from "./transport.js";
import { uuid } from "../uuid.js";
import { ALL_TYPES, CORE_CTX, LOOPBACK } from "../model.js";

// Tenant-scoped call; the default tenant sends no header (6.3.14).
export function api(space, path, opts = {}) {
  const headers = { ...(opts.headers ?? {}) };
  if (space !== "default") headers["NGSILD-Tenant"] = space;
  return brokerFetch(path, { ...opts, headers });
}

const postLd = (space, path, body) =>
  api(space, path, {
    method: "POST",
    headers: { "Content-Type": "application/ld+json" },
    body: JSON.stringify({ "@context": CORE_CTX, ...body }),
  });

export const health = async () => (await brokerFetch("/q/health")).json();

export async function listEntities(space, { local = false, type = ALL_TYPES } = {}) {
  const r = await api(space, `/ngsi-ld/v1/entities?type=${type}&limit=100${local ? "&local=true" : ""}`);
  return r.ok ? r.json() : [];
}

export async function listCsrs(space) {
  // 5.10.2.4: registration discovery needs a discriminating input.
  const r = await api(space, `/ngsi-ld/v1/csourceRegistrations?type=${ALL_TYPES}&limit=100`);
  return r.ok ? r.json() : [];
}

export async function listSubscriptions(space) {
  const r = await api(space, "/ngsi-ld/v1/subscriptions?limit=100");
  return r.ok ? r.json() : [];
}

export const createEntity = (space, doc) => postLd(space, "/ngsi-ld/v1/entities", doc);

export function patchAttr(space, id, attr, fragment) {
  return api(space, `/ngsi-ld/v1/entities/${encodeURIComponent(id)}/attrs/${attr}`, {
    method: "PATCH",
    headers: { "Content-Type": "application/ld+json" },
    body: JSON.stringify({ ...fragment, "@context": CORE_CTX }),
  });
}

export const deleteEntity = (space, id) =>
  api(space, `/ngsi-ld/v1/entities/${encodeURIComponent(id)}`, { method: "DELETE" });

export function batchUpsert(space, docs) {
  return api(space, "/ngsi-ld/v1/entityOperations/upsert", {
    method: "POST",
    headers: { "Content-Type": "application/ld+json" },
    body: JSON.stringify(docs.map((d) => ({ ...d, "@context": CORE_CTX }))),
  });
}

export function batchDelete(space, ids) {
  return api(space, "/ngsi-ld/v1/entityOperations/delete", {
    method: "POST",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify(ids),
  });
}

// The spec-clean cross-tenant link: endpoint = this same broker via the
// loopback host, `tenant` = the peer space (5.2.9, 4.3.6.2 inclusive).
export async function registerLink(from, to, type, types) {
  const entities = type ? [{ type }] : Object.keys(types).map((t) => ({ type: t }));
  const r = await postLd(from, "/ngsi-ld/v1/csourceRegistrations", {
    id: `urn:ngsi-ld:ContextSourceRegistration:${from}-to-${to}-${uuid().slice(0, 6)}`,
    type: "ContextSourceRegistration",
    information: [{ entities }],
    endpoint: LOOPBACK,
    mode: "inclusive",
    operations: ["federationOps"],
    ...(to !== "default" ? { tenant: to } : {}),
  });
  return r.status === 201;
}

export const deleteCsr = (space, id) =>
  api(space, `/ngsi-ld/v1/csourceRegistrations/${encodeURIComponent(id)}`, { method: "DELETE" });

export async function subscribeAll(space, endpoint, typeNames) {
  const r = await postLd(space, "/ngsi-ld/v1/subscriptions", {
    id: `urn:ngsi-ld:Subscription:${space}-${uuid().slice(0, 8)}`,
    type: "Subscription",
    entities: typeNames.map((type) => ({ type })),
    notification: { endpoint: { uri: endpoint, accept: "application/json" } },
  });
  return r.status === 201;
}

// API-level wipe of one space: subscriptions, CSRs, entities.
export async function cleanupSpace(space) {
  for (const s of (await listSubscriptions(space)) ?? []) {
    await api(space, `/ngsi-ld/v1/subscriptions/${encodeURIComponent(s.id)}`, { method: "DELETE" });
  }
  for (const reg of (await listCsrs(space)) ?? []) {
    await deleteCsr(space, reg.id);
  }
  const list = await listEntities(space, { local: true });
  if (list.length) await batchDelete(space, list.map((e) => e.id));
}

// Temporal history of one attribute: [{ at: ms, value }], oldest first.
// The store records history automatically; 404 just means "none yet".
export async function attrHistory(space, id, attr, lastN = 60) {
  const timeAt = new Date(Date.now() + 60_000).toISOString();
  const r = await api(
    space,
    `/ngsi-ld/v1/temporal/entities/${encodeURIComponent(id)}` +
      `?attrs=${attr}&lastN=${lastN}&timerel=before&timeAt=${encodeURIComponent(timeAt)}`,
  );
  if (!r.ok && r.status !== 206) return [];
  const doc = await r.json();
  let instances = doc?.[attr] ?? [];
  if (!Array.isArray(instances)) instances = [instances];
  return instances
    .map((i) => ({
      at: Date.parse(i.observedAt ?? i.modifiedAt ?? 0),
      value: typeof i.value === "number" ? i.value : Number(i.value),
    }))
    .filter((p) => Number.isFinite(p.value) && Number.isFinite(p.at))
    .sort((a, b) => a.at - b.at);
}
