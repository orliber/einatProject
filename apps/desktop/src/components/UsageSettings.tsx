// What the AI costs this month, and a ceiling that stops sending (numbers only, no text).
import { useState } from "react";
import { useQuery, useQueryClient } from "@tanstack/react-query";
import { useApp } from "../App";
import { ipc } from "../ipc/client";
import { ErrorLine } from "./ui";

function dollars(cents: number): string {
  return `$${(cents / 100).toFixed(2)}`;
}

export function UsageSettings() {
  const { status, notify, fail } = useApp();
  const qc = useQueryClient();
  const usage = useQuery({ queryKey: ["usage"], queryFn: ipc.usageSummary });
  const [cap, setCap] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  const u = usage.data;
  const capText = cap ?? (u?.cap_usd != null ? String(u.cap_usd) : "");

  async function save() {
    setError(null);
    const value = capText.trim() === "" ? null : Number(capText);
    if (value !== null && (!Number.isInteger(value) || value < 1)) {
      setError("התקרה היא מספר שלם של דולרים (1 ומעלה), או שדה ריק בלי תקרה.");
      return;
    }
    try {
      await ipc.setMonthlyCap(value);
      setCap(null);
      await qc.invalidateQueries({ queryKey: ["usage"] });
      notify(value === null ? "התקרה החודשית בוטלה." : `התקרה החודשית נשמרה: $${value}.`);
    } catch (e) {
      setError(fail(e as never));
    }
  }

  return (
    <section className="card setting" aria-labelledby="s-usage">
      <h2 id="s-usage">שימוש ועלות</h2>
      {u && (
        <p>
          {u.requests === 0
            ? "החודש עוד לא נשלחו בקשות."
            : `החודש: ${u.requests} בקשות, עלות משוערת ${dollars(u.estimated_cents)}`}
          {u.cap_usd != null && ` מתוך תקרה של $${u.cap_usd}`}
          {u.cap_usd != null && u.estimated_cents >= u.cap_usd * 100 && (
            <strong className="warn-text"> · התקרה הושגה: השליחה עצורה עד שהתקרה תועלה או עד החודש הבא.</strong>
          )}
        </p>
      )}
      {u && u.unpriced_models.length > 0 && (
        <span className="hint">לחלק מהדגמים אין כאן מחיר ({u.unpriced_models.join(", ")}), ולכן הם נספרים רק במספר הבקשות.</span>
      )}
      <div className="field">
        <label htmlFor="cap">תקרת הוצאה חודשית (דולרים, ריק = בלי תקרה)</label>
        <div className="row">
          <input id="cap" className="input narrow" type="number" min={1} inputMode="numeric" value={capText}
            onChange={(e) => setCap(e.target.value)} />
          <button type="button" className="btn" onClick={() => void save()}>שמירה</button>
        </div>
        <span className="hint">
          הערכה לפי המחירון הרשמי. החשבון עצמו מופיע במסוף של ספק ה-AI, ושם כדאי להגדיר גם תקרה משלו.
          {status.demo_mode && " במצב הדגמה שום דבר לא נשלח ואין עלות."}
        </span>
      </div>
      <ErrorLine error={error} />
    </section>
  );
}
