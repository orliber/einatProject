// Where a case stands: one word, one line, one bar (round 47, canvas "תיקים"; D-203 §2).
// Pure, so the table and the "continue" card say the same thing and a test can check it.
import type { CaseSummary } from "../../ipc/client";

/** The institute's report; the core gives each case its own count (an older case has 15). */
export const TOTAL_SECTIONS = 16;

/** How many sections the case's report has, as its table of contents counts them. */
export function totalSections(c: CaseSummary): number {
  return c.total_sections > 0 ? c.total_sections : TOTAL_SECTIONS;
}

/** Approved sections, as the report counts them (the signature aside). */
export function approvedCount(c: CaseSummary): number {
  return c.approved_sections.filter((k) => k !== "signature").length;
}

/** One of the three steps of a case: not started, in progress, or done. */
export type Step = "empty" | "now" | "done";

export type CaseStatus = {
  /** "בכתיבה", "מוכן להוצאה"… */
  word: string;
  /** What that means right now ("2 מתוך 16 סעיפים אושרו"). */
  detail: string;
  /** Approved sections, as the report counts them: the bar's value. */
  done: number;
  /** The sections of its report (the signature aside): the bar's end. */
  total: number;
  /** Materials, writing, export. The "איפה זה עומד" sort ranks by it. */
  steps: [Step, Step, Step];
  /** The whole picture, for the tooltip. */
  full: string;
  /** Something is wrong (a failed send, a missing consent): the word is shown in the warning color. */
  attention: boolean;
  /** Where to open the case from the list. */
  view: "materials" | "report";
  /** The button text in the row. */
  button: string;
};

export function caseStatus(c: CaseSummary): CaseStatus {
  const n = approvedCount(c);
  const total = totalSections(c);
  const writing = `כתיבה: ${n} מתוך ${total}`;
  if (c.failed_at) {
    return {
      word: "השליחה נכשלה", detail: "לנסות שוב", attention: true, done: n, total,
      steps: n > 0 ? ["done", "now", "empty"] : ["now", "empty", "empty"],
      full: `חומרים: יש · ${writing} · הוצאה: עוד לא · השליחה האחרונה נכשלה`,
      view: n > 0 ? "report" : "materials", button: "לפתוח",
    };
  }
  if (!c.meta.consent) {
    return {
      word: "חסרה הסכמה", detail: "לרשום את הסכמת ההורים לפני שממשיכים", attention: true, done: n, total,
      steps: ["now", "empty", "empty"], full: "הסכמת ההורים עוד לא נרשמה. בלי הסכמה שום דבר לא נשלח.",
      view: "materials", button: "לפתוח",
    };
  }
  if (n >= total) {
    return {
      word: "מוכן להוצאה", detail: "כל הסעיפים אושרו", attention: false, done: n, total,
      steps: ["done", "done", "now"], full: `חומרים: הושלמו · ${writing} · הוצאה: מחכה לך`,
      view: "report", button: "להוציא",
    };
  }
  if (n > 0) {
    return {
      word: "בכתיבה", detail: `${n} מתוך ${total} סעיפים אושרו`, attention: false, done: n, total,
      steps: ["done", "now", "empty"], full: `חומרים: הושלמו · ${writing} · הוצאה: עוד לא`,
      view: "report", button: "להמשיך",
    };
  }
  return {
    word: "אוספים חומרים", detail: "עוד אין טיוטה", attention: false, done: n, total,
    steps: ["now", "empty", "empty"], full: `חומרים: בתהליך · ${writing} · הוצאה: עוד לא`,
    view: "materials", button: "לפתוח",
  };
}
