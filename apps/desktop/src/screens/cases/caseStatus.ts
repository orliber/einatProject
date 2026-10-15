// Where a case stands, in one word, one line and three steps (round 47, canvas "תיקים").
// Pure, so the table and the "continue" strip say the same thing and a test can check it.
import type { CaseSummary } from "../../ipc/client";

export const TOTAL_SECTIONS = 16;

/** One of the three steps of a case: not started, in progress, or done. */
export type Step = "empty" | "now" | "done";

export type CaseStatus = {
  /** "בכתיבה", "מוכן להוצאה"… */
  word: string;
  /** What that means right now ("2 מתוך 16 סעיפים אושרו"). */
  detail: string;
  /** Materials, writing, export. */
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
  const n = c.approved_sections.length;
  const writing = `כתיבה: ${n} מתוך ${TOTAL_SECTIONS}`;
  if (c.failed_at) {
    return {
      word: "השליחה נכשלה", detail: "לפתוח ולנסות שוב", attention: true,
      steps: n > 0 ? ["done", "now", "empty"] : ["now", "empty", "empty"],
      full: `חומרים: יש · ${writing} · הוצאה: עוד לא · השליחה האחרונה נכשלה`,
      view: n > 0 ? "report" : "materials", button: "לפתוח",
    };
  }
  if (!c.meta.consent) {
    return {
      word: "חסרה הסכמה", detail: "לרשום את הסכמת ההורים לפני שממשיכים", attention: true,
      steps: ["now", "empty", "empty"], full: "הסכמת ההורים עוד לא נרשמה. בלי הסכמה שום דבר לא נשלח.",
      view: "materials", button: "לפתוח",
    };
  }
  if (n >= TOTAL_SECTIONS) {
    return {
      word: "מוכן להוצאה", detail: "כל הסעיפים אושרו", attention: false,
      steps: ["done", "done", "now"], full: `חומרים: הושלמו · ${writing} · הוצאה: מחכה לך`,
      view: "report", button: "להוציא",
    };
  }
  if (n > 0) {
    return {
      word: "בכתיבה", detail: `${n} מתוך ${TOTAL_SECTIONS} סעיפים אושרו`, attention: false,
      steps: ["done", "now", "empty"], full: `חומרים: הושלמו · ${writing} · הוצאה: עוד לא`,
      view: "report", button: "להמשיך",
    };
  }
  return {
    word: "אוספים חומרים", detail: "עוד אין טיוטה", attention: false,
    steps: ["now", "empty", "empty"], full: `חומרים: בתהליך · ${writing} · הוצאה: עוד לא`,
    view: "materials", button: "לפתוח",
  };
}
