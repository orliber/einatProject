// What not to forget ahead of a visit (D-207): "לדבר עם הגננת", "להביא את השאלונים". A short
// list she writes when she sets the visit or any time after, and ticks as each thing is done.
// It belongs to this visit only. In discreet mode a list on a screen shows only how many are
// left; the lines themselves are in the visit's dialog.
import { useState } from "react";
import { useApp } from "../../App";
import type { RememberItem } from "../../ipc/generated/RememberItem";
import type { Visit } from "../../ipc/generated/Visit";
import { useSaveVisit } from "./clinicData";
import { formatDateLong, formatDateShort, formatTime } from "./format";

/** The most lines one visit holds, and the longest line (the core's limits). */
export const REMEMBER_MAX = 20;
const LINE_MAX = 200;

/** The lines still not done. */
export function leftToRemember(items: RememberItem[] | undefined): RememberItem[] {
  return (items ?? []).filter((i) => !i.done);
}

/** "לא לשכוח: לדבר עם הגננת · להביא שאלונים", or "2 דברים שלא לשכוח" when discreet. Empty when nothing is left. */
export function rememberLine(items: RememberItem[] | undefined, discreet: boolean): string {
  const left = leftToRemember(items);
  if (left.length === 0) return "";
  if (discreet) return left.length === 1 ? "דבר אחד שלא לשכוח" : `${left.length} דברים שלא לשכוח`;
  return `לא לשכוח: ${left.map((i) => i.text).join(" · ")}`;
}

export function RememberList({ id, items, onChange }: { id: string; items: RememberItem[]; onChange: (items: RememberItem[]) => void }) {
  const [text, setText] = useState("");
  const full = items.length >= REMEMBER_MAX;
  function add() {
    const line = text.replace(/\s+/g, " ").trim();
    if (line === "" || full) return;
    onChange([...items, { text: line, done: false }]);
    setText("");
  }
  return (
    <fieldset className="clinic-group clinic-remember">
      <legend id={`${id}-legend`}>לא לשכוח לקראת הפגישה</legend>
      {items.length > 0 && (
        <ul className="clinic-remember-list">
          {items.map((item, i) => (
            <li key={i} className={item.done ? "clinic-remember-done" : undefined}>
              <input id={`${id}-${i}`} type="checkbox" checked={item.done}
                onChange={(e) => onChange(items.map((x, j) => (j === i ? { ...x, done: e.target.checked } : x)))} />
              <label htmlFor={`${id}-${i}`} className="grow">{item.text}</label>
              <button type="button" className="btn btn-small btn-quiet" aria-label={`הסרה: ${item.text}`}
                onClick={() => onChange(items.filter((_, j) => j !== i))}>הסרה</button>
            </li>
          ))}
        </ul>
      )}
      <div className="row clinic-remember-add">
        <label htmlFor={`${id}-new`} className="visually-hidden">דבר חדש שלא לשכוח</label>
        <input id={`${id}-new`} className="input grow" value={text} maxLength={LINE_MAX} disabled={full}
          placeholder={items.length === 0 ? "למשל: לדבר עם הגננת" : "עוד משהו שלא לשכוח"}
          onChange={(e) => setText(e.target.value)}
          onKeyDown={(e) => {
            if (e.key === "Enter") {
              e.preventDefault();
              add();
            }
          }} />
        <button type="button" className="btn btn-small" disabled={full || text.trim() === ""} onClick={add}>הוספה</button>
      </div>
      {full && <p className="hint">אפשר לרשום עד {REMEMBER_MAX} דברים לפגישה אחת.</p>}
    </fieldset>
  );
}

/** The patient's next planned visit after `after` (a visit, or the day the note is written). */
export function nextPlanned(visits: Visit[], after: { date: string; start: number; id: string | null }): Visit | null {
  return visits
    // A visit of a kind this version does not know cannot be saved here (D-178).
    .filter((v) => v.status === "planned" && v.kind !== "unknown" && v.id !== after.id && (v.date > after.date || (v.date === after.date && v.start > after.start)))
    .sort((a, b) => (a.date === b.date ? a.start - b.start : a.date < b.date ? -1 : 1))[0] ?? null;
}

/**
 * Writing the summary of a session is when she knows what to bring up next time ("לדבר עם
 * הגננת"): a line here goes straight onto the list of the patient's next planned visit (D-207),
 * saved at once, apart from the note.
 */
export function NextVisitRemember({ next }: { next: Visit | null }) {
  const { notify } = useApp();
  const save = useSaveVisit();
  const [text, setText] = useState("");
  const items = next?.remember ?? [];
  const full = items.length >= REMEMBER_MAX;
  async function add() {
    const line = text.replace(/\s+/g, " ").trim();
    if (next === null || line === "" || full || save.isPending) return;
    try {
      await save.mutateAsync({ visit: { ...next, remember: [...items, { text: line, done: false }] }, scope: "one" });
      setText("");
      notify(`נוסף לפגישה של ${formatDateShort(next.date)}`);
    } catch {
      // The failure is already on the screen (as every write shows it); what she typed stays.
    }
  }
  return (
    <fieldset className="clinic-group clinic-remember">
      <legend>לא לשכוח לפגישה הבאה</legend>
      {next === null ? (
        <p className="hint">אין עוד פגישה מתוכננת. מה שלא לשכוח אפשר לרשום כשקובעים את הפגישה הבאה.</p>
      ) : (
        <>
          <p className="hint">
            {`ייכנס לרשימה של הפגישה ב${formatDateLong(next.date)}, ${formatTime(next.start)}, ויופיע ראשון כשתפתחי אותה.`}
          </p>
          {items.length > 0 && (
            <ul className="clinic-remember-list clinic-remember-next">
              {items.map((item, i) => <li key={i} className={item.done ? "clinic-remember-done" : undefined}>{item.text}</li>)}
            </ul>
          )}
          <div className="row clinic-remember-add">
            <label htmlFor="note-next-remember" className="visually-hidden">דבר שלא לשכוח בפגישה הבאה</label>
            <input id="note-next-remember" className="input grow" value={text} maxLength={LINE_MAX} disabled={full}
              placeholder="למשל: לשאול על השינה, לדבר עם הגננת"
              onChange={(e) => setText(e.target.value)}
              onKeyDown={(e) => {
                if (e.key === "Enter") {
                  e.preventDefault();
                  void add();
                }
              }} />
            <button type="button" className="btn btn-small" disabled={full || text.trim() === "" || save.isPending} onClick={() => void add()}>הוספה</button>
          </div>
          {full && <p className="hint">בפגישה הבאה כבר רשומים {REMEMBER_MAX} דברים, וזה המקסימום.</p>}
        </>
      )}
    </fieldset>
  );
}
