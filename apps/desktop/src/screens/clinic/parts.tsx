// The clinic's shared building blocks: how a patient's name, an amount, a visit and an alert
// look on every tab, so the tabs only arrange them. Everything shown is plain text.
import { createContext, useCallback, useContext, useId, useEffect, useLayoutEffect, useMemo, useRef, useState, type KeyboardEvent, type ReactNode } from "react";
import { useDiscreet } from "../../discreet";
import { ChevronIcon, Dialog, ErrorLine, Skeleton, WarnIcon } from "../../components/ui";
import { failureFor } from "../../errors";
import type { Alert } from "../../ipc/generated/Alert";
import type { AlertKind } from "../../ipc/generated/AlertKind";
import type { HistoryEntry } from "../../ipc/generated/HistoryEntry";
import type { VisitKind } from "../../ipc/generated/VisitKind";
import type { VisitStatus } from "../../ipc/generated/VisitStatus";
import type { VisitView } from "../../ipc/generated/VisitView";
import { useHistory, usePatients, useReloadClinic, useToday } from "./clinicData";
import {
  ALERT_KIND_LABEL, formatDate, formatSpan, initials, ownVisit, moneyInput, moneyNumber, parseMoney, parseTime, PATIENT_STATUS_LABEL,
  VISIT_KIND_LABEL, VISIT_STATUS_LABEL,
} from "./format";
import { searchForm } from "../../searchForm";
import { rememberLine } from "./RememberList";
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
export function EmptyState({ title, children, actions, top }: {
  title: string; children?: ReactNode; actions?: ReactNode;
  /** Not under a section's heading: its title is the next level after the page's. */
  top?: boolean;
}) {
  return (
    <div className="clinic-empty">
      {top ? <h2>{title}</h2> : <h3>{title}</h3>}
      {children && <div className="clinic-empty-text muted">{children}</div>}
      {actions && <div className="row clinic-empty-actions">{actions}</div>}
    </div>
  );
}

/** A titled card of a tab: a heading, something at its end (a count, a button), and the content. */
export function Section({ title, aside, children, label, hidden }: { title: string; aside?: ReactNode; children: ReactNode; label?: string; hidden?: boolean }) {
  const id = useId();
  return (
    <section className="card clinic-section" aria-labelledby={label ? undefined : id} aria-label={label} hidden={hidden}>
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

/** The kinds of "מה דורש אותי" in the order she would deal with them: rows that were not read
 *  first (every figure is without them, D-178), then what is quick to close, then what takes
 *  a decision. */
export const ALERT_ORDER: AlertKind[] = ["unreadable", "unmarked", "no_note", "task_overdue", "feedback_soon", "materials_missing", "report_due", "debt", "no_next_visit", "waiting"];

/** How many lines of a kind show before "להציג את כולן". */
export const ALERTS_SHOWN = 4;

/** The alerts by kind, each kind keeping the order the clinic gave (oldest first). */
export function alertGroups(alerts: Alert[]): { kind: AlertKind; alerts: Alert[] }[] {
  // A kind that is not in the order still gets its group, after the others: no alert is dropped.
  const kinds = [...ALERT_ORDER, ...alerts.map((a) => a.kind).filter((kind) => !ALERT_ORDER.includes(kind))];
  return [...new Set(kinds)]
    .map((kind) => ({ kind, alerts: alerts.filter((a) => a.kind === kind) }))
    .filter((g) => g.alerts.length > 0);
}

/**
 * "מה דורש אותי": each alert as a line, the patient's name as a button that opens the card,
 * then the sentence the clinic wrote (which never carries the name). `onVisit` and `onTask`
 * add a step right on the line, when the tab has one to offer. `onCard`: the list is shown on
 * this patient's own card, so their name is left out: it is the page's heading (D-204), and a
 * link to where she already is would lead nowhere.
 * An alert about no patient (rows that were not read, D-178) has no name at all.
 * `grouped`: the list sits under its kind's heading (`AlertGroups`), so a line has no icon and
 * no kind of its own, and the sentence is quiet. `holdOf`: the step that marks the visit of a
 * "פגישה שלא סומנה" as held, when the screen offers one ("התקיימה" in place of opening it).
 */
export function AlertList({ alerts, onVisit, onTask, empty, onCard, grouped = false, holdOf }: {
  alerts: Alert[];
  onVisit?: ((alert: Alert, visitId: string) => void) | undefined;
  onTask?: (alert: Alert, taskId: string) => void;
  empty?: ReactNode;
  onCard?: string;
  grouped?: boolean;
  holdOf?: ((visitId: string) => (() => void) | null) | undefined;
}) {
  if (alerts.length === 0) return <p className="muted clinic-quiet">{empty ?? "אין כרגע שום דבר שמחכה."}</p>;
  return (
    <ul className="clinic-list clinic-alerts">
      {alerts.map((a) => {
        const visitId = a.visit_id;
        const taskId = a.task_id;
        const hold = a.kind === "unmarked" && visitId !== null ? holdOf?.(visitId) ?? null : null;
        return (
          <li key={`${a.kind}-${a.patient_id ?? ""}-${visitId ?? ""}-${taskId ?? ""}-${a.since}`} className="clinic-alert">
            {!grouped && <span className="clinic-alert-icon"><WarnIcon /></span>}
            <div className="grow stack clinic-alert-body">
              {(a.patient_id !== null || !grouped) && (
                <span className="row clinic-alert-head">
                  {a.patient_id !== null && a.patient_id !== onCard && <PatientName name={a.patient_name} id={a.patient_id} strong />}
                  {!grouped && <span className="chip chip-warn">{ALERT_KIND_LABEL[a.kind]}</span>}
                </span>
              )}
              {grouped ? <span className="muted">{a.text}</span> : <span>{a.text}</span>}
            </div>
            {hold
              ? <button type="button" className="btn btn-small" onClick={hold}>התקיימה</button>
              : visitId !== null && onVisit && (
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

/** The kinds that tell her of something coming, not of something late: a quiet chip. */
const UPCOMING: AlertKind[] = ["feedback_soon", "materials_missing"];

/**
 * "מה דורש אותי" by topic (review item 4 of 18.11.2026, D-204): one row per kind that opens and
 * folds, with its count, and under it the lines of that kind (`ALERTS_SHOWN` of them, then
 * "להציג את כל ה-N"). The first kind is open; one she opened or folded stays so. When the line
 * she was on goes (she dealt with it), the focus goes to its kind's row, or, when the whole
 * kind went, to the row now in its place, or to the empty line (WCAG 2.4.3).
 */
export function AlertGroups({ alerts, onVisit, holdOf, empty }: {
  alerts: Alert[];
  onVisit?: (a: Alert, visitId: string) => void;
  holdOf?: (visitId: string) => (() => void) | null;
  empty: ReactNode;
}) {
  const groups = useMemo(() => alertGroups(alerts), [alerts]);
  /** A kind she opened or folded herself; otherwise the first one is open. */
  const [folds, setFolds] = useState<Partial<Record<AlertKind, boolean>>>({});
  /** A kind whose every line she asked to see. */
  const [all, setAll] = useState<Partial<Record<AlertKind, boolean>>>({});
  const base = useId();
  const toggles = useRef(new Map<AlertKind, HTMLButtonElement>());
  const emptyRef = useRef<HTMLParagraphElement>(null);
  /** Where the focus was last inside a kind, to find a place for it when that place goes. */
  const last = useRef<{ el: HTMLElement; kind: AlertKind; index: number } | null>(null);

  useLayoutEffect(() => {
    const was = last.current;
    if (!was || was.el.isConnected) return;
    // Only when the focus fell with it: a click elsewhere (or on nothing) keeps hers.
    const active = document.activeElement;
    if (active && active !== document.body) return;
    last.current = null;
    const kinds = groups.map((g) => g.kind);
    const sameIndex = kinds[Math.min(was.index, kinds.length - 1)];
    (toggles.current.get(was.kind) ?? (sameIndex === undefined ? undefined : toggles.current.get(sameIndex)) ?? emptyRef.current)?.focus();
  }, [groups]);

  if (groups.length === 0) return <p ref={emptyRef} className="muted clinic-quiet" tabIndex={-1}>{empty}</p>;
  return (
    <div className="clinic-need-groups">
      {groups.map((g, i) => {
        const n = g.alerts.length;
        const label = ALERT_KIND_LABEL[g.kind] ?? "דורש תשומת לב";
        const open = folds[g.kind] ?? i === 0;
        const bodyId = `${base}-${g.kind}`;
        return (
          <div className="clinic-need-group" key={g.kind} onFocusCapture={(e) => { last.current = { el: e.target as HTMLElement, kind: g.kind, index: i }; }}>
            <h3 className="clinic-need-title">
              <button type="button" className="clinic-need-head" aria-expanded={open} aria-controls={bodyId}
                ref={(el) => { if (el) toggles.current.set(g.kind, el); else toggles.current.delete(g.kind); }}
                onClick={() => setFolds((f) => ({ ...f, [g.kind]: !open }))}>
                <span className={UPCOMING.includes(g.kind) ? "chip clinic-need-count" : "chip chip-warn clinic-need-count"} aria-hidden="true">{n}</span>
                {/* The name is one piece of text, "פגישה שלא סומנה: 2": a browser joins pieces in
                    separate boxes with a space ("… : 2"), and a hidden piece is a box of its own. */}
                <span className="grow">
                  <span aria-hidden="true">{label}</span>
                  <span className="visually-hidden">{`${label}: ${n}`}</span>
                </span>
                <span className="clinic-need-chevron"><ChevronIcon /></span>
              </button>
            </h3>
            <div id={bodyId} className="clinic-need-body" hidden={!open}>
              {open && (
                <>
                  <AlertList alerts={all[g.kind] ? g.alerts : g.alerts.slice(0, ALERTS_SHOWN)} grouped onVisit={onVisit} holdOf={holdOf} />
                  {n > ALERTS_SHOWN && (
                    <button type="button" className="link-btn" aria-expanded={all[g.kind] ?? false} onClick={() => setAll((x) => ({ ...x, [g.kind]: !x[g.kind] }))}>
                      {all[g.kind] ? "להציג פחות" : `להציג את כל ה-${n}`}
                    </button>
                  )}
                </>
              )}
            </div>
          </div>
        );
      })}
    </div>
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
export function VisitRow({ view, actions, onOpen, showDate = false, quietPlanned = false, soon, onCard = false }: {
  view: VisitView;
  /** On the patient's own card: the name is the page's title, so a row does not repeat it (and
   *  does not link the card to itself). */
  onCard?: boolean;
  actions?: ReactNode;
  onOpen?: () => void;
  showDate?: boolean;
  /** A planned visit has no chip: "מתוכננת" is said to a screen reader only (as the calendar grid does). */
  quietPlanned?: boolean;
  /** When it is, said last on its quiet line ("בעוד 48 דקות"). */
  soon?: string;
}) {
  const v = view.visit;
  const shown = useShownName();
  const discreet = useDiscreet();
  const span = formatSpan(v.start, v.minutes);
  const who = ownVisit(v) ? (v.title || VISIT_KIND_LABEL[v.kind]) : shown(view.patient_name);
  const time = <span className="num clinic-visit-time" dir="ltr">{span}</span>;
  // A space between the date and the hours: they stand one above the other, and the text she
  // sees ("15.11.2026 09:00–09:50") is the start of the button's name, not "15.11.202609:00".
  const date = showDate && <><span>{formatDate(v.date)}</span>{" "}</>;
  const openName = `${showDate ? `${formatDate(v.date)} ` : ""}${span}, פתיחת הפגישה של ${who}`;
  // What happened, in her words, may say more than a list should while a family looks on: in
  // discreet mode it stays in the visit's dialog.
  const more = [!ownVisit(v) ? v.title : "", v.place, discreet ? "" : v.status_note, v.note, soon ?? ""].filter(Boolean).join(" · ");
  // What not to forget, while the visit is ahead (D-207); in discreet mode only how many.
  const remember = v.status === "planned" ? rememberLine(v.remember, discreet) : "";
  return (
    <li className={v.status === "cancelled" ? "clinic-visit clinic-visit-off" : "clinic-visit"} data-status={v.status} data-kind={v.kind}>
      {onOpen
        ? <button type="button" className="clinic-visit-when clinic-visit-open" aria-label={openName} onClick={onOpen}>{date}{time}</button>
        : <span className="clinic-visit-when">{date}{time}</span>}
      <div className="grow stack clinic-visit-body">
        <span className="row clinic-visit-head">
          {ownVisit(v)
            ? <span className="clinic-name clinic-name-strong">{who}</span>
            : !onCard && <PatientName name={view.patient_name} id={v.patient_id} strong />}
          <KindBadge kind={v.kind} />
          {!ownVisit(v) && (quietPlanned && v.status === "planned"
            ? <span className="visually-hidden">{VISIT_STATUS_LABEL.planned}</span>
            : <StatusBadge status={v.status} />)}
          {view.has_note && <span className="chip chip-ok">יש סיכום</span>}
          {view.overlaps && <span className="chip chip-warn">חופפת לפגישה אחרת</span>}
        </span>
        {more !== "" && <span className="muted small clinic-visit-more">{more}</span>}
        {remember !== "" && <span className="small clinic-visit-remember">{remember}</span>}
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
 * The patients whose name holds what she typed, as the patients' search reads it (a final
 * letter is its regular form: "שלומ" finds "שלום"). A name that starts with it comes first,
 * then one with a word that starts with it, then the rest: "ת" lists תמר before איתי. In
 * discreet mode, where only initials are shown, this order is what tells them apart.
 */
export function pickerMatches<R extends { patient: { name: string; status: string } }>(all: readonly R[], typed: string): R[] {
  const query = searchForm(typed.trim());
  if (query === "") return all.filter((r) => r.patient.status !== "closed");
  const rank = (name: string) => {
    const at = searchForm(name).indexOf(query);
    if (at < 0) return -1;
    if (at === 0) return 0;
    return /[\s\-־'"׳]/.test(name.charAt(at - 1)) ? 1 : 2;
  };
  return all
    .map((r, i) => ({ r, i, rank: rank(r.patient.name) }))
    .filter((x) => x.rank >= 0)
    .sort((a, b) => a.rank - b.rank || a.i - b.i)
    .map((x) => x.r);
}

/**
 * Chooses a patient by name: a field the size of a name, and under it a short list that opens
 * when she types, clicks the field or presses the down arrow, and closes once she chose, pressed
 * Escape or moved on (Einat, 2026-11-17: the list as wide as the window was heavy on the eye).
 * The focus stays in the field: the arrows mark a name, Enter or a click chooses it (a combobox,
 * so a screen reader says how many names there are and which one is marked). The list follows
 * every letter; families whose work ended are offered only when she searches for them. Enter
 * while names are listed never sends the form: one name left is chosen, otherwise the first is marked.
 * Once one is chosen the field shows the name, with "החלפה" beside it. The keyboard stays on the
 * chosen name, where Enter sends the form as from any field (a follow-up "with a child, or just
 * Enter"), and comes back to the empty field after "החלפה"; `locked`: the name only, for a record
 * that stays on its card.
 */
export function PatientPicker({ id, value, onChange, autoFocus = false, locked = false, label = "מטופל" }: {
  id: string; value: string | null; onChange: (patientId: string | null) => void; autoFocus?: boolean; locked?: boolean;
  /** The field's name, "מטופל" unless the record may also stand without one. */
  label?: string;
}) {
  const now = useToday();
  const rows = usePatients(now.date);
  const [text, setText] = useState("");
  const [open, setOpen] = useState(false);
  const [active, setActive] = useState(-1);
  // Where the keyboard goes when the field and the chosen name swap places (the one that had the
  // focus is gone): to the chosen name after a choice, back to the field after "החלפה". Not to
  // "החלפה" itself, where the next Enter would undo the choice.
  const focusNext = useRef<"chosen" | "field" | null>(null);
  const picked = useRef<HTMLSpanElement>(null);
  const field = useRef<HTMLInputElement>(null);
  const all = rows.data ?? [];
  const chosenName = value === null ? undefined : all.find((r) => r.patient.id === value)?.patient.name;
  const query = text.trim();
  const matches = pickerMatches(all, query).slice(0, PICKER_ROWS);
  const marked = open && active >= 0 && active < matches.length ? active : -1;
  const optionId = (at: number) => `${id}-option-${at}`;
  // The list floats over what is under the field: inside a short window that scrolls, the window
  // scrolls just enough to show it, and then the marked name.
  useEffect(() => {
    if (open) document.getElementById(`${id}-results`)?.scrollIntoView({ block: "nearest" });
  }, [open, matches.length, id]);
  useEffect(() => {
    if (marked >= 0) document.getElementById(`${id}-option-${marked}`)?.scrollIntoView({ block: "nearest" });
  }, [marked, id]);
  useEffect(() => {
    if (focusNext.current === "chosen") picked.current?.focus();
    else if (focusNext.current === "field") field.current?.focus();
    focusNext.current = null;
  }, [value]);
  if (value !== null) {
    return (
      <div className="field">
        <span className="label" id={`${id}-label`}>{label}</span>
        <span ref={picked} className="row clinic-picked" role="group" aria-labelledby={`${id}-label`} aria-describedby={`${id}-name`}
          tabIndex={-1} onKeyDown={(e) => {
            if (e.key !== "Enter" || e.target !== e.currentTarget) return;
            e.preventDefault();
            // As Enter in a field does: through the form's own button, and not while it is greyed
            // out (a save under way) or for a held-down key.
            if (e.repeat) return;
            const go = e.currentTarget.closest("form")?.querySelector<HTMLButtonElement>('button[type="submit"]');
            if (go && !go.disabled) go.click();
          }}>
          <span id={`${id}-name`}>{rows.isPending ? <span className="muted">טוען…</span> : <PatientName name={chosenName} strong />}</span>
          {!locked && <button type="button" className="link-btn" onClick={() => { focusNext.current = "field"; onChange(null); }}>החלפה</button>}
        </span>
      </div>
    );
  }
  const choose = (patientId: string) => {
    setOpen(false);
    setActive(-1);
    setText("");
    focusNext.current = "chosen";
    onChange(patientId);
  };
  const shown = open && !rows.isPending && matches.length > 0;
  const keys = (e: KeyboardEvent<HTMLInputElement>) => {
    if (e.nativeEvent.isComposing) return;
    if (e.key === "Escape" && open) {
      // Closes the list and leaves the window open.
      e.stopPropagation();
      setOpen(false);
      setActive(-1);
    } else if (e.key === "ArrowDown" || e.key === "ArrowUp") {
      e.preventDefault();
      const last = matches.length - 1;
      if (!open) {
        setOpen(true);
        setActive(e.key === "ArrowDown" ? 0 : last);
      } else if (e.key === "ArrowDown") {
        setActive(Math.min(marked + 1, last));
      } else {
        setActive(marked - 1); // up from the first name: back to what she typed, nothing marked
      }
    } else if (e.key === "Enter" && shown) {
      // Not the form's Enter while names are listed: a follow-up is not saved without the child she was looking for.
      e.preventDefault();
      const r = marked >= 0 ? matches[marked] : matches.length === 1 ? matches[0] : undefined;
      if (r) choose(r.patient.id);
      else setActive(0);
    }
  };
  return (
    <div className="field clinic-picker">
      <label htmlFor={id}>{label}</label>
      <input id={id} ref={field} className="input" value={text} placeholder="חיפוש לפי שם" autoFocus={autoFocus} autoComplete="off"
        role="combobox" aria-autocomplete="list" aria-expanded={shown}
        aria-controls={shown ? `${id}-list` : undefined} aria-activedescendant={shown && marked >= 0 ? optionId(marked) : undefined}
        aria-describedby={open && !shown ? `${id}-results` : undefined}
        onChange={(e) => { setText(e.target.value); setOpen(true); setActive(-1); }} onClick={() => setOpen(true)}
        onKeyDown={keys} onBlur={() => { setOpen(false); setActive(-1); }} />
      {open && (
        // A press on a name keeps the focus in the field until the name is chosen.
        <div id={`${id}-results`} className="clinic-picker-results" onMouseDown={(e) => e.preventDefault()}>
          {rows.isPending ? <Skeleton lines={2} label="טוען את המטופלים" />
            : matches.length === 0 ? <p className="hint" role="status">{all.length === 0 ? "עוד אין מטופלים בקליניקה. אפשר להוסיף מטופל, ואז לקבוע לו פגישה." : "לא נמצא מטופל בשם הזה."}</p>
            : (
              // Not a stop for Tab (a list that scrolls would be one): Tab goes on to the next field.
              <ul id={`${id}-list`} role="listbox" tabIndex={-1} className="clinic-list clinic-picker-list" aria-label="מטופלים לבחירה">
                {matches.map((r, at) => (
                  // The field keeps the keyboard; a click here is the same as Enter on a marked name.
                  <li key={r.patient.id} id={optionId(at)} role="option" aria-selected={at === marked} className="clinic-picker-row"
                    onClick={() => choose(r.patient.id)}>
                    <PatientName name={r.patient.name} />
                    {r.patient.status !== "active" && <span className="chip chip-sand">{PATIENT_STATUS_LABEL[r.patient.status]}</span>}
                  </li>
                ))}
              </ul>
            )}
        </div>
      )}
    </div>
  );
}
