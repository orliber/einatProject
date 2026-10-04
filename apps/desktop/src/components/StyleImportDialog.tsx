// A past report for the writing-style profile (D-043): exactly what would be kept, part by part,
// with what was hidden shown as roles. Nothing is stored until "שמירה"; the file never is.
import { useState } from "react";
import { useApp } from "../App";
import { ipc, type StyleImportPreview, type StyleSourceView } from "../ipc/client";
import { Dialog, ErrorLine, ShieldIcon, displayTag } from "./ui";
import "./StyleImportDialog.css";

/** Neutralized text: tags as role chips, everything else plain text. */
export function TaggedText({ text }: { text: string }) {
  const parts = text.split(/(\[[^[\]\n]{1,40}\])/);
  return (
    <>
      {parts.map((p, i) =>
        /^\[[^[\]\n]{1,40}\]$/.test(p)
          ? <span key={i} className="mark-role" title="הוסתר">{displayTag(p)}</span>
          : <span key={i}>{p}</span>,
      )}
    </>
  );
}

const PREVIEW_CHARS = 420;

export function StyleImportDialog(props: {
  preview: StyleImportPreview;
  sectionTitle: (key: string | null) => string;
  onClose: () => void;
  onSaved: (s: StyleSourceView) => void;
}) {
  const { fail } = useApp();
  const p = props.preview;
  const [chosen, setChosen] = useState<Set<number>>(() => new Set(p.parts.filter((x) => x.included).map((x) => x.index)));
  const [open, setOpen] = useState<Set<number>>(new Set());
  const [title, setTitle] = useState(p.title);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const words = p.parts.filter((x) => chosen.has(x.index)).reduce((n, x) => n + x.words, 0);
  const toggle = (set: Set<number>, i: number) => {
    const next = new Set(set);
    if (next.has(i)) next.delete(i);
    else next.add(i);
    return next;
  };

  async function save() {
    setBusy(true);
    setError(null);
    try {
      const s = await ipc.saveStyleSource(p.token, [...chosen], title.trim() === p.title ? null : title.trim());
      props.onSaved(s);
    } catch (e) {
      setError(fail(e as never));
      setBusy(false);
    }
  }

  function close() {
    void ipc.discardStyleUpload().catch(() => undefined);
    props.onClose();
  }

  return (
    <Dialog wide title="דוח ישן לפרופיל הסגנון" icon={<ShieldIcon />}
      subtitle="כך ייראה הטקסט שיישמר. הקובץ עצמו לא נשמר."
      onClose={close}
      footer={<>
        <span className="muted small grow">{chosen.size === 0 ? "לא סומן אף קטע" : `${chosen.size} קטעים · ${words.toLocaleString("he-IL")} מילים`}</span>
        <button type="button" className="btn" onClick={close}>ביטול</button>
        <button type="button" className="btn btn-primary" disabled={busy || chosen.size === 0} onClick={() => void save()}>
          שמירת הקטעים שסומנו
        </button>
      </>}>
      <div className="stack">
        <div className="style-import-summary">
          <span className="chip chip-ok">הוסתרו {p.hidden} פרטים מזהים</span>
          {p.numbers > 0 && <span className="chip chip-ok">{p.numbers} מספרים הפכו ל"מספר"</span>}
          <span className="hint">שמות, מקומות, תאריכים ומספרים הוחלפו בתפקיד (למשל "ילד"). שום שאלה לא נשאלת: כל חשד מוסתר.</span>
        </div>
        {p.warnings.map((w) => <p key={w} className="note-warn">{w}</p>)}
        <div className="field">
          <label htmlFor="style-title">שם לדוח (רק אצלך)</label>
          <input id="style-title" className="input" value={title} maxLength={80} onChange={(e) => setTitle(e.target.value)} />
          <span className="hint">גם השם עובר סינון לפני שהוא נשמר.</span>
        </div>
        <p className="hint">
          סימנתי מראש את הקטעים שמראים הכי טוב איך את כותבת (התרשמות, ממצאים, סיכום והמלצות). רקע, הריון ולידה ותיאור ההורים לא מסומנים:
          יש בהם יותר פרטים אישיים, ופחות סגנון. אפשר לשנות.
        </p>
        <ul className="style-parts">
          {p.parts.map((part) => {
            const long = part.text.length > PREVIEW_CHARS;
            const shown = long && !open.has(part.index) ? `${part.text.slice(0, PREVIEW_CHARS)}…` : part.text;
            return (
              <li key={part.index} className={chosen.has(part.index) ? "style-part style-part-on" : "style-part"}>
                <label className="row style-part-head">
                  <input type="checkbox" checked={chosen.has(part.index)} onChange={() => setChosen((c) => toggle(c, part.index))} />
                  <strong className="grow"><TaggedText text={part.heading} /></strong>
                  <span className="chip chip-sand">{props.sectionTitle(part.section)}</span>
                  <span className="muted small">{part.words} מילים</span>
                </label>
                <p className="style-part-text"><TaggedText text={shown} /></p>
                {long && (
                  <button type="button" className="btn btn-ghost btn-small style-more" onClick={() => setOpen((o) => toggle(o, part.index))}>
                    {open.has(part.index) ? "פחות" : "הצגת הכול"}
                  </button>
                )}
              </li>
            );
          })}
        </ul>
        <ErrorLine error={error} />
      </div>
    </Dialog>
  );
}
