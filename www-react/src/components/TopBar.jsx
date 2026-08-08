import React, { useRef, useState } from "react";
import { transport } from "../broker/transport.js";
import { useTransport } from "../hooks.js";
import { addSpace, applyBoardTemplate, boardTemplate, board, createDemo, resetBoard, toast } from "../state/board.js";
import { addPipe, deletePipe, startPipe } from "../state/pipes.js";
import { GENERATORS, TENANT_RE, TYPES } from "../model.js";

export default function TopBar({ health }) {
  useTransport();
  const [busy, setBusy] = useState(false);
  const [tpl, setTpl] = useState(null); // template dialog text or null
  const [pipeDlg, setPipeDlg] = useState(false);
  const genRef = useRef(); const kindRef = useRef(); const fromRef = useRef();
  const typeRef = useRef(); const intoRef = useRef(); const secsRef = useRef();

  const run = (fn) => async () => {
    setBusy(true);
    try { await fn(); } finally { setBusy(false); }
  };

  return (
    <header className="topbar">
      <h1>⭐ Antares <span className="accent">NGSI-LD</span> playground</h1>
      <span className="pill" title="ETSI CIM 009 — NGSI-LD API as WebAssembly in this tab">
        in-browser wasm broker
      </span>
      <span className="pill" data-mode={transport.mode}>mode: <strong>{transport.mode}</strong></span>
      {health && <span className="pill">store: {health.store} · {health.status}</span>}
      <span className="grow" />
      <button disabled={busy} data-testid="btn-demo"
        onClick={run(() => createDemo({ startPipe }))}>▶ demo</button>
      <button data-testid="btn-template"
        onClick={() => setTpl(JSON.stringify(boardTemplate("smart-city"), null, 2))}>{"{ } template"}</button>
      <button onClick={() => {
        const name = window.prompt("new context space (A-Za-z0-9-)");
        if (name == null) return;
        if (!addSpace(name.trim())) toast(`invalid name "${name}" — A-Za-z0-9 and - only`);
      }}>＋ space</button>
      <button onClick={() => setPipeDlg(true)}>＋ pipeline</button>
      <button disabled={busy} data-testid="btn-reset"
        onClick={run(async () => {
          if (window.confirm("Remove EVERYTHING? All data in every space, all CSRs and pipelines."))
            await resetBoard({ deletePipe });
        })}>🧨 reset</button>

      {tpl !== null && (
        <div className="modal" onClick={(e) => e.target === e.currentTarget && setTpl(null)}>
          <div className="modal-box">
            <h3>{"{ }"} Board template — JSON</h3>
            <p className="sub">Structure as data: spaces, CSRs, pipelines. Edit and apply — entities are data, not template.</p>
            <textarea value={tpl} onChange={(e) => setTpl(e.target.value)} spellCheck={false} data-testid="tpl-text" />
            <div className="row">
              <button onClick={() => navigator.clipboard?.writeText(tpl).then(() => toast("copied"))}>⧉ copy</button>
              <button className="primary" disabled={busy} data-testid="tpl-apply"
                onClick={run(async () => {
                  let parsed;
                  try { parsed = JSON.parse(tpl); } catch (e) { toast(`not JSON: ${e.message}`); return; }
                  await applyBoardTemplate(parsed, { startPipe });
                  toast("template applied");
                  setTpl(null);
                })}>⇪ apply</button>
              <button onClick={() => setTpl(null)}>close</button>
            </div>
          </div>
        </div>
      )}

      {pipeDlg && (
        <div className="modal" onClick={(e) => e.target === e.currentTarget && setPipeDlg(false)}>
          <div className="modal-box">
            <h3>＋ pipeline</h3>
            <label>kind
              <select ref={kindRef} defaultValue="source">
                <option value="source">data source → space (simulated device)</option>
                <option value="sync">space → space (periodic copy)</option>
              </select>
            </label>
            <label>device <select ref={genRef}>{Object.keys(GENERATORS).map((g) => <option key={g}>{g}</option>)}</select></label>
            <label>from (copy only) <select ref={fromRef}>{board.spaces.map((s) => <option key={s.name}>{s.name}</option>)}</select></label>
            <label>entity type (copy only) <select ref={typeRef}>{Object.keys(TYPES).map((t) => <option key={t}>{t}</option>)}</select></label>
            <label>into <select ref={intoRef}>{board.spaces.map((s) => <option key={s.name}>{s.name}</option>)}</select></label>
            <label>every N seconds <input ref={secsRef} type="number" min="1" max="60" defaultValue="3" /></label>
            <div className="row">
              <button className="primary" onClick={() => {
                const kind = kindRef.current.value;
                const secs = Math.max(1, Number(secsRef.current.value) || 3);
                const into = intoRef.current.value;
                if (kind === "source") {
                  const gen = genRef.current.value;
                  addPipe({ kind, gen, type: GENERATORS[gen].type, into, secs });
                } else {
                  const from = fromRef.current.value;
                  if (from === into) return toast("copy needs two different spaces");
                  addPipe({ kind: "sync", from, into, type: typeRef.current.value, secs });
                }
                setPipeDlg(false);
              }}>start</button>
              <button onClick={() => setPipeDlg(false)}>cancel</button>
            </div>
          </div>
        </div>
      )}
    </header>
  );
}
