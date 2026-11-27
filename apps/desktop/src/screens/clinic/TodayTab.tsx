// The clinic's front page: today's visits, what needs her, what is open to do, and four small
// figures. The reference for the other tabs: data through the hooks of clinicData, names and
// money through parts, quick actions that show first and roll back if the vault refuses.
// Each visit offers one step, the one its time asks for, and the rest under "⋯"
// (`VisitActions`, D-204): "התקיימה" once it has begun, "לכתוב סיכום" once it was held,
// "תזכורת" while it is ahead. A line "עכשיו" stands between the visits that began and the
// ones still ahead, and "מה דורש אותי" is grouped by topic (`AlertGroups`).
import { useApp } from "../../App";
import { Fragment, useState } from "react";
import { PlusIcon, Skeleton } from "../../components/ui";
import { clinicApi } from "../../ipc/clinic";
import type { Alert } from "../../ipc/generated/Alert";
import type { ClinicTask } from "../../ipc/generated/ClinicTask";
import type { Overview } from "../../ipc/generated/Overview";
import type { TaskView } from "../../ipc/generated/TaskView";
import type { Visit } from "../../ipc/generated/Visit";
import type { VisitStatus } from "../../ipc/generated/VisitStatus";
import type { VisitView } from "../../ipc/generated/VisitView";
import { clinicKeys, useOptimistic, useOverview, useReloadClinic, useToday } from "./clinicData";
import { formatDate, formatDateLong, formatMoney, formatTime, greeting, nowIndex, ownVisit, TASK_KIND_LABEL, todayCountWords, untilWords } from "./format";
import { DayClose } from "./DayClose";
import { NoteDialog } from "./NoteDialog";
import { AlertGroups, EmptyState, PatientName, ReadError, Section, useClinicNav, VisitRow } from "./parts";
import { PatientDialog } from "./PatientDialog";
import { VisitPaymentDialog } from "./PaymentDialog";
import { MARKED, VisitActions } from "./VisitActions";
import { VisitDialog } from "./VisitDialog";

/** What is open over the tab: one dialog at a time. */
type Open =
  | { kind: "visit"; view?: VisitView }
  | { kind: "patient" }
  | { kind: "note"; patientId: string; visit: Visit; record?: boolean | undefined }
  | { kind: "payment"; patientId: string; visit: Visit }
  | null;

const markAs = (v: VisitView, id: string, status: VisitStatus): VisitView => (v.visit.id === id ? { ...v, visit: { ...v.visit, status } } : v);


export function TodayTab() {
  const now = useToday();
  const overview = useOverview(now.date);
  const nav = useClinicNav();
  const [open, setOpen] = useState<Open>(null);
  const reload = useReloadClinic();
  const { notify } = useApp();

  // A visit marked "held" (or "לא הגיעו") shows so at once, on today's list and on the next
  // day's, and its "פגישה שלא סומנה" leaves "מה דורש אותי". The "בלי סיכום" that follows comes
  // with the next read. "ביטול" in the toast puts the visit back as it was ("planned").
  const mark = useOptimistic<Overview, { visit: Visit; status: VisitStatus }>(
    clinicKeys.overview(now.date),
    (o, { visit, status }) => ({
      ...o,
      visits: o.visits.map((v) => markAs(v, visit.id, status)),
      next_visits: o.next_visits.map((v) => markAs(v, visit.id, status)),
      alerts: status === "planned" ? o.alerts : o.alerts.filter((a) => !(a.kind === "unmarked" && a.visit_id === visit.id)),
    }),
    ({ visit, status }) => clinicApi.saveVisit(status === "planned" ? visit : { ...visit, status, status_note: "", charge_missed: false }),
  );
  /** Marks a visit from its row; the toast offers to take the mark back. */
  async function markVisit(visit: Visit, status: "held" | "no_show") {
    if (!(await mark({ visit, status }))) return;
    notify(MARKED[status], {
      label: "ביטול",
      run: () => void mark({ visit, status: "planned" }).then((ok) => { if (ok) notify("הסימון בוטל"); }),
    });
  }
  const hold = (visit: Visit) => markVisit(visit, "held");
  // "כולן התקיימו" in the day's closing: one visit after the other, so a refusal names the one
  // that stopped it and the ones before it stay marked; the page is read again either way.
  const holdAll = async (visits: Visit[]) => {
    try {
      for (const visit of visits) await clinicApi.saveVisit({ ...visit, status: "held", status_note: "", charge_missed: false });
      notify(`${visits.length} פגישות סומנו "התקיימה"`);
    } finally {
      reload();
    }
  };
  // A task ticked off shows done at once; the next read takes it off the list.
  const finish = useOptimistic<Overview, ClinicTask>(
    clinicKeys.overview(now.date),
    (o, task) => ({ ...o, tasks: o.tasks.map((t) => (t.task.id === task.id ? { ...t, overdue: false, task: { ...t.task, done_on: now.date } } : t)) }),
    (task) => clinicApi.saveTask({ ...task, done_on: now.date }),
  );

  if (overview.error && !overview.data) {
    return <ReadError error={overview.error.message} retry={reload} />;
  }
  if (!overview.data) {
    return (
      <div className="clinic-today" aria-busy="true">
        <Skeleton lines={1} label="טוען את היום" />
        <div className="clinic-columns">
          <Skeleton card lines={5} label="טוען את הפגישות" />
          <Skeleton card lines={4} label="טוען את מה שדורש טיפול" />
        </div>
      </div>
    );
  }

  const o = overview.data;
  const fresh = o.active === 0 && o.waiting === 0 && o.visits.length === 0 && o.next_visits.length === 0 && o.tasks.length === 0;

  // Today's list: "עכשיו" goes between the visits that began and the ones ahead (never above
  // the first or under the last), and the next visit she has says how soon it is, even when
  // no line is drawn. Time she kept for herself and a cancelled visit are not "next".
  const at = nowIndex(o.visits.map((v) => v.visit.start), now.minute);
  const after = at > 0 ? o.visits.length - at : 0;
  const next = o.visits.find((v) => v.visit.start > now.minute && !ownVisit(v.visit) && v.visit.status !== "cancelled");
  const soon = next ? { id: next.visit.id, text: untilWords(next.visit.start - now.minute) } : null;

  /** A list of visits, each with its one step and its "⋯". The key is the visit's, and the line
   *  "עכשיו" sits inside its fragment: a visit keeps its row (and the focus on its button)
   *  when it is marked, and when the line moves past it. */
  const rows = (views: VisitView[], line = -1, ahead: { id: string; text: string } | null = null) => (
    <ul className="clinic-list clinic-visits">
      {views.map((view, i) => {
        const v = view.visit;
        // Time she kept for herself has no step and no "⋯" (and no empty actions box).
        const pid = !ownVisit(v) ? v.patient_id : null;
        return (
          <Fragment key={v.id}>
            {i === line && <NowLine minute={now.minute} />}
            <VisitRow view={view} quietPlanned {...(ahead?.id === v.id ? { soon: ahead.text } : {})}
              onOpen={() => setOpen({ kind: "visit", view })}
              actions={pid !== null
                ? <VisitActions view={view} patientId={pid} now={now} onHold={(x) => void hold(x)} onNoShow={(x) => void markVisit(x, "no_show")}
                    onNote={(x, record) => setOpen({ kind: "note", patientId: pid, visit: x, record })} onOpen={() => setOpen({ kind: "visit", view })}
                    onPay={(x) => setOpen({ kind: "payment", patientId: pid, visit: x })} />
                : undefined} />
          </Fragment>
        );
      })}
    </ul>
  );
  const visitOf = (id: string) => [...o.visits, ...o.next_visits].find((v) => v.visit.id === id);
  /** "התקיימה" on a line of "פגישה שלא סומנה", for a visit of today's list; one of an earlier
   *  day keeps "לפתוח את הפגישה". */
  const holdOf = (id: string) => {
    const view = o.visits.find((x) => x.visit.id === id);
    return view && view.visit.status === "planned" ? () => void hold(view.visit) : null;
  };

  /** The step an alert about a visit offers: its note, or the visit itself. A visit that is
   *  not on this page is on the patient's card. */
  function openAlertVisit(alert: Alert, visitId: string) {
    const view = visitOf(visitId);
    if (view && view.visit.patient_id !== null) {
      setOpen(alert.kind === "no_note" ? { kind: "note", patientId: view.visit.patient_id, visit: view.visit } : { kind: "visit", view });
    } else if (alert.patient_id !== null) {
      nav.openPatient(alert.patient_id);
    }
  }

  return (
    <div className="clinic-today">
      <header className="clinic-today-head">
        <div className="stack clinic-greeting">
          <h2>{greeting(now.minute)}</h2>
          <span className="muted">{formatDateLong(o.date)}</span>
        </div>
        <span className="grow" />
        <button type="button" className="btn" onClick={() => setOpen({ kind: "patient" })}><PlusIcon /> מטופל חדש</button>
        <button type="button" className="btn btn-primary" onClick={() => setOpen({ kind: "visit" })}><PlusIcon /> פגישה חדשה</button>
      </header>

      {fresh ? (
        <EmptyState title="הקליניקה מוכנה, ועוד ריקה"
          actions={<>
            <button type="button" className="btn btn-primary" onClick={() => setOpen({ kind: "patient" })}>1. להוסיף מטופל</button>
            <button type="button" className="btn" onClick={() => setOpen({ kind: "visit" })}>2. לקבוע פגישה</button>
          </>}>
          <p>מתחילים בשני צעדים: מוסיפים את המטופל הראשון, וקובעים לו פגישה. מכאן היומן, הסיכומים והתשלומים מתמלאים מעצמם.</p>
        </EmptyState>
      ) : (
        <>
          <ul className="clinic-figures" aria-label="הקליניקה במספרים">
            <Figure label="פגישות השבוע" value={String(o.week_visits)} onOpen={() => nav.openTab("calendar")} />
            <Figure label="התקבל החודש" value={formatMoney(o.month_received)} onOpen={() => nav.openTab("money")} />
            <Figure label="יתרות פתוחות" value={formatMoney(o.total_due)} tone={o.total_due > 0 ? "owed" : undefined} onOpen={() => nav.openTab("money")} />
            <Figure label="ברשימת ההמתנה" value={String(o.waiting)} onOpen={() => nav.openTab("waiting")} />
          </ul>

          <div className="clinic-columns">
            <div className="stack clinic-column">
              <Section title="הפגישות של היום" aside={<span className="muted small">{todayCountWords(o.visits.length, after)}</span>}>
                {o.visits.length === 0
                  ? <p className="muted clinic-quiet">אין פגישות היום.</p>
                  : rows(o.visits, at, soon)}
              </Section>
              <DayClose overview={o} now={now} onHoldAll={holdAll} />
              {o.next_day !== null && (
                <Section title={`הפגישות הבאות: ${formatDateLong(o.next_day)}`}>
                  {rows(o.next_visits)}
                </Section>
              )}
            </div>

            <div className="stack clinic-column">
              <Section title="מה דורש אותי" aside={o.alerts.length > 0 ? <span className="muted small">{o.alerts.length === 1 ? "דבר אחד" : `${o.alerts.length} דברים`}</span> : undefined}>
                <AlertGroups alerts={o.alerts} onVisit={openAlertVisit} holdOf={holdOf} empty="אין כרגע שום דבר שמחכה לטיפול." />
              </Section>
              <Section title="מעקבים פתוחים" aside={<button type="button" className="link-btn" onClick={() => nav.openTab("tasks")}>לכל המעקבים</button>}>
                {o.tasks.length === 0
                  ? <p className="muted clinic-quiet">אין מעקבים פתוחים.</p>
                  : (
                    <ul className="clinic-list clinic-tasks">
                      {o.tasks.map((t) => <TaskLine key={t.task.id} view={t} onDone={() => void finish(t.task)} />)}
                    </ul>
                  )}
              </Section>
            </div>
          </div>
        </>
      )}

      {open?.kind === "visit" && (
        open.view ? <VisitDialog view={open.view} onClose={() => setOpen(null)} /> : <VisitDialog onClose={() => setOpen(null)} />
      )}
      {open?.kind === "patient" && <PatientDialog onClose={() => setOpen(null)} />}
      {open?.kind === "note" && <NoteDialog patientId={open.patientId} visit={open.visit} record={open.record} onClose={() => setOpen(null)} />}
      {open?.kind === "payment" && <VisitPaymentDialog patientId={open.patientId} visit={open.visit} today={now.date} onClose={() => setOpen(null)} />}
    </div>
  );
}

/** "עכשיו 15:12" with a line across, between two visits of today's list. Plain text in the
 *  list, read in its place: no label of its own and no live region (it moves once a minute). */
function NowLine({ minute }: { minute: number }) {
  return (
    <li className="clinic-now">
      <span className="clinic-now-label">עכשיו <span className="clinic-visit-time" dir="ltr">{formatTime(minute)}</span></span>
      <span className="clinic-now-rule" aria-hidden="true" />
    </li>
  );
}

/** One small figure; pressing it opens the tab that tells the rest. */
function Figure({ label, value, tone, onOpen }: { label: string; value: string; tone?: "owed" | undefined; onOpen: () => void }) {
  return (
    <li>
      <button type="button" className="card clinic-figure" onClick={onOpen}>
        <span className={tone === "owed" ? "clinic-figure-value clinic-money-owed" : "clinic-figure-value"}>{value}</span>
        <span className="muted small">{label}</span>
      </button>
    </li>
  );
}

/** An open task with its checkbox: ticking it is "done today". */
function TaskLine({ view, onDone }: { view: TaskView; onDone: () => void }) {
  const t = view.task;
  const done = t.done_on !== null;
  const id = `today-task-${t.id}`;
  return (
    <li className={done ? "clinic-task clinic-task-done" : "clinic-task"}>
      <input id={id} type="checkbox" checked={done} disabled={done} onChange={onDone} />
      <div className="grow stack clinic-task-body">
        <label htmlFor={id}>{t.title}</label>
        <span className="row muted small clinic-task-more">
          <span className="chip chip-sand">{TASK_KIND_LABEL[t.kind]}</span>
          {t.patient_id !== null && <PatientName name={view.patient_name} id={t.patient_id} />}
          {t.due !== null && <span className={view.overdue ? "clinic-overdue" : undefined}>{view.overdue ? `עד ${formatDate(t.due)}, עבר` : `עד ${formatDate(t.due)}`}</span>}
        </span>
      </div>
    </li>
  );
}
