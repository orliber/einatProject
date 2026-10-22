// Light or dark: by the computer by default, kept on this computer, never breaking when storage is off.
import { THEMES, applyTheme, readTheme } from "./theme";

afterEach(() => {
  vi.restoreAllMocks();
  localStorage.clear();
  document.documentElement.removeAttribute("data-theme");
});

describe("theme", () => {
  it("is by the computer when nothing, or something unexpected, is stored", () => {
    expect(readTheme()).toBe("system");
    for (const bad of ["", "Dark", "blue", "system", "null"]) {
      localStorage.setItem("dv.theme", bad);
      expect(readTheme()).toBe("system");
    }
  });

  it("offers the three choices, by the computer first", () => {
    expect(THEMES.map((t) => t.value)).toEqual(["system", "light", "dark"]);
    expect(THEMES.map((t) => t.label)).toEqual(["לפי המחשב", "בהיר", "כהה"]);
  });

  it("applies a choice to the whole window and reads it back", () => {
    applyTheme("dark");
    expect(document.documentElement.getAttribute("data-theme")).toBe("dark");
    expect(readTheme()).toBe("dark");
    applyTheme("light");
    expect(document.documentElement.getAttribute("data-theme")).toBe("light");
    expect(readTheme()).toBe("light");
  });

  it("by the computer removes the attribute and what was kept", () => {
    applyTheme("dark");
    applyTheme("system");
    expect(document.documentElement.hasAttribute("data-theme")).toBe(false);
    expect(localStorage.getItem("dv.theme")).toBeNull();
    expect(readTheme()).toBe("system");
  });

  it("blocked storage: reading falls back to the computer and applying still changes the window", () => {
    vi.spyOn(Storage.prototype, "getItem").mockImplementation(() => {
      throw new Error("SecurityError");
    });
    vi.spyOn(Storage.prototype, "setItem").mockImplementation(() => {
      throw new Error("QuotaExceededError");
    });
    expect(readTheme()).toBe("system");
    expect(() => applyTheme("dark")).not.toThrow();
    expect(document.documentElement.getAttribute("data-theme")).toBe("dark");
  });
});
