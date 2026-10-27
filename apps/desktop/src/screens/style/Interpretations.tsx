import { useState } from "react";
import { useQueryClient } from "@tanstack/react-query";
import { useApp } from "../../App";
import { ErrorLine } from "../../components/ui";
import { ipc, type InterpretationView, type StyleOverview } from "../../ipc/client";
import { ConfirmDialog } from "../library/LibraryDialogs";

// הסגנון שלי · interpretation (round 33)

/** How she interprets each subtest's score, learned from her reports; nothing is used before she approves it. */
export function Interpretations({ o, running, onRead }: { o: StyleOverview; running: boolean; onRead: () => void }) {
  const { fail } = useApp();
  const qc = useQueryClient();
  const [editing, setEditing] = useState<string | null>(null);
  const [draft, setDraft] = useState("");
  const [error, setError] = useState<string | null>(null);
  // The item being changed: its buttons wait, so a double click does it once (the review of stage 4).
  const [busyId, setBusyId] = useState<string | null>(null);
  const [deleting, setDeleting] = useState<InterpretationView | null>(null);
  if (o.scored_measures === 0 && o.interpretations.length === 0) {
    return <p className="muted small">בדוחות שלך עוד לא נמצאה תוצאה ליד שם של תת-מבחן.</p>;
  }
  const waiting = o.interpretations.filter((i) => !i.item.approved).length;
  const groups = new Map<string, InterpretationView[]>();
  for (const i of o.interpretations) {
    const key = `${i.instrument_name} · ${i.measure_name}`;
    groups.set(key, [...(groups.get(key) ?? []), i]);
  }

  async function act(id: string, f: () => Promise<unknown>) {
    if (busyId !== null) return;
    setError(null);
    setBusyId(id);
    try {
      await f();
      setEditing(null);
      await qc.invalidateQueries({ queryKey: ["style"] });
    } catch (e) {
      setError(fail(e as never));
    } finally {
      setBusyId(null);
    }
  }
  const save = (id: string) => void act(id, () => ipc.updateInterpretation(id, draft, true, null));

  return (
    <div className="stack style-interpret">
      <p className="hint">
        מהמשפטים בדוחות הישנים שבהם כתבת על תוצאה של תת-מבחן. נשלחים רק המשפטים האלה, בלי פרטים מזהים, ונשמרת פרשנות כללית.
        שום פרשנות לא משמשת בדוח לפני שאישרת אותה.
      </p>
      <div className="row">
        <button type="button" className="btn btn-small" disabled={running || o.scored_measures === 0} onClick={() => { setError(null); onRead(); }}>
          {o.interpretations.length === 0 ? "ללמוד מהדוחות איך פירשתי תוצאות" : "לקרוא שוב את הדוחות"}
        </button>
        {o.scored_measures > 0 && <span className="hint">{o.scored_measures === 1 ? "תת-מבחן אחד" : `${o.scored_measures} תת-מבחנים`} עם תוצאות בדוחות.</span>}
        {waiting > 0 && <span className="chip">{waiting === 1 ? "אחת מחכה" : `${waiting} מחכות`} לאישור</span>}
      </div>
      <ErrorLine error={error} />
      {[...groups.entries()].map(([title, items]) => (
        <div key={title} className="style-group" role="group" aria-label={title}>
          <h3>{title}</h3>
          <ul className="style-items">
            {items.map((view) => {
              const { item } = view;
              const busy = busyId === item.id;
              return (
                <li key={item.id} className={item.enabled ? "style-item" : "style-item style-item-off"}>
                  <span className="chip">{item.case}</span>{" "}
                  {editing === item.id ? (
                    <span className="stack grow">
                      {/* One line: the core keeps a single paragraph of up to 600 characters. */}
                      <textarea className="textarea" aria-label="הפרשנות" value={draft} rows={2} maxLength={600} autoFocus
                        onChange={(e) => setDraft(e.target.value)}
                        onKeyDown={(e) => {
                          if (e.key === "Escape") {
                            e.preventDefault();
                            setEditing(null);
                          } else if (e.key === "Enter" && (e.ctrlKey || e.metaKey)) {
                            e.preventDefault();
                            save(item.id);
                          }
                        }} />
                      <span className="row">
                        <button type="button" className="btn btn-small btn-primary" disabled={busy || !draft.trim()} onClick={() => save(item.id)}>שמירה ואישור</button>
                        <button type="button" className="btn btn-small" onClick={() => setEditing(null)}>ביטול</button>
                      </span>
                    </span>
                  ) : (
                    <span className="stack grow">
                      <span className="style-item-text">{item.text}</span>
                      <span className="row">
                        {!item.approved && <span className="chip chip-warn">מחכה לאישור</span>}
                        {item.support > 1 && <span className="hint">ב-{item.support} דוחות</span>}
                        {!item.approved && <button type="button" className="btn btn-small btn-primary" disabled={busy} onClick={() => void act(item.id, () => ipc.updateInterpretation(item.id, null, true, null))}>אישור</button>}
                        {/* One editor at a time: what she typed in the open one is never dropped silently. */}
                        <button type="button" className="btn btn-small" disabled={busy || editing !== null}
                          title={editing !== null ? "קודם לשמור או לבטל את העריכה הפתוחה" : undefined}
                          onClick={() => { setEditing(item.id); setDraft(item.text); }}>עריכה</button>
                        {item.approved && (
                          <button type="button" className="btn btn-small" disabled={busy} onClick={() => void act(item.id, () => ipc.updateInterpretation(item.id, null, null, !item.enabled))}>
                            {item.enabled ? "כיבוי" : "הדלקה"}
                          </button>
                        )}
                        <button type="button" className="btn btn-small btn-danger-quiet" disabled={busy} onClick={() => setDeleting(view)}>מחיקה</button>
                      </span>
                    </span>
                  )}
                </li>
              );
            })}
          </ul>
        </div>
      ))}
      {deleting && (
        <ConfirmDialog danger title="מחיקת פרשנות" action="מחיקה"
          text={`"${deleting.item.text}" יימחק, וכבר לא ישמש בטיוטות.`}
          onClose={() => setDeleting(null)}
          onConfirm={async () => {
            await ipc.deleteInterpretation(deleting.item.id);
            setDeleting(null);
            setError(null);
            await qc.invalidateQueries({ queryKey: ["style"] });
          }} />
      )}
    </div>
  );
}
