import React, { useEffect, useState } from "react";
import { bootTransport, onNotification, transport } from "../broker/transport.js";
import * as apiMod from "../broker/api.js";
import { board, createDemo, refreshAll, startPolling, toast } from "../state/board.js";
import { startAllPipes, startPipe } from "../state/pipes.js";
import { useBoard, useTransport } from "../hooks.js";
import TopBar from "./TopBar.jsx";
import Board from "./Board.jsx";
import TenantSheet from "./TenantSheet.jsx";
import History from "./History.jsx";
import RequestLog from "./RequestLog.jsx";

export default function App() {
  useBoard();
  useTransport();
  const [health, setHealth] = useState(null);
  const [selectedSpace, setSelectedSpace] = useState("smart-city");
  const [picked, setPicked] = useState(null); // {space,id,attr,emoji,type}
  const [drawerW, setDrawerW] = useState(
    () => Number(localStorage.getItem("antares.drawerw")) || 420,
  );

  useEffect(() => {
    let alive = true;
    (async () => {
      await bootTransport();
      const h = await apiMod.health().catch(() => null);
      if (!alive) return;
      setHealth(h);
      // First visit: the board demos itself — once, pristine boards only.
      if (!localStorage.getItem("antares.demoed")) {
        await refreshAll();
        const pristine =
          !board.pipes.length &&
          ![...board.links.values()].some((ls) => ls.length) &&
          ![...board.ents.values()].some((c) => c.local.length);
        if (pristine) await createDemo({ startPipe });
        localStorage.setItem("antares.demoed", "1");
      }
      await refreshAll();
      startAllPipes();
      startPolling();
    })();
    const off = onNotification((n) => {
      const ids = (n.data ?? []).map((e) => e.id.split(":").pop()).join(", ");
      toast(`🔔 ${ids || n.id || "notification"}`);
    });
    return () => {
      alive = false;
      off();
    };
  }, []);

  const spaceExists = board.spaces.some((s) => s.name === selectedSpace);
  const shownSpace = spaceExists ? selectedSpace : "default";

  return (
    <div className="app">
      <TopBar health={health} />
      <div className="stage">
        <Board
          selected={shownSpace}
          onSelectSpace={(name) => {
            setSelectedSpace(name);
            setPicked(null);
          }}
        />
        <div
          className="drawer-resize"
          title="drag to resize"
          onPointerDown={(e) => {
            e.preventDefault();
            const startX = e.clientX, startW = drawerW;
            const move = (ev) => {
              const w = Math.min(900, Math.max(300, startW + startX - ev.clientX));
              setDrawerW(w);
              localStorage.setItem("antares.drawerw", String(w));
            };
            const up = () => {
              window.removeEventListener("pointermove", move);
              window.removeEventListener("pointerup", up);
            };
            window.addEventListener("pointermove", move);
            window.addEventListener("pointerup", up);
          }}
        />
        <aside className="drawer" data-testid="drawer" style={{ width: drawerW }}>
          <TenantSheet space={shownSpace} picked={picked} onPick={setPicked} />
          {picked && <History key={`${picked.space}/${picked.id}`} {...picked} />}
        </aside>
      </div>
      <div className="toasts">
        {board.toasts.map((t) => (
          <div key={t.id} className="toast">{t.text}</div>
        ))}
      </div>
      <RequestLog />
      {transport.mode !== "opfs-worker" && transport.mode !== "booting" && (
        <div className="banner" title={transport.bootError ?? ""}>
          ⚠ ephemeral store — no persistence in {transport.mode} mode; close the
          tab owning antares.redb and reload
        </div>
      )}
    </div>
  );
}
