import { describe, expect, it } from "vitest";
import { heToIso, isoToHe, todayIso } from "./DateField";

describe("typed dates", () => {
  it("reads the way dates are written in Hebrew", () => {
    expect(heToIso("28.9.2026")).toBe("2026-09-28");
    expect(heToIso("28/09/26")).toBe("2026-09-28");
    expect(heToIso(" 1-2-2025 ")).toBe("2025-02-01");
  });
  it("refuses dates that do not exist", () => {
    expect(heToIso("31.2.2026")).toBeNull();
    expect(heToIso("2026-09-28")).toBeNull();
    expect(heToIso("28.13.2026")).toBeNull();
    expect(heToIso("")).toBeNull();
  });
  it("shows ISO dates day first", () => {
    expect(isoToHe("2026-09-28")).toBe("28.09.2026");
    expect(isoToHe("garbage")).toBe("");
    expect(todayIso()).toMatch(/^\d{4}-\d{2}-\d{2}$/);
  });
});
