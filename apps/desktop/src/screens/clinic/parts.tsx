// The clinic's shared building blocks: how a patient's name, an amount, a visit and an alert
// look on every tab, so the tabs only arrange them. Everything shown is plain text.
import { createContext, useCallback, useContext, useId, useMemo, useRef, useState, type KeyboardEvent, type ReactNode } from "react";
import { useDiscreet } from "../../discreet";
import { Dialog, ErrorLine, Skeleton, WarnIcon } from "../../components/ui";
import { failureFor } from "../../errors";
import type { Alert } from "../../ipc/generated/Alert";
import type { HistoryEntry } from "../../ipc/generated/HistoryEntry";
import type { VisitKind } from "../../ipc/generated/VisitKind";
import type { VisitStatus } from "../../ipc/generated/VisitStatus";
import type { VisitView } from "../../ipc/generated/VisitView";
import { useDebounced, useHistory, usePatients, useReloadClinic, useToday } from "./clinicData";
import {
  ALERT_KIND_LABEL, formatDate, formatSpan, initials, moneyInput, moneyNumber, parseMoney, parseTime, PATIENT_STATUS_LABEL,
  VISIT_KIND_LABEL, VISIT_STATUS_LABEL,
} from "./format";
import type { ClinicTab } from "./tabs";
import "./ClinicScreen.css";

// ------------------------------------------------------------------ the clinic's context

/** Discreet mode: one switch for the whole program, in the top bar (D-192). */
export { useDiscreet };

/** Where a clinic screen can send her: a patient's card, or another tab. Both go through the
 *  app's route, so "back" and a link from another tab land in the same place. */
export interface ClinicNav {
  openPatient: (id: string) => void;
  openTab: (tab: ClinicTab) => void;
}

export const ClinicNavContext = createContext<ClinicNav | null>(null);

export function useClinicNav(): ClinicNav {
  const nav = useContext(ClinicNavContext);
  if (!nav) throw new Error("useClinicNav outside the clinic screen");
  return nav;
}

// ------------------------------------------------------------------ names and money

/** The name as the clinic may show it now: whole, or initials in discreet mode. */
export function useShownName(): (name: string | null | undefined) => string {
  const discreet = useDiscreet();
  return useMemo(() => (name) => {
    const full = (name ?? "").trim();
    if (full === "") return "בלי שם";
    return discreet ? initials(full) : full;
  }, [discreet]);
}

/**
 * A patient's name. Every clinic screen shows a name through this (or `useShownName`, for a
 * label): in discreet mode it is initials only ("נ. ל."), for the eye and for a screen reader.
 * With `id` it is a button that opens the patient's card.
 */
export function PatientName({ name, id, strong = false }: { name: string | null | undefined; id?: string | null; strong?: boolean }) {
  const shown = useShownName()(name);
  const nav = useContext(ClinicNavContext);
  const className = strong ? "clinic-name clinic-name-strong" : "clinic-name";
  if (id && nav) {
    return <button type="button" className={`${className} clinic-name-link`} onClick={() => nav.openPatient(id)}>{shown}</button>;
  }
  return <span className={className}>{shown}</span>;
}

/** An amount: "450 ₪", "450.50 ₪". `owed` paints what is still to pay; a credit reads "זכות". */
export function Money({ agorot, tone }: { agorot: number; tone?: "owed" | "ok" | "muted" }) {
  const className = tone ? `clinic-money clinic-money-${tone}` : "clinic-money";
  // The number keeps its own direction, so a minus stays in front of it in a Hebrew line.
  return <span className={className}><span dir="ltr">{moneyNumber(agorot)}</span> ₪</span>;
}

// ------------------------------------------------------------------ badges

// A status this version does not know (D-178) is said plainly, in the colour of something to look at.
const STATUS_CHIP: Record<VisitStatus, string> = { planned: "chip", held: "chip chip-ok", cancelled: "chip chip-sand", no_show: "chip chip-warn", unknown: "chip chip-warn" };

export function StatusBadge({ status }: { status: VisitStatus }) {
  return <span className={STATUS_CHIP[status]}>{VISIT_STATUS_LABEL[status]}</span>;
}

/** The kind of a visit; her own title, when she gave one, is said beside it by the caller. */
export function KindBadge({ kind }: { kind: VisitKind }) {
  return <span className="chip chip-sand clinic-kind" data-kind={kind}>{VISIT_KIND_LABEL[kind]}</span>;
}

// ------------------------------------------------------------------ layout pieces

/** A quiet block for "nothing here yet", with what she can do about it. */
export function EmptyState({ title, children, actions }: { title: string; children?: ReactNode; actions?: ReactNode }) {
  return (
    <div className="clinic-empty">
      <h3>{title}</h3>
      {children && <div className="clinic-empty-text muted">{children}</div>}
      {actions && <div className="row clinic-empty-actions">{actions}</div>}
    </div>
  );
}

/** A titled card of a tab: a heading, something at its end (a count, a button), and the content. */
export function Section({ title, aside, children, label }: { title: string; aside?: ReactNode; children: ReactNode; label?: string }) {
  const id = useId();
  return (
    <section className="card clinic-section" aria-labelledby={label ? undefined : id} aria-label={label}>
      <header className="clinic-section-head">
        <h2 id={id}>{title}</h2>
        <span className="grow" />
        {aside}
      </header>
      {children}
    </section>
  );
}

// ------------------------------------------------------------------ a read that failed

/**
 * A read that failed, where the screen (a tab, a list, a dialog's body) has nothing else to
 * show: the message with the step the core chose for it, and always "לנסות שוב". The app's
 * error line offers that button only for a failure the core marked for it; for any other
 * one (a refusal, a fault to report, a locked vault) it would leave "הבנתי" alone, and after
 * it an empty screen. Here the way to read again stays, whatever the failure was.
 */
export function ReadError({ error, retry }: { error: string; retry: () => void }) {
  const failure = failureFor(error);
  // The cases in which the error line shows "לנסות שוב" by itself (see `ErrorLine`).
  const offered = failure === null || failure.action.kind === "retry" || failure.action.kind === "raise_cap";
  return (
    <div className="stack clinic-read-error">
      <ErrorLine error={error} retry={retry} />
      {!offered && (
        <p className="row clinic-read-again">
          <span className="muted">אפשר לקרוא את הנתונים שוב.</span>
          <button type="button" className="btn btn-small" onClick={retry}>לנסות שוב</button>
        </p>
      )}
    </div>
  );
}

// ------------------------------------------------------------------ form fields

/** A label over its control, with one line of help and, when there is one, what is wrong. */
export function Field({ id, label, hint, problem, children, className }: { id: string; label: string; hint?: ReactNode; problem?: string | null | undefined; children: ReactNode; className?: string | undefined }) {
  return (
    <div className={className ? `field ${className}` : "field"}>
      <label htmlFor={id}>{label}</label>
      {children}
      {hint && <p className="hint" id={`${id}-hint`}>{hint}</p>}
      {problem && <span className="error small" id={`${id}-problem`}>{problem}</span>}
    </div>
  );
}

const describedBy = (id: string, hint: unknown, problem: unknown) => [hint ? `${id}-hint` : "", problem ? `${id}-problem` : ""].filter(Boolean).join(" ") || undefined;

export function TextField(props: {
  id: string; label: string; value: string; onChange: (value: string) => void; hint?: ReactNode; problem?: string | null | undefined;
  maxLength?: number; placeholder?: string | undefined; autoFocus?: boolean; dir?: "ltr" | "rtl"; inputMode?: "text" | "tel" | "email" | "numeric";
  list?: string; className?: string;
}) {
  return (
    <Field id={props.id} label={props.label} hint={props.hint} problem={props.problem} className={props.className}>
      <input id={props.id} className={props.problem ? "input bad" : "input"} value={props.value} maxLength={props.maxLength}
        placeholder={props.placeholder} autoFocus={props.autoFocus} dir={props.dir} inputMode={props.inputMode} list={props.list}
        aria-invalid={props.problem ? true : undefined} aria-describedby={describedBy(props.id, props.hint, props.problem)}
        onChange={(e) => props.onChange(e.target.value)} />
    </Field>
  );
}

export function TextAreaField(props: {
  id: string; label: string; value: string; onChange: (value: string) => void; hint?: ReactNode; rows?: number; maxLength?: number;
  placeholder?: string; className?: string;
}) {
  return (
    <Field id={props.id} label={props.label} hint={props.hint} className={props.className}>
      <textarea id={props.id} className="textarea" rows={props.rows ?? 3} value={props.value} maxLength={props.maxLength} placeholder={props.placeholder}
        aria-describedby={describedBy(props.id, props.hint, null)} onChange={(e) => props.onChange(e.target.value)} />
    </Field>
  );
}

export function SelectField<T extends string>(props: {
  id: string; label: string; value: T; options: { value: T; label: string }[]; onChange: (value: T) => void; hint?: ReactNode; className?: string;
}) {
  return (
    <Field id={props.id} label={props.label} hint={props.hint} className={props.className}>
      <select id={props.id} className="select" value={props.value} aria-describedby={describedBy(props.id, props.hint, null)}
        onChange={(e) => props.onChange(e.target.value as T)}>
        {props.options.map((o) => <option key={o.value} value={o.value}>{o.label}</option>)}
      </select>
    </Field>
  );
}

/** What is wrong with an amount as typed; `null` when it is one (or empty and that is allowed). */
export function moneyProblem(text: string, optional = false): string | null {
  if (text.trim() === "") return optional ? null : "צריך לכתוב סכום.";
  return parseMoney(text) === null ? "כותבים סכום בשקלים, למשל 450 או 450.50, עד 100,000." : null;
}

/** An amount in shekels, typed as text ("450", "450.50"). The caller keeps the text and reads
 *  it with `parseMoney`; the problem shows once she left the field. */
export function MoneyField(props: { id: string; label: string; value: string; onChange: (text: string) => void; optional?: boolean; hint?: ReactNode; className?: string }) {
  const [touched, setTouched] = useState(false);
  const problem = touched ? moneyProblem(props.value, props.optional) : null;
  return (
    <Field id={props.id} label={props.label} hint={props.hint} problem={problem} className={props.className}>
      <span className="clinic-money-input">
        <input id={props.id} className={problem ? "input num bad" : "input num"} dir="ltr" inputMode="decimal" value={props.value}
          aria-invalid={problem ? true : undefined} aria-describedby={describedBy(props.id, props.hint, problem)}
          onChange={(e) => props.onChange(e.target.value)} onBlur={() => setTouched(true)} />
        <span aria-hidden="true">₪</span>
      </span>
    </Field>
  );
}

/** The text of an amount for a `MoneyField`: "450", and "" for none. */
export const moneyText = moneyInput;

/** A time of day typed as "9:30" or "0930"; the caller keeps the text and reads it with `parseTime`. */
export function TimeField(props: { id: string; label: string; value: string; onChange: (text: string) => void; className?: string }) {
  const [touched, setTouched] = useState(false);
  const problem = touched && parseTime(props.value) === null ? "כותבים שעה, למשל 9:30." : null;
  return (
    <Field id={props.id} label={props.label} problem={problem} className={props.className}>
      <input id={props.id} className={problem ? "input num clinic-time-input bad" : "input num clinic-time-input"} dir="ltr" inputMode="numeric"
        placeholder="09:30" value={props.value} aria-invalid={problem ? true : undefined} aria-describedby={describedBy(props.id, null, problem)}
        onChange={(e) => props.onChange(e.target.value)} onBlur={() => setTouched(true)} />
    </Field>
  );
}

/** A plain checkbox with its words beside it. */
export function CheckField({ id, label, checked, onChange, hint }: { id: string; label: string; checked: boolean; onChange: (on: boolean) => void; hint?: ReactNode }) {
  return (
    <div className="clinic-check">
      <input id={id} type="checkbox" checked={checked} aria-describedby={hint ? `${id}-hint` : undefined} onChange={(e) => onChange(e.target.checked)} />
      <label htmlFor={id}>{label}</label>
      {hint && <p className="hint" id={`${id}-hint`}>{hint}</p>}
    </div>
  );
}

// ------------------------------------------------------------------ confirming

/** "Are you sure?" for a step that is not undone by one click. The action's own failure is
 *  shown in the dialog, which stays open. */
export function ConfirmDialog(props: {
  title: string; children: ReactNode; action: string; danger?: boolean; onClose: () => void; onConfirm: () => Promise<unknown>;
}) {
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  async function confirm() {
    setBusy(true);
    setError(null);
    try {
      await props.onConfirm();
    } catch (e) {
      setError((e as { message?: string }).message ?? "הפעולה לא בוצעה. אפשר לנסות שוב.");
      setBusy(false);
    }
  }
  return (
    <Dialog narrow title={props.title} onClose={props.onClose}
      footer={<>
        <button type="button" className="btn" onClick={props.onClose}>ביטול</button>
        <button type="button" className={props.danger ? "btn btn-danger" : "btn btn-primary"} disabled={busy} onClick={() => void confirm()}>{props.action}</button>
      </>}>
      <div className="stack">{props.children}</div>
      <ErrorLine error={error} />
    </Dialog>
  );
}

// ------------------------------------------------------------------ what needs her

/**
 * "מה דורש אותי": each alert as a line, the patient's name as a button that opens the card,
 * then the sentence the clinic wrote (which never carries the name). `onVisit` and `onTask`
 * add a step right on the line, when the tab has one to offer. `onCard`: the list is shown on
 * this patient's own card, so their name is plain text there, not a link to where she is.
 * An alert about no patient (rows that were not read, D-178) has no name at all.
 */
export function AlertList({ alerts, onVisit, onTask, empty, onCard }: {
  alerts: Alert[];
  onVisit?: (alert: Alert, visitId: string) => void;
  onTask?: (alert: Alert, taskId: string) => void;
  empty?: ReactNode;
  onCard?: string;
}) {
  if (alerts.length === 0) return <p className="muted clinic-quiet">{empty ?? "אין כרגע שום דבר שמחכה."}</p>;
  return (
    <ul className="clinic-list clinic-alerts">
      {alerts.map((a) => {
        const visitId = a.visit_id;
        const taskId = a.task_id;
        return (
          <li key={`${a.kind}-${a.patient_id ?? ""}-${visitId ?? ""}-${taskId ?? ""}-${a.since}`} className="clinic-alert">
            <span className="clinic-alert-icon"><WarnIcon /></span>
            <div className="grow stack clinic-alert-body">
              <span className="row clinic-alert-head">
                {a.patient_id !== null && <PatientName name={a.patient_name} id={a.patient_id === onCard ? null : a.patient_id} strong />}
                <span className="chip chip-warn">{ALERT_KIND_LABEL[a.kind]}</span>
              </span>
              <span>{a.text}</span>
            </div>
            {visitId !== null && onVisit && (
              <button type="button" className="btn btn-small" onClick={() => onVisit(a, visitId)}>
                {a.kind === "no_note" ? "לכתוב סיכום" : "לפתוח את הפגישה"}
              </button>
            )}
            {taskId !== null && onTask && <button type="button" className="btn btn-small" onClick={() => onTask(a, taskId)}>בוצע</button>}
          </li>
        );
      })}
    </ul>
  );
}

// ------------------------------------------------------------------ a visit on a list

/**
 * One visit as a row: its time, who it is with (or her own title, for time she kept for
 * herself), its kind and status, where, and the quick actions the tab gives it. `onOpen`
 * makes the time a button that opens the visit. The button's name starts with what it shows
 * (the date, when it is shown, and the hours), so it can be asked for by what she sees, and
 * two visits at the same hour of different days are not called the same.
 */
export function VisitRow({ view, actions, onOpen, showDate = false }: { view: VisitView; actions?: ReactNode; onOpen?: () => void; showDate?: boolean }) {
  const v = view.visit;
  const shown = useShownName();
  const discreet = useDiscreet();
  const span = formatSpan(v.start, v.minutes);
  const who = v.kind === "block" ? (v.title || VISIT_KIND_LABEL.block) : shown(view.patient_name);
  const time = <span className="num clinic-visit-time" dir="ltr">{span}</span>;
  const openName = `${showDate ? `${formatDate(v.date)} ` : ""}${span}, פתיחת הפגישה של ${who}`;
  // What happened, in her words, may say more than a list should while a family looks on: in
  // discreet mode it stays in the visit's dialog.
  const more = [v.kind !== "block" ? v.title : "", v.place, discreet ? "" : v.status_note, v.note].filter(Boolean).join(" · ");
  return (
    <li className={v.status === "cancelled" ? "clinic-visit clinic-visit-off" : "clinic-visit"} data-status={v.status}>
      {onOpen
        ? <button type="button" className="clinic-visit-when clinic-visit-open" aria-label={openName} onClick={onOpen}>{showDate && <span>{formatDate(v.date)}</span>}{time}</button>
        : <span className="clinic-visit-when">{showDate && <span>{formatDate(v.date)}</span>}{time}</span>}
      <div className="grow stack clinic-visit-body">
        <span className="row clinic-visit-head">
          {v.kind === "block"
            ? <span className="clinic-name clinic-name-strong">{who}</span>
            : <PatientName name={view.patient_name} id={v.patient_id} strong />}
          <KindBadge kind={v.kind} />
          {v.kind !== "block" && <StatusBadge status={v.status} />}
          {view.has_note && <span className="chip chip-ok">יש סיכום</span>}
          {view.overlaps && <span className="chip chip-warn">חופפת לפגישה אחרת</span>}
        </span>
        {more !== "" && <span className="muted small clinic-visit-more">{more}</span>}
      </div>
      {actions && <div className="row clinic-visit-actions">{actions}</div>}
    </li>
  );
}

// ------------------------------------------------------------------ long lists

/** A long list in pages: what to show now, and the pager's state. A list that got shorter
 *  never leaves her on a page that is no longer there. */
export function usePaged<T>(items: T[], size = 20, resetKey = ""): { shown: T[]; page: number; pages: number; setPage: (page: number) => void } {
  // The page is remembered with the list it belongs to: another month or filter starts at its first page.
  const [at, setAt] = useState({ key: resetKey, page: 0 });
  const asked = at.key === resetKey ? at.page : 0;
  const setPage = useCallback((page: number) => setAt({ key: resetKey, page }), [resetKey]);
  const pages = Math.max(1, Math.ceil(items.length / size));
  const page = Math.min(asked, pages - 1);
  const shown = useMemo(() => items.slice(page * size, (page + 1) * size), [items, page, size]);
  return { shown, page, pages, setPage };
}

/** "הקודם · עמוד 2 מתוך 5 · הבא"; nothing at all when everything fits on one page. */
export function Pager({ page, pages, onPage, label = "עמודים" }: { page: number; pages: number; onPage: (page: number) => void; label?: string }) {
  if (pages <= 1) return null;
  return (
    <nav className="row clinic-pager" aria-label={label}>
      <button type="button" className="btn btn-small" disabled={page === 0} onClick={() => onPage(page - 1)}>הקודם</button>
      <span className="muted small" role="status">עמוד {page + 1} מתוך {pages}</span>
      <button type="button" className="btn btn-small" disabled={page >= pages - 1} onClick={() => onPage(page + 1)}>הבא</button>
    </nav>
  );
}

// ------------------------------------------------------------------ history

const HISTORY_ACTION: Record<string, string> = { created: "נוצר", changed: "שונה", voided: "בוטל", deleted: "נמחק" };

function historyWhen(at: number): string {
  const d = new Date(at * 1000);
  const two = (n: number) => String(n).padStart(2, "0");
  return `${two(d.getDate())}.${two(d.getMonth() + 1)}.${d.getFullYear()}, ${two(d.getHours())}:${two(d.getMinutes())}`;
}

/** Every saved state of a record, newest first, each as the lines the clinic wrote for it.
 *  The lines carry names, the whole record and who paid: in discreet mode only what was done
 *  and when is shown. */
export function HistoryEntries({ entries }: { entries: HistoryEntry[] }) {
  const discreet = useDiscreet();
  if (entries.length === 0) return <p className="muted clinic-quiet">עוד לא נשמרו שינויים.</p>;
  return (
    <>
      {discreet && <p className="hint clinic-history-hidden">פרטי השינויים מוסתרים במצב דיסקרטי.</p>}
      <ol className="clinic-list clinic-history">
        {[...entries].reverse().map((e, i) => (
          <li key={`${e.at}-${i}`} className="clinic-history-entry">
            <span className="row"><span className="chip chip-sand">{HISTORY_ACTION[e.action] ?? e.action}</span><span className="muted small num" dir="ltr">{historyWhen(e.at)}</span></span>
            {!discreet && <span className="clinic-history-lines">{e.lines.join("\n")}</span>}
          </li>
        ))}
      </ol>
    </>
  );
}

/** "היסטוריית שינויים" of one record (a visit, a session note, a charge, a payment): closed
 *  until she asks, and read only then. */
export function RecordHistory({ recordId }: { recordId: string }) {
  const [open, setOpen] = useState(false);
  const history = useHistory(open ? recordId : null);
  const reload = useReloadClinic();
  const panel = useId();
  return (
    <div className="stack clinic-history-box">
      <button type="button" className="link-btn" aria-expanded={open} aria-controls={panel} onClick={() => setOpen((o) => !o)}>היסטוריית שינויים</button>
      <div id={panel} hidden={!open}>
        {open && (history.data ? <HistoryEntries entries={history.data} />
          : history.error ? <ReadError error={history.error.message} retry={reload} />
          : <Skeleton lines={3} label="טוען את ההיסטוריה" />)}
      </div>
    </div>
  );
}

// ------------------------------------------------------------------ choosing a patient

/** How many matches the picker lists at once; typing narrows them. */
const PICKER_ROWS = 8;

/**
 * Chooses a patient by name: a field the size of a name, and under it a short list that opens
 * when she types, clicks the field or presses the down arrow, and closes once she chose, pressed
 * Escape or moved on (Einat, 2026-11-17: the list as wide as the window was heavy on the eye).
 * The search waits a moment after she types; families whose work ended are offered only when
 * she searches for them. Once one is chosen the field shows the name, with "החלפה" beside it;
 * `locked`: the name only, for a record that stays on its card.
 */
export function PatientPicker({ id, value, onChange, autoFocus = false, locked = false }: {
  id: string; value: string | null; onChange: (patientId: string | null) => void; autoFocus?: boolean; locked?: boolean;
}) {
  const now = useToday();
  const rows = usePatients(now.date);
  const [text, setText] = useState("");
  const [open, setOpen] = useState(false);
  const box = useRef<HTMLDivElement>(null);
  const input = useRef<HTMLInputElement>(null);
  const query = useDebounced(text.trim(), 200);
  const all = rows.data ?? [];
  const chosen = value === null ? null : (all.find((r) => r.patient.id === value) ?? null);
  if (value !== null) {
    return (
      <div className="field">
        <span className="label" id={`${id}-label`}>מטופל</span>
        <span className="row clinic-picked" role="group" aria-labelledby={`${id}-label`}>
          {rows.isPending ? <span className="muted">טוען…</span> : <PatientName name={chosen?.patient.name} strong />}
          {!locked && <button type="button" className="link-btn" onClick={() => onChange(null)}>החלפה</button>}
        </span>
      </div>
    );
  }
  const matches = all
    .filter((r) => (query === "" ? r.patient.status !== "closed" : r.patient.name.includes(query)))
    .slice(0, PICKER_ROWS);
  const choices = () => Array.from(box.current?.querySelectorAll<HTMLButtonElement>(".clinic-picker-row") ?? []);
  // The arrows walk the list and back up to the field; Escape closes it and leaves her typing.
  const keys = (e: KeyboardEvent<HTMLDivElement>) => {
    if (e.key === "Escape" && open) {
      e.stopPropagation();
      setOpen(false);
      input.current?.focus();
      return;
    }
    if (e.key !== "ArrowDown" && e.key !== "ArrowUp") return;
    e.preventDefault();
    if (!open) {
      if (e.key === "ArrowDown") setOpen(true);
      return;
    }
    const list = choices();
    const at = list.findIndex((b) => b === document.activeElement);
    const next = e.key === "ArrowDown" ? Math.min(at + 1, list.length - 1) : at - 1;
    if (next < 0) input.current?.focus();
    else list[next]?.focus();
  };
  return (
    <div className="field clinic-picker" ref={box} onKeyDown={keys}
      onBlur={(e) => { if (!box.current?.contains(e.relatedTarget as Node | null)) setOpen(false); }}>
      <label htmlFor={id}>מטופל</label>
      <input id={id} ref={input} className="input" value={text} placeholder="חיפוש לפי שם" autoFocus={autoFocus} autoComplete="off"
        aria-describedby={open ? `${id}-results` : undefined}
        onChange={(e) => { setText(e.target.value); setOpen(true); }} onClick={() => setOpen(true)} />
      {open && (
        // A click on a name keeps the focus in the field until the name is chosen (WebKit does
        // not focus a clicked button, and the list would close under the pointer).
        <div id={`${id}-results`} className="clinic-picker-results" onMouseDown={(e) => e.preventDefault()}>
          {rows.isPending ? <Skeleton lines={2} label="טוען את המטופלים" />
            : matches.length === 0 ? <p className="hint" role="status">{all.length === 0 ? "עוד אין מטופלים בקליניקה. אפשר להוסיף מטופל, ואז לקבוע לו פגישה." : "לא נמצא מטופל בשם הזה."}</p>
            : (
              <ul className="clinic-list clinic-picker-list" aria-label="מטופלים לבחירה">
                {matches.map((r) => (
                  <li key={r.patient.id}>
                    <button type="button" className="clinic-picker-row" onClick={() => { setOpen(false); onChange(r.patient.id); }}>
                      <PatientName name={r.patient.name} />
                      {r.patient.status !== "active" && <span className="chip chip-sand">{PATIENT_STATUS_LABEL[r.patient.status]}</span>}
                    </button>
                  </li>
                ))}
              </ul>
            )}
        </div>
      )}
    </div>
  );
}
