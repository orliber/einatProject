// The parts of Settings, one page each, like the settings of other professional programs.

export type SettingsPane = "general" | "ai" | "privacy" | "report" | "tests" | "backup" | "security" | "about";

export const SETTINGS_PANES: { id: SettingsPane; label: string }[] = [
  { id: "general", label: "כללי" },
  { id: "ai", label: "חיבור ל-AI" },
  { id: "privacy", label: "פרטיות" },
  { id: "report", label: "הדוח" },
  { id: "tests", label: "המבחנים" },
  { id: "backup", label: "גיבוי" },
  { id: "security", label: "סיסמה ואבטחה" },
  { id: "about", label: "עדכונים ועזרה" },
];
