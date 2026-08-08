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
        <aside className="drawer" data-testid="drawer">
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
