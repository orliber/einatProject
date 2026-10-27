// Small shared pieces: icons, dialog frame, marked text, toast.
import { useContext, useEffect, useRef, useState, type ReactNode } from "react";
import { useQueryClient } from "@tanstack/react-query";
import { AppContext } from "../App";
import { failureFor, rememberFailure, replayFor } from "../errors";
import { asUiError, ipc } from "../ipc/client";
import { SETTINGS_PANES, type SettingsPane } from "../screens/settingsPanes";
import { BugReportDialog } from "./BugReport";
import type { Segment } from "../ipc/generated/Segment";

export function LockIcon({ size = 18, color = "currentColor" }: { size?: number; color?: string }) {
  return (
    <svg width={size} height={size} viewBox="0 0 24 24" fill="none" stroke={color} strokeWidth="1.9"
      strokeLinecap="round" strokeLinejoin="round" aria-hidden="true">
      <rect x="5" y="11" width="14" height="10" rx="2" />
      <path d="M8 11V7a4 4 0 0 1 8 0v4" />
    </svg>
  );
}

export function ShieldIcon({ size = 22 }: { size?: number }) {
  return (
    <svg width={size} height={size} viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2"
      strokeLinecap="round" strokeLinejoin="round" aria-hidden="true">
      <path d="M12 22s8-4 8-10V5l-8-3-8 3v7c0 6 8 10 8 10z" />
    </svg>
  );
}

export function WarnIcon() {
  return (
    <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2"
      strokeLinecap="round" strokeLinejoin="round" aria-hidden="true" className="warn-icon">
      <path d="M12 9v4" />
      <path d="M12 17h.01" />
      <path d="M10.3 3.9 1.8 18a2 2 0 0 0 1.7 3h17a2 2 0 0 0 1.7-3L13.7 3.9a2 2 0 0 0-3.4 0z" />
    </svg>
  );
}

export function UploadIcon() {
  return (
    <svg width="17" height="17" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2"
      strokeLinecap="round" strokeLinejoin="round" aria-hidden="true">
      <path d="M12 16V4" />
      <path d="m7 9 5-5 5 5" />
      <path d="M5 20h14" />
    </svg>
  );
}

export function SendIcon() {
  return (
    <svg width="18" height="18" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2"
      strokeLinecap="round" strokeLinejoin="round" aria-hidden="true">
      <path d="M19 12H5" />
      <path d="m12 19-7-7 7-7" />
    </svg>
  );
}

function Glyph({ size, children }: { size: number; children: ReactNode }) {
  return (
    <svg width={size} height={size} viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2"
      strokeLinecap="round" strokeLinejoin="round" aria-hidden="true" className="glyph">
      {children}
    </svg>
  );
}

export function CloseIcon({ size = 18 }: { size?: number }) {
  return <Glyph size={size}><path d="M18 6 6 18" /><path d="m6 6 12 12" /></Glyph>;
}

export function MoreIcon({ size = 18 }: { size?: number }) {
  return (
    <Glyph size={size}>
      <circle cx="5" cy="12" r="1" fill="currentColor" />
      <circle cx="12" cy="12" r="1" fill="currentColor" />
      <circle cx="19" cy="12" r="1" fill="currentColor" />
    </Glyph>
  );
}

export function PlusIcon({ size = 17 }: { size?: number }) {
  return <Glyph size={size}><path d="M12 5v14" /><path d="M5 12h14" /></Glyph>;
}

export function EditIcon({ size = 17 }: { size?: number }) {
  return <Glyph size={size}><path d="M12 20h9" /><path d="M16.5 3.5a2.1 2.1 0 0 1 3 3L7 19l-4 1 1-4Z" /></Glyph>;
}

export function SparkIcon({ size = 14 }: { size?: number }) {
  return <Glyph size={size}><path d="M12 3c.5 4.5 2.5 6.5 7 7-4.5.5-6.5 2.5-7 7-.5-4.5-2.5-6.5-7-7 4.5-.5 6.5-2.5 7-7Z" fill="currentColor" /></Glyph>;
}

/** The open dialogs, in mount order. Only the one on top answers Escape and Tab. */
const openDialogs: HTMLElement[] = [];

/** The dialog on top: the last mounted one that has no other open dialog inside it
 *  (a nested dialog's effect runs before its parent's, so mount order alone is not enough). */
function topDialog(): HTMLElement | undefined {
  for (let i = openDialogs.length - 1; i >= 0; i--) {
    const d = openDialogs[i];
    if (d && !openDialogs.some((o) => o !== d && d.contains(o))) return d;
  }
  return undefined;
}

/** Whether any modal dialog is open: window-level shortcuts stay quiet then. */
export function dialogOpen(): boolean {
  return openDialogs.length > 0;
}

const FOCUSABLE =
  'a[href], button:not([disabled]), input:not([disabled]):not([type="hidden"]), select:not([disabled]), ' +
  'textarea:not([disabled]), [tabindex]:not([tabindex="-1"]), [contenteditable="true"]';

/** Keeps Tab and Shift+Tab cycling inside the frame. */
function trapTab(frame: HTMLElement, e: KeyboardEvent) {
  const items = Array.from(frame.querySelectorAll<HTMLElement>(FOCUSABLE)).filter(
    (el) => el.getAttribute("aria-hidden") !== "true",
  );
  if (items.length === 0) {
    e.preventDefault();
    frame.focus();
    return;
  }
  const first = items[0] as HTMLElement;
  const last = items[items.length - 1] as HTMLElement;
  const active = document.activeElement;
  const inside = active instanceof HTMLElement && frame.contains(active) && active !== frame;
  if (e.shiftKey) {
    if (!inside || active === first) {
      e.preventDefault();
      last.focus();
    }
  } else if (!inside || active === last) {
    e.preventDefault();
    first.focus();
  }
}

/** A modal frame. Escape closes it; focus moves into it when it opens and back to the opener on close. */
export function Dialog(props: {
  title: string;
  subtitle?: ReactNode;
  icon?: ReactNode;
  narrow?: boolean;
  wide?: boolean;
  onClose?: () => void;
  /** Typed text that closing would lose: Escape and × ask first (F6). */
  dirty?: boolean;
  footer?: ReactNode;
  children: ReactNode;
}) {
  const ref = useRef<HTMLElement>(null);
  const { onClose, dirty } = props;
  const [confirming, setConfirming] = useState(false);
  const close = onClose && (dirty ? () => setConfirming(true) : onClose);
  const closeRef = useRef(close);
  const confirmingRef = useRef(confirming);
  useEffect(() => {
    closeRef.current = close;
    confirmingRef.current = confirming;
  });
  const closable = onClose !== undefined;
  useEffect(() => {
    // A field marked autoFocus inside has already taken focus (React applies it before effects):
    // keep it there, so she can type at once. Otherwise the frame takes focus.
    const frame = ref.current;
    const opener = document.activeElement;
    if (frame && !frame.contains(opener)) frame.focus();
    return () => {
      // Focus goes back to whatever opened the dialog, if it is still on the page.
      if (opener instanceof HTMLElement && opener.isConnected) opener.focus();
    };
  }, []);
  useEffect(() => {
    const frame = ref.current;
    if (!frame) return;
    openDialogs.push(frame);
    const onKey = (e: KeyboardEvent) => {
      // Only the dialog on top answers keys: Escape closes one dialog at a time, Tab stays inside.
      if (topDialog() !== frame) return;
      if (e.key === "Tab") {
        trapTab(frame, e);
        return;
      }
      if (e.key !== "Escape" || !closable) return;
      if (confirmingRef.current) setConfirming(false);
      else closeRef.current?.();
    };
    window.addEventListener("keydown", onKey);
    return () => {
      window.removeEventListener("keydown", onKey);
      const i = openDialogs.lastIndexOf(frame);
      if (i >= 0) openDialogs.splice(i, 1);
    };
  }, [closable]);
  return (
    <div className="overlay">
      <section ref={ref} tabIndex={-1} role="dialog" aria-modal="true" aria-label={props.title}
        className={props.narrow ? "dialog dialog-narrow" : props.wide ? "dialog dialog-wide" : "dialog"}>
        <header className="dialog-head">
          {props.icon}
          <div className="grow stack" style={{ gap: 4 }}>
            <h2>{props.title}</h2>
            {props.subtitle && <span className="muted small">{props.subtitle}</span>}
          </div>
          {onClose && (
            <button type="button" className="btn icon-btn" aria-label="סגירה" onClick={close}><CloseIcon /></button>
          )}
        </header>
        <div className="dialog-body">{props.children}</div>
        {confirming ? (
          <footer className="dialog-foot" role="alertdialog" aria-label="לסגור בלי לשמור">
            <span className="grow">מה שהקלדת עדיין לא נשמר. לסגור בכל זאת?</span>
            <button type="button" className="btn btn-primary" autoFocus onClick={() => setConfirming(false)}>להמשיך לערוך</button>
            <button type="button" className="btn" onClick={onClose}>לסגור בלי לשמור</button>
          </footer>
        ) : (
          props.footer && <footer className="dialog-foot">{props.footer}</footer>
        )}
      </section>
    </div>
  );
}

/** Text from the filter, with what is (or will be) hidden marked. Always plain text. */
export function Segments({ segments, side }: { segments: Segment[]; side: "original" | "outgoing" }) {
  return (
    <>
      {segments.map((s, i) => {
        if (!s.mark) return <span key={i}>{s.text}</span>;
        if (s.mark === "suspect") {
          return <span key={i} className="mark-suspect" title={s.label ?? undefined}>{s.text}</span>;
        }
        if (side === "original") {
          return <mark key={i} className="mark-real" title={s.label ? `יוסתר: ${s.label}` : undefined}>{s.text}</mark>;
        }
        return <span key={i} className="mark-role" title={s.label ?? undefined}>{displayTag(s.text)}</span>;
      })}
    </>
  );
}

/** "[ילד]" → "הילד": tags read as roles on screen. */
export function displayTag(tag: string): string {
  const inner = tag.replace(/[[\]]/g, "");
  return inner;
}

export function Spinner() {
  return <span className="spinner" aria-hidden="true" />;
}

const SKELETON_WIDTHS = [92, 76, 85, 64, 88, 70];

/**
 * The shape of what is loading, in place of an empty screen (V1). Grey bars that shimmer
 * gently; with reduced motion they stay still.
 */
export function Skeleton({ lines = 3, label = "טוען", card = false }: { lines?: number; label?: string; card?: boolean }) {
  return (
    <div className={card ? "card skeleton" : "skeleton"} role="status" aria-busy="true" aria-label={label}>
      {Array.from({ length: lines }, (_, i) => (
        <span key={i} className={i === 0 && card ? "skeleton-line skeleton-title" : "skeleton-line"}
          style={{ width: `${SKELETON_WIDTHS[i % SKELETON_WIDTHS.length] ?? 80}%` }} />
      ))}
    </div>
  );
}

/** A short confirmation that goes away by itself. The timer starts once per message:
 *  a new `onDone` on every render of the parent must not keep it on screen. */
/** An action offered on a toast, such as "ביטול" right after something was saved or removed. */
export interface ToastAction {
  label: string;
  run: () => void;
}

/**
 * A short notice under the header. With an action it stays longer (7s instead of 3.5s) and holds
 * while the pointer or focus is on it, so the action can be reached in time.
 */
export function Toast({ text, onDone, action }: { text: string; onDone: () => void; action?: ToastAction | undefined }) {
  const done = useRef(onDone);
  const [held, setHeld] = useState(false);
  useEffect(() => {
    done.current = onDone;
  });
  useEffect(() => {
    if (held) return;
    const t = window.setTimeout(() => done.current(), action ? 7000 : 3500);
    return () => window.clearTimeout(t);
  }, [text, action, held]);
  if (!action) return <div className="toast" role="status">{text}</div>;
  return (
    <div className="toast toast-action" role="status"
      onPointerEnter={() => setHeld(true)} onPointerLeave={() => setHeld(false)}
      onFocus={() => setHeld(true)} onBlur={() => setHeld(false)}>
      <span>{text}</span>
      <button type="button" className="toast-btn" onClick={() => { action.run(); done.current(); }}>{action.label}</button>
    </div>
  );
}

/**
 * Every error on screen, the same way, with its one next step (F6): the button the core chose
 * for it. "לנסות שוב" runs the failed call again (or the caller's own retry), and the line goes
 * once it worked; settings open on the pane that fixes it; a missing consent opens the case
 * details; a fault only Or can read offers the report for him; her own slip closes with
 * "הבנתי". A message that is not the remembered failure (written by the screen itself) gets
 * the caller's retry when there is one, else "הבנתי". Never a dead end.
 */
export function ErrorLine({ error: given, retry, report: always }: { error: string | null; retry?: (() => void) | undefined; report?: boolean }) {
  const app = useContext(AppContext);
  const qc = useOptionalQueryClient();
  const [reporting, setReporting] = useState(false);
  const [closed, setClosed] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  // A try again that failed shows its own failure, with its own next step, in place of the
  // first (never the old line as if nothing happened).
  const [again_, setAgain] = useState<{ after: string; message: string } | null>(null);
  // "הבנתי" closes this message, not the next one like it: a second file refused for the
  // same reason shows the line again, since the screen clears the error before each try.
  const [last, setLast] = useState(given);
  if (given !== last) {
    setLast(given);
    if (given === null && closed !== null) setClosed(null);
  }
  const error = given !== null && again_?.after === given ? again_.message : given;
  const f = error ? failureFor(error) : null;
  const action = f?.action ?? null;
  if (!error || given === null || closed === given) return null;
  const route = app?.route;
  const replay = replayFor(f);
  const buttons: { label: string; run: () => void; wait?: number }[] = [];
  const first = given;
  const again = retry ?? (replay ? () => {
    setBusy(true);
    void replay()
      .then(() => {
        setClosed(first);
        void qc?.invalidateQueries();
        void app?.refresh();
      })
      .catch((e: unknown) => {
        const u = asUiError(e);
        let message = u.message;
        if (app) message = app.fail(u);
        else rememberFailure(u);
        setAgain({ after: first, message });
      })
      .finally(() => setBusy(false));
  } : undefined);
  switch (action?.kind) {
    case "retry":
      if (again) buttons.push({ label: "לנסות שוב", run: again, wait: action.after_seconds });
      break;
    case "settings": {
      const pane: SettingsPane = SETTINGS_PANES.find((p) => p.id === action.pane)?.id ?? "general";
      if (app) buttons.push({ label: "פתיחת ההגדרות", run: () => app.go({ name: "settings", pane }) });
      break;
    }
    case "consent":
      if (app && route?.name === "case") {
        const id = route.id;
        buttons.push({ label: "רישום ההסכמה", run: () => app.go({ name: "case", id, view: "details" }) });
      }
      break;
    case "unlock":
      if (app) buttons.push({ label: "פתיחת הכספת", run: () => void app.refresh() });
      break;
    case "raise_cap": {
      // One click: the ceiling goes up to what the message says, and the same request goes.
      const to = action.to_usd;
      if (again) buttons.push({ label: `להעלות ל-$${to} ולהמשיך`, run: () => void ipc.setMonthlyCap(to).then(again, (e: unknown) => setAgain({ after: first, message: asUiError(e).message })) });
      else if (app) buttons.push({ label: "פתיחת ההגדרות", run: () => app.go({ name: "settings", pane: "ai" }) });
      break;
    }
    default:
      if (!f && retry) buttons.push({ label: "לנסות שוב", run: retry });
  }
  const report = always === true || action?.kind === "report" || (f !== null && f.details.length > 0);
  if (buttons.length === 0 && !report) buttons.push({ label: "הבנתי", run: () => setClosed(first) });
  return (
    <div className="error-line">
      <p className="error" role="alert">{error}</p>
      <div className="row error-actions">
        {buttons.map((a) => a.wait !== undefined
          ? <WaitButton key={`${a.label}-${error}`} label={a.label} seconds={a.wait} busy={busy} run={a.run} />
          : <button key={a.label} type="button" className="btn btn-small" onClick={a.run}>{a.label}</button>)}
        {report && (
          <button type="button" className="btn btn-small" onClick={() => setReporting(true)}>דוח תקלה לאור</button>
        )}
      </div>
      {reporting && (
        <BugReportDialog failure={f ? { code: f.code, details: f.details } : null} onClose={() => setReporting(false)} />
      )}
    </div>
  );
}

/** "לנסות שוב" that waits out the seconds the core asked for (too many tries, a busy AI). */
function WaitButton({ label, seconds, busy, run }: { label: string; seconds: number; busy: boolean; run: () => void }) {
  const [left, setLeft] = useState(seconds);
  useEffect(() => {
    if (seconds <= 0) return;
    const t = window.setInterval(() => setLeft((w) => (w > 1 ? w - 1 : 0)), 1000);
    return () => window.clearInterval(t);
  }, [seconds]);
  return (
    <button type="button" className="btn btn-small" disabled={left > 0 || busy} onClick={run}>
      {left > 0 ? `${label} (${left})` : label}
    </button>
  );
}

/** The query client when there is one (a screen under the app), so a retry that worked
 *  refreshes what the screen shows; `null` in a bare test render. */
function useOptionalQueryClient() {
  try {
    return useQueryClient();
  } catch {
    return null;
  }
}
