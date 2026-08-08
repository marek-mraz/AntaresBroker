// The 🛰 panel: every NGSI-LD call the app makes, newest first.
import React, { useState } from "react";
import { setReqlog, transport } from "../broker/transport.js";
import { useTransport } from "../hooks.js";

export default function RequestLog() {
  useTransport();
  const [open, setOpen] = useState(false);

  return (
    <div className={`reqlog${open ? " open" : ""}`}>
      <div className="row">
        <button onClick={() => setOpen(!open)} data-testid="reqlog-toggle">
          🛰 requests {transport.reqlogOn ? `(${transport.requests.length})` : "(off)"}
        </button>
        <button onClick={() => setReqlog(!transport.reqlogOn)}>
          {transport.reqlogOn ? "⏸ stop logging" : "▶ log requests"}
        </button>
      </div>
      {open && (
        <div className="entries" data-testid="reqlog-entries">
          {transport.requests.slice(0, 100).map((r, i) => (
            <div key={i} className={r.status >= 400 ? "err" : ""}>
              <span className="mono">[{r.tenant}] {r.method} {r.path}</span> → {r.status}
            </div>
          ))}
        </div>
      )}
    </div>
  );
}
