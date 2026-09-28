import { useState } from "react";
import { DateField, todayIso } from "./DateField";
import type { Age } from "../ipc/generated/Age";

/** Age on `on` of a child born on `birth` (ISO dates); `null` if the dates are out of order. */
export function ageFrom(birth: string, on: string): Age | null {
  const [by, bm, bd] = birth.split("-").map(Number);
  const [oy, om, od] = on.split("-").map(Number);
  if (!by || !bm || !bd || !oy || !om || !od) return null;
  let months = (oy - by) * 12 + (om - bm);
  if (od < bd) months -= 1;
  if (months < 0) return null;
  return { years: Math.floor(months / 12), months: months % 12 };
}

/** "5 שנים ו-4 חודשים", "שנה ו-3 חודשים", "4 שנים". */
export function ageWords(age: Age): string {
  const y = age.years === 0 ? "" : age.years === 1 ? "שנה" : age.years === 2 ? "שנתיים" : `${age.years} שנים`;
  const m = age.months === 0 ? "" : age.months === 1 ? "חודש" : age.months === 2 ? "חודשיים" : `${age.months} חודשים`;
  if (y && m) return `${y} ו${m.startsWith("ח") ? "" : "-"}${m}`;
  return y || m || "פחות מחודש";
}

/** Years and months stay strings while typed; this is the age they make, if any. */
export function parseAge(years: string, months: string): Age | null {
  if (years === "") return null;
  const y = Number(years);
  const m = months === "" ? 0 : Number(months);
  return y >= 0 && y < 25 && m >= 0 && m < 12 ? { years: y, months: m } : null;
}

/**
 * Age at assessment, typed as years and months (each with its own label, so the order can't be
 * misread in right-to-left text), or computed from a birth date. The birth date is not kept.
 */
export function AgeField(props: { idPrefix: string; years: string; months: string; onChange: (years: string, months: string) => void }) {
  const [calc, setCalc] = useState(false);
  const [birth, setBirth] = useState("");
  const [on, setOn] = useState(todayIso());
  const age = parseAge(props.years, props.months);
  const monthsBad = props.months !== "" && Number(props.months) > 11;
  const computed = birth && on ? ageFrom(birth, on) : null;
  const y = `${props.idPrefix}-years`;
  const m = `${props.idPrefix}-months`;

  return (
    <fieldset className="field plain age-field">
      <legend className="label">גיל בעת האבחון</legend>
      <div className="age-row">
        <span className="age-part">
          <input id={y} className="input num age" inputMode="numeric" value={props.years}
            onChange={(e) => props.onChange(e.target.value.replace(/\D/g, "").slice(0, 2), props.months)} />
          <label htmlFor={y}>שנים</label>
        </span>
        <span className="age-part">
          <input id={m} className={monthsBad ? "input num age bad" : "input num age"} inputMode="numeric" value={props.months}
            aria-invalid={monthsBad} onChange={(e) => props.onChange(props.years, e.target.value.replace(/\D/g, "").slice(0, 2))} />
          <label htmlFor={m}>חודשים</label>
        </span>
        <span className="age-words" aria-live="polite">
          {monthsBad ? <span className="error">חודשים: 0 עד 11</span> : age ? <>= {ageWords(age)} <span className="muted">({age.years}:{age.months})</span></> : null}
        </span>
      </div>
      {!calc ? (
        <button type="button" className="link-btn small" onClick={() => setCalc(true)}>חישוב מתאריך לידה</button>
      ) : (
        <div className="age-calc">
          <div className="field">
            <label htmlFor={`${props.idPrefix}-birth`}>תאריך לידה</label>
            <DateField id={`${props.idPrefix}-birth`} value="" onChange={setBirth} notAfterToday />
          </div>
          <div className="field">
            <label htmlFor={`${props.idPrefix}-on`}>תאריך האבחון</label>
            <DateField id={`${props.idPrefix}-on`} value={on} onChange={setOn} />
          </div>
          <div className="age-calc-result">
            {computed ? (
              <button type="button" className="btn btn-small btn-primary"
                onClick={() => { props.onChange(String(computed.years), String(computed.months)); setCalc(false); setBirth(""); }}>
                שימוש בגיל {computed.years}:{computed.months}
              </button>
            ) : (
              <span className="small muted">{birth && on ? "תאריך האבחון לפני תאריך הלידה" : " "}</span>
            )}
          </div>
          <p className="hint age-calc-note">תאריך הלידה משמש רק לחישוב ולא נשמר. בתיק נשמר הגיל בלבד.</p>
        </div>
      )}
    </fieldset>
  );
}
