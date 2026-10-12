// Text size: kept on this computer, within a fixed list, and never breaking when storage is off.
import { TEXT_SIZES, applyTextSize, readTextSize, stepTextSize } from "./textSize";

afterEach(() => {
  vi.restoreAllMocks();
  localStorage.clear();
  document.documentElement.style.removeProperty("zoom");
});

describe("text size", () => {
  it("reads 100% when nothing, or something unexpected, is stored", () => {
    expect(readTextSize()).toBe(100);
    for (const bad of ["", "abc", "105", "-1", "1e3", "Infinity"]) {
      localStorage.setItem("dv.textSize", bad);
      expect(readTextSize()).toBe(100);
    }
  });

  it("applies a size to the whole window and reads it back", () => {
    applyTextSize(125);
    expect(document.documentElement.style.getPropertyValue("zoom")).toBe("125%");
    expect(readTextSize()).toBe(125);
    applyTextSize(100);
    expect(document.documentElement.style.getPropertyValue("zoom")).toBe("");
    expect(readTextSize()).toBe(100);
  });

  it("steps within the list and stops at both ends", () => {
    expect(stepTextSize(100, 1)).toBe(110);
    expect(stepTextSize(100, -1)).toBe(90);
    expect(stepTextSize(TEXT_SIZES[0], -1)).toBe(TEXT_SIZES[0]);
    const last = TEXT_SIZES[TEXT_SIZES.length - 1] ?? 0;
    expect(stepTextSize(last, 1)).toBe(last);
    // A size not in the list is treated as 100%.
    expect(stepTextSize(117, 1)).toBe(110);
    expect(stepTextSize(117, -1)).toBe(90);
  });

  it("blocked storage: reading falls back to 100% and applying still changes the window", () => {
    vi.spyOn(Storage.prototype, "getItem").mockImplementation(() => {
      throw new Error("SecurityError");
    });
    vi.spyOn(Storage.prototype, "setItem").mockImplementation(() => {
      throw new Error("QuotaExceededError");
    });
    expect(readTextSize()).toBe(100);
    expect(() => applyTextSize(140)).not.toThrow();
    expect(document.documentElement.style.getPropertyValue("zoom")).toBe("140%");
  });
});
