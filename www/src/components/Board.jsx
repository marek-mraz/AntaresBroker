// The context-space graph on React Flow: canvas pans with the mouse, wheel
// zooms, nodes drag, minimap for orientation. Edge motion is EVIDENCE ONLY:
// an edge animates + shows a count exactly while a burst mark is fresh.
import React, { useCallback, useMemo } from "react";
import {
  Background,
  BaseEdge,
  Controls,
  EdgeLabelRenderer,
  Handle,
  MarkerType,
  MiniMap,
  Position,
  ReactFlow,
  useInternalNode,
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

// Sensors are rounded-rect CHIPS (hardware), spaces are circles (places) —
// the shape difference is what keeps the board readable at a glance.
function DeviceNode({ data }) {
  return (
    <div className={`device-node${data.running ? "" : " paused"}`}>
      <Handle type="target" position={Position.Top} className="port" />
      <div className="head">
        <span className="emoji">{data.emoji}</span>
        <span className="ticks" title="accepted writes">{data.ticks}⇢</span>
      </div>
      <div className="dtype">{data.type}</div>
      <Handle type="source" position={Position.Bottom} className="port" />
    </div>
  );
}

const nodeTypes = { space: SpaceNode, device: DeviceNode };

// Center-to-center quadratic edges, trimmed at the bubble borders and aimed
// at the control point — so edges RADIATE from a node instead of piling
// into one handle, and parallel edges (a CSR and a copy between the same
// pair) fan out on distinct curvature slots. Ported from www/'s SVG.
function BubbleEdge({ id, source, target, style, markerEnd, label, labelStyle, data }) {
  const a = useInternalNode(source);
  const b = useInternalNode(target);
  if (!a || !b) return null;
  const ca = {
    x: a.internals.positionAbsolute.x + (a.measured?.width ?? 110) / 2,
    y: a.internals.positionAbsolute.y + (a.measured?.height ?? 110) / 2,
    r: (a.measured?.width ?? 110) / 2,
  };
  const cb = {
    x: b.internals.positionAbsolute.x + (b.measured?.width ?? 110) / 2,
    y: b.internals.positionAbsolute.y + (b.measured?.height ?? 110) / 2,
    r: (b.measured?.width ?? 110) / 2,
  };
  const dx = cb.x - ca.x, dy = cb.y - ca.y;
  const dist = Math.hypot(dx, dy) || 1;
  const px = -dy / dist, py = dx / dist;
  const slot = data?.slot ?? 0;
  const bend = slot * 56 + (slot === 0 ? 0 : Math.sign(slot) * 10);
  const mx = (ca.x + cb.x) / 2 + px * bend;
  const my = (ca.y + cb.y) / 2 + py * bend;
  const aim = (P, r, tx, ty) => {
    const vx = tx - P.x, vy = ty - P.y, vd = Math.hypot(vx, vy) || 1;
    return { x: P.x + (vx / vd) * (r + 4), y: P.y + (vy / vd) * (r + 4) };
  };
  const s = aim(ca, ca.r, mx, my);
  const t = aim(cb, cb.r + 5, mx, my);
  const path = `M ${s.x} ${s.y} Q ${mx} ${my} ${t.x} ${t.y}`;
  // point ON the curve at t=0.5 for the label
  const lx = 0.25 * s.x + 0.5 * mx + 0.25 * t.x;
  const ly = 0.25 * s.y + 0.5 * my + 0.25 * t.y;
  return (
    <>
      <BaseEdge id={id} path={path} style={style} markerEnd={markerEnd} />
      {label && (
        <EdgeLabelRenderer>
          <div
            className="edge-label"
            style={{
              transform: `translate(-50%, -50%) translate(${lx}px, ${ly}px)`,
              color: labelStyle?.fill,
            }}
          >
            {label}
          </div>
        </EdgeLabelRenderer>
      )}
    </>
  );
}
const edgeTypes = { bubble: BubbleEdge };

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
            type: p.type,
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
          type: "bubble",
          // registration/query direction: the registering space points at the
          // peer it reads (smart-city → old-town); the animated dashes flow
          // the same way, and ONLY while a federated read actually runs
          // (burst-gated below) — the return traffic is the `N ⇢` counter.
          source: `s:${from}`,
          target: `s:${l.to}`,
          animated: !!on,
          label: `CSR · ${l.type ?? "all"}${on && b.count ? `  ·  ${b.count} ⇢` : ""}`,
          labelStyle: { fill: FED, fontWeight: 650 },
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
        type: "bubble",
        source: p.kind === "sync" ? `s:${p.from}` : `p:${p.id}`,
        target: `s:${p.into}`,
        animated: !!on,
        label: `⏱ ${p.kind === "sync" ? `${p.type} / ` : ""}${p.secs}s${on && b.count ? `  ·  ${b.count} ⇢` : ""}`,
        labelStyle: { fill: OK },
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
    // parallel edges between the same pair get distinct curvature slots
    const groups = new Map();
    for (const e of out) {
      const k = [e.source, e.target].sort().join("|");
      if (!groups.has(k)) groups.set(k, []);
      groups.get(k).push(e);
    }
    for (const g of groups.values()) {
      g.forEach((e, i) => {
        e.data.slot = i - (g.length - 1) / 2;
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
        key={board.layoutEpoch ?? 0}
        nodes={nodes}
        edges={edges}
        nodeTypes={nodeTypes}
        edgeTypes={edgeTypes}
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
      <div className="legend">
        <span><i className="lg-space" /> context space (tenant)</span>
        <span><i className="lg-device" /> sensor / source</span>
        <span style={{ color: FED }}>┄┄ CSR federation</span>
        <span style={{ color: OK }}>┈┈ pipeline</span>
      </div>
    </div>
  );
}
