// Small shared pieces: icons, dialog frame, marked text, toast.
import { useEffect, useRef, type ReactNode } from "react";
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

export function SendIcon() {
  return (
    <svg width="18" height="18" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2"
      strokeLinecap="round" strokeLinejoin="round" aria-hidden="true">
      <path d="M19 12H5" />
      <path d="m12 19-7-7 7-7" />
    </svg>
  );
}

/** A modal frame. Escape closes it; focus moves into it when it opens. */
export function Dialog(props: {
  title: string;
  subtitle?: ReactNode;
  icon?: ReactNode;
  narrow?: boolean;
  onClose?: () => void;
  footer?: ReactNode;
  children: ReactNode;
}) {
  const ref = useRef<HTMLElement>(null);
  const { onClose } = props;
  useEffect(() => {
    ref.current?.focus();
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape" && onClose) onClose();
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [onClose]);
  return (
    <div className="overlay">
      <section ref={ref} tabIndex={-1} role="dialog" aria-modal="true" aria-label={props.title}
        className={props.narrow ? "dialog dialog-narrow" : "dialog"}>
        <header className="dialog-head">
          {props.icon}
          <div className="grow stack" style={{ gap: 4 }}>
            <h2>{props.title}</h2>
            {props.subtitle && <span className="muted small">{props.subtitle}</span>}
          </div>
          {onClose && (
            <button type="button" className="btn icon-btn" aria-label="סגירה" onClick={onClose}>×</button>
          )}
        </header>
        <div className="dialog-body">{props.children}</div>
        {props.footer && <footer className="dialog-foot">{props.footer}</footer>}
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
  const inner = tag.replace(/^\[|\]$/g, "");
  return inner;
}

export function Spinner() {
  return <span className="spinner" aria-hidden="true" />;
}

export function Toast({ text, onDone }: { text: string; onDone: () => void }) {
  useEffect(() => {
    const t = window.setTimeout(onDone, 6000);
    return () => window.clearTimeout(t);
  }, [onDone]);
  return <div className="toast" role="status">{text}</div>;
}

export function ErrorLine({ error }: { error: string | null }) {
  if (!error) return null;
  return <p className="error" role="alert">{error}</p>;
}
