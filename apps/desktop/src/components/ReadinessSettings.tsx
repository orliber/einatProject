// "Ready for real cases": what must be true before a real child's material goes in (stage 8).
// It informs and points to where each item is fixed; it never blocks work.
import { useQuery, useQueryClient } from "@tanstack/react-query";
import { useState } from "react";
import { useApp } from "../App";
import { ipc, type Readiness } from "../ipc/client";
import { ErrorLine } from "./ui";
import type { SettingsPane } from "../screens/settingsPanes";

const ITEMS: Record<string, { label: string; hint: string; jump?: SettingsPane }> = {
  zdr: {
    label: "אישור בכתב מספק ה-AI שהמידע לא נשמר אצלו (ZDR)",
    hint: "נוסח הבקשה ל-Anthropic מוכן אצל המפתח.",
  },
  consent_form: {
    label: "טופס הסכמת ההורים מעודכן ומזכיר את ספק ה-AI",
    hint: "",
  },
  score_tables: {
    label: "ענית על הטבלאות של כל אחד מהמבחנים",
    hint: "מאושר, או לא מעבירה את זה.",
    jump: "tests",
  },
  legal: {
    label: "התייעצתי עם יועץ משפטי או יועץ פרטיות",
    hint: "רשימת שאלות מוכנה אצל המפתח.",
  },
  api_key: { label: "מוגדר מפתח API", hint: "", jump: "ai" },
  backup: { label: "יש גיבוי מהשבועיים האחרונים", hint: "", jump: "backup" },
  disk: { label: "הצפנת הדיסק דלוקה", hint: "", jump: "security" },
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

  const r = ready.data;
  const missing = r?.items.filter((i) => !i.done).length ?? 0;
  return (
    <section className="card setting" aria-labelledby="s-ready">
      <h2 id="s-ready">מוכנה לתיקים אמיתיים?</h2>
      {r && (
        <p className={r.all_done ? "" : "warn-text"}>
          {r.all_done
            ? "הכל מוכן."
            : `חסרים ${missing === 1 ? "פריט אחד" : `${missing} פריטים`}. עד אז, רק תיקים בדויים.`}
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
                {(item.hint || i.confirmed_at != null) && (
                  <span className="hint">
                    {item.hint}
                    {i.confirmed_at != null && ` אושר ב-${new Date(i.confirmed_at * 1000).toLocaleDateString("he-IL")}.`}
                  </span>
                )}
                {!i.done && item.jump && (
                  <button type="button" className="link-small align-start" onClick={() => go({ name: "settings", pane: item.jump ?? "general" })}>לתיקון</button>
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
