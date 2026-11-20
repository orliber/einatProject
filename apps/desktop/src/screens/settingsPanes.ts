// The parts of Settings, one page each, like the settings of other professional programs.

export type SettingsPane = "general" | "ai" | "privacy" | "report" | "tests" | "clinic" | "backup" | "security" | "about";

export const SETTINGS_PANES: { id: SettingsPane; label: string }[] = [
  { id: "general", label: "כללי" },
  { id: "ai", label: "חיבור ל-AI" },
  { id: "privacy", label: "פרטיות" },
  { id: "report", label: "הדוח" },
  { id: "tests", label: "המבחנים" },
  { id: "clinic", label: "הקליניקה" },
  { id: "backup", label: "גיבוי" },
  { id: "security", label: "סיסמה ואבטחה" },
  { id: "about", label: "עדכונים ועזרה" },
];

/** The part she was in last: the gear opens there again while the program is open (memory
 *  only, nothing about a case). The first time, and after the program opens, it is "כללי". */
let lastPane: SettingsPane = "general";

export function lastSettingsPane(): SettingsPane {
  return lastPane;
}

export function rememberSettingsPane(pane: SettingsPane): void {
  lastPane = pane;
}

/** For the tests: each starts on "כללי". */
export function forgetSettingsPane(): void {
  lastPane = "general";
}

/** A new part (or a new small tab) starts at its top: the page was perhaps scrolled far down
 *  in the part before, and the new one would open in its middle. */
export function toTop(): void {
  if (typeof window !== "undefined" && window.scrollY > 0) window.scrollTo({ top: 0 });
}
