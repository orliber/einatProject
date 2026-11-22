// What not to forget ahead of a visit (D-207): "לדבר עם הגננת", "להביא את השאלונים". A short
// list she writes when she sets the visit or any time after, and ticks as each thing is done.
// It belongs to this visit only. In discreet mode a list on a screen shows only how many are
// left; the lines themselves are in the visit's dialog.
import { useState } from "react";
import type { RememberItem } from "../../ipc/generated/RememberItem";

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
