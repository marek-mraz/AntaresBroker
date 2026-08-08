// TenantSheet: rows derive from the store; filters narrow by text, type and
// origin; every non-local row carries its origin chip (README rule 2).
import React from "react";
import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it } from "vitest";
import { board, emit } from "../src/state/board.js";
import TenantSheet, { filterRows } from "../src/components/TenantSheet.jsx";

const doc = (id, type, attr, value) => ({ id, type, [attr]: { type: "Property", value } });

beforeEach(() => {
  board.spaces = [{ name: "default" }, { name: "smart-city" }, { name: "harbor" }];
  board.fedView = new Set(["smart-city"]);
  board.links = new Map([["smart-city", [{ id: "urn:r:1", to: "harbor", type: undefined }]]]);
  board.ents = new Map([
    ["smart-city", {
      local: [doc("urn:ngsi-ld:Room:a1", "Room", "temperature", 21)],
      remote: [
        doc("urn:ngsi-ld:ParkingSpot:h1", "ParkingSpot", "occupied", 1),
        doc("urn:ngsi-ld:ParkingSpot:h2", "ParkingSpot", "occupied", 0),
      ],
    }],
    ["harbor", {
      local: [
        doc("urn:ngsi-ld:ParkingSpot:h1", "ParkingSpot", "occupied", 1),
        doc("urn:ngsi-ld:ParkingSpot:h2", "ParkingSpot", "occupied", 0),
      ],
      remote: [],
    }],
  ]);
  emit();
});
afterEach(cleanup);

describe("filterRows (pure)", () => {
  const rows = [
    { id: "urn:a", type: "Room", attr: "temperature", origin: "local" },
    { id: "urn:b", type: "ParkingSpot", attr: "occupied", origin: "harbor" },
  ];
  it("filters by type, origin and free text", () => {
    expect(filterRows(rows, { text: "", type: "all", origin: "all" })).toHaveLength(2);
    expect(filterRows(rows, { text: "", type: "Room", origin: "all" })).toHaveLength(1);
    expect(filterRows(rows, { text: "", type: "all", origin: "harbor" })).toHaveLength(1);
    expect(filterRows(rows, { text: "occup", type: "all", origin: "all" })).toHaveLength(1);
    expect(filterRows(rows, { text: "zzz", type: "all", origin: "all" })).toHaveLength(0);
  });
});

describe("<TenantSheet>", () => {
  it("shows local + federated rows with origin chips", () => {
    render(<TenantSheet space="smart-city" picked={null} onPick={() => {}} />);
    const rows = screen.getAllByTestId("sheet-row");
    expect(rows).toHaveLength(3);
    // the two federated ParkingSpots carry their true origin: harbor
    expect(rows.filter((r) => /← .*harbor/.test(r.textContent))).toHaveLength(2);
    expect(rows.filter((r) => r.textContent.includes("🏠 local"))).toHaveLength(1);
  });

  it("type filter narrows the sheet", () => {
    render(<TenantSheet space="smart-city" picked={null} onPick={() => {}} />);
    fireEvent.change(screen.getByTestId("filter-type"), { target: { value: "Room" } });
    const rows = screen.getAllByTestId("sheet-row");
    expect(rows).toHaveLength(1);
    expect(rows[0]).toHaveTextContent("Room");
  });

  it("origin filter isolates one peer", () => {
    render(<TenantSheet space="smart-city" picked={null} onPick={() => {}} />);
    fireEvent.change(screen.getByTestId("filter-origin"), { target: { value: "harbor" } });
    expect(screen.getAllByTestId("sheet-row")).toHaveLength(2);
    fireEvent.change(screen.getByTestId("filter-origin"), { target: { value: "local" } });
    expect(screen.getAllByTestId("sheet-row")).toHaveLength(1);
  });

  it("text filter matches ids", () => {
    render(<TenantSheet space="smart-city" picked={null} onPick={() => {}} />);
    fireEvent.change(screen.getByTestId("filter-text"), { target: { value: "h2" } });
    expect(screen.getAllByTestId("sheet-row")).toHaveLength(1);
  });

  it("name leads with icon, then value, then origin; type and id close the row", () => {
    render(<TenantSheet space="smart-city" picked={null} onPick={() => {}} />);
    const cells = screen.getAllByTestId("sheet-row")[0].querySelectorAll("td");
    expect(cells).toHaveLength(6);
    expect(cells[0]).toHaveTextContent("Room a1");     // name = type + short id
    expect(cells[1]).toHaveTextContent("21");          // current value after the name
    expect(cells[2]).toHaveTextContent("local");       // origin after the value
    expect(cells[4]).toHaveTextContent("Room");        // type at the end
    expect(cells[5]).toHaveTextContent("a1");          // id at the end
  });

  it("row click reports the picked entity for the history panel", () => {
    let picked = null;
    render(<TenantSheet space="smart-city" picked={null} onPick={(p) => (picked = p)} />);
    fireEvent.click(screen.getAllByTestId("sheet-row")[1]);
    expect(picked).toMatchObject({ id: "urn:ngsi-ld:ParkingSpot:h1", attr: "occupied", space: "harbor" });
  });
});
