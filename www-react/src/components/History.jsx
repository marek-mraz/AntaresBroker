// Temporal history for one attribute — the broker records it automatically;
// this just asks /temporal/entities and draws what actually happened.
import React, { useEffect, useState } from "react";
import { attrHistory } from "../broker/api.js";

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

  const W = 340, H = 90, PAD = 6;
  let path = "";
  if (points?.length > 1) {
    const xs = points.map((p) => p.at);
    const ys = points.map((p) => p.value);
    const x0 = Math.min(...xs), x1 = Math.max(...xs);
    const y0 = Math.min(...ys), y1 = Math.max(...ys);
    const sx = (x) => PAD + ((x - x0) / Math.max(1, x1 - x0)) * (W - 2 * PAD);
    const sy = (y) => H - PAD - ((y - y0) / Math.max(1e-9, y1 - y0)) * (H - 2 * PAD);
    path = points.map((p, i) => `${i ? "L" : "M"} ${sx(p.at).toFixed(1)} ${sy(p.value).toFixed(1)}`).join(" ");
  }
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
          <svg width={W} height={H} className="spark">
            {path && <path d={path} fill="none" stroke="var(--accent)" strokeWidth="2" />}
            {points.length === 1 && (
              <circle cx={W / 2} cy={H / 2} r="3" fill="var(--accent)" />
            )}
          </svg>
          <div className="sub">
            {points.length} instance{points.length === 1 ? "" : "s"} · latest{" "}
            <strong>{last.value}</strong> at {new Date(last.at).toLocaleTimeString()}
          </div>
        </>
      )}
    </div>
  );
}
