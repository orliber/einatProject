import { useState } from "react";

/** Today in local time as "YYYY-MM-DD" (toISOString would give yesterday after midnight in Israel). */
export function todayIso(): string {
  const d = new Date();
  return `${d.getFullYear()}-${String(d.getMonth() + 1).padStart(2, "0")}-${String(d.getDate()).padStart(2, "0")}`;
}

/** "2026-09-28" → "28.09.2026". */
export function isoToHe(iso: string): string {
  const m = /^(\d{4})-(\d{2})-(\d{2})$/.exec(iso);
  return m ? `${m[3]}.${m[2]}.${m[1]}` : "";
}

/** "28.9.2026", "28/09/26", "28-9-2026" → "2026-09-28"; `null` for anything that is not a real date. */
export function heToIso(text: string): string | null {
  const m = /^\s*(\d{1,2})[./-](\d{1,2})[./-](\d{2}|\d{4})\s*$/.exec(text);
  const [, dd, mm, yy] = m ?? [];
  if (!dd || !mm || !yy) return null;
  const day = Number(dd);
  const month = Number(mm);
  const year = yy.length === 2 ? 2000 + Number(yy) : Number(yy);
  const d = new Date(year, month - 1, day);
  if (d.getFullYear() !== year || d.getMonth() !== month - 1 || d.getDate() !== day) return null;
  return `${year}-${String(month).padStart(2, "0")}-${String(day).padStart(2, "0")}`;
}

/**
 * A date typed as day.month.year. The native date picker lays its parts out in a mixed order
 * inside right-to-left text ("2026/28/09"), so dates are typed the way they are written.
 * `value` and `onChange` use ISO dates; an unfinished or impossible date reports "".
 */
export function DateField(props: {
  id: string;
  value: string;
  onChange: (iso: string) => void;
  notAfterToday?: boolean;
}) {
  const [text, setText] = useState(() => isoToHe(props.value));
  const [touched, setTouched] = useState(false);
  const iso = heToIso(text);
  const future = iso !== null && props.notAfterToday === true && iso > todayIso();
  const problem = touched && text.trim() !== "" ? (iso === null ? "תאריך לא תקין. כותבים יום.חודש.שנה" : future ? "התאריך עוד לא הגיע" : null) : null;

  return (
    <span className="date-field">
      <input id={props.id} className={problem ? "input num date-input bad" : "input num date-input"} dir="ltr"
        inputMode="numeric" placeholder="יום.חודש.שנה" value={text} aria-invalid={problem !== null}
        aria-describedby={problem ? `${props.id}-err` : undefined}
        onChange={(e) => {
          setText(e.target.value);
          const next = heToIso(e.target.value);
          props.onChange(next !== null && !(props.notAfterToday === true && next > todayIso()) ? next : "");
        }}
        onBlur={() => setTouched(true)} />
      {problem && <span id={`${props.id}-err`} className="error small">{problem}</span>}
    </span>
  );
}
