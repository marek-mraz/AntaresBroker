// The spreadsheet: every entity of the selected tenant with filters — free
// text, type, origin (local / a specific CSR peer). Every row always carries
// its origin (README rule 2). Row click opens the temporal history.
import React, { useEffect, useMemo, useState } from "react";
import { uuid } from "../uuid.js";
import { board, originOf, refreshSpace, removeSpace, setFedView, emit, toast } from "../state/board.js";
import { useBoard } from "../hooks.js";
import { avatarOf, colorOf, entLabel, TENANT_RE, TYPES } from "../model.js";
import { attrHistory, createEntity, registerLink, subscribeAll } from "../broker/api.js";
import { deletePipe } from "../state/pipes.js";

// Inline historical values next to the current one — refetched when the
// visible value changes, so it stays as fresh as the cell without polling.
function RowSpark({ space, id, attr, value }) {
  const [pts, setPts] = useState([]);
  useEffect(() => {
    let alive = true;
    attrHistory(space, id, attr, 20)
      .then((p) => alive && setPts(p))
      .catch(() => {});
    return () => {
      alive = false;
    };
  }, [space, id, attr, value]);
  if (pts.length < 2) return null;
  const W = 64, H = 16, PAD = 2;
  const xs = pts.map((p) => p.at), ys = pts.map((p) => p.value);
  const x0 = Math.min(...xs), x1 = Math.max(...xs);
  const y0 = Math.min(...ys), y1 = Math.max(...ys);
  const d = pts
    .map((p) =>
      `${(PAD + ((p.at - x0) / Math.max(1, x1 - x0)) * (W - 2 * PAD)).toFixed(1)},` +
      `${(H - PAD - ((p.value - y0) / Math.max(1e-9, y1 - y0)) * (H - 2 * PAD)).toFixed(1)}`)
    .join(" ");
  return (
    <svg width={W} height={H} className="row-spark" data-testid="row-spark">
      <polyline points={d} fill="none" stroke="var(--accent)" strokeWidth="1.5" />
      <title>{pts.map((p) => p.value).join(" → ")}</title>
    </svg>
  );
}

export function filterRows(rows, { text, type, origin }) {
  return rows.filter((r) => {
    if (type !== "all" && r.type !== type) return false;
    if (origin !== "all" && r.origin !== origin) return false;
    if (text && !`${r.id} ${r.type} ${r.attr ?? ""}`.toLowerCase().includes(text.toLowerCase())) return false;
    return true;
  });
}

export default function TenantSheet({ space, picked, onPick }) {
  const v = useBoard();
  const [text, setText] = useState("");
  const [type, setType] = useState("all");
  const [origin, setOrigin] = useState("all");
  const [fedTo, setFedTo] = useState("");

  const rows = useMemo(() => {
    const cur = board.ents.get(space) ?? { local: [], remote: [] };
    const mk = (doc, org) => {
      const { type: t, emoji, attr, value } = entLabel(doc);
      return {
        id: doc.id, type: t, emoji, attr, value,
        name: `${t} ${doc.id.split(":").pop()}`,
        origin: org, modifiedAt: doc.modifiedAt,
        space: org === "local" ? space : org,
      };
    };
    return [
      ...cur.local.map((d) => mk(d, "local")),
      ...cur.remote.map((d) => mk(d, originOf(d.id, space) ?? "fed")),
    ];
  }, [v, space]);

  const typeOptions = useMemo(() => [...new Set(rows.map((r) => r.type))].sort(), [rows]);
  const originOptions = useMemo(() => [...new Set(rows.map((r) => r.origin))].sort(), [rows]);
  const shown = filterRows(rows, { text, type, origin });
  const fedOn = board.fedView.has(space);
  const peers = board.spaces.map((s) => s.name).filter((n) => n !== space);

  return (
    <div className="sheet" data-testid="sheet">
      <div className="sheet-head">
        <h2>
          {avatarOf(space)} {space}
          <span className="sub"> tenant · 🏠 {rows.filter((r) => r.origin === "local").length} local
            {fedOn ? ` · 🌐 ${rows.filter((r) => r.origin !== "local").length} federated` : " · fed view off"}</span>
        </h2>
        <div className="row">
          <button
            onClick={async () => {
              const names = Object.keys(TYPES);
              const t = names[Math.floor(Math.random() * names.length)];
              await createEntity(space, {
                id: `urn:ngsi-ld:${t}:${uuid().slice(0, 8)}`,
                type: t,
                [TYPES[t].attr]: {
                  type: "Property",
                  value: TYPES[t].gen(Date.now()),
                  observedAt: new Date().toISOString(),
                },
              });
              await refreshSpace(space);
              emit();
            }}
          >＋ entity</button>
          <button
            onClick={async () => {
              (await subscribeAll(space, "http://page.local/demo", Object.keys(TYPES)))
                ? toast(`🔔 watching ${space}`)
                : toast("subscription failed");
            }}
          >🔔 watch</button>
          <button className={fedOn ? "primary" : ""} onClick={() => setFedView(space, !fedOn)}>
            🌐 fed view
          </button>
          {space !== "default" && (
            <button
              title="remove this space"
              onClick={async () => {
                if (window.confirm(`Remove space "${space}" (entities, subs, CSRs)?`)) {
                  await removeSpace(space, { deletePipe });
                }
              }}
            >✕ space</button>
          )}
        </div>
        <div className="row">
          <select value={fedTo} onChange={(e) => setFedTo(e.target.value)} data-testid="fed-to">
            <option value="">federate with…</option>
            {peers.map((p) => <option key={p}>{p}</option>)}
          </select>
          <button
            disabled={!fedTo || !TENANT_RE.test(fedTo)}
            onClick={async () => {
              await registerLink(space, fedTo, null, TYPES);
              setFedView(space, true);
              setFedTo("");
            }}
          >🔗 register CSR</button>
        </div>
        <div className="row filters">
          <input
            placeholder="filter…"
            value={text}
            onChange={(e) => setText(e.target.value)}
            data-testid="filter-text"
          />
          <select value={type} onChange={(e) => setType(e.target.value)} data-testid="filter-type">
            <option value="all">all types</option>
            {typeOptions.map((t) => <option key={t}>{t}</option>)}
          </select>
          <select value={origin} onChange={(e) => setOrigin(e.target.value)} data-testid="filter-origin">
            <option value="all">any origin</option>
            {originOptions.map((o) => (
              <option key={o} value={o}>{o === "local" ? "🏠 local" : `🌐 ${o}`}</option>
            ))}
          </select>
        </div>
      </div>
      <div className="sheet-table">
        <table>
          <thead>
            <tr><th>name</th><th>value</th><th>origin</th><th>attribute</th><th>type</th><th>id</th></tr>
          </thead>
          <tbody>
            {shown.map((r) => (
              <tr
                key={r.id}
                data-testid="sheet-row"
                className={picked?.id === r.id ? "picked" : ""}
                onClick={() => onPick({ space: r.space, viewer: space, id: r.id, attr: r.attr, emoji: r.emoji, type: r.type })}
              >
                <td data-testid="cell-name">{r.emoji} {r.name}</td>
                <td className="num">
                  {r.value ?? "·"}{" "}
                  {r.attr && <RowSpark space={r.space} id={r.id} attr={r.attr} value={r.value} />}
                </td>
                <td>
                  {r.origin === "local"
                    ? <span className="chip">🏠 local</span>
                    : <span className="chip fed" style={{ borderColor: colorOf(r.origin) }}>🌐 ← {avatarOf(r.origin)} {r.origin}</span>}
                </td>
                <td>{r.attr ?? "—"}</td>
                <td>{r.type}</td>
                <td className="mono" title={r.id}>{r.id.split(":").pop()}</td>
              </tr>
            ))}
            {!shown.length && (
              <tr><td colSpan={6} className="empty">no entities match — create one or turn on 🌐 fed view</td></tr>
            )}
          </tbody>
        </table>
      </div>
    </div>
  );
}
