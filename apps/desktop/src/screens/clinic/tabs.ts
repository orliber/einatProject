// The clinic's tabs, apart from the screen so the app's route can name one without loading
// the clinic's code. The clinic's settings are not a tab: they are a part of the program's
// settings ("הקליניקה", D-205).

export type ClinicTab = "today" | "calendar" | "patients" | "waiting" | "tasks" | "money" | "insights";

export const CLINIC_TABS: { id: ClinicTab; label: string }[] = [
  { id: "today", label: "היום" },
  { id: "calendar", label: "יומן" },
  { id: "patients", label: "מטופלים" },
  { id: "waiting", label: "המתנה" },
  { id: "tasks", label: "מעקבים" },
  { id: "money", label: "כספים" },
  { id: "insights", label: "תמונת מצב" },
];
