// "איך לכתוב את הסעיף הזה" (round 29): the section's guide at the top of the section. The
// institute's text until she edits it (for every case), and lines of her own for this case.
// It goes out with each draft request of the section, filtered like her request. Plain text.
import { useState } from "react";
import { useApp } from "../App";
import { ipc } from "../ipc/client";
import { ErrorLine } from "./ui";
import "./SectionGuide.css";

type Props = {
  caseId: string;
  sectionKey: string;
  guide: string;
  isDefault: boolean;
  caseGuide: string;
  /** After a save: the case is read again. */
  onSaved: () => Promise<void> | void;
};

function Lines({ text }: { text: string }) {
  const lines = text.split("\n").map((l) => l.trim()).filter(Boolean);
  if (lines.length < 2) return <p className="guide-text">{lines[0]}</p>;
  return (
    <ol className="guide-lines">
      {lines.map((l, i) => <li key={i}>{l}</li>)}
    </ol>
  );
}

/** One editor: a box, save and cancel. Its text lives here, not in the section's state. */
function Editor(props: { label: string; initial: string; onSave: (text: string) => Promise<void>; onCancel: () => void }) {
  const [text, setText] = useState(props.initial);
  const [busy, setBusy] = useState(false);
  const id = `guide-${props.label}`;
  return (
    <form className="stack guide-editor" onSubmit={(e) => {
      e.preventDefault();
      setBusy(true);
      void props.onSave(text).finally(() => setBusy(false));
    }}>
      <label className="small" htmlFor={id}>{props.label}</label>
      <textarea id={id} className="input" rows={Math.min(8, Math.max(3, text.split("\n").length + 1))}
        value={text} autoFocus onChange={(e) => setText(e.target.value)} />
      <div className="row">
        <button type="submit" className="btn btn-small btn-primary" disabled={busy}>שמירה</button>
        <button type="button" className="btn btn-small" onClick={props.onCancel}>ביטול</button>
      </div>
    </form>
  );
}

export function SectionGuide(props: Props) {
  const { fail } = useApp();
  const [editing, setEditing] = useState<"general" | "case" | null>(null);
  const [error, setError] = useState<string | null>(null);
  const save = async (run: () => Promise<unknown>) => {
    setError(null);
    try {
      await run();
      setEditing(null);
      await props.onSaved();
    } catch (e) {
      setError(fail(e as never));
    }
  };
  const empty = !props.guide.trim() && !props.caseGuide.trim();
  return (
    <details className="section-guide">
      <summary>איך לכתוב את הסעיף הזה</summary>
      <div className="stack guide-body">
        {editing === "general" ? (
          <Editor label="ההנחיה לסעיף, לכל התיקים" initial={props.guide}
            onSave={(t) => save(() => ipc.setSectionGuide(props.sectionKey, t))} onCancel={() => setEditing(null)} />
        ) : (
          <>
            {props.guide.trim() ? <Lines text={props.guide} /> : <p className="muted small">אין עדיין הנחיה לסעיף הזה.</p>}
            <div className="row small wrap-row">
              <span className="muted">{props.isDefault ? "הנוסח של המכון." : "הנוסח שלך, לכל התיקים."}</span>
              <button type="button" className="btn btn-small" onClick={() => setEditing("general")}>עריכה</button>
              {!props.isDefault && (
                <button type="button" className="btn btn-small" onClick={() => void save(() => ipc.setSectionGuide(props.sectionKey, ""))}>
                  החזרה לנוסח המכון
                </button>
              )}
            </div>
          </>
        )}
        {editing === "case" ? (
          <Editor label="תוספת לתיק הזה בלבד" initial={props.caseGuide}
            onSave={(t) => save(() => ipc.setCaseGuide(props.caseId, props.sectionKey, t))} onCancel={() => setEditing(null)} />
        ) : props.caseGuide.trim() ? (
          <div className="stack guide-case">
            <b className="small">בתיק הזה גם:</b>
            <Lines text={props.caseGuide} />
            <div className="row">
              <button type="button" className="btn btn-small" onClick={() => setEditing("case")}>עריכת התוספת</button>
            </div>
          </div>
        ) : (
          editing === null && (
            <div className="row">
              <button type="button" className="btn btn-small btn-ghost" onClick={() => setEditing("case")}>+ תוספת לתיק הזה</button>
            </div>
          )
        )}
        {!empty && <p className="muted small">ההנחיה נשלחת עם כל בקשה לטיוטה של הסעיף, ושמות מוסתרים בה כמו בבקשה שלך. שורה שאין עליה פסקה בטיוטה הראשונה מופיעה מתחת לטיוטה.</p>}
        <ErrorLine error={error} />
      </div>
    </details>
  );
}
