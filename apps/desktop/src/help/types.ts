// The help of each screen and window (D-206). Static copy written in code, in her words: never
// data, names or model output, and always shown as text (never HTML). Nothing here reaches the
// network: the pictures are files inside the program, taken from the preview's invented cases.

/** Every screen and window that has its own help. */
export const HELP_IDS = [
  // כתיבת הדוחות
  "cases", "case-details", "case-materials", "case-scores", "case-report", "case-finish", "consult", "style",
  "new-case", "import", "material", "review", "export", "report-run", "sent-history", "case-search", "style-import",
  // ניהול הקליניקה
  "clinic-today", "clinic-calendar", "clinic-patients", "clinic-patient", "clinic-waiting", "clinic-tasks",
  "clinic-money", "clinic-insights",
  "visit", "patient", "visit-note", "pathway", "payment", "task",
  // הגדרות
  "settings-general", "settings-ai", "settings-privacy", "settings-report", "settings-tests", "settings-clinic",
  "settings-backup", "settings-security", "settings-about",
  "password",
] as const;

export type HelpId = (typeof HELP_IDS)[number];

/** Where a topic belongs, for the list of all topics. */
export type HelpGroup = "reports" | "clinic" | "settings";

export interface HelpTopic {
  id: HelpId;
  group: HelpGroup;
  /** The screen's or window's name as she sees it: "רשימת התיקים". */
  title: string;
  /** One or two sentences: what this place is for. */
  intro: string;
  /** A picture of the place, by its file name in `shots/` without ".webp" (invented data only). */
  shot?: string;
  /** What the picture shows, for a screen reader and when it does not load. */
  shotAlt?: string;
  /** "מה אפשר לעשות כאן": a short name for each action, and how. */
  can: { title: string; text: string }[];
  /** Short tips that save time or worry. */
  tips?: string[];
  /** Questions she may ask, with short answers. */
  faq?: { q: string; a: string }[];
  /** Topics worth reading next. */
  related?: HelpId[];
}
