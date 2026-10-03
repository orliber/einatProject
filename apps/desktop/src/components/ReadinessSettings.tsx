// "Ready for real cases": what must be true before a real child's material goes in (stage 8).
// It informs and points to where each item is fixed; it never blocks work.
import { useQuery, useQueryClient } from "@tanstack/react-query";
import { useState } from "react";
import { useApp } from "../App";
import { ipc, type Readiness } from "../ipc/client";
import { ErrorLine } from "./ui";

const ITEMS: Record<string, { label: string; hint: string; jump?: string }> = {
  zdr: {
    label: "יש אישור בכתב מספק ה-AI שהמידע לא נשמר אצלו (ZDR)",
    hint: "בלי אישור כזה, הספק שומר את מה שנשלח לתקופה מסוימת. נוסח הבקשה ל-Anthropic מוכן אצל המפתח.",
  },
  consent_form: {
    label: "טופס הסכמת ההורים לשימוש ב-AI מעודכן, ומזכיר את הספק שנבחר",
    hint: "ההסכמה נרשמת בכל תיק לפני השליחה הראשונה.",
  },
  score_tables: {
    label: "עברתי על טבלאות הציונים ואישרתי את תיאורי הטווחים",
    hint: "התוכנה מפרשת ציונים לפי הטבלאות האלה, ו-AI מקבל את הטקסט המוכן.",
  },
  legal: {
    label: "התייעצתי עם יועץ משפטי או יועץ פרטיות על השימוש בתוכנה",
    hint: "חוק הגנת הפרטיות (תיקון 13) וחובת הסודיות המקצועית. רשימת שאלות מוכנה אצל המפתח.",
  },
  api_key: { label: "מוגדר מפתח API לחשבון של AI", hint: "בלי מפתח התוכנה במצב הדגמה.", jump: "s-claude" },
  backup: { label: "יש גיבוי מוצפן מהשבועיים האחרונים", hint: "מומלץ לשמור אותו על דיסק נייד.", jump: "s-backup" },
  disk: { label: "הצפנת הדיסק של המחשב (BitLocker) דלוקה", hint: "מגינה גם על קבצים אחרים אם המחשב נגנב.", jump: "s-sec" },
};

function jumpTo(id: string) {
  document.getElementById(id)?.closest("section")?.scrollIntoView({ block: "start", behavior: "smooth" });
}

export function ReadinessSettings() {
  const { fail } = useApp();
  const qc = useQueryClient();
  const ready = useQuery({ queryKey: ["readiness"], queryFn: ipc.readiness });
  const [error, setError] = useState<string | null>(null);

  async function confirm(key: string, done: boolean) {
    setError(null);
    try {
      qc.setQueryData<Readiness>(["readiness"], await ipc.confirmReadiness(key, done));
    } catch (e) {
      setError(fail(e as never));
    }
  }

  const r = ready.data;
  const missing = r?.items.filter((i) => !i.done).length ?? 0;
  return (
    <section className="card setting" aria-labelledby="s-ready">
      <h2 id="s-ready">מוכנה לעבודה עם תיקים אמיתיים?</h2>
      {r && (
        <p className={r.all_done ? "" : "warn-text"}>
          {r.all_done
            ? "הכל מוכן. אפשר לעבוד עם תיקים אמיתיים."
            : `חסרים עוד ${missing === 1 ? "פריט אחד" : `${missing} פריטים`} לפני שמכניסים מידע אמיתי על ילדים. עד אז מומלץ לעבוד רק עם תיקים בדויים.`}
        </p>
      )}
      <ul className="ready-list">
        {r?.items.map((i) => {
          const item = ITEMS[i.key];
          if (!item) return null;
          return (
            <li key={i.key} className={i.done ? "ok" : "todo"}>
              {i.checked_by_program ? (
                <span className="ready-mark" aria-label={i.done ? "תקין" : "חסר"}>{i.done ? "✓" : "!"}</span>
              ) : (
                <input type="checkbox" id={`ready-${i.key}`} checked={i.done}
                  onChange={(e) => void confirm(i.key, e.target.checked)} />
              )}
              <div className="stack">
                <label htmlFor={i.checked_by_program ? undefined : `ready-${i.key}`}>{item.label}</label>
                <span className="hint">
                  {item.hint}
                  {i.confirmed_at != null && ` אושר ב-${new Date(i.confirmed_at * 1000).toLocaleDateString("he-IL")}.`}
                </span>
                {!i.done && item.jump && (
                  <button type="button" className="link-small align-start" onClick={() => jumpTo(item.jump ?? "")}>לתיקון</button>
                )}
              </div>
            </li>
          );
        })}
      </ul>
      <ErrorLine error={error} />
    </section>
  );
}
