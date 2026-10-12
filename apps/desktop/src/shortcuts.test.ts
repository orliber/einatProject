// Keyboard shortcuts go by the physical key, so they work with the Hebrew layout on.
import { KEYS, SHEET, isCombo, isTyping } from "./shortcuts";

type Ev = Parameters<typeof isCombo>[0];
const ev = (over: Partial<Ev>): Ev => ({ code: "", key: "", ctrlKey: false, metaKey: false, shiftKey: false, altKey: false, ...over });

describe("isCombo", () => {
  it("matches by physical key whatever the layout types", () => {
    expect(isCombo(ev({ code: "KeyL", key: "ך", ctrlKey: true }), KEYS.lock)).toBe(true);
    expect(isCombo(ev({ code: "KeyK", key: "ל", ctrlKey: true }), KEYS.changeWithAi)).toBe(true);
  });

  it("⌘ on macOS counts as Ctrl", () => {
    expect(isCombo(ev({ code: "KeyL", key: "l", metaKey: true }), KEYS.lock)).toBe(true);
  });

  it("modifiers must match exactly: an extra or a missing one is a different shortcut", () => {
    expect(isCombo(ev({ code: "KeyL", key: "l" }), KEYS.lock)).toBe(false);
    expect(isCombo(ev({ code: "KeyL", key: "L", ctrlKey: true, shiftKey: true }), KEYS.lock)).toBe(false);
    expect(isCombo(ev({ code: "KeyL", key: "l", ctrlKey: true, altKey: true }), KEYS.lock)).toBe(false);
    // Ctrl+Enter approves one paragraph; Ctrl+Shift+Enter approves the whole draft.
    const enter = ev({ code: "Enter", key: "Enter", ctrlKey: true });
    expect(isCombo(enter, KEYS.approve)).toBe(true);
    expect(isCombo(enter, KEYS.approveDraft)).toBe(false);
    expect(isCombo({ ...enter, shiftKey: true }, KEYS.approveDraft)).toBe(true);
    expect(isCombo({ ...enter, shiftKey: true }, KEYS.approve)).toBe(false);
  });

  it("a different physical key does not match even if it types the same letter", () => {
    // On some layouts another key types "l"; the shortcut is the key, not the letter.
    expect(isCombo(ev({ code: "KeyP", key: "l", ctrlKey: true }), KEYS.lock)).toBe(false);
  });

  it("falls back to the key when the environment gives no code", () => {
    expect(isCombo(ev({ key: "L", ctrlKey: true }), KEYS.lock)).toBe(true);
    expect(isCombo(ev({ key: "F1" }), KEYS.help)).toBe(true);
    expect(isCombo(ev({ key: "ArrowDown", altKey: true }), KEYS.nextSection)).toBe(true);
    expect(isCombo(ev({ key: "x", ctrlKey: true }), KEYS.lock)).toBe(false);
  });
});

describe("isTyping", () => {
  it("is true in fields and editable text, false elsewhere", () => {
    for (const tag of ["input", "textarea", "select"]) expect(isTyping(document.createElement(tag))).toBe(true);
    const editable = document.createElement("div");
    editable.contentEditable = "true";
    // jsdom does not compute isContentEditable from the attribute.
    Object.defineProperty(editable, "isContentEditable", { value: true });
    expect(isTyping(editable)).toBe(true);
    expect(isTyping(document.createElement("button"))).toBe(false);
    expect(isTyping(document.createElement("p"))).toBe(false);
    expect(isTyping(null)).toBe(false);
    expect(isTyping(window)).toBe(false);
  });
});

describe("the shortcuts sheet", () => {
  it("lists every shortcut that the app binds", () => {
    const listed = SHEET.flatMap((g) => g.items.map(([keys]) => keys)).join(" ");
    for (const keys of ["Ctrl+L", "F1", "Ctrl+/", "Alt+↓", "Alt+↑", "Ctrl+Enter", "Ctrl+Shift+Enter", "Ctrl+K", "Ctrl+Shift+H", "Esc"]) {
      expect(listed).toContain(keys);
    }
  });
});
