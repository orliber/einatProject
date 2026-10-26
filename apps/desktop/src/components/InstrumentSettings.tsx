// Settings → המבחנים (round 59, D-098): her answer for each instrument's score tables.
// "מאושר", or "לא מעבירה את זה" (then it is in no request and not in the score table).
// Until she answers, an instrument "מחכה לאישור": it works, for invented cases.
import { useQuery, useQueryClient } from "@tanstack/react-query";
import { useState } from "react";
import { useApp } from "../App";
import { ipc, type Instrument, type InstrumentApproval, type InstrumentStatus } from "../ipc/client";
import type { Measure } from "../ipc/generated/Measure";
import { ageSpan, tableOrder } from "./ScoresView";
import { Segmented } from "./Switch";
import { ErrorLine } from "./ui";

const STATUS_WORDS: Record<InstrumentStatus, string> = {
  awaiting: "מחכה לאישור",
  approved: "מאושר",
  not_used: "לא מעבירה את זה",
};

/** Her two answers; "awaiting" is where every instrument starts, not a choice. */
const CHOICES: { value: Exclude<InstrumentStatus, "awaiting">; label: string }[] = [
  { value: "approved", label: STATUS_WORDS.approved },
  { value: "not_used", label: STATUS_WORDS.not_used },
];

function casesWords(n: number): string {
  return n === 1 ? "בתיק פתוח אחד" : `ב-${n} תיקים פתוחים`;
}

/** The measures that share one table, in the instrument's order: the table is shown once. */
function byTable(instrument: Instrument): { first: Measure; measures: Measure[] }[] {
  const out: { key: string; first: Measure; measures: Measure[] }[] = [];
  for (const m of tableOrder(instrument)) {
    const key = JSON.stringify([m.min, m.max, m.bands]);
    const same = out.find((g) => g.key === key);
    if (same) same.measures.push(m);
    else out.push({ key, first: m, measures: [m] });
  }
  return out;
}

/** What she approves: the edition (in the name), the ages, every measure and its table. */
function InstrumentTable({ instrument }: { instrument: Instrument }) {
  return (
    <details className="instrument-table">
      <summary>הטבלה שבתוכנה</summary>
      <p className="hint">
        <span dir="ltr">{instrument.name}</span> · גילאים {ageSpan(instrument.min_age_months, instrument.max_age_months)}
        {instrument.scale_note_he && ` · ${instrument.scale_note_he}`}
      </p>
      {byTable(instrument).map((g) => (
        <div key={g.first.key} className="instrument-group">
          <p className="instrument-measures">
            {g.measures.map((m) => `${m.name_he} (${m.abbr})`).join(" · ")}
            {` · ציון ${g.first.min}–${g.first.max}`}
          </p>
          <ul className="instrument-bands">
            {g.first.bands.map((b) => (
              <li key={`${b.min}-${b.max}`}>
                <span dir="ltr">{b.min === b.max ? b.min : `${b.min}–${b.max}`}</span>: {b.label}
              </li>
            ))}
          </ul>
        </div>
      ))}
    </details>
  );
}

export function InstrumentSettings() {
  const { fail } = useApp();
  const qc = useQueryClient();
  const list = useQuery({ queryKey: ["instruments"], queryFn: ipc.instrumentApprovals });
  const [error, setError] = useState<string | null>(null);

  async function answer(key: string, status: Exclude<InstrumentStatus, "awaiting">) {
    setError(null);
    try {
      qc.setQueryData<InstrumentApproval[]>(["instruments"], await ipc.setInstrumentStatus(key, status));
      // The readiness line follows her answers.
      await qc.invalidateQueries({ queryKey: ["readiness"] });
    } catch (e) {
      setError(fail(e as never));
    }
  }

  const items = list.data ?? [];
  const waiting = items.filter((a) => a.status === "awaiting" && a.open_cases > 0).length;
  return (
    <section className="card setting" aria-labelledby="s-instruments">
      <h2 id="s-instruments">טבלאות המבחנים</h2>
      <p className="hint">
        הטווחים והאחוזונים מחושבים בתוכנה מטבלאות קבועות. לכל מבחן: מאושר אחרי שעברת על הטבלה שלו,
        או "לא מעבירה את זה", ואז הוא לא מוצע בטבלת הציונים ולא נשלח בשום בקשה.
        עד שתעני, המבחן עובד כרגיל, רק בתיקים בדויים.
      </p>
      {waiting > 0 && (
        <p className="warn-text">
          {waiting === 1 ? "מבחן אחד בתיק פתוח מחכה" : `${waiting} מבחנים בתיקים פתוחים מחכים`} לאישור שלך.
        </p>
      )}
      <ErrorLine error={list.error ? fail(list.error as never) : error} />
      <ul className="instrument-list">
        {items.map((a) => (
          <li key={a.instrument.key} className="instrument-row">
           <div className="set-row">
            <div className="set-row-text">
              <span className="set-row-label" dir="ltr">{a.instrument.name}</span>
              <span className="hint">
                {a.instrument.description_he}
                {a.open_cases > 0 && ` · ${casesWords(a.open_cases)}`}
                {a.status === "awaiting" && ` · ${STATUS_WORDS.awaiting}`}
              </span>
            </div>
            <div className="set-row-control">
              <Segmented<InstrumentStatus>
                label={`${a.instrument.name}: ${STATUS_WORDS[a.status]}`}
                value={a.status}
                options={CHOICES}
                onChange={(s) => {
                  if (s !== "awaiting" && s !== a.status) void answer(a.instrument.key, s);
                }}
              />
            </div>
           </div>
           <InstrumentTable instrument={a.instrument} />
          </li>
        ))}
      </ul>
    </section>
  );
}
