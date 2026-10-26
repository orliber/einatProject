// "Ready for real cases": what must be true before a real child's material goes in (stage 8).
// It informs and points to where each item is fixed; it never blocks work. One card per item
// (D-108): its mark, one line on what it is, and its one button on the same line.
import { useQuery, useQueryClient } from "@tanstack/react-query";
import { useState } from "react";
import { useApp } from "../App";
import { ipc, type Readiness } from "../ipc/client";
import { ErrorLine } from "./ui";
import type { SettingsPane } from "../screens/settingsPanes";

/** Each item: its name, one line on it, and, for what the program checks, where it is fixed. */
const ITEMS: Record<string, { label: string; hint: string; jump?: { pane: SettingsPane; label: string } }> = {
  zdr: {
    label: "אישור בכתב מספק ה-AI שהמידע לא נשמר אצלו (ZDR)",
    hint: "נוסח הבקשה ל-Anthropic מוכן אצל המפתח.",
  },
  consent_form: {
    label: "טופס הסכמת ההורים מעודכן ומזכיר את ספק ה-AI",
    hint: "ההורים יודעים שטקסט, אחרי הסתרת הפרטים, נשלח לניסוח.",
  },
  score_tables: {
    label: "ענית על הטבלאות של כל אחד מהמבחנים",
    hint: "לכל מבחן: \"מאושר\" או \"לא מעבירה את זה\".",
    jump: { pane: "tests", label: "למבחנים" },
  },
  legal: {
    label: "התייעצתי עם יועץ משפטי או יועץ פרטיות",
    hint: "רשימת שאלות מוכנה אצל המפתח.",
  },
  api_key: {
    label: "מוגדר מפתח API",
    hint: "בלי מפתח התוכנה במצב הדגמה ושום דבר לא נשלח.",
    jump: { pane: "ai", label: "להגדרת מפתח" },
  },
  backup: {
    label: "יש גיבוי מהשבוע האחרון",
    hint: "מוצפן, ונעשה אחרי החלפת הסיסמה האחרונה.",
    jump: { pane: "backup", label: "לגיבוי" },
  },
  disk: {
    label: "הצפנת הדיסק דלוקה",
    hint: "מגינה על כל המחשב אם הוא נגנב.",
    jump: { pane: "security", label: "לשכבות ההגנה" },
  },
};

export function ReadinessSettings() {
  const { fail, go } = useApp();
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

  const items = (ready.data?.items ?? []).filter((i) => i.key in ITEMS);
  const done = items.filter((i) => i.done).length;
  const all = ready.data?.all_done ?? false;
  return (
    <section className="card setting" aria-labelledby="s-ready">
      <div className="ready-head">
        <h2 id="s-ready">מוכנה לתיקים אמיתיים?</h2>
        {ready.data && (
          <span className={all ? "ready-count ok" : "ready-count"}>{done} מתוך {items.length} מוכנים</span>
        )}
      </div>
      {ready.data && (
        <>
          <progress className="ready-meter" value={done} max={items.length} aria-hidden="true" />
          <p className={all ? "hint" : "hint warn-text"}>
            {all ? "הכל מוכן." : "עד שהכל מוכן, רק תיקים בדויים."}
          </p>
        </>
      )}
      {ready.isError && <ErrorLine error={fail(ready.error as never)} retry={() => void qc.invalidateQueries({ queryKey: ["readiness"] })} />}
      <ul className="ready-list">
        {items.map((i) => {
          const item = ITEMS[i.key];
          if (!item) return null;
          const line = !i.checked_by_program && i.confirmed_at != null
            ? `אישרת ב-${new Date(i.confirmed_at * 1000).toLocaleDateString("he-IL")}.`
            : item.hint;
          return (
            <li key={i.key} className={i.done ? "ready-item ok" : "ready-item todo"}>
              <span className="ready-mark" aria-hidden="true">{i.done ? "✓" : "!"}</span>
              <div className="ready-text">
                <span className="ready-label">
                  {item.label}
                  <span className="visually-hidden">{i.done ? " (מוכן)" : " (חסר)"}</span>
                </span>
                <span className="hint">{line}</span>
              </div>
              {!i.checked_by_program && !i.done && (
                <button type="button" className="btn btn-small" aria-label={`אישרתי: ${item.label}`}
                  onClick={() => void confirm(i.key, true)}>אישרתי</button>
              )}
              {!i.checked_by_program && i.done && (
                <button type="button" className="link-btn" aria-label={`ביטול האישור: ${item.label}`}
                  onClick={() => void confirm(i.key, false)}>ביטול האישור</button>
              )}
              {i.checked_by_program && !i.done && item.jump && (
                <button type="button" className="btn btn-small"
                  onClick={() => go({ name: "settings", pane: item.jump?.pane ?? "general" })}>{item.jump.label}</button>
              )}
            </li>
          );
        })}
      </ul>
      <ErrorLine error={error} />
    </section>
  );
}
