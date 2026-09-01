import { beforeEach, describe, expect, it, vi } from "vitest";
import { load, save } from "../src/persist.js";

describe("persisted UI state", () => {
  beforeEach(() => localStorage.clear());

  it("round-trips a value", () => {
    save("k", { a: [1, 2] });
    expect(load("k", null)).toEqual({ a: [1, 2] });
  });

  it("falls back for a key that was never written", () => {
    expect(load("missing", 42)).toBe(42);
  });

  it("falls back for a value another page left that is not JSON", () => {
    localStorage.setItem("k", "{not json");
    expect(load("k", "fallback")).toBe("fallback");
  });

  it("falls back for a stored null rather than handing null on", () => {
    save("k", null);
    expect(load("k", 7)).toBe(7);
  });

  it("falls back when the browser refuses the accessor", () => {
    const spy = vi.spyOn(Storage.prototype, "getItem").mockImplementation(() => {
      throw new DOMException("blocked", "SecurityError");
    });
    expect(load("k", "fallback")).toBe("fallback");
    spy.mockRestore();
  });

  it("a refused write loses the preference, not the page", () => {
    const spy = vi.spyOn(Storage.prototype, "setItem").mockImplementation(() => {
      throw new DOMException("quota", "QuotaExceededError");
    });
    expect(() => save("k", 1)).not.toThrow();
    spy.mockRestore();
  });
});
