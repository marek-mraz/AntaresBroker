// The context-space graph on React Flow: canvas pans with the mouse, wheel
// zooms, nodes drag, minimap for orientation. Edge motion is EVIDENCE ONLY:
// an edge animates + shows a count exactly while a burst mark is fresh.
import React, { useCallback, useMemo } from "react";
import {
  Background,
  Controls,
  Handle,
  MarkerType,
  MiniMap,
  Position,
  ReactFlow,
} from "@xyflow/react";
import { board, emit, save } from "../state/board.js";
import { useBoard } from "../hooks.js";
import { avatarOf, colorOf } from "../model.js";
import { deleteCsr } from "../broker/api.js";
import { refreshSpace } from "../state/board.js";
import { togglePipe } from "../state/pipes.js";

const FED = "#b45fd9";
const OK = "#19a974";

function SpaceNode({ data }) {
  return (
    <div
      className={`space-node${data.selected ? " selected" : ""}`}
      style={{ borderColor: data.color, background: `color-mix(in srgb, ${data.color} 16%, var(--card))` }}
    >
      <Handle type="target" position={Position.Top} className="port" />
      <div className="emoji">{data.emoji}</div>
      <div className="name">{data.name}</div>
      <div className="counts">
        🏠 {data.local}{data.fedOn ? <> · 🌐 {data.fed}</> : null}
      </div>
      <Handle type="source" position={Position.Bottom} className="port" />
    </div>
  );
}

function DeviceNode({ data }) {
  return (
    <div className={`device-node${data.running ? "" : " paused"}`}>
      <Handle type="target" position={Position.Top} className="port" />
      <div className="emoji">{data.emoji}</div>
      <div className="ticks">{data.ticks}</div>
      <Handle type="source" position={Position.Bottom} className="port" />
    </div>
  );
}

const nodeTypes = { space: SpaceNode, device: DeviceNode };

function fallbackPos(key, i) {
  if (!board.positions[key]) {
    const ang = i * 2.399963;
    const rad = 120 + 46 * Math.sqrt(i + 1);
    board.positions[key] = { x: 550 + rad * Math.cos(ang), y: 380 + rad * Math.sin(ang) };
  }
  return board.positions[key];
}

export default function Board({ selected, onSelectSpace }) {
  const v = useBoard();

  const nodes = useMemo(() => {
    const out = board.spaces.map((s, i) => ({
      id: `s:${s.name}`,
      type: "space",
      position: fallbackPos(`s:${s.name}`, i),
      data: {
        name: s.name,
        emoji: avatarOf(s.name),
        color: colorOf(s.name),
        local: board.ents.get(s.name)?.local.length ?? 0,
        fed: board.ents.get(s.name)?.remote.length ?? 0,
        fedOn: board.fedView.has(s.name),
        selected: selected === s.name,
      },
    }));
    board.pipes
      .filter((p) => p.kind === "source")
      .forEach((p, i) => {
        out.push({
          id: `p:${p.id}`,
          type: "device",
          position: fallbackPos(`p:${p.id}`, board.spaces.length + i),
          data: {
            emoji: (p.gen ?? "⚙").slice(0, 2).trim(),
            ticks: p.ticks ?? 0,
            running: p.running,
          },
        });
      });
    return out;
  }, [v, selected]);

  const edges = useMemo(() => {
    const out = [];
    const now = Date.now();
    for (const [from, ls] of board.links) {
      for (const l of ls) {
        const key = `fed:${l.id}`;
        const b = board.bursts.get(key);
        const on = b && b.until > now;
        out.push({
          id: key,
          source: `s:${l.to}`,
          target: `s:${from}`,
          animated: !!on,
          label: `CSR · ${l.type ?? "all"}${on && b.count ? `  ·  ${b.count} ⇢` : ""}`,
          labelStyle: { fill: FED, fontWeight: 650 },
          labelBgStyle: { fill: "var(--card)", fillOpacity: 0.85 },
          style: { stroke: FED, strokeWidth: on ? 3.5 : 2.5, strokeDasharray: "7 6" },
          markerEnd: { type: MarkerType.ArrowClosed, color: FED },
          data: { kind: "fed", space: from, regId: l.id },
        });
      }
    }
    for (const p of board.pipes) {
      const key = `pipe:${p.id}`;
      const b = board.bursts.get(key);
      const on = b && b.until > now;
      out.push({
        id: key,
        source: p.kind === "sync" ? `s:${p.from}` : `p:${p.id}`,
        target: `s:${p.into}`,
        animated: !!on,
        label: `⏱ ${p.kind === "sync" ? `${p.type} / ` : ""}${p.secs}s${on && b.count ? `  ·  ${b.count} ⇢` : ""}`,
        labelStyle: { fill: OK },
        labelBgStyle: { fill: "var(--card)", fillOpacity: 0.85 },
        style: {
          stroke: OK,
          strokeWidth: on ? 3 : 2,
          strokeDasharray: "2 7",
          opacity: p.running ? 1 : 0.3,
        },
        markerEnd: { type: MarkerType.ArrowClosed, color: OK },
        data: { kind: "pipe", pipeId: p.id },
      });
    }
    return out;
  }, [v]);

  const onNodesChange = useCallback((changes) => {
    let moved = false;
    for (const c of changes) {
      if (c.type === "position" && c.position) {
        board.positions[c.id] = { x: c.position.x, y: c.position.y };
        moved = true;
        if (c.dragging === false) save("antares.pos", board.positions);
      }
    }
    if (moved) emit();
  }, []);

  const onNodeClick = useCallback(
    (_, node) => {
      if (node.id.startsWith("s:")) return onSelectSpace(node.id.slice(2));
      const p = board.pipes.find((x) => `p:${x.id}` === node.id);
      if (p) onSelectSpace(p.into);
    },
    [onSelectSpace],
  );

  const onEdgeClick = useCallback(async (_, edge) => {
    if (edge.data?.kind === "pipe") return togglePipe(edge.data.pipeId);
    if (edge.data?.kind === "fed" && window.confirm("Delete this Context Source Registration?")) {
      await deleteCsr(edge.data.space, edge.data.regId);
      await refreshSpace(edge.data.space);
      emit();
    }
  }, []);

  return (
    <div className="board" data-testid="board">
      <ReactFlow
        nodes={nodes}
        edges={edges}
        nodeTypes={nodeTypes}
        onNodesChange={onNodesChange}
        onNodeClick={onNodeClick}
        onEdgeClick={onEdgeClick}
        fitView
        minZoom={0.2}
        maxZoom={2}
        proOptions={{ hideAttribution: true }}
      >
        <Background gap={24} />
        <Controls />
        <MiniMap pannable zoomable nodeColor={(n) => (n.type === "space" ? colorOf(n.data.name) : OK)} />
      </ReactFlow>
    </div>
  );
}
