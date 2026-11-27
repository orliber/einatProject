// "סגירת היום": once the last visit of the day is over, the front page says in one quiet box
// whether the day is closed. Every visit marked, every session that was held summarized, what
// was received today, and what tomorrow (the next day with visits) starts with. When all is
// done it says so in a line, so she can lock the program and go home without wondering what
// she forgot. The same facts are in "מה דורש אותי", mixed with older days; this box is about
// today only. Each line that is still open is done from its visit's row, above; when two or
// more visits were left unmarked, "כולן התקיימו" marks them in one step, after a window that
// lists them (the usual end of a full day: she marks nothing until the last family leaves).
// The box itself holds no names, so it reads the same in discreet mode.
import { useState, type ReactNode } from "react";
import type { Overview } from "../../ipc/generated/Overview";
import type { Today } from "../../ipc/generated/Today";
import type { Visit } from "../../ipc/generated/Visit";
import type { VisitView } from "../../ipc/generated/VisitView";
import { useDiary } from "./clinicData";
import { formatDateLong, formatMoney, formatTime, ownVisit, visitEnded, VISIT_KIND_LABEL } from "./format";
import { ConfirmDialog, PatientName, Section, useClinicNav } from "./parts";
import { visitCharge } from "./PaymentDialog";

export interface DayCloseFacts {
  /** Visits with a family today that were not cancelled. */
  sessions: number;
  /** Of them, still marked "מתוכננת". */
  unmarked: number;
  /** Held today, without a session note. */
  noNote: number;
  /** The first visit of the next day that has any, and how many it has. */
  next: { date: string; count: number; start: number } | null;
}

/** What the day's closing says; `null` while the day is not over (a visit with a family is
 *  still ahead or under way), or when no family was seen today. */
export function dayClose(o: Overview, now: Today): DayCloseFacts | null {
  const mine = o.visits.filter((v) => !ownVisit(v.visit) && v.visit.patient_id !== null && v.visit.status !== "cancelled");
  if (mine.length === 0 || mine.some((v) => !visitEnded(v.visit, now))) return null;
  const ahead = o.next_visits.filter((v) => v.visit.status !== "cancelled");
  const first = ahead[0];
  return {
    sessions: mine.length,
    unmarked: mine.filter((v) => v.visit.status === "planned").length,
    noNote: mine.filter((v) => v.visit.status === "held" && !v.has_note).length,
    next: o.next_day !== null && first !== undefined ? { date: o.next_day, count: ahead.length, start: Math.min(...ahead.map((v) => v.visit.start)) } : null,
  };
}

/** Today's visits with a family that are over and still "מתוכננת", in the order of the day. */
export function unmarkedToday(o: Overview, now: Today): VisitView[] {
  return o.visits.filter((v) => !ownVisit(v.visit) && v.visit.patient_id !== null && v.visit.status === "planned" && visitEnded(v.visit, now));
}

const count = (n: number, one: string, many: string) => (n === 1 ? one : `${n} ${many}`);

export function DayClose({ overview, now, onHoldAll }: {
  overview: Overview; now: Today;
  /** Marks these visits "התקיימה", one after the other; throws on the first the vault refuses. */
  onHoldAll?: (visits: Visit[]) => Promise<void>;
}) {
  const facts = dayClose(overview, now);
  const [asking, setAsking] = useState(false);
  // The month's diary is read only once the day is over.
  const diary = useDiary(now.date.slice(0, 7), facts !== null);
  const nav = useClinicNav();
  if (facts === null) return null;
  const received = diary.data?.days.find((d) => d.date === now.date)?.received ?? (diary.data ? 0 : null);
  const closed = facts.unmarked === 0 && facts.noNote === 0;
  const left = unmarkedToday(overview, now);
  // Families may pay later, so a day without a payment can still close; but it is not said to
  // be "all recorded" in silence: the line says how many visits charged and nothing came in.
  const charged = overview.visits.filter((v) => !ownVisit(v.visit) && v.visit.patient_id !== null && visitCharge(v.visit) > 0).length;
  const unpaid = received === 0 && charged > 0;
  return (
    <Section title="סגירת היום" aside={<span className={closed ? "chip chip-ok" : "chip chip-sand"}>{closed ? "היום סגור" : "נשאר לסגור"}</span>}>
      <ul className="clinic-list clinic-dayclose">
        <Line done={facts.unmarked === 0}
          text={facts.unmarked === 0 ? count(facts.sessions, "הפגישה של היום סומנה", "הפגישות של היום סומנו") : count(facts.unmarked, "פגישה אחת עוד לא סומנה", "פגישות עוד לא סומנו")}>
          {onHoldAll && left.length >= 2 && <button type="button" className="btn btn-small" onClick={() => setAsking(true)}>כולן התקיימו</button>}
        </Line>
        <Line done={facts.noNote === 0}
          text={facts.noNote === 0 ? "אין פגישה שהתקיימה היום בלי סיכום" : count(facts.noNote, "פגישה אחת שהתקיימה היום בלי סיכום", "פגישות שהתקיימו היום בלי סיכום")} />
        {received !== null && (
          <li className={unpaid ? "clinic-dayclose-line clinic-dayclose-open" : "clinic-dayclose-line"}>
            <span className="clinic-dayclose-mark" aria-hidden="true">₪</span>
            <span className="grow">
              {received > 0 ? <>התקבל היום: <span className="num">{formatMoney(received)}</span></>
                : unpaid ? count(charged, "פגישה אחת חויבה היום, ולא נרשם תקבול", "פגישות חויבו היום, ולא נרשם תקבול")
                : "לא נרשם תקבול היום"}
            </span>
            <button type="button" className="link-btn" onClick={() => nav.openTab("money")}>ליומן ההכנסות</button>
          </li>
        )}
        {facts.next !== null && (
          <li className="clinic-dayclose-line">
            <span className="clinic-dayclose-mark" aria-hidden="true">←</span>
            <span className="grow">
              {formatDateLong(facts.next.date)}: {count(facts.next.count, "פגישה אחת", "פגישות")}, {facts.next.count === 1 ? "בשעה" : "הראשונה בשעה"}{" "}
              <span className="num" dir="ltr">{formatTime(facts.next.start)}</span>
            </span>
          </li>
        )}
      </ul>
      {closed && (
        <p className="muted clinic-quiet clinic-dayclose-end">
          {unpaid ? "הפגישות סומנו והסיכומים שמורים בכספת. תקבול שיגיע אחר כך נרשם ביומן ההכנסות. אפשר לנעול ולסיים את היום."
            : "הפגישות סומנו והסיכומים שמורים בכספת. אפשר לנעול ולסיים את היום."}
        </p>
      )}
      {asking && onHoldAll && (
        <ConfirmDialog title="לסמן את הפגישות של היום" action={`לסמן ${left.length} פגישות כ"התקיימה"`} onClose={() => setAsking(false)}
          onConfirm={async () => {
            await onHoldAll(left.map((v) => v.visit));
            setAsking(false);
          }}>
          <p>הפגישות האלה יסומנו "התקיימה", וכל אחת תחויב במחיר שלה:</p>
          <ul className="clinic-list clinic-dayclose-all" aria-label="הפגישות שיסומנו">
            {left.map((v) => (
              <li key={v.visit.id} className="row">
                <span className="num" dir="ltr">{formatTime(v.visit.start)}</span>
                <PatientName name={v.patient_name ?? ""} strong />
                <span className="muted">{VISIT_KIND_LABEL[v.visit.kind]}</span>
              </li>
            ))}
          </ul>
          <p className="muted">פגישה שלא הגיעו אליה: סוגרים את החלון ומסמנים אותה קודם ב"לא הגיעו", בתפריט "⋯" שבשורה שלה. פגישה שבוטלה: פותחים אותה ומסמנים בה "בוטלה". סימון שנעשה בטעות אפשר לתקן אחר כך מהפגישה.</p>
        </ConfirmDialog>
      )}
    </Section>
  );
}

/** One line: done ("✓") or still open ("○"); the words say which, the mark only repeats it. */
function Line({ done, text, children }: { done: boolean; text: string; children?: ReactNode }) {
  return (
    <li className={done ? "clinic-dayclose-line clinic-dayclose-done" : "clinic-dayclose-line clinic-dayclose-open"}>
      <span className="clinic-dayclose-mark" aria-hidden="true">{done ? "✓" : "○"}</span>
      <span className="grow">{text}</span>
      {children}
    </li>
  );
}
