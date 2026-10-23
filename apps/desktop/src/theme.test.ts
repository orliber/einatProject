// Light or dark: by the computer by default, kept on this computer, never breaking when storage is off.
import { THEMES, applyTheme, readTheme } from "./theme";
import tokens from "./styles/tokens.css?raw";

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

describe("the dark colours", () => {
  // Dark comes two ways (by the computer, or chosen in Settings), each in its own block of
  // tokens.css: the two must never drift apart.
  it("are the same whether the computer or Settings chose dark", () => {
    const block = (selector: string) => {
      const start = tokens.indexOf(`${selector} {`);
      expect(start, selector).toBeGreaterThan(-1);
      const body = tokens.slice(tokens.indexOf("{", start) + 1, tokens.indexOf("}", start));
      return body.split("\n").map((l) => l.trim()).filter((l) => l && !l.startsWith("/*"));
    };
    const bySystem = block(':root:not([data-theme="light"])');
    const chosen = block(':root[data-theme="dark"]');
    expect(bySystem.length).toBeGreaterThan(10);
    expect(chosen).toEqual(bySystem);
  });
});
