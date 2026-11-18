// The small "?" beside a fixed explanation (the design round of 18.11.2026, D-204): a toggletip,
// not a title tooltip, so the keyboard, a touch and a screen reader reach it too. A click opens a
// short card under it and a second click, Escape, a press outside or Tab away closes it.
// The children are static copy written in code: never data, names or model output, and always
// text (never HTML).
import { useEffect, useId, useLayoutEffect, useRef, useState, type FocusEvent, type ReactNode } from "react";
import { HelpIcon } from "./ui";
import "./HelpTip.css";

export function HelpTip({ label, children }: { label: string; children: ReactNode }) {
  const [open, setOpen] = useState(false);
  const [flip, setFlip] = useState(false);
  const id = useId();
  const root = useRef<HTMLDivElement>(null);
  useEffect(() => {
    if (!open) return;
    const away = (e: PointerEvent) => {
      if (!root.current?.contains(e.target as Node)) setOpen(false);
    };
    document.addEventListener("pointerdown", away);
    return () => document.removeEventListener("pointerdown", away);
  }, [open]);
  // The card opens toward the page; near the window's left edge (RTL) it opens the other way.
  useLayoutEffect(() => {
    if (!open || !root.current) return;
    const width = Math.min(340, window.innerWidth - 32);
    setFlip(root.current.getBoundingClientRect().right < width + 16);
  }, [open]);
  // Focus that moved to something outside closes it (Tab to the next control). A click on the
  // card's own text moves focus to nothing (no relatedTarget): that is not away, so the card
  // stays; a press outside is handled by the listener above.
  const blur = (e: FocusEvent<HTMLDivElement>) => {
    const to = e.relatedTarget as Node | null;
    if (to && !root.current?.contains(to)) setOpen(false);
  };
  return (
    <div
      ref={root}
      className={flip ? "help-tip help-tip-flip" : "help-tip"}
      onKeyDown={(e) => {
        if (e.key === "Escape" && open) {
          e.stopPropagation();
          setOpen(false);
        }
      }}
      onBlur={blur}
    >
      <button type="button" className="btn icon-btn" aria-label={label} title={label}
        aria-expanded={open} aria-controls={id} onClick={() => setOpen(!open)}>
        <HelpIcon />
      </button>
      <div id={id} role="status" className="help-tip-live">
        {open && <div className="help-tip-card">{children}</div>}
      </div>
    </div>
  );
}
