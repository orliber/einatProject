// The words of "הסגנון שלי": what each kind of item is called, and dates in words.
import type { StyleKind, StyleSuggestion } from "../../ipc/client";

export const KIND_LABEL: Record<StyleKind, string> = {
  rule: "כללי כתיבה",
  phrase: "ביטויים אופייניים",
  avoid: "מה את לא כותבת",
  template: "תבניות משפט",
  example: "דוגמאות בדויות",
  argument: "מבנה הטיעון",
  integration: "חיבור בין כלים וסעיפים",
  criterion: "ניסוח קריטריונים (DSM-5)",
  inference: "איך את מסיקה: מהממצא למסקנה",
};
export const KIND_HINT: Record<StyleKind, string> = {
  rule: "",
  phrase: "עד 5 מילים.",
  avoid: "",
  template: "במקום התוכן באים סוגריים מסולסלים, למשל {תחום}.",
  example: "פסקאות על ילד בדוי. הן מראות ל-AI את הסגנון, ואין בהן שום דבר מהדוחות.",
  argument: "השלבים של פסקה, לפי הסדר.",
  integration: "",
  criterion: "איך את מנסחת קריטריון, ואיזה סוג ממצא את קושרת אליו.",
  inference: "סוג הממצא ← מה את מסיקה ← איך את מנסחת ← ההמלצה שנובעת. בטיוטה הכלל משמש רק כשיש בחומרים ממצא כזה.",
};
/** The tag of one item in the queue: what kind of thing she is asked about, in one or two words. */
export const KIND_TAG: Record<StyleKind, string> = {
  rule: "כלל כתיבה",
  phrase: "ביטוי שחוזר אצלך",
  avoid: "משהו שאת לא כותבת",
  template: "תבנית משפט",
  example: "דוגמה בדויה",
  argument: "מבנה הטיעון",
  integration: "חיבור בין כלים",
  criterion: "ניסוח קריטריון",
  inference: "איך את מסיקה",
};
export const SUGGESTION_TAG: Record<StyleSuggestion["kind"], string> = {
  replace: "מילה שאת מחליפה",
  avoid: "משהו שאת מוחקת",
  phrase: "ביטוי שחוזר אצלך",
  opening: "איך את פותחת סעיף",
  closing: "איך את מסיימת סעיף",
  formula: "משפט שחוזר אצלך",
};
export const KINDS: StyleKind[] = ["rule", "phrase", "avoid", "template", "inference", "argument", "integration", "criterion", "example"];
export const RECOMMENDED = 3;

export function dateOf(unix: number): string {
  return new Date(unix * 1000).toLocaleDateString("he-IL", { day: "numeric", month: "long", year: "numeric" });
}

/** "היום", "לפני שבוע", "ב-3 במרץ 2026": when the last report was added, in words. */
export function sinceLabel(unix: number, now = Date.now()): string {
  const days = Math.floor((now / 1000 - unix) / 86_400);
  if (days <= 0) return "היום";
  if (days === 1) return "אתמול";
  if (days < 7) return `לפני ${days} ימים`;
  if (days < 14) return "לפני שבוע";
  if (days < 31) return `לפני ${Math.floor(days / 7)} שבועות`;
  return `ב-${dateOf(unix)}`;
}
