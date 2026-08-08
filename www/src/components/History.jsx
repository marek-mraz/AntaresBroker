// Temporal history for one attribute — the broker records it automatically;
// this asks /temporal/entities and draws what actually happened: the value
// line over time, and below it the change (Δ) each recorded instance made.
import React, { useEffect, useState } from "react";
import { attrHistory } from "../broker/api.js";

// Δ between consecutive instances: [{ at, delta }], length n-1.
export const deltas = (points) =>
  points.slice(1).map((p, i) => ({ at: p.at, delta: p.value - points[i].value }));

const fmt = (n) => (Number.isInteger(n) ? String(n) : n.toFixed(2));
const at = (ms) => new Date(ms).toLocaleTimeString();

export default function History({ space, viewer, id, attr, emoji, type }) {
  const [points, setPoints] = useState(null);

  useEffect(() => {
    let alive = true;
    const pull = async () => {
      // history lives where the entity LIVES (its owning tenant)
      const pts = await attrHistory(space, id, attr).catch(() => []);
      if (alive) setPoints(pts);
    };
    pull();
    const t = setInterval(pull, 4000);
    return () => {
      alive = false;
      clearInterval(t);
    };
  }, [space, id, attr]);

  const W = 340, PAD = 6, VH = 90, CH = 56;

  // values line
  let path = "", dots = [], y0 = 0, y1 = 0, sx = null;
  if (points?.length) {
    const xs = points.map((p) => p.at);
    const ys = points.map((p) => p.value);
    const x0 = Math.min(...xs), x1 = Math.max(...xs);
    y0 = Math.min(...ys);
    y1 = Math.max(...ys);
    sx = (x) => PAD + ((x - x0) / Math.max(1, x1 - x0)) * (W - 2 * PAD);
    const sy = (y) => VH - PAD - ((y - y0) / Math.max(1e-9, y1 - y0)) * (VH - 2 * PAD);
    if (points.length > 1) {
      path = points
        .map((p, i) => `${i ? "L" : "M"} ${sx(p.at).toFixed(1)} ${sy(p.value).toFixed(1)}`)
        .join(" ");
    }
    dots = points.map((p) => ({ x: sx(p.at), y: sy(p.value), p }));
  }

  // changes bars — their own chart with their own scale and a zero baseline
  // (two measures never share one axis)
  const ds = points ? deltas(points) : [];
  const dmax = Math.max(1e-9, ...ds.map((d) => Math.abs(d.delta)));
  const mid = CH / 2;
  const barW = ds.length ? Math.max(1, (W - 2 * PAD) / ds.length - 2) : 0;

  const last = points?.at(-1);

  return (
    <div className="history" data-testid="history">
      <h3>⏱ {emoji} {type} · <span className="mono">{attr}</span>
        {space !== viewer && <span className="sub"> (history read from {space})</span>}
      </h3>
      {points === null && <div className="sub">loading…</div>}
      {points?.length === 0 && (
        <div className="sub">no temporal history yet — instances appear as the value changes</div>
      )}
      {points?.length > 0 && (
        <>
          <svg width={W} height={VH} className="spark" data-testid="values-chart">
            {path && <path d={path} fill="none" stroke="var(--accent)" strokeWidth="2" />}
            {dots.map((d, i) => {
              const isLast = i === dots.length - 1;
              return (
                <circle
                  key={i}
                  cx={d.x.toFixed(1)}
                  cy={d.y.toFixed(1)}
                  r={isLast ? 3 : 6}
                  fill={isLast ? "var(--accent)" : "transparent"}
                >
                  <title>{`${fmt(d.p.value)} at ${at(d.p.at)}`}</title>
                </circle>
              );
            })}
            {points.length > 1 && (
              <>
                <text x={PAD} y={PAD + 8} fontSize="9" fill="var(--muted)">{fmt(y1)}</text>
                <text x={PAD} y={VH - PAD} fontSize="9" fill="var(--muted)">{fmt(y0)}</text>
              </>
            )}
          </svg>
          {ds.length > 0 && (
            <svg width={W} height={CH} className="spark" data-testid="changes-chart">
              <line x1={PAD} y1={mid} x2={W - PAD} y2={mid} stroke="var(--line)" />
              {ds.map((d, i) => {
                const h = (Math.abs(d.delta) / dmax) * (mid - PAD);
                const x = (sx(d.at) - barW / 2).toFixed(1);
                return d.delta === 0 ? (
                  <rect key={i} data-testid="delta-bar" data-sign="zero"
                    x={x} y={mid - 1} width={barW} height="2" rx="1" fill="var(--muted)">
                    <title>{`Δ 0 at ${at(d.at)}`}</title>
                  </rect>
                ) : (
                  <rect key={i} data-testid="delta-bar" data-sign={d.delta > 0 ? "up" : "down"}
                    x={x} y={(d.delta > 0 ? mid - h : mid).toFixed(1)}
                    width={barW} height={Math.max(1, h).toFixed(1)} rx="1"
                    fill={d.delta > 0 ? "var(--ok)" : "var(--err)"}>
                    <title>{`Δ ${d.delta > 0 ? "+" : ""}${fmt(d.delta)} at ${at(d.at)}`}</title>
                  </rect>
                );
              })}
            </svg>
          )}
          <div className="sub">
            {points.length} instance{points.length === 1 ? "" : "s"} ·{" "}
            {ds.length} change{ds.length === 1 ? "" : "s"} · latest{" "}
            <strong>{fmt(last.value)}</strong> at {at(last.at)}
          </div>
        </>
      )}
    </div>
  );
}
