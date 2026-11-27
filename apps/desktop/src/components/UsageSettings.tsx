// What the AI costs this month, and a ceiling that stops sending (numbers only, no text).
import { useState } from "react";
import { useQuery, useQueryClient } from "@tanstack/react-query";
import { useApp } from "../App";
import { ipc } from "../ipc/client";
import { ErrorLine } from "./ui";
import { ConfirmDialog } from "../screens/library/LibraryDialogs";

function dollars(cents: number): string {
  return `$${(cents / 100).toFixed(2)}`;
}

export function UsageSettings() {
  const { notify, fail } = useApp();
  const qc = useQueryClient();
  const usage = useQuery({ queryKey: ["usage"], queryFn: ipc.usageSummary });
  const [cap, setCap] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  /** Emptying the field of a ceiling that is set asks first: without one, nothing stops sending. */
  const [askRemove, setAskRemove] = useState(false);
  const u = usage.data;
  const capText = cap ?? (u?.cap_usd != null ? String(u.cap_usd) : "");

  /** What she typed, read as it is: a slip ("5o", "abc") is said, never read as another number or as no ceiling. */
  function save() {
    setError(null);
    const raw = capText.trim();
    if (raw === "") {
      if (u?.cap_usd != null) setAskRemove(true);
      else void store(null);
      return;
    }
    if (!/^\d+$/.test(raw) || Number(raw) < 1) {
      setError("התקרה היא מספר שלם של דולרים (1 ומעלה), בספרות בלבד. כדי לבטל אותה, מרוקנים את השדה.");
      return;
    }
    void store(Number(raw));
  }

  async function store(value: number | null) {
    try {
      await ipc.setMonthlyCap(value);
      setCap(null);
      await qc.invalidateQueries({ queryKey: ["usage"] });
      notify(value === null ? "התקרה החודשית בוטלה." : `התקרה החודשית נשמרה: $${value}.`);
    } catch (e) {
      setError(fail(e as never));
    }
  }

  const capNote = u?.cap_usd != null ? ` (תקרה: $${u.cap_usd})` : "";

  return (
    <section className="card setting" aria-labelledby="s-usage">
      <h2 id="s-usage">שימוש ועלות</h2>
      {u && (
        <p>
          {u.requests === 0
            ? `החודש עוד לא נשלחו בקשות${capNote}.`
            : `החודש: ${u.requests} בקשות, עלות משוערת ${dollars(u.estimated_cents)}${capNote}.`}
          {u.cap_usd != null && u.estimated_cents >= u.cap_usd * 100 && (
            <strong className="warn-text"> · התקרה הושגה: השליחה עצורה עד שהתקרה תועלה או עד החודש הבא.</strong>
          )}
        </p>
      )}
      {u && u.unpriced_models.length > 0 && (
        <span className="hint">לחלק מהדגמים אין כאן מחיר ({u.unpriced_models.join(", ")}), ולכן הם נספרים רק במספר הבקשות.</span>
      )}
      <div className="field">
        <label htmlFor="cap">תקרה חודשית בדולרים (ריק: בלי תקרה)</label>
        <div className="row">
          <input id="cap" className="input narrow" type="text" inputMode="numeric" dir="ltr" autoComplete="off" value={capText}
            onChange={(e) => setCap(e.target.value)} onKeyDown={(e) => { if (e.key === "Enter") { e.preventDefault(); save(); } }} />
          <button type="button" className="btn" onClick={save}>שמירה</button>
        </div>
        <span className="hint">הערכה לפי המחירון. החשבון המדויק במסוף של ספק ה-AI.</span>
      </div>
      <ErrorLine error={error} />
      {askRemove && u?.cap_usd != null && (
        <ConfirmDialog title="לבטל את התקרה החודשית?" action="לבטל את התקרה"
          text={`עכשיו התקרה היא $${u.cap_usd}. בלי תקרה, השליחה ל-AI לא נעצרת בשום סכום. אפשר להגדיר תקרה חדשה בכל רגע.`}
          // Kept: the field shows the ceiling again, as it is.
          onClose={() => { setAskRemove(false); setCap(null); }}
          onConfirm={async () => { setAskRemove(false); await store(null); }} />
      )}
    </section>
  );
}
