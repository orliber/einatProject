// The clinic's front page: today's visits, what needs her, what is open to do, and four small
// figures. The reference for the other tabs: data through the hooks of clinicData, names and
// money through parts, quick actions that show first and roll back if the vault refuses.
import { useState } from "react";
import { PlusIcon, Skeleton } from "../../components/ui";
import { clinicApi } from "../../ipc/clinic";
import type { Alert } from "../../ipc/generated/Alert";
import type { ClinicTask } from "../../ipc/generated/ClinicTask";
import type { Overview } from "../../ipc/generated/Overview";
import type { TaskView } from "../../ipc/generated/TaskView";
import type { Visit } from "../../ipc/generated/Visit";
import type { VisitView } from "../../ipc/generated/VisitView";
import { clinicKeys, useOptimistic, useOverview, useReloadClinic, useToday } from "./clinicData";
import { formatDate, formatDateLong, formatMoney, greeting, TASK_KIND_LABEL } from "./format";
import { NoteDialog } from "./NoteDialog";
import { AlertList, EmptyState, PatientName, ReadError, Section, useClinicNav, VisitRow } from "./parts";
import { PatientDialog } from "./PatientDialog";
import { SendMenu } from "./SendMenu";
import { VisitDialog } from "./VisitDialog";

/** What is open over the tab: one dialog at a time. */
type Open =
  | { kind: "visit"; view?: VisitView }
  | { kind: "patient" }
  | { kind: "note"; patientId: string; visit: Visit }
  | null;

const markHeld = (v: VisitView, id: string): VisitView => (v.visit.id === id ? { ...v, visit: { ...v.visit, status: "held" } } : v);

export function TodayTab() {
  const now = useToday();
  const overview = useOverview(now.date);
  const nav = useClinicNav();
  const [open, setOpen] = useState<Open>(null);
  const reload = useReloadClinic();

  // A visit marked "held" shows so at once, on today's list and on the next day's.
  const hold = useOptimistic<Overview, Visit>(
    clinicKeys.overview(now.date),
    (o, visit) => ({ ...o, visits: o.visits.map((v) => markHeld(v, visit.id)), next_visits: o.next_visits.map((v) => markHeld(v, visit.id)) }),
    (visit) => clinicApi.saveVisit({ ...visit, status: "held", status_note: "", charge_missed: false }),
  );
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

  const actionsOf = (view: VisitView) => {
    const v = view.visit;
    if (v.kind === "block" || v.patient_id === null) return null;
    const patientId = v.patient_id;
    // A visit still ahead (the next days' list) has not happened yet: as on the card.
    const due = v.date <= now.date;
    return (
      <>
        {v.status === "planned" && due && <button type="button" className="btn btn-small" onClick={() => void hold(v)}>התקיימה</button>}
        {v.status !== "cancelled" && (
          <button type="button" className="btn btn-small" onClick={() => setOpen({ kind: "note", patientId, visit: v })}>סיכום</button>
        )}
        {v.status === "planned" && <SendMenu kind="reminder" patientId={patientId} visitId={v.id} label="תזכורת" />}
      </>
    );
  };
  const rows = (views: VisitView[]) => (
    <ul className="clinic-list clinic-visits">
      {views.map((view) => <VisitRow key={view.visit.id} view={view} actions={actionsOf(view)} onOpen={() => setOpen({ kind: "visit", view })} />)}
    </ul>
  );
  const visitOf = (id: string) => [...o.visits, ...o.next_visits].find((v) => v.visit.id === id);

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
              <Section title="הפגישות של היום" aside={<span className="muted small">{o.visits.length === 0 ? "" : o.visits.length === 1 ? "פגישה אחת" : `${o.visits.length} פגישות`}</span>}>
                {o.visits.length === 0
                  ? <p className="muted clinic-quiet">אין פגישות היום.</p>
                  : rows(o.visits)}
              </Section>
              {o.next_day !== null && (
                <Section title={`הפגישות הבאות: ${formatDateLong(o.next_day)}`}>
                  {rows(o.next_visits)}
                </Section>
              )}
            </div>

            <div className="stack clinic-column">
              <Section title="מה דורש אותי" aside={o.alerts.length > 0 ? <span className="chip chip-warn">{o.alerts.length}</span> : undefined}>
                <AlertList alerts={o.alerts} onVisit={openAlertVisit} empty="אין כרגע שום דבר שמחכה לטיפול." />
              </Section>
              <Section title="משימות פתוחות" aside={<button type="button" className="link-btn" onClick={() => nav.openTab("tasks")}>לכל המעקבים</button>}>
                {o.tasks.length === 0
                  ? <p className="muted clinic-quiet">אין משימות פתוחות.</p>
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
      {open?.kind === "note" && <NoteDialog patientId={open.patientId} visit={open.visit} onClose={() => setOpen(null)} />}
    </div>
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
