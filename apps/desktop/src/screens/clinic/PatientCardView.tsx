// A patient's card: the child's story at a glance. Who they are and how to reach the family,
// where the assessment stands, what needs her, the diagnostic case, the visits, the session
// notes, the money and the follow-ups; and, quietly at the bottom, erasing the card.
// Everything shown is plain text. Every name goes through `PatientName` / `useShownName`, so
// discreet mode holds; in discreet mode phone numbers and personal notes are not shown at all.
import { useId, useMemo, useState, type ReactNode } from "react";
import { useQueryClient } from "@tanstack/react-query";
import { useApp } from "../../App";
import { ageOf } from "../../components/AgeField";
import { DateField } from "../../components/DateField";
import { Dialog, ErrorLine, PlusIcon, Skeleton } from "../../components/ui";
import { asUiError, ipc } from "../../ipc/client";
import { clinicApi } from "../../ipc/clinic";
import type { Alert } from "../../ipc/generated/Alert";
import type { CaseLink } from "../../ipc/generated/CaseLink";
import type { Charge } from "../../ipc/generated/Charge";
import type { ClinicTask } from "../../ipc/generated/ClinicTask";
import type { Patient } from "../../ipc/generated/Patient";
import type { PatientCard } from "../../ipc/generated/PatientCard";
import type { PayMethod } from "../../ipc/generated/PayMethod";
import type { Payment } from "../../ipc/generated/Payment";
import type { ProcessStep } from "../../ipc/generated/ProcessStep";
import type { SessionNote } from "../../ipc/generated/SessionNote";
import type { StepState } from "../../ipc/generated/StepState";
import type { TaskKind } from "../../ipc/generated/TaskKind";
import type { Visit } from "../../ipc/generated/Visit";
import type { VisitStatus } from "../../ipc/generated/VisitStatus";
import type { VisitView } from "../../ipc/generated/VisitView";
import {
  clinicKeys, useCaseChoices, useCreateCase, useLinkCase, useOptimistic, usePatientCard, useReloadClinic, useSaveCharge, useSavePatient,
  useSavePayment, useSaveTask, useStarterTasks, useToday, useVoidCharge, useVoidPayment,
} from "./clinicData";
import {
  CONTACT_ROLE_LABEL, formatDate, optionsWith, parseMoney, PATIENT_STATUS_LABEL, PAY_METHOD_LABEL, PAY_METHODS, TASK_KIND_LABEL, TASK_KINDS,
  VISIT_KIND_LABEL, VISIT_STATUS_LABEL,
} from "./format";
import { NoteDialog } from "./NoteDialog";
import {
  AlertList, ConfirmDialog, EmptyState, Field, Money, MoneyField, moneyText, Pager, PatientName, ReadError, RecordHistory, Section, SelectField,
  TextAreaField, TextField, useDiscreet, usePaged, useShownName, VisitRow,
} from "./parts";
import { PathwayDialog } from "./PathwayDialog";
import { PatientDialog } from "./PatientDialog";
import { VisitDialog } from "./VisitDialog";
import { SendMenu } from "./SendMenu";
import "./PatientCardView.css";

/** What is open over the card: one dialog at a time. */
type Open =
  | { kind: "visit"; view?: VisitView }
  | { kind: "note"; visit?: Visit; noteId?: string }
  | { kind: "edit" }
  | { kind: "close" }
  | { kind: "erase" }
  | { kind: "pathway" }
  | null;

/** When the protected clipboard lets go of a copy (the app says how long). */
function clearsIn(seconds: number): string {
  return seconds === 60 ? "יימחק מהלוח בעוד דקה" : `יימחק מהלוח בעוד ${seconds} שניות`;
}

const messageOf = (e: unknown, fallback: string) => (e as { message?: string }).message ?? fallback;

/** A name as it is compared before erasing: without spaces around it or doubled inside it. */
const cleanName = (s: string) => s.trim().split(/\s+/).join(" ");

export function PatientCardView({ patientId, onBack }: { patientId: string; onBack: () => void }) {
  const now = useToday();
  const card = usePatientCard(patientId, now.date);
  const reload = useReloadClinic();
  const back = <button type="button" className="link-btn pcard-back" onClick={onBack}>חזרה לרשימת המטופלים</button>;

  // A card that is no longer there (erased, or a link that went stale) is said plainly.
  if (card.error?.code === "not_found") {
    return (
      <EmptyState title="הכרטיס לא נמצא" actions={<button type="button" className="btn" onClick={onBack}>חזרה לרשימת המטופלים</button>}>
        <p>ייתכן שהכרטיס נמחק. שאר המטופלים נמצאים ברשימה.</p>
      </EmptyState>
    );
  }
  if (card.error && !card.data) {
    return (
      <div className="stack pcard">
        {back}
        <ReadError error={card.error.message} retry={reload} />
      </div>
    );
  }
  if (!card.data) {
    return (
      <div className="stack pcard" aria-busy="true">
        {back}
        <Skeleton card lines={4} label="טוען את כרטיס המטופל" />
        <Skeleton card lines={2} label="טוען את מסלול האבחון" />
        <Skeleton card lines={5} label="טוען את הפגישות" />
      </div>
    );
  }
  return <CardBody card={card.data} today={now.date} back={back} onBack={onBack} />;
}

function CardBody({ card, today, back, onBack }: { card: PatientCard; today: string; back: ReactNode; onBack: () => void }) {
  const p = card.patient;
  const { notify, fail } = useApp();
  const discreet = useDiscreet();
  const shown = useShownName();
  const save = useSavePatient();
  const [open, setOpen] = useState<Open>(null);
  const starter = useStarterTasks();
  const key = clinicKeys.card(p.id, today);

  /** The starter kit (D-199): the questionnaires and documents every family is asked for. */
  async function openStarter() {
    try {
      const made = await starter.mutateAsync(p.id);
      notify(made.length === 1 ? "נפתח מעקב אחד מערכת הפתיחה" : `נפתחו ${made.length} מעקבים מערכת הפתיחה`);
    } catch (e) {
      // The app already heard of the failure; the sentence says what happened (the kit is
      // already open, or the card is gone).
      notify(asUiError(e).message);
    }
  }

  // What happened in a visit shows on its row at once; the vault's refusal puts it back.
  const mark = useOptimistic<PatientCard, { visit: Visit; status: VisitStatus }>(
    key,
    (c, a) => ({ ...c, visits: c.visits.map((v) => (v.visit.id === a.visit.id ? { ...v, visit: { ...v.visit, status: a.status } } : v)) }),
    (a) => clinicApi.saveVisit({ ...a.visit, status: a.status, status_note: "", charge_missed: false }),
  );
  // A follow-up ticked (or unticked) shows so at once.
  const tick = useOptimistic<PatientCard, { task: ClinicTask; done: boolean }>(
    key,
    (c, a) => ({ ...c, tasks: c.tasks.map((t) => (t.id === a.task.id ? { ...t, done_on: a.done ? today : null } : t)) }),
    (a) => clinicApi.saveTask({ ...a.task, done_on: a.done ? today : null }),
  );

  const upcoming = useMemo(
    () => card.visits.filter((v) => v.visit.status === "planned" && v.visit.date >= today)
      .sort((a, b) => a.visit.date.localeCompare(b.visit.date) || a.visit.start - b.visit.start),
    [card.visits, today],
  );
  const earlier = useMemo(() => card.visits.filter((v) => !(v.visit.status === "planned" && v.visit.date >= today)), [card.visits, today]);
  const past = usePaged(earlier, 8);

  /** Copies a phone number through the protected clipboard only (it identifies the family). */
  async function copyPhone(phone: string) {
    try {
      const seconds = await ipc.copySecret(phone);
      notify(`מספר הטלפון הועתק ללוח המוגן. ${clearsIn(seconds)}.`);
    } catch (e) {
      notify(fail(asUiError(e)));
    }
  }

  async function reopen() {
    try {
      // A family taken off the waiting list before any visit goes back to waiting, not into the process.
      await save.mutateAsync({ ...p, status: p.waiting_since && card.visits.length === 0 ? "waiting" : "active" });
      notify("הכרטיס נפתח מחדש");
    } catch (e) {
      notify(messageOf(e, "הכרטיס לא נפתח מחדש. אפשר לנסות שוב."));
    }
  }

  /** The step an alert about a visit offers: its note, or the visit itself. */
  function openAlertVisit(alert: Alert, visitId: string) {
    const view = card.visits.find((v) => v.visit.id === visitId);
    if (!view) return;
    setOpen(alert.kind === "no_note" ? { kind: "note", visit: view.visit } : { kind: "visit", view });
  }
  function finishAlertTask(_alert: Alert, taskId: string) {
    const task = card.tasks.find((t) => t.id === taskId);
    if (task) void tick({ task, done: true });
  }

  const actionsOf = (view: VisitView) => {
    const v = view.visit;
    const due = v.date <= today;
    return (
      <>
        {v.status === "planned" && due && <button type="button" className="btn btn-small" onClick={() => void mark({ visit: v, status: "held" })}>התקיימה</button>}
        {v.status === "planned" && due && <button type="button" className="btn btn-small" onClick={() => void mark({ visit: v, status: "no_show" })}>לא הגיעו</button>}
        {v.status !== "cancelled" && due && <button type="button" className="btn btn-small" onClick={() => setOpen({ kind: "note", visit: v })}>סיכום</button>}
      </>
    );
  };
  const rows = (views: VisitView[]) => (
    <ul className="clinic-list clinic-visits">
      {views.map((view) => <VisitRow key={view.visit.id} view={view} showDate actions={actionsOf(view)} onOpen={() => setOpen({ kind: "visit", view })} />)}
    </ul>
  );

  const contacts = p.contacts.filter((c) => c.name !== "" || c.phone !== "" || c.email !== "");
  const age = ageOf(card.age, p.gender);

  return (
    <div className="stack pcard">
      {back}

      <header className="card pcard-head">
        <div className="pcard-head-top">
          <div className="stack pcard-who">
            <h2 className="pcard-name"><PatientName name={p.name} strong /></h2>
            <span className="row pcard-facts">
              <span className={p.status === "active" ? "chip chip-ok" : "chip chip-sand"}>{PATIENT_STATUS_LABEL[p.status]}</span>
              {p.status === "waiting" && p.urgent && <span className="chip chip-warn">דחוף</span>}
              {age && <span>{age}</span>}
              {p.birth_date && <span className="muted small">תאריך לידה: <span className="num" dir="ltr">{formatDate(p.birth_date)}</span></span>}
              {p.framework && !discreet && <span className="muted">{p.framework}</span>}
            </span>
          </div>
          <span className="grow" />
          <div className="row pcard-actions">
            <button type="button" className="btn btn-primary" onClick={() => setOpen({ kind: "visit" })}><PlusIcon /> פגישה חדשה</button>
            <button type="button" className="btn" onClick={() => setOpen({ kind: "edit" })}>עריכת הפרטים</button>
            {p.status === "closed"
              ? <button type="button" className="btn" disabled={save.isPending} onClick={() => void reopen()}>פתיחה מחדש</button>
              : <button type="button" className="btn" onClick={() => setOpen({ kind: "close" })}>סגירת הכרטיס</button>}
          </div>
        </div>

        {contacts.length === 0
          ? <p className="muted clinic-quiet">עוד אין אנשי קשר בכרטיס. אפשר להוסיף אותם ב"עריכת הפרטים".</p>
          : (
            <ul className="clinic-list pcard-contacts" aria-label="אנשי קשר">
              {contacts.map((c, i) => (
                <li key={i} className="pcard-contact">
                  <span className="pcard-contact-who">
                    <span className="muted">{CONTACT_ROLE_LABEL[c.role]}</span>
                    {c.name !== "" && <span className="clinic-name clinic-name-strong">{shown(c.name)}</span>}
                  </span>
                  {c.phone !== "" && (
                    <span className="row pcard-phone">
                      {discreet ? <span className="muted small">הטלפון מוסתר במצב דיסקרטי</span> : <span className="num" dir="ltr">{c.phone}</span>}
                      <button type="button" className="btn btn-small" aria-label={`העתקת הטלפון של ${CONTACT_ROLE_LABEL[c.role]}${c.name !== "" ? ` ${shown(c.name)}` : ""}`}
                        onClick={() => void copyPhone(c.phone)}>העתקה</button>
                    </span>
                  )}
                  {c.email !== "" && !discreet && <span className="muted small num" dir="ltr">{c.email}</span>}
                </li>
              ))}
            </ul>
          )}

        {(p.reason || p.referred_by || p.notes) && discreet && (
          <p className="muted small">סיבת הפנייה וההערות מוסתרות במצב דיסקרטי.</p>
        )}
        {(p.reason || p.referred_by || p.notes) && !discreet && (
          <dl className="pcard-about">
            {p.reason && <div><dt>סיבת הפנייה</dt><dd>{p.reason}</dd></div>}
            {p.referred_by && <div><dt>מי הפנה</dt><dd>{p.referred_by}</dd></div>}
            {p.notes && <div><dt>הערות</dt><dd>{p.notes}</dd></div>}
          </dl>
        )}
      </header>

      <Section title="מסלול האבחון" aside={p.status === "closed" ? undefined : (
        <div className="row pcard-plan">
          <button type="button" className="btn btn-small" disabled={starter.isPending} onClick={() => void openStarter()}
            title="פותח את השאלונים והמסמכים שמבקשים מכל משפחה, כמעקבים עם תאריך יעד">ערכת פתיחה</button>
          <button type="button" className="btn btn-small btn-primary" onClick={() => setOpen({ kind: "pathway" })}
            title="קובע את כל פגישות האבחון בשעות הפנויות הראשונות, ומראה אותן לפני השמירה">לקבוע את כל המסלול</button>
        </div>
      )}>
        <ProcessSteps steps={card.process} />
      </Section>

      <div className="pcard-columns">
        <Section title="מה דורש אותי" aside={card.alerts.length > 0 ? <span className="chip chip-warn">{card.alerts.length}</span> : undefined}>
          <AlertList alerts={card.alerts} onVisit={openAlertVisit} onTask={finishAlertTask} onCard={p.id} empty="אין כרגע שום דבר שמחכה בכרטיס הזה." />
        </Section>
        <CaseBox patient={p} link={card.case} />
      </div>

      <Section title="פגישות" aside={<button type="button" className="btn btn-small" onClick={() => setOpen({ kind: "visit" })}><PlusIcon /> פגישה חדשה</button>}>
        {card.visits.length === 0 ? <p className="muted clinic-quiet">עוד לא נקבעה פגישה. אפשר לקבוע את הראשונה כבר עכשיו.</p> : (
          <>
            <div className="stack pcard-group">
              <h3>הפגישות הקרובות</h3>
              {upcoming.length === 0 ? <p className="muted clinic-quiet">אין פגישה מתוכננת ביומן.</p> : rows(upcoming)}
            </div>
            {earlier.length > 0 && (
              <div className="stack pcard-group">
                <h3>פגישות שהיו</h3>
                {rows(past.shown)}
                <Pager page={past.page} pages={past.pages} onPage={past.setPage} label="עמודי הפגישות שהיו" />
              </div>
            )}
          </>
        )}
      </Section>

      <NotesBox card={card} onNew={() => setOpen({ kind: "note" })} onEdit={(noteId) => setOpen({ kind: "note", noteId })} />
      <MoneyBox card={card} today={today} />
      <TasksBox card={card} today={today} onTick={(task, done) => void tick({ task, done })} />

      <footer className="pcard-erase">
        <span className="muted small">הכרטיס נפתח ביום {formatDate(p.created_on)}.</span>
        <button type="button" className="link-btn" onClick={() => setOpen({ kind: "erase" })}>מחיקת הכרטיס</button>
      </footer>

      {open?.kind === "visit" && (
        open.view ? <VisitDialog view={open.view} onClose={() => setOpen(null)} /> : <VisitDialog start={{ patientId: p.id }} onClose={() => setOpen(null)} />
      )}
      {open?.kind === "note" && (
        open.visit ? <NoteDialog patientId={p.id} visit={open.visit} onClose={() => setOpen(null)} />
          : open.noteId !== undefined ? <NoteDialog patientId={p.id} noteId={open.noteId} onClose={() => setOpen(null)} />
          : <NoteDialog patientId={p.id} onClose={() => setOpen(null)} />
      )}
      {open?.kind === "edit" && <PatientDialog patient={p} onClose={() => setOpen(null)} />}
      {open?.kind === "pathway" && <PathwayDialog patientId={p.id} patientName={p.name} onClose={() => setOpen(null)} />}
      {open?.kind === "close" && (
        <ConfirmDialog title="סגירת הכרטיס" action="לסגור את הכרטיס" onClose={() => setOpen(null)}
          onConfirm={async () => {
            await save.mutateAsync({ ...p, status: "closed", urgent: false });
            notify("הכרטיס נסגר, ונשמר תחת \"הסתיים\"");
            setOpen(null);
          }}>
          <p>הכרטיס של <PatientName name={p.name} strong /> יעבור ל"הסתיים", ויצא מהרשימה הפעילה.</p>
          <p className="muted">שום דבר לא נמחק: הפגישות, הסיכומים והתשלומים נשארים שמורים, ואפשר לפתוח את הכרטיס מחדש בכל רגע.</p>
          {upcoming.length > 0 && (
            <p className="note-warn">{upcoming.length === 1 ? "יש עוד פגישה מתוכננת אחת ביומן. היא נשארת ביומן עד שמבטלים אותה." : `יש עוד ${upcoming.length} פגישות מתוכננות ביומן. הן נשארות ביומן עד שמבטלים אותן.`}</p>
          )}
        </ConfirmDialog>
      )}
      {open?.kind === "erase" && <EraseDialog patient={p} onClose={() => setOpen(null)} onErased={onBack} />}
    </div>
  );
}

// ------------------------------------------------------------------ the way of the assessment

const STEP_WORD: Record<StepState, string> = { done: "הושלם", now: "עכשיו", todo: "בהמשך" };
const STEP_MARK: Record<StepState, string> = { done: "✓", now: "●", todo: "" };

/** The assessment as a row of steps. Each state has its own form (a tick, a full dot, an empty
 *  dashed ring) and its own word for a screen reader, so it never rests on colour alone. */
function ProcessSteps({ steps }: { steps: ProcessStep[] }) {
  if (steps.length === 0) return <p className="muted clinic-quiet">המסלול יופיע כאן אחרי שתיקבע הפגישה הראשונה.</p>;
  return (
    <ol className="clinic-list pcard-steps" aria-label="שלבי האבחון">
      {steps.map((s) => (
        <li key={s.key} className={`pcard-step pcard-step-${s.state}`} aria-current={s.state === "now" ? "step" : undefined}>
          <span className="pcard-step-mark" aria-hidden="true">{STEP_MARK[s.state]}</span>
          <span className="stack pcard-step-body">
            <span className="pcard-step-label">{s.label}<span className="visually-hidden">: {STEP_WORD[s.state]}</span></span>
            <span className="muted small">{s.detail}</span>
          </span>
        </li>
      ))}
    </ol>
  );
}

// ------------------------------------------------------------------ the diagnostic case

/** The link to the diagnostic case: open it, unlink it; or open a new one, or link one that exists. */
function CaseBox({ patient, link }: { patient: Patient; link: CaseLink | null }) {
  const { go, notify } = useApp();
  const create = useCreateCase();
  const linkCase = useLinkCase();
  const [picking, setPicking] = useState(false);
  const [unlinking, setUnlinking] = useState(false);
  const [opened, setOpened] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);

  async function openNew() {
    setError(null);
    try {
      const made = await create.mutateAsync(patient.id);
      setOpened(made.id);
      notify("תיק האבחון נפתח");
    } catch (e) {
      setError(messageOf(e, "התיק לא נפתח. אפשר לנסות שוב."));
    }
  }

  const unlink = <button type="button" className="btn btn-small" onClick={() => setUnlinking(true)}>לנתק</button>;
  return (
    <Section title="תיק האבחון">
      {link === null ? (
        <div className="stack pcard-case">
          <p className="clinic-quiet">
            {patient.case_id !== null
              ? "התיק שהיה מקושר לכרטיס כבר לא קיים. אפשר לפתוח תיק חדש, או לקשר לתיק אחר."
              : "לכרטיס הזה עוד אין תיק אבחון. בתיק נכתב הדוח, ואליו אפשר להעביר סיכומי פגישות."}
          </p>
          <span className="row pcard-case-actions">
            <button type="button" className="btn btn-primary btn-small" disabled={create.isPending} onClick={() => void openNew()}>לפתוח תיק אבחון</button>
            <button type="button" className="btn btn-small" onClick={() => setPicking(true)}>לקשר לתיק קיים</button>
          </span>
        </div>
      ) : link.in_trash ? (
        <div className="stack pcard-case">
          <p className="note-warn clinic-quiet">התיק <span className="num">{link.code}</span> נמצא בסל המחזור. אפשר לשחזר אותו ברשימת התיקים, או לנתק אותו מהכרטיס.</p>
          <span className="row pcard-case-actions">
            <button type="button" className="btn btn-small" onClick={() => go({ name: "cases" })}>לרשימת התיקים</button>
            {unlink}
          </span>
        </div>
      ) : (
        <div className="stack pcard-case">
          {opened === link.id && <p className="note-sand clinic-quiet" role="status">התיק נפתח, והוא מקושר לכרטיס. מעכשיו אפשר להעביר אליו סיכומי פגישות.</p>}
          <p className="clinic-quiet">
            <span className="pcard-case-code num">{link.code}</span>
            <span className="muted"> · </span>
            {link.total > 0 && link.approved >= link.total ? "כל סעיפי הדוח אושרו" : `${link.approved} מתוך ${link.total} סעיפים אושרו`}
          </p>
          <span className="row pcard-case-actions">
            {/* As the case list opens a case: its materials while nothing is written, the report after. */}
            <button type="button" className="btn btn-primary btn-small" onClick={() => go({ name: "case", id: link.id, view: link.approved > 0 ? "report" : "materials" })}>
              {opened === link.id ? "לעבור לתיק" : "לפתוח את התיק"}
            </button>
            {unlink}
          </span>
        </div>
      )}
      <ErrorLine error={error} />

      {picking && <CasePicker patientId={patient.id} onClose={() => setPicking(false)} />}
      {unlinking && link !== null && (
        <ConfirmDialog title="לנתק את התיק מהכרטיס" action="לנתק" onClose={() => setUnlinking(false)}
          onConfirm={async () => {
            await linkCase.mutateAsync({ patientId: patient.id, caseId: null });
            notify("התיק נותק מהכרטיס");
            setUnlinking(false);
          }}>
          <p>רק הקישור בין הכרטיס לתיק <span className="num">{link.code}</span> יוסר.</p>
          <p className="muted">התיק עצמו לא נמחק ולא משתנה, וסיכומים שכבר הועברו אליו נשארים בו. אפשר לקשר אותו שוב בכל רגע.</p>
        </ConfirmDialog>
      )}
    </Section>
  );
}

/** Chooses a case that is not linked to any patient. The label of a case carries the child's
 *  name, so it is shown through discreet mode like every other name. */
function CasePicker({ patientId, onClose }: { patientId: string; onClose: () => void }) {
  const { notify } = useApp();
  const choices = useCaseChoices();
  const reload = useReloadClinic();
  const linkCase = useLinkCase();
  const shown = useShownName();
  const [error, setError] = useState<string | null>(null);

  async function choose(caseId: string) {
    setError(null);
    try {
      await linkCase.mutateAsync({ patientId, caseId });
      notify("התיק קושר לכרטיס");
      onClose();
    } catch (e) {
      setError(messageOf(e, "התיק לא קושר. אפשר לנסות שוב."));
    }
  }
  /** "נועם · תיק-2026-1101" → the name (as discreet mode allows it) and the code. */
  const labelOf = (label: string) => {
    const at = label.lastIndexOf(" · ");
    return at < 0 ? label : `${shown(label.slice(0, at))} · ${label.slice(at + 3)}`;
  };
  return (
    <Dialog narrow title="לקשר לתיק קיים" subtitle="תיק מקושר למטופל אחד בלבד. תיקים שבסל המחזור לא מוצעים." onClose={onClose}
      footer={<><span className="grow" /><button type="button" className="btn" onClick={onClose}>ביטול</button></>}>
      {choices.error && !choices.data ? <ReadError error={choices.error.message} retry={reload} />
        : !choices.data ? <Skeleton lines={3} label="טוען את התיקים" />
        : choices.data.length === 0 ? <p className="muted clinic-quiet" role="status">אין כרגע תיק שאפשר לקשר: כל התיקים כבר מקושרים למטופלים, או שעוד לא נפתח תיק.</p>
        : (
          <ul className="clinic-list clinic-picker-list" aria-label="תיקים לבחירה">
            {choices.data.map((c) => (
              <li key={c.id}>
                <button type="button" className="clinic-picker-row" disabled={linkCase.isPending} onClick={() => void choose(c.id)}>{labelOf(c.label)}</button>
              </li>
            ))}
          </ul>
        )}
      <ErrorLine error={error} />
    </Dialog>
  );
}

// ------------------------------------------------------------------ session notes

/** From how much text a record is folded to a few lines. */
const LONG_RECORD = 260;

function NotesBox({ card, onNew, onEdit }: { card: PatientCard; onNew: () => void; onEdit: (noteId: string) => void }) {
  const paged = usePaged(card.notes, 5);
  const visits = useMemo(() => new Map(card.visits.map((v) => [v.visit.id, v.visit])), [card.visits]);
  return (
    <Section title="סיכומי פגישות" aside={<button type="button" className="btn btn-small" onClick={onNew}><PlusIcon /> סיכום שיחה</button>}>
      {card.notes.length === 0
        ? <p className="muted clinic-quiet">עוד אין סיכומים. סיכום של פגישה נכתב מהשורה שלה, בלחיצה על "סיכום".</p>
        : (
          <ul className="clinic-list pcard-notes">
            {paged.shown.map((n) => <NoteItem key={n.id} note={n} visit={n.visit_id === null ? null : (visits.get(n.visit_id) ?? null)} caseCode={card.case?.code ?? null} onEdit={() => onEdit(n.id)} />)}
          </ul>
        )}
      <Pager page={paged.page} pages={paged.pages} onPage={paged.setPage} label="עמודי הסיכומים" />
    </Section>
  );
}

/** One session note: the record (folded when long), and, clearly apart, the mark that a
 *  personal note exists. Its text is shown only when she asks, and never in discreet mode. */
function NoteItem({ note, visit, caseCode, onEdit }: { note: SessionNote; visit: Visit | null; caseCode: string | null; onEdit: () => void }) {
  const discreet = useDiscreet();
  const [whole, setWhole] = useState(false);
  const [personal, setPersonal] = useState(false);
  const recordId = useId();
  const personalId = useId();
  const record = note.record.trim();
  const long = record.length > LONG_RECORD || record.split("\n").length > 4;
  const showPersonal = personal && !discreet;
  const when = formatDate(note.date);
  return (
    <li className="pcard-note">
      <div className="row pcard-note-head">
        <span className="num pcard-note-date" dir="ltr">{when}</span>
        <span className="chip chip-sand">{visit ? VISIT_KIND_LABEL[visit.kind] : "שיחה, לא צמודה לפגישה"}</span>
        {note.case_input_id !== null && <span className="chip chip-ok">{caseCode ? `הועבר לתיק ${caseCode}` : "הועבר לתיק"}</span>}
        <span className="grow" />
        <button type="button" className="btn btn-small" aria-label={`עריכת הסיכום מיום ${when}`} onClick={onEdit}>עריכה</button>
      </div>

      {record === "" ? <p className="muted clinic-quiet">לא נכתבה רשומה.</p> : discreet ? <p className="muted small">הרשומה מוסתרת במצב דיסקרטי.</p> : (
        <div className="stack pcard-note-record">
          <p id={recordId} className={long && !whole ? "pcard-note-text pcard-note-folded" : "pcard-note-text"}>{record}</p>
          {long && (
            <button type="button" className="link-btn" aria-expanded={whole} aria-controls={recordId} onClick={() => setWhole((w) => !w)}>
              {whole ? "לקצר" : "להציג את כל הרשומה"}
            </button>
          )}
        </div>
      )}

      {note.personal.trim() !== "" && (
        <div className="pcard-personal">
          <span className="row pcard-personal-head">
            <span className="chip">יש תרשומת אישית</span>
            {discreet
              ? <span className="muted small">לא מוצגת במצב דיסקרטי.</span>
              : (
                <button type="button" className="link-btn" aria-expanded={showPersonal} aria-controls={personalId} onClick={() => setPersonal((x) => !x)}>
                  {showPersonal ? "להסתיר את התרשומת האישית" : "להציג את התרשומת האישית"}
                </button>
              )}
          </span>
          <div id={personalId} hidden={!showPersonal}>
            {showPersonal && (
              <>
                <p className="pcard-note-text">{note.personal}</p>
                <p className="hint">שלך בלבד. לא מודפסת, לא מיוצאת ולא עוברת לתיק האבחון.</p>
              </>
            )}
          </div>
        </div>
      )}

      <RecordHistory recordId={note.id} />
    </li>
  );
}

// ------------------------------------------------------------------ money

/** One dated line of the card's account: a session that was charged, a charge, or a payment. */
type Line =
  | { type: "visit"; id: string; date: string; visit: Visit; amount: number }
  | { type: "charge"; id: string; date: string; charge: Charge }
  | { type: "payment"; id: string; date: string; payment: Payment };

/** What a visit charges (the core's rule): its fee when held, or when missed and charged anyway. */
function visitCharge(v: Visit): number {
  if (v.status === "held") return v.fee;
  return (v.status === "cancelled" || v.status === "no_show") && v.charge_missed ? v.fee : 0;
}

type MoneyOpen =
  | { kind: "charge" }
  | { kind: "payment" }
  | { kind: "void"; line: Extract<Line, { type: "charge" | "payment" }> }
  | null;

function MoneyBox({ card, today }: { card: PatientCard; today: string }) {
  const [open, setOpen] = useState<MoneyOpen>(null);
  const b = card.balance;

  // The sessions that were charged are listed with the charges and the payments, so the three
  // sums above the list can be read off it.
  const lines = useMemo(() => {
    const all: Line[] = [
      ...card.visits.filter((v) => visitCharge(v.visit) > 0).map((v): Line => ({ type: "visit", id: v.visit.id, date: v.visit.date, visit: v.visit, amount: visitCharge(v.visit) })),
      ...card.charges.map((charge): Line => ({ type: "charge", id: charge.id, date: charge.date, charge })),
      ...card.payments.map((payment): Line => ({ type: "payment", id: payment.id, date: payment.date, payment })),
    ];
    return all.sort((x, y) => y.date.localeCompare(x.date) || x.id.localeCompare(y.id));
  }, [card.visits, card.charges, card.payments]);
  const paged = usePaged(lines, 10);

  return (
    <Section title="כספים"
      aside={(
        <span className="row pcard-money-actions">
          {b.due > 0 && <SendMenu kind="payment" patientId={card.patient.id} label="הודעת תשלום" />}
          <button type="button" className="btn btn-small" onClick={() => setOpen({ kind: "charge" })}><PlusIcon /> חיוב חדש</button>
          <button type="button" className="btn btn-small btn-primary" onClick={() => setOpen({ kind: "payment" })}><PlusIcon /> תקבול חדש</button>
        </span>
      )}>
      <dl className="pcard-balance">
        <div><dt>חויב</dt><dd><Money agorot={b.charged} /></dd></div>
        <div><dt>שולם</dt><dd><Money agorot={b.paid} /></dd></div>
        <div className="pcard-balance-due">
          <dt>{b.due < 0 ? "יתרת זכות" : "יתרה לתשלום"}</dt>
          <dd>{b.due > 0 ? <Money agorot={b.due} tone="owed" /> : b.due < 0 ? <Money agorot={-b.due} tone="ok" /> : <span className="muted">אין</span>}</dd>
        </div>
      </dl>

      {lines.length === 0
        ? <p className="muted clinic-quiet">עוד לא נרשמו חיובים או תקבולים. פגישה שהתקיימה מחויבת מעצמה, לפי המחיר שלה.</p>
        : (
          <ul className="clinic-list pcard-lines" aria-label="חיובים ותקבולים, מהחדש לישן">
            {paged.shown.map((line) => <MoneyLine key={`${line.type}-${line.id}`} line={line} onVoid={line.type === "visit" ? undefined : () => setOpen({ kind: "void", line })} />)}
          </ul>
        )}
      <Pager page={paged.page} pages={paged.pages} onPage={paged.setPage} label="עמודי החיובים והתקבולים" />

      {open?.kind === "charge" && <ChargeDialog patientId={card.patient.id} today={today} onClose={() => setOpen(null)} />}
      {open?.kind === "payment" && <PaymentDialog patientId={card.patient.id} today={today} due={b.due} onClose={() => setOpen(null)} />}
      {open?.kind === "void" && <VoidDialog line={open.line} onClose={() => setOpen(null)} />}
    </Section>
  );
}

function MoneyLine({ line, onVoid }: { line: Line; onVoid: (() => void) | undefined }) {
  const discreet = useDiscreet();
  const shown = useShownName();
  const when = formatDate(line.date);
  const voided = line.type === "charge" ? line.charge.voided : line.type === "payment" ? line.payment.voided : false;
  const reason = line.type === "charge" ? line.charge.void_reason : line.type === "payment" ? line.payment.void_reason : "";
  const amount = line.type === "visit" ? line.amount : line.type === "charge" ? line.charge.amount : line.payment.amount;
  const what = line.type === "visit"
    ? `${VISIT_KIND_LABEL[line.visit.kind]}${line.visit.status === "held" ? "" : ` (${VISIT_STATUS_LABEL[line.visit.status]}, בחיוב)`}`
    : line.type === "charge" ? line.charge.what
    : [
      PAY_METHOD_LABEL[line.payment.method],
      line.payment.receipt ? `קבלה ${line.payment.receipt}` : "בלי מספר קבלה",
      // Whoever paid may be a person: in discreet mode, initials, and her own note is left out.
      line.payment.payer ? `שילם: ${shown(line.payment.payer)}` : "",
      discreet ? "" : line.payment.note,
    ].filter(Boolean).join(" · ");
  return (
    <li className={voided ? "pcard-line pcard-line-voided" : "pcard-line"}>
      <div className="pcard-line-row">
        <span className="num pcard-line-date" dir="ltr">{when}</span>
        <span className="row grow pcard-line-what">
          <span className={line.type === "payment" ? "chip chip-ok" : "chip chip-sand"}>{line.type === "payment" ? "תקבול" : line.type === "charge" ? "חיוב" : "פגישה"}</span>
          <span className="pcard-line-text">{what}</span>
          {voided && <span className="chip chip-warn">בוטל</span>}
        </span>
        <span className="pcard-line-amount">
          {voided ? <Money agorot={amount} tone="muted" /> : line.type === "payment" ? <Money agorot={amount} tone="ok" /> : <Money agorot={amount} />}
        </span>
        {onVoid && !voided
          ? <button type="button" className="btn btn-small" aria-label={`ביטול ${line.type === "payment" ? "התקבול" : "החיוב"} מיום ${when}`} onClick={onVoid}>ביטול</button>
          : <span className="pcard-line-gap" />}
      </div>
      {voided && <span className="muted small">סיבת הביטול: {reason}</span>}
      {line.type !== "visit" && <RecordHistory recordId={line.id} />}
    </li>
  );
}

/** "חיוב חדש": a charge that is not a session (the assessment as a package, writing the report). */
function ChargeDialog({ patientId, today, onClose }: { patientId: string; today: string; onClose: () => void }) {
  const { notify } = useApp();
  const save = useSaveCharge();
  const [what, setWhat] = useState("");
  const [amount, setAmount] = useState("");
  const [date, setDate] = useState(today);
  const [error, setError] = useState<string | null>(null);

  async function submit() {
    const agorot = parseMoney(amount);
    if (what.trim() === "") return setError("צריך לכתוב על מה החיוב.");
    if (agorot === null || agorot === 0) return setError("צריך לכתוב סכום גדול מאפס, בשקלים. למשל 1200.");
    if (date === "") return setError("צריך לכתוב את תאריך החיוב: יום.חודש.שנה.");
    setError(null);
    try {
      await save.mutateAsync({ id: "", patient_id: patientId, date, amount: agorot, what, voided: false, void_reason: "" });
      notify("החיוב נרשם");
      onClose();
    } catch (e) {
      setError(messageOf(e, "החיוב לא נרשם. אפשר לנסות שוב."));
    }
  }
  return (
    <Dialog narrow title="חיוב חדש" subtitle="לחיוב שאינו פגישה, כמו כתיבת הדוח. פגישה שהתקיימה מחויבת מעצמה." dirty={what !== "" || amount !== ""} onClose={onClose}
      footer={(close) => <>
        <span className="grow" />
        <button type="button" className="btn" onClick={close}>ביטול</button>
        <button type="button" className="btn btn-primary" disabled={save.isPending} onClick={() => void submit()}>רישום החיוב</button>
      </>}>
      <div className="stack clinic-form">
        <TextField id="charge-what" label="על מה החיוב" value={what} maxLength={200} autoFocus placeholder="למשל: כתיבת דוח אבחון" onChange={setWhat} />
        <div className="clinic-form-row">
          <MoneyField id="charge-amount" label="סכום" value={amount} onChange={setAmount} />
          <Field id="charge-date" label="תאריך"><DateField id="charge-date" value={date} onChange={setDate} /></Field>
        </div>
        <ErrorLine error={error} />
      </div>
    </Dialog>
  );
}

/** "תקבול חדש": money that was received. The amount starts from what is still owed. */
function PaymentDialog({ patientId, today, due, onClose }: { patientId: string; today: string; due: number; onClose: () => void }) {
  const { notify } = useApp();
  const save = useSavePayment();
  const [first] = useState({ amount: moneyText(due > 0 ? due : null), method: "transfer" as PayMethod, receipt: "", payer: "", date: today, note: "" });
  const [form, setForm] = useState(first);
  const [error, setError] = useState<string | null>(null);
  const set = (patch: Partial<typeof first>) => setForm((f) => ({ ...f, ...patch }));

  async function submit() {
    const agorot = parseMoney(form.amount);
    if (agorot === null || agorot === 0) return setError("צריך לכתוב סכום גדול מאפס, בשקלים. למשל 450.");
    if (form.date === "") return setError("צריך לכתוב את תאריך התקבול: יום.חודש.שנה.");
    setError(null);
    try {
      await save.mutateAsync({
        id: "", patient_id: patientId, date: form.date, amount: agorot, method: form.method, receipt: form.receipt, payer: form.payer, note: form.note,
        voided: false, void_reason: "",
      });
      notify("התקבול נרשם");
      onClose();
    } catch (e) {
      setError(messageOf(e, "התקבול לא נרשם. אפשר לנסות שוב."));
    }
  }
  return (
    <Dialog narrow title="תקבול חדש" dirty={JSON.stringify(form) !== JSON.stringify(first)} onClose={onClose}
      footer={(close) => <>
        <span className="grow" />
        <button type="button" className="btn" onClick={close}>ביטול</button>
        <button type="button" className="btn btn-primary" disabled={save.isPending} onClick={() => void submit()}>רישום התקבול</button>
      </>}>
      <div className="stack clinic-form">
        <div className="clinic-form-row">
          <MoneyField id="payment-amount" label="סכום" value={form.amount} onChange={(amount) => set({ amount })} />
          <SelectField<PayMethod> id="payment-method" label="אמצעי תשלום" value={form.method} onChange={(method) => set({ method })}
            options={PAY_METHODS.map((m) => ({ value: m, label: PAY_METHOD_LABEL[m] }))} />
          <Field id="payment-date" label="תאריך"><DateField id="payment-date" value={form.date} onChange={(date) => set({ date })} /></Field>
        </div>
        <div className="clinic-form-row">
          <TextField id="payment-receipt" label="מספר קבלה" value={form.receipt} maxLength={200} dir="ltr" className="grow fit-s"
            hint="המספר של הקבלה שהופקה בתוכנת החשבוניות. אפשר להשלים אחר כך." onChange={(receipt) => set({ receipt })} />
          <TextField id="payment-payer" label="מי שילם (לא חובה)" value={form.payer} maxLength={200} className="grow"
            hint="כשלא ההורים שילמו: קופה, מוסד." onChange={(payer) => set({ payer })} />
        </div>
        <TextAreaField id="payment-note" label="הערה (לא חובה)" value={form.note} rows={2} maxLength={4000} onChange={(note) => set({ note })} />
        <ErrorLine error={error} />
      </div>
    </Dialog>
  );
}

/** A charge or a payment is never deleted: it is voided, with a reason, and stays on the list. */
function VoidDialog({ line, onClose }: { line: Extract<Line, { type: "charge" | "payment" }>; onClose: () => void }) {
  const { notify } = useApp();
  const voidCharge = useVoidCharge();
  const voidPayment = useVoidPayment();
  const [reason, setReason] = useState("");
  const isPayment = line.type === "payment";
  const amount = isPayment ? line.payment.amount : line.charge.amount;
  return (
    <ConfirmDialog title={isPayment ? "ביטול התקבול" : "ביטול החיוב"} action={isPayment ? "לבטל את התקבול" : "לבטל את החיוב"} danger onClose={onClose}
      onConfirm={async () => {
        if (reason.trim() === "") throw { message: "צריך לכתוב למה הרישום מבוטל." };
        if (isPayment) await voidPayment.mutateAsync({ id: line.id, reason });
        else await voidCharge.mutateAsync({ id: line.id, reason });
        notify(isPayment ? "התקבול בוטל" : "החיוב בוטל");
        onClose();
      }}>
      <p>
        {isPayment ? "התקבול" : "החיוב"} מיום <span className="num" dir="ltr">{formatDate(line.date)}</span>, על סך <Money agorot={amount} />, יסומן כמבוטל ולא ייספר ביתרה.
      </p>
      <p className="muted">הרישום לא נמחק: הוא נשאר ברשימה וביומן ההכנסות, עם הסיבה.</p>
      <TextField id="void-reason" label="סיבת הביטול" value={reason} maxLength={200} autoFocus placeholder="למשל: נרשם בטעות פעמיים" onChange={setReason} />
    </ConfirmDialog>
  );
}

// ------------------------------------------------------------------ follow-ups

function TasksBox({ card, today, onTick }: { card: PatientCard; today: string; onTick: (task: ClinicTask, done: boolean) => void }) {
  const discreet = useDiscreet();
  const [editing, setEditing] = useState<ClinicTask | "new" | null>(null);
  // The open ones by their date (the undated last), then what was done, the latest first.
  const ordered = useMemo(() => [...card.tasks].sort((a, b) => {
    const done = Number(a.done_on !== null) - Number(b.done_on !== null);
    if (done !== 0) return done;
    if (a.done_on !== null && b.done_on !== null) return b.done_on.localeCompare(a.done_on) || a.id.localeCompare(b.id);
    return (a.due ?? "9999").localeCompare(b.due ?? "9999") || a.id.localeCompare(b.id);
  }), [card.tasks]);
  const paged = usePaged(ordered, 10);
  return (
    <Section title="מעקבים" aside={<button type="button" className="btn btn-small" onClick={() => setEditing("new")}><PlusIcon /> מעקב חדש</button>}>
      {ordered.length === 0
        ? <p className="muted clinic-quiet">אין מעקבים בכרטיס. אפשר להוסיף כאן שיחה שצריך לעשות, שאלון לשלוח או מסמך לבקש.</p>
        : (
          <ul className="clinic-list clinic-tasks">
            {paged.shown.map((t) => {
              const done = t.done_on !== null;
              const overdue = !done && t.due !== null && t.due < today;
              const id = `pcard-task-${t.id}`;
              return (
                <li key={t.id} className={done ? "clinic-task clinic-task-done" : "clinic-task"}>
                  <input id={id} type="checkbox" checked={done} onChange={(e) => onTick(t, e.target.checked)} />
                  <div className="grow stack clinic-task-body">
                    <label htmlFor={id}>{t.title}</label>
                    <span className="row muted small clinic-task-more">
                      <span className="chip chip-sand">{TASK_KIND_LABEL[t.kind]}</span>
                      {done && t.done_on !== null ? <span>בוצע ביום {formatDate(t.done_on)}</span>
                        : t.due !== null ? <span className={overdue ? "clinic-overdue" : undefined}>{overdue ? `עד ${formatDate(t.due)}, עבר` : `עד ${formatDate(t.due)}`}</span>
                        : null}
                      {t.note && !discreet && <span>{t.note}</span>}
                    </span>
                  </div>
                  <button type="button" className="btn btn-small" aria-label={`עריכת המעקב: ${t.title}`} onClick={() => setEditing(t)}>עריכה</button>
                </li>
              );
            })}
          </ul>
        )}
      <Pager page={paged.page} pages={paged.pages} onPage={paged.setPage} label="עמודי המעקבים" />
      {editing !== null && (
        editing === "new" ? <TaskDialog patientId={card.patient.id} onClose={() => setEditing(null)} />
          : <TaskDialog patientId={card.patient.id} task={editing} onClose={() => setEditing(null)} />
      )}
    </Section>
  );
}

function TaskDialog({ patientId, task, onClose }: { patientId: string; task?: ClinicTask; onClose: () => void }) {
  const { notify } = useApp();
  const save = useSaveTask();
  const [first] = useState({ title: task?.title ?? "", kind: task?.kind ?? ("follow_up" as TaskKind), due: task?.due ?? "", note: task?.note ?? "" });
  const [form, setForm] = useState(first);
  const [error, setError] = useState<string | null>(null);
  const set = (patch: Partial<typeof first>) => setForm((f) => ({ ...f, ...patch }));

  async function submit() {
    if (form.title.trim() === "") return setError("צריך לכתוב מה צריך לעשות.");
    setError(null);
    try {
      await save.mutateAsync({
        id: task?.id ?? "", patient_id: patientId, kind: form.kind, title: form.title, due: form.due === "" ? null : form.due,
        done_on: task?.done_on ?? null, note: form.note,
      });
      notify(task ? "המעקב נשמר" : "המעקב נוסף");
      onClose();
    } catch (e) {
      setError(messageOf(e, "המעקב לא נשמר. אפשר לנסות שוב."));
    }
  }
  return (
    <Dialog narrow title={task ? "עריכת מעקב" : "מעקב חדש"} dirty={JSON.stringify(form) !== JSON.stringify(first)} onClose={onClose}
      footer={(close) => <>
        <span className="grow" />
        <button type="button" className="btn" onClick={close}>ביטול</button>
        <button type="button" className="btn btn-primary" disabled={save.isPending} onClick={() => void submit()}>{task ? "שמירה" : "הוספה"}</button>
      </>}>
      <div className="stack clinic-form">
        <TextField id="task-title" label="מה צריך לעשות" value={form.title} maxLength={200} autoFocus={!task} placeholder="למשל: לשלוח שאלון לגננת"
          onChange={(title) => set({ title })} className="fit-l" />
        <div className="clinic-form-row">
          <SelectField<TaskKind> id="task-kind" label="סוג" value={form.kind} onChange={(kind) => set({ kind })}
            options={optionsWith(TASK_KINDS, TASK_KIND_LABEL, form.kind)} />
          <Field id="task-due" label="עד מתי (לא חובה)"><DateField id="task-due" value={form.due} onChange={(due) => set({ due })} /></Field>
        </div>
        <TextAreaField id="task-note" label="הערה (לא חובה)" value={form.note} rows={2} maxLength={4000} onChange={(note) => set({ note })} />
        <ErrorLine error={error} />
      </div>
    </Dialog>
  );
}

// ------------------------------------------------------------------ erasing the card

/**
 * "מחיקת הכרטיס" (D-174): she types the child's name. The core allows it only for a card where
 * nothing happened, or after the retention period; its refusal is shown as it is, and closing
 * the card is offered in its place.
 */
function EraseDialog({ patient, onClose, onErased }: { patient: Patient; onClose: () => void; onErased: () => void }) {
  const { notify, fail } = useApp();
  const qc = useQueryClient();
  const reload = useReloadClinic();
  const save = useSavePatient();
  const [typed, setTyped] = useState("");
  const [problem, setProblem] = useState<string | null>(null);
  const [refusal, setRefusal] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);

  async function erase() {
    setRefusal(null);
    if (cleanName(typed) !== cleanName(patient.name)) {
      setProblem("השם שהוקלד לא זהה לשם שבכרטיס. כדי למחוק צריך להקליד אותו בדיוק כמו שהוא כתוב בכרטיס.");
      return;
    }
    setProblem(null);
    setBusy(true);
    try {
      await clinicApi.erasePatient(patient.id, typed);
    } catch (e) {
      const err = asUiError(e);
      fail(err);
      setRefusal(err.message);
      setBusy(false);
      return;
    }
    // The card is gone: leave its page first, so nothing asks for it again, then read the rest anew.
    notify("הכרטיס נמחק");
    onErased();
    qc.removeQueries({ queryKey: ["clinic", "card", patient.id] });
    reload();
  }

  async function closeInstead() {
    setBusy(true);
    try {
      await save.mutateAsync({ ...patient, status: "closed", urgent: false });
      notify("הכרטיס נסגר, ונשמר תחת \"הסתיים\"");
      onClose();
    } catch (e) {
      setRefusal(messageOf(e, "הכרטיס לא נסגר. אפשר לנסות שוב."));
      setBusy(false);
    }
  }

  return (
    <Dialog narrow title="מחיקת הכרטיס" onClose={onClose}
      footer={<>
        <button type="button" className="btn" onClick={onClose}>ביטול</button>
        <span className="grow" />
        {refusal !== null && patient.status !== "closed" && (
          <button type="button" className="btn btn-primary" disabled={busy} onClick={() => void closeInstead()}>לסגור את הכרטיס</button>
        )}
        <button type="button" className="btn btn-danger" disabled={busy || typed.trim() === ""} onClick={() => void erase()}>למחוק לצמיתות</button>
      </>}>
      <div className="stack">
        <p>מחיקה מסירה את הכרטיס של <PatientName name={patient.name} strong /> וכל מה שנרשם בו, ואי אפשר לבטל אותה.</p>
        <dl className="pcard-erase-what">
          <div>
            <dt>מה נמחק</dt>
            <dd>הכרטיס ופרטי הקשר, הפגישות, סיכומי הפגישות, החיובים, התקבולים, המעקבים, וההיסטוריה של כל אלה.</dd>
          </div>
          <div>
            <dt>מה נשאר</dt>
            <dd>מספרים בלבד, בלי שם, בלי תאריך ובלי טקסט: כמה פגישות היו, כמה חויב וכמה התקבל בכל חודש. כך הסיכום השנתי ויומן ההכנסות של שנים שעברו לא משתנים. כרטיס שלא קרה בו כלום לא משאיר אחריו דבר.</dd>
          </div>
        </dl>
        <p className="muted">
          אפשר למחוק רק כרטיס שעוד לא קרה בו דבר, או כרטיס שתקופת השמירה שלו עברה. כרטיס שהיו בו פגישות שסומנו, סיכומים או תשלומים סוגרים, ולא מוחקים.
          {patient.case_id !== null ? " תיק האבחון המקושר לא נמחק יחד עם הכרטיס." : ""}
        </p>
        <TextField id="erase-name" label="כדי למחוק, יש להקליד את שם הילד או הילדה כפי שהוא בכרטיס" value={typed} maxLength={120} autoFocus problem={problem}
          onChange={(text) => { setTyped(text); setProblem(null); }} />
        {refusal !== null && <p className="error" role="alert">{refusal}</p>}
        {refusal !== null && patient.status !== "closed" && <p className="muted">אפשר לסגור את הכרטיס במקום: הוא יוצא מהרשימה הפעילה, והכל נשאר שמור.</p>}
      </div>
    </Dialog>
  );
}
