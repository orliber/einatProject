// Keyboard shortcuts (UX-5). Matched by the physical key (`code`), so they work the same
// with the Hebrew keyboard layout on: Ctrl+L there types "ך", but the key is still KeyL.

export interface Combo {
  ctrl?: boolean;
  shift?: boolean;
  alt?: boolean;
  /** `KeyboardEvent.code`, e.g. "KeyL", "Enter", "ArrowDown", "F1". */
  code: string;
}

/** True when the event is exactly this combination (Ctrl or ⌘ count as one). */
export function isCombo(e: Pick<KeyboardEvent, "code" | "key" | "ctrlKey" | "metaKey" | "shiftKey" | "altKey">, c: Combo): boolean {
  const ctrl = e.ctrlKey || e.metaKey;
  if (ctrl !== Boolean(c.ctrl) || e.shiftKey !== Boolean(c.shift) || e.altKey !== Boolean(c.alt)) return false;
  if (e.code) return e.code === c.code;
  // Some test environments and older webviews leave `code` empty: fall back to the key.
  return e.key.toLowerCase() === c.code.replace(/^Key/, "").toLowerCase() || e.key === c.code;
}

/** While typing in a field, single-key shortcuts must not take the letters. */
export function isTyping(target: EventTarget | null): boolean {
  if (!(target instanceof HTMLElement)) return false;
  return target.isContentEditable || ["INPUT", "TEXTAREA", "SELECT"].includes(target.tagName);
}

/** The key the sheet names for Ctrl: on a Mac the same shortcuts are on ⌘. */
export const MOD = typeof navigator !== "undefined" && /Mac|iPhone|iPad/.test(navigator.platform) ? "⌘" : "Ctrl";

export const KEYS = {
  lock: { ctrl: true, code: "KeyL" },
  help: { code: "F1" },
  helpAlt: { ctrl: true, code: "Slash" },
  nextSection: { alt: true, code: "ArrowDown" },
  prevSection: { alt: true, code: "ArrowUp" },
  approve: { ctrl: true, code: "Enter" },
  approveDraft: { ctrl: true, shift: true, code: "Enter" },
  changeWithAi: { ctrl: true, code: "KeyK" },
  versions: { ctrl: true, shift: true, code: "KeyH" },
} satisfies Record<string, Combo>;

/** The help sheet: what each key does, in her words. Shown in the order she meets them. */
export const SHEET: { group: string; items: [string, string][] }[] = [
  {
    group: "בכל מקום",
    items: [
      [`${MOD}+L`, "נעילה מיידית של הכספת"],
      [`F1 או ${MOD}+/`, "הדף הזה: קיצורי המקלדת"],
      [`${MOD} + / ${MOD} − / ${MOD} 0`, "הגדלה, הקטנה וגודל רגיל של הטקסט"],
      ["Tab / Shift+Tab", "מעבר בין כפתורים ושדות"],
      ["Esc", "סגירת חלון או ביטול עריכה"],
    ],
  },
  {
    group: "בכתיבת הדוח",
    items: [
      ["Alt+↓ / Alt+↑", "לסעיף הבא / לסעיף הקודם"],
      ["Enter על כותרת הסעיף", "אישור הטיוטה ומעבר לטיוטה הבאה"],
      [`${MOD}+Enter על כותרת הסעיף`, "לשנות את הטיוטה (Esc סוגר)"],
      ["Enter על פסקה", "עריכת הפסקה שמסומנת"],
      [`${MOD}+Enter`, "בפסקה שממתינה: אישור. בעריכה: שמירה. בשיחה: שליחה"],
      [`${MOD}+Shift+Enter`, "אישור כל הטיוטה של הסעיף"],
      [`${MOD}+K`, "לשנות את הפסקה שמסומנת עם AI"],
      [`${MOD}+Shift+H`, "גרסאות קודמות של הפסקה שמסומנת"],
    ],
  },
];
