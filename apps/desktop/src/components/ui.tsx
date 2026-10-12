// Small shared pieces: icons, dialog frame, marked text, toast.
import { useContext, useEffect, useRef, useState, type ReactNode } from "react";
import { AppContext } from "../App";
import { failureFor } from "../errors";
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

/** A modal frame. Escape closes it; focus moves into it when it opens. */
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
    if (frame && !frame.contains(document.activeElement)) frame.focus();
  }, []);
  useEffect(() => {
    if (!closable) return;
    const onKey = (e: KeyboardEvent) => {
      if (e.key !== "Escape") return;
      if (confirmingRef.current) setConfirming(false);
      else closeRef.current?.();
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
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

/** An error with the next step beside it: a missing consent opens the consent form, a bad
 *  key opens the settings, a technical failure can be copied for Or (F2, F4, F5). */
/** Failures that are her own input, fixed where she types; no bug report for these. */
const OWN_SLIPS = new Set(["wrong_secret", "recovery_invalid", "weak_password", "exists", "backoff", "locked", "not_found", "refused"]);

/**
 * Every error on screen, with the next step (U5): the place that fixes it when there is one,
 * "לנסות שוב" when the caller can retry, and otherwise a bug report for Or. Never a dead end.
 */
export function ErrorLine({ error, retry, report: always }: { error: string | null; retry?: (() => void) | undefined; report?: boolean }) {
  const app = useContext(AppContext);
  const [reporting, setReporting] = useState(false);
  if (!error) return null;
  const f = failureFor(error);
  const route = app?.route;
  const actions: { label: string; run: () => void }[] = [];
  if (f && app) {
    if (f.code === "consent_missing" && route?.name === "case") {
      const id = route.id;
      actions.push({ label: "רישום ההסכמה", run: () => app.go({ name: "case", id, view: "details" }) });
    }
    // The key, the zero-retention confirmation, the monthly ceiling: all fixed in settings.
    if ((f.code === "egress" && f.message.includes("מפתח")) || f.message.includes("בהגדרות")) {
      actions.push({ label: "פתיחת ההגדרות", run: () => app.go({ name: "settings" }) });
    }
  }
  if (retry) actions.push({ label: "לנסות שוב", run: retry });
  // A fault only Or can read, or a failure from the core with nothing else to offer: the report
  // for him. Her own slips (a wrong password, a field left empty) are not faults.
  const report = always === true || (f !== null && (f.details.length > 0 || (actions.length === 0 && !OWN_SLIPS.has(f.code))));
  return (
    <div className="error-line">
      <p className="error" role="alert">{error}</p>
      {(actions.length > 0 || report) && <div className="row error-actions">
        {actions.map((a) => (
          <button key={a.label} type="button" className="btn btn-small" onClick={a.run}>{a.label}</button>
        ))}
        {report && (
          <button type="button" className="btn btn-small" onClick={() => setReporting(true)}>דוח תקלה לאור</button>
        )}
      </div>}
      {reporting && (
        <BugReportDialog failure={f ? { code: f.code, details: f.details } : null} onClose={() => setReporting(false)} />
      )}
    </div>
  );
}
