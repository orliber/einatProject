import { useEffect, useState } from "react";
import { Dialog } from "./ui";
import { isCombo, KEYS, SHEET } from "../shortcuts";

/** The keyboard shortcuts sheet (UX-5): F1 or Ctrl+/ anywhere, or from the menu. */
export function ShortcutsSheet({ onClose }: { onClose: () => void }) {
  return (
    <Dialog title="קיצורי מקלדת" subtitle="אפשר לפתוח את הדף הזה בכל רגע עם F1." onClose={onClose}>
      {SHEET.map((g) => (
        <section key={g.group} className="shortcuts-group" aria-label={g.group}>
          <h3>{g.group}</h3>
          <dl className="shortcuts">
            {g.items.map(([keys, what]) => (
              <div key={keys} className="shortcut-row">
                <dt><kbd dir="ltr">{keys}</kbd></dt>
                <dd>{what}</dd>
              </div>
            ))}
          </dl>
        </section>
      ))}
      <p className="small muted">
        הכתבה בקול: ההכתבה של Windows (Win+H) שולחת את הקול לשרתים של Microsoft, בלי הסתרת השמות של התוכנה, ועוד לא תומכת בעברית.
        לכן לא משתמשים בה לפרטי התיק.
      </p>
    </Dialog>
  );
}

/** Opens the sheet on F1 or Ctrl+/, and lets a menu item open it too. */
export function useShortcutsSheet(enabled: boolean) {
  const [open, setOpen] = useState(false);
  useEffect(() => {
    if (!enabled) return;
    const onKey = (e: KeyboardEvent) => {
      if (isCombo(e, KEYS.help) || isCombo(e, KEYS.helpAlt)) {
        e.preventDefault();
        setOpen(true);
      }
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [enabled]);
  return { open, show: () => setOpen(true), close: () => setOpen(false) };
}
