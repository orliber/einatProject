// The clinic's tabs, apart from the screen so the app's route can name one without loading
// the clinic's code.

export type ClinicTab = "today" | "calendar" | "patients" | "waiting" | "tasks" | "money" | "insights" | "settings";

export const CLINIC_TABS: { id: ClinicTab; label: string }[] = [
  { id: "today", label: "היום" },
  { id: "calendar", label: "יומן" },
  { id: "patients", label: "מטופלים" },
  { id: "waiting", label: "המתנה" },
  { id: "tasks", label: "מעקבים" },
  { id: "money", label: "כספים" },
  { id: "insights", label: "תמונת מצב" },
  { id: "settings", label: "הגדרות הקליניקה" },
];
