// Shared user-facing Hebrew strings. Text used by one screen lives with that screen.
import type { InputKind } from "../ipc/generated/InputKind";
import type { Role } from "../ipc/generated/Role";

export const he = {
  appName: "כספת האבחון",
  encrypted: "מוצפן, שמור במחשב הזה",
  lock: {
    lockedAfterIdle: (minutes: number) => `הכספת ננעלה אחרי ${minutes} דקות ללא פעילות.`,
    password: "סיסמה",
    open: "פתיחה",
    footer:
      "כל המידע מוצפן ושמור רק במחשב הזה. לנו אין עותק ואין סיסמה, ולכן אי אפשר לשחזר אותה בלי ערכת השחזור המודפסת.",
  },
  core: {
    connected: "התוכנה פועלת כראוי",
    disconnected: "התוכנה לא מגיבה. כדאי לסגור ולפתוח אותה",
    checking: "בודק חיבור…",
  },
} as const;

export const roleLabel: Record<Role, string> = {
  child: "הילד/ה",
  mother: "אם",
  father: "אב",
  brother: "אח",
  sister: "אחות",
  teacher: "גננת / מחנכת",
  assistant: "סייעת",
  doctor: "רופא/ה",
  slp: "קלינאית תקשורת",
  psychologist: "פסיכולוג/ית",
  therapist: "מטפל/ת",
  other_child: "ילד/ה אחר/ת",
  kindergarten: "גן / מסגרת",
  school: "בית ספר",
  town: "יישוב",
  institution: "מוסד",
  other: "אחר",
};

/** Roles offered when adding a person to a case (the child is set separately). */
export const personRoles: Role[] = [
  "mother",
  "father",
  "brother",
  "sister",
  "teacher",
  "assistant",
  "doctor",
  "slp",
  "therapist",
  "psychologist",
  "other_child",
  "kindergarten",
  "school",
  "town",
  "institution",
  "other",
];

export const kindLabel: Record<InputKind, string> = {
  intake: "אינטייק",
  prior_report: "דוח של איש מקצוע",
  professional: "שיחה עם איש מקצוע",
  kindergarten: "מסגרת חינוכית",
  test_scores: "תוצאות מבחנים",
  observation: "תצפית",
  session_note: "הסיכום שלי",
  free_text: "הערה",
};

export const kindOrder: InputKind[] = [
  "prior_report",
  "test_scores",
  "intake",
  "kindergarten",
  "professional",
  "observation",
  "session_note",
  "free_text",
];

export function greeting(date = new Date()): string {
  const h = date.getHours();
  if (h < 5) return "לילה טוב";
  if (h < 12) return "בוקר טוב";
  if (h < 17) return "צהריים טובים";
  if (h < 21) return "ערב טוב";
  return "לילה טוב";
}
