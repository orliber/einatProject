// "לקראת הפגישה": what she wants in front of her in the five minutes before a family walks
// in, shown when a visit that is still ahead is opened. What the last session left (its
// record, folded), what is still open to do for this family, and whether money is owed. For a
// first meeting, why the family came. Everything is read from the patient's card, inside the
// vault; nothing is written and nothing leaves. In discreet mode the texts are not shown
// (a family may be in the room): only that they exist, and how many.
import { useDiscreet } from "../../discreet";
import type { PatientCard } from "../../ipc/generated/PatientCard";
import type { SessionNote } from "../../ipc/generated/SessionNote";
import type { Visit } from "../../ipc/generated/Visit";
import { usePatientCard } from "./clinicData";
import { FoldedText } from "./FoldedText";
import { formatDate, formatMoney, VISIT_KIND_LABEL } from "./format";

/** How many open tasks are listed by name; the rest are counted. */
const TASKS_SHOWN = 3;

/** The newest note with a record from before this visit (never this visit's own note). The
 *  card lists its notes newest first. */
export function lastRecord(card: PatientCard, visit: Visit): SessionNote | null {
  return card.notes.find((n) => n.visit_id !== visit.id && n.date <= visit.date && n.record.trim() !== "") ?? null;
}

/** What "לקראת הפגישה" has to say; `null` when there is nothing (the box is not drawn). */
export function prepOf(card: PatientCard, visit: Visit) {
  const last = lastRecord(card, visit);
  const tasks = card.tasks.filter((t) => t.done_on === null);
  const due = card.balance.due;
  // A first meeting: nothing was held with this family before this visit.
  const first = !card.visits.some((v) => v.visit.id !== visit.id && v.visit.status === "held" && v.visit.date <= visit.date);
  const reason = first && last === null ? card.patient.reason.trim() : "";
  if (last === null && tasks.length === 0 && due <= 0 && reason === "") return null;
  return { last, tasks, due, reason };
}

export function VisitPrep({ patientId, visit, today }: { patientId: string; visit: Visit; today: string }) {
  const card = usePatientCard(patientId, today);
  const discreet = useDiscreet();
  // While the card is read (and if it cannot be), the dialog is simply without the box: the
  // visit itself is what she opened.
  if (!card.data) return null;
  const prep = prepOf(card.data, visit);
  if (prep === null) return null;
  const { last, tasks, due, reason } = prep;
  const lastVisit = last?.visit_id == null ? null : card.data.visits.find((v) => v.visit.id === last.visit_id)?.visit ?? null;
  return (
    <section className="clinic-prep" aria-labelledby="visit-prep-title">
      <h3 id="visit-prep-title" className="clinic-prep-title">לקראת הפגישה</h3>
      {reason !== "" && (
        <div className="stack clinic-prep-part">
          <span className="label">פגישה ראשונה עם המשפחה. סיבת הפנייה:</span>
          {discreet ? <p className="muted small clinic-quiet">מוסתרת במצב דיסקרטי.</p> : <p className="pcard-note-text">{reason}</p>}
        </div>
      )}
      {last !== null && (
        <div className="stack clinic-prep-part">
          <span className="label">
            בפעם הקודמת, <span className="num" dir="ltr">{formatDate(last.date)}</span>
            {lastVisit ? ` (${VISIT_KIND_LABEL[lastVisit.kind]})` : " (שיחה)"}
          </span>
          {discreet
            ? <p className="muted small clinic-quiet">הרשומה מוסתרת במצב דיסקרטי.</p>
            : <FoldedText text={last.record.trim()} more="להציג את כל הרשומה" less="לקצר" />}
        </div>
      )}
      {tasks.length > 0 && (
        <div className="stack clinic-prep-part">
          <span className="label">{tasks.length === 1 ? "מעקב פתוח אחד" : `${tasks.length} מעקבים פתוחים`}</span>
          {!discreet && (
            <ul className="clinic-prep-tasks">
              {tasks.slice(0, TASKS_SHOWN).map((t) => (
                <li key={t.id}>{t.title}{t.due !== null && <span className="muted small"> · עד <span className="num" dir="ltr">{formatDate(t.due)}</span></span>}</li>
              ))}
              {tasks.length > TASKS_SHOWN && <li className="muted small">ועוד {tasks.length - TASKS_SHOWN} בכרטיס</li>}
            </ul>
          )}
        </div>
      )}
      {due > 0 && (
        <p className="clinic-quiet clinic-prep-part">יתרה לתשלום: <span className="clinic-money-owed num">{formatMoney(due)}</span></p>
      )}
    </section>
  );
}
