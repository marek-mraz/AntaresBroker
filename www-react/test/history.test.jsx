// History panel: value line + change (Δ) bars from the temporal API.
import React from "react";
import { cleanup, render, screen } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";

vi.mock("../src/broker/api.js", () => ({ attrHistory: vi.fn() }));
import { attrHistory } from "../src/broker/api.js";
import History, { deltas } from "../src/components/History.jsx";

const P = (at, value) => ({ at, value });
const props = { space: "s", viewer: "s", id: "urn:x", attr: "v", emoji: "🌡", type: "T" };

afterEach(() => {
  cleanup();
  vi.clearAllMocks();
});

describe("deltas (pure)", () => {
  it("computes n-1 signed changes", () => {
    expect(deltas([P(1, 1), P(2, 3), P(3, 3), P(4, 2)])).toEqual([
      { at: 2, delta: 2 },
      { at: 3, delta: 0 },
      { at: 4, delta: -1 },
    ]);
  });
  it("fewer than two points → no changes", () => {
    expect(deltas([])).toEqual([]);
    expect(deltas([P(1, 5)])).toEqual([]);
  });
});

describe("<History>", () => {
  it("draws the value line and one Δ bar per change, signed", async () => {
    attrHistory.mockResolvedValue([P(1000, 1), P(2000, 3), P(3000, 3), P(4000, 2)]);
    render(<History {...props} />);
    expect(await screen.findByTestId("values-chart")).toBeInTheDocument();
    expect(screen.getByTestId("values-chart").querySelector("path")).toBeInTheDocument();
    const bars = screen.getAllByTestId("delta-bar");
    expect(bars).toHaveLength(3);
    expect(bars.map((b) => b.dataset.sign)).toEqual(["up", "zero", "down"]);
    expect(screen.getByText(/4 instances/)).toBeInTheDocument();
    expect(screen.getByText(/3 changes/)).toBeInTheDocument();
  });

  it("single instance → value dot, no changes chart", async () => {
    attrHistory.mockResolvedValue([P(1000, 7)]);
    render(<History {...props} />);
    expect(await screen.findByTestId("values-chart")).toBeInTheDocument();
    expect(screen.queryByTestId("changes-chart")).not.toBeInTheDocument();
    expect(screen.getByText(/1 instance ·/)).toBeInTheDocument();
  });

  it("no history yet → hint, no charts", async () => {
    attrHistory.mockResolvedValue([]);
    render(<History {...props} />);
    expect(await screen.findByText(/no temporal history yet/)).toBeInTheDocument();
    expect(screen.queryByTestId("values-chart")).not.toBeInTheDocument();
  });
});
