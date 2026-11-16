// Follow-ups: what the clinic noticed by itself ("מה דורש אותי", for the whole clinic, by
// kind), and under it her own list: one line to add a follow-up, the open ones by when they
// are due (late first), and the ones that were done, folded away. Ticking one shows at once
// and rolls back if the vault refuses.
import { useMemo, useRef, useState, type FormEvent } from "react";
import { useApp } from "../../App";
import { DateField } from "../../components/DateField";
import { CloseIcon, Dialog, EditIcon, ErrorLine, PlusIcon, Skeleton } from "../../components/ui";
import { clinicApi } from "../../ipc/clinic";
import type { Alert } from "../../ipc/generated/Alert";
import type { AlertKind } from "../../ipc/generated/AlertKind";
import type { ClinicTask } from "../../ipc/generated/ClinicTask";
import type { TaskKind } from "../../ipc/generated/TaskKind";
import type { TaskView } from "../../ipc/generated/TaskView";
import type { Visit } from "../../ipc/generated/Visit";
import type { VisitView } from "../../ipc/generated/VisitView";
import { clinicKeys, useDeleteTask, useOptimistic, useOverview, useReloadClinic, useSaveTask, useTasks, useToday } from "./clinicData";
import { ALERT_KIND_LABEL, formatDate, optionsWith, TASK_KIND_LABEL, TASK_KINDS, weekRange } from "./format";
import { NoteDialog } from "./NoteDialog";
import {
  AlertList, ConfirmDialog, EmptyState, Field, Pager, PatientName, PatientPicker, ReadError, Section, SelectField, TextAreaField, TextField, useClinicNav, usePaged,
} from "./parts";
import { VisitDialog } from "./VisitDialog";
import "./TasksTab.css";

// ------------------------------------------------------------------ pure pieces

/** The kinds of "מה דורש אותי" in the order she would deal with them: rows that were not read
 *  first (every figure is without them, D-178), then what is quick to close, then what takes
 *  a decision. */
const ALERT_ORDER: AlertKind[] = ["unreadable", "unmarked", "no_note", "task_overdue", "feedback_soon", "report_due", "debt", "no_next_visit", "waiting"];

/** How many lines of a kind show before "להציג את כולן". */
const ALERTS_SHOWN = 4;

/** The alerts by kind, each kind keeping the order the clinic gave (oldest first). */
export function alertGroups(alerts: Alert[]): { kind: AlertKind; alerts: Alert[] }[] {
  // A kind that is not in the order still gets its group, after the others: no alert is dropped.
  const kinds = [...ALERT_ORDER, ...alerts.map((a) => a.kind).filter((kind) => !ALERT_ORDER.includes(kind))];
  return [...new Set(kinds)]
    .map((kind) => ({ kind, alerts: alerts.filter((a) => a.kind === kind) }))
    .filter((g) => g.alerts.length > 0);
}

export type TaskGroupId = "late" | "today" | "week" | "later" | "undated";

export const TASK_GROUP_LABEL: Record<TaskGroupId, string> = {
  late: "באיחור",
  today: "היום",
  week: "השבוע",
  later: "בהמשך",
  undated: "בלי תאריך",
};

const GROUP_ORDER: TaskGroupId[] = ["late", "today", "week", "later", "undated"];

/** Where an open follow-up belongs on `today`. The week is hers: Sunday to Saturday. */
export function taskGroupOf(due: string | null, today: string): TaskGroupId {
  if (due === null) return "undated";
  if (due < today) return "late";
  if (due === today) return "today";
  return due <= weekRange(today).to ? "week" : "later";
}

/** The open follow-ups by group (the order inside a group is the clinic's: by date), and the
 *  ones that were done, the latest first. */
export function groupTasks(views: TaskView[], today: string): { open: { id: TaskGroupId; tasks: TaskView[] }[]; done: TaskView[]; openCount: number } {
  const by: Record<TaskGroupId, TaskView[]> = { late: [], today: [], week: [], later: [], undated: [] };
  const done: TaskView[] = [];
  for (const view of views) {
    if (view.task.done_on !== null) done.push(view);
    else by[taskGroupOf(view.task.due, today)].push(view);
  }
  const open = GROUP_ORDER.map((id) => ({ id, tasks: by[id] })).filter((g) => g.tasks.length > 0);
  return { open, done, openCount: views.length - done.length };
}

/** The same day `months` later; the last day of the month when it has no such day. */
export function addMonths(iso: string, months: number): string {
  const [y = 1970, m = 1, d = 1] = iso.split("-").map(Number);
  const first = new Date(Date.UTC(y, m - 1 + months, 1));
  const last = new Date(Date.UTC(first.getUTCFullYear(), first.getUTCMonth() + 1, 0)).getUTCDate();
  const two = (n: number) => String(n).padStart(2, "0");
  return `${first.getUTCFullYear()}-${two(first.getUTCMonth() + 1)}-${two(Math.min(d, last))}`;
}

/** The follow-ups of an assessment clinic, ready to pick: the words, the kind, and for the
 *  one that is about time, when. */
export const SUGGESTIONS: { title: string; kind: TaskKind; months?: number }[] = [
  { title: "לשלוח שאלון להורים", kind: "questionnaire" },
  { title: "לקבל שאלון מהגן", kind: "questionnaire" },
  { title: "לתאם תצפית בגן", kind: "call" },
  { title: "לכתוב את הדוח", kind: "report" },
  { title: "לתאם שיחת משוב", kind: "call" },
  { title: "מעקב אחרי חצי שנה", kind: "follow_up", months: 6 },
];

const KIND_OPTIONS = TASK_KINDS.map((k) => ({ value: k, label: TASK_KIND_LABEL[k] }));

/** The most a title holds (the core's limit for a short field). */
const TITLE_MAX = 200;
const NOTE_MAX = 4000;

type Open =
  | { kind: "edit"; task: ClinicTask }
  | { kind: "delete"; task: ClinicTask }
  | { kind: "visit"; view: VisitView }
  | { kind: "note"; patientId: string; visit: Visit }
  | null;

// ------------------------------------------------------------------ the tab

export function TasksTab() {
  const now = useToday();
  const tasks = useTasks(now.date);
  const overview = useOverview(now.date);
  const reload = useReloadClinic();
  const nav = useClinicNav();
  const { notify } = useApp();
  const remove = useDeleteTask();
  const [open, setOpen] = useState<Open>(null);
  const [showDone, setShowDone] = useState(false);

  // Done or open again, on screen before the vault answers.
  const tick = useOptimistic<TaskView[], { task: ClinicTask; done: boolean }>(
    clinicKeys.tasks(now.date),
    (list, { task, done }) => list.map((t) => (t.task.id === task.id
      ? { ...t, overdue: !done && t.task.due !== null && t.task.due < now.date, task: { ...t.task, done_on: done ? now.date : null } }
      : t)),
    ({ task, done }) => clinicApi.saveTask({ ...task, done_on: done ? now.date : null }),
  );

  const grouped = useMemo(() => groupTasks(tasks.data ?? [], now.date), [tasks.data, now.date]);
  const alerts = overview.data?.alerts;
  const byKind = useMemo(() => alertGroups(alerts ?? []), [alerts]);

  /** An alert about a visit: its note or the visit itself when it is on today's page, else the card. */
  function openAlertVisit(alert: Alert, visitId: string) {
    const o = overview.data;
    const view = o ? [...o.visits, ...o.next_visits].find((v) => v.visit.id === visitId) : undefined;
    if (view && view.visit.patient_id !== null) {
      setOpen(alert.kind === "no_note" ? { kind: "note", patientId: view.visit.patient_id, visit: view.visit } : { kind: "visit", view });
    } else if (alert.patient_id !== null) {
      nav.openPatient(alert.patient_id);
    }
  }

  /** "בוצע" on an overdue follow-up, right on its alert. */
  function finishFromAlert(_: Alert, taskId: string) {
    const view = tasks.data?.find((t) => t.task.id === taskId);
    if (view) void tick({ task: view.task, done: true });
  }

  return (
    <div className="stack clinic-followups">
      <Section title="מה דורש אותי" aside={alerts && alerts.length > 0 ? <span className="chip chip-warn">{alerts.length}</span> : undefined}>
        {overview.error && !overview.data ? <ReadError error={overview.error.message} retry={reload} />
          : !alerts ? <Skeleton lines={4} label="טוען את מה שדורש טיפול" />
          : alerts.length === 0 ? <p className="muted clinic-quiet">אין כרגע שום דבר שמחכה לטיפול בקליניקה.</p>
          : (
            <div className="stack clinic-needs">
              <p className="hint clinic-quiet">את זה התוכנה מזהה לבד, בכל הקליניקה. שורה נעלמת מכאן כשמטפלים במה שהיא מצביעה עליו.</p>
              {byKind.map((g) => <AlertGroup key={g.kind} kind={g.kind} alerts={g.alerts} onVisit={openAlertVisit} onTask={finishFromAlert} />)}
            </div>
          )}
      </Section>

      <Section title="המעקבים שלי" aside={tasks.data && grouped.openCount > 0 ? <span className="muted small">{grouped.openCount === 1 ? "מעקב פתוח אחד" : `${grouped.openCount} מעקבים פתוחים`}</span> : undefined}>
        <QuickAdd today={now.date} />

        {tasks.error && !tasks.data ? <ReadError error={tasks.error.message} retry={reload} />
          : !tasks.data ? <Skeleton lines={5} label="טוען את המעקבים" />
          : tasks.data.length === 0 ? (
            <EmptyState title="עוד אין מעקבים">
              <p>כאן נרשם כל מה שלא רוצים לשכוח: שאלון שצריך לחזור מהגן, טלפון לרופאה, דוח שמחכה לכתיבה. כותבים למעלה בשורה אחת, או בוחרים אחת מההצעות.</p>
            </EmptyState>
          ) : (
            <>
              {grouped.open.length === 0 && <p className="muted clinic-quiet">אין כרגע מעקבים פתוחים.</p>}
              {grouped.open.map((g) => (
                <TaskGroup key={g.id} id={g.id} tasks={g.tasks}
                  onTick={(task) => void tick({ task, done: true })}
                  onEdit={(task) => setOpen({ kind: "edit", task })}
                  onDelete={(task) => setOpen({ kind: "delete", task })} />
              ))}

              {grouped.done.length > 0 && (
                <div className="stack clinic-done">
                  <button type="button" className="link-btn clinic-done-toggle" aria-expanded={showDone} aria-controls="clinic-done-list" onClick={() => setShowDone((on) => !on)}>
                    הושלמו לאחרונה ({grouped.done.length})
                  </button>
                  <div id="clinic-done-list" hidden={!showDone}>
                    {showDone && (
                      <DoneList tasks={grouped.done}
                        onReopen={(task) => void tick({ task, done: false })}
                        onDelete={(task) => setOpen({ kind: "delete", task })} />
                    )}
                  </div>
                </div>
              )}
            </>
          )}
      </Section>

      {open?.kind === "edit" && <TaskDialog task={open.task} onClose={() => setOpen(null)} />}
      {open?.kind === "delete" && (
        <ConfirmDialog title="מחיקת המעקב" action="מחיקה" danger onClose={() => setOpen(null)}
          onConfirm={async () => {
            await remove.mutateAsync(open.task.id);
            notify("המעקב נמחק");
            setOpen(null);
          }}>
          <p>המעקב "{open.task.title}" יימחק מהרשימה. אם הוא פשוט בוצע, אפשר לסמן אותו במקום למחוק.</p>
        </ConfirmDialog>
      )}
      {open?.kind === "visit" && <VisitDialog view={open.view} onClose={() => setOpen(null)} />}
      {open?.kind === "note" && <NoteDialog patientId={open.patientId} visit={open.visit} onClose={() => setOpen(null)} />}
    </div>
  );
}

// ------------------------------------------------------------------ what needs her, by kind

function AlertGroup({ kind, alerts, onVisit, onTask }: {
  kind: AlertKind; alerts: Alert[]; onVisit: (alert: Alert, visitId: string) => void; onTask: (alert: Alert, taskId: string) => void;
}) {
  const [all, setAll] = useState(false);
  const shown = all ? alerts : alerts.slice(0, ALERTS_SHOWN);
  const label = ALERT_KIND_LABEL[kind] ?? "דורש תשומת לב";
  return (
    <section className="stack clinic-need" aria-label={`${label}: ${alerts.length}`}>
      <h3 className="row clinic-group-head">
        <span>{label}</span>
        <span className="chip chip-sand" aria-hidden="true">{alerts.length}</span>
      </h3>
      <AlertList alerts={shown} onVisit={onVisit} onTask={onTask} />
      {alerts.length > ALERTS_SHOWN && (
        <button type="button" className="link-btn clinic-more" aria-expanded={all} onClick={() => setAll((on) => !on)}>
          {all ? "להציג פחות" : `להציג את כל ה-${alerts.length}`}
        </button>
      )}
    </section>
  );
}

// ------------------------------------------------------------------ adding in one line

function QuickAdd({ today }: { today: string }) {
  const { notify } = useApp();
  const save = useSaveTask();
  const [title, setTitle] = useState("");
  const [kind, setKind] = useState<TaskKind>("follow_up");
  const [due, setDue] = useState("");
  /** A new key gives the date box its text again after a suggestion or a save. */
  const [dueKey, setDueKey] = useState(0);
  const [patientId, setPatientId] = useState<string | null>(null);
  const [forChild, setForChild] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const titleBox = useRef<HTMLInputElement>(null);

  function setDate(iso: string) {
    setDue(iso);
    setDueKey((k) => k + 1);
  }

  function suggest(s: (typeof SUGGESTIONS)[number]) {
    setTitle(s.title);
    setKind(s.kind);
    setDate(s.months === undefined ? "" : addMonths(today, s.months));
    setError(null);
    // She goes on from the title: a child, a date, or just Enter.
    titleBox.current?.focus();
  }

  async function submit(e: FormEvent) {
    e.preventDefault();
    if (title.trim() === "") {
      setError("צריך לכתוב מה המעקב.");
      titleBox.current?.focus();
      return;
    }
    setBusy(true);
    setError(null);
    try {
      await save.mutateAsync({ id: "", patient_id: patientId, kind, title, due: due === "" ? null : due, done_on: null, note: "" });
      notify("המעקב נוסף");
      setTitle("");
      setKind("follow_up");
      setDate("");
      setPatientId(null);
      setForChild(false);
      titleBox.current?.focus();
    } catch (err) {
      setError((err as { message?: string }).message ?? "המעקב לא נשמר. אפשר לנסות שוב.");
    } finally {
      setBusy(false);
    }
  }

  return (
    <form className="stack clinic-quick-add" aria-label="מעקב חדש" onSubmit={(e) => void submit(e)}>
      <div className="clinic-quick-line">
        <div className="field grow">
          <label htmlFor="task-new-title">מעקב חדש</label>
          <input id="task-new-title" ref={titleBox} className="input" value={title} maxLength={TITLE_MAX} autoComplete="off"
            placeholder="מה לא לשכוח?" onChange={(e) => setTitle(e.target.value)} />
        </div>
        <SelectField<TaskKind> id="task-new-kind" label="סוג" value={kind} options={KIND_OPTIONS} onChange={setKind} />
        <Field id="task-new-due" label="עד מתי (לא חובה)">
          <DateField key={dueKey} id="task-new-due" value={due} onChange={setDue} />
        </Field>
        <button type="submit" className="btn btn-primary clinic-quick-go" disabled={busy}><PlusIcon /> הוספה</button>
      </div>

      <div className="row clinic-suggest" role="group" aria-label="הצעות למעקב">
        <span className="muted small">הצעות:</span>
        {SUGGESTIONS.map((s) => (
          <button key={s.title} type="button" className="chip clinic-suggest-chip" onClick={() => suggest(s)}>{s.title}</button>
        ))}
      </div>

      {forChild || patientId !== null ? (
        <div className="row clinic-quick-child">
          <PatientPicker id="task-new-patient" value={patientId} onChange={setPatientId} autoFocus={patientId === null} />
          {patientId === null && <button type="button" className="link-btn" onClick={() => setForChild(false)}>בלי ילד</button>}
        </div>
      ) : (
        <button type="button" className="link-btn clinic-add" onClick={() => setForChild(true)}>לשייך לילד או לילדה (לא חובה)</button>
      )}

      <ErrorLine error={error} />
    </form>
  );
}

// ------------------------------------------------------------------ the lists

function TaskGroup({ id, tasks, onTick, onEdit, onDelete }: {
  id: TaskGroupId; tasks: TaskView[]; onTick: (task: ClinicTask) => void; onEdit: (task: ClinicTask) => void; onDelete: (task: ClinicTask) => void;
}) {
  const paged = usePaged(tasks, 20);
  const label = TASK_GROUP_LABEL[id];
  return (
    <section className="stack clinic-task-group" data-group={id} aria-label={`${label}: ${tasks.length}`}>
      <h3 className="row clinic-group-head">
        <span className={id === "late" ? "clinic-overdue" : undefined}>{label}</span>
        <span className={id === "late" ? "chip chip-warn" : "chip chip-sand"} aria-hidden="true">{tasks.length}</span>
      </h3>
      <ul className="clinic-list clinic-tasks">
        {paged.shown.map((view) => (
          <TaskLine key={view.task.id} view={view} onTick={() => onTick(view.task)} onEdit={() => onEdit(view.task)} onDelete={() => onDelete(view.task)} />
        ))}
      </ul>
      <Pager page={paged.page} pages={paged.pages} onPage={paged.setPage} label={`עמודים של ${label}`} />
    </section>
  );
}

function DoneList({ tasks, onReopen, onDelete }: { tasks: TaskView[]; onReopen: (task: ClinicTask) => void; onDelete: (task: ClinicTask) => void }) {
  const paged = usePaged(tasks, 10);
  return (
    <>
      <ul className="clinic-list clinic-tasks" aria-label="מעקבים שהושלמו">
        {paged.shown.map((view) => <TaskLine key={view.task.id} view={view} onTick={() => onReopen(view.task)} onDelete={() => onDelete(view.task)} />)}
      </ul>
      <Pager page={paged.page} pages={paged.pages} onPage={paged.setPage} label="עמודים של מעקבים שהושלמו" />
    </>
  );
}

/** One follow-up: its checkbox (done today, or open again), what it is, whose, until when. */
function TaskLine({ view, onTick, onEdit, onDelete }: { view: TaskView; onTick: () => void; onEdit?: () => void; onDelete: () => void }) {
  const t = view.task;
  const done = t.done_on !== null;
  const id = `task-${t.id}`;
  return (
    <li className={done ? "clinic-task clinic-task-done" : "clinic-task"}>
      <input id={id} type="checkbox" checked={done} onChange={onTick} />
      <div className="grow stack clinic-task-body">
        <label htmlFor={id}>{t.title}</label>
        <span className="row muted small clinic-task-more">
          <span className="chip chip-sand">{TASK_KIND_LABEL[t.kind]}</span>
          {t.patient_id !== null && <PatientName name={view.patient_name} id={t.patient_id} />}
          {done && t.done_on !== null ? <span>בוצע ביום {formatDate(t.done_on)}</span>
            : t.due !== null && <span className={view.overdue ? "clinic-overdue" : undefined}>{view.overdue ? `עד ${formatDate(t.due)}, עבר` : `עד ${formatDate(t.due)}`}</span>}
        </span>
        {t.note !== "" && <span className="small clinic-task-note">{t.note}</span>}
      </div>
      <div className="row clinic-task-actions">
        {onEdit && <button type="button" className="btn btn-small icon-btn" aria-label={`עריכת המעקב: ${t.title}`} onClick={onEdit}><EditIcon /></button>}
        <button type="button" className="btn btn-small icon-btn" aria-label={`מחיקת המעקב: ${t.title}`} onClick={onDelete}><CloseIcon /></button>
      </div>
    </li>
  );
}

// ------------------------------------------------------------------ editing

function TaskDialog({ task, onClose }: { task: ClinicTask; onClose: () => void }) {
  const { notify } = useApp();
  const save = useSaveTask();
  const [title, setTitle] = useState(task.title);
  const [kind, setKind] = useState(task.kind);
  const [due, setDue] = useState(task.due ?? "");
  const [patientId, setPatientId] = useState(task.patient_id);
  const [note, setNote] = useState(task.note);
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const dirty = title !== task.title || kind !== task.kind || due !== (task.due ?? "") || patientId !== task.patient_id || note !== task.note;

  async function submit() {
    if (title.trim() === "") {
      setError("צריך לכתוב מה המעקב.");
      return;
    }
    setBusy(true);
    setError(null);
    try {
      await save.mutateAsync({ ...task, title, kind, due: due === "" ? null : due, patient_id: patientId, note });
      notify("המעקב נשמר");
      onClose();
    } catch (e) {
      setError((e as { message?: string }).message ?? "המעקב לא נשמר. אפשר לנסות שוב.");
      setBusy(false);
    }
  }

  return (
    <Dialog title="עריכת מעקב" dirty={dirty} onClose={onClose}
      footer={<>
        <span className="grow" />
        <button type="button" className="btn" onClick={onClose}>ביטול</button>
        <button type="button" className="btn btn-primary" disabled={busy || !dirty} onClick={() => void submit()}>שמירה</button>
      </>}>
      <div className="stack clinic-form">
        <TextField id="task-edit-title" label="מה המעקב" value={title} maxLength={TITLE_MAX} autoFocus onChange={setTitle} />
        <div className="clinic-form-row">
          <SelectField<TaskKind> id="task-edit-kind" label="סוג" value={kind} options={optionsWith(TASK_KINDS, TASK_KIND_LABEL, kind)} onChange={setKind}
            hint={kind === "unknown" ? "הסוג שנשמר לא מוכר לגרסה הזו של התוכנה. כדי לשמור צריך לבחור סוג מהרשימה." : undefined} />
          <Field id="task-edit-due" label="עד מתי (לא חובה)" hint="ריק: בלי תאריך.">
            <DateField id="task-edit-due" value={due} onChange={setDue} />
          </Field>
        </div>
        <PatientPicker id="task-edit-patient" value={patientId} onChange={setPatientId} />
        <TextAreaField id="task-edit-note" label="הערה (לא חובה)" value={note} rows={3} maxLength={NOTE_MAX} onChange={setNote} />
        <ErrorLine error={error} />
      </div>
    </Dialog>
  );
}
