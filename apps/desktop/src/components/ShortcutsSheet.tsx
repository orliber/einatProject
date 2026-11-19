import { useCallback, useEffect, useState } from "react";
import { isCombo, KEYS, SHEET } from "../shortcuts";
import { setHelpOpener } from "../help/where";
import type { HelpId } from "../help/types";

/** The keyboard shortcuts (UX-5), folded at the bottom of the help since D-206. */
export function ShortcutsList() {
  return (
    <>
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
    </>
  );
}

/**
 * The help (D-206): F1 or Ctrl+/ anywhere, the "?" of the top bar, the case's bar or a window.
 * `open` is the topic to open on, or null while closed; without a topic the caller picks the
 * help of what is on the screen when it opens.
 */
export function useHelpSheet(enabled: boolean, here: () => HelpId) {
  const [open, setOpen] = useState<HelpId | null>(null);
  const show = useCallback((id?: HelpId) => setOpen(id ?? here()), [here]);
  useEffect(() => {
    if (!enabled) return;
    const onKey = (e: KeyboardEvent) => {
      if (isCombo(e, KEYS.help) || isCombo(e, KEYS.helpAlt)) {
        e.preventDefault();
        // Already open: F1 again does not change the topic she is reading.
        setOpen((now) => now ?? here());
      }
    };
    window.addEventListener("keydown", onKey);
    setHelpOpener(show);
    return () => {
      window.removeEventListener("keydown", onKey);
      setHelpOpener(null);
    };
  }, [enabled, here, show]);
  const close = useCallback(() => setOpen(null), []);
  return { open, show, close };
}
