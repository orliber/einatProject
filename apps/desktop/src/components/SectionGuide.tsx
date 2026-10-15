// "איך לכתוב את הסעיף הזה" (round 29): the section's guide at the top of the section. The
// institute's text until she edits it (for every case), or the order she writes the section in
// when it was learned from her past reports (K3), and lines of her own for this case.
// It goes out with each draft request of the section, filtered like her request. Plain text.
import { useState } from "react";
import { useApp } from "../App";
import { ipc } from "../ipc/client";
import { forgetKept, useKept, useKeptValue } from "../kept";
import { useHoldUnsaved } from "./Unsaved";
import { ErrorLine } from "./ui";
import "./SectionGuide.css";

type Props = {
  caseId: string;
  sectionKey: string;
  guide: string;
  isDefault: boolean;
  /** The guide is the order she writes the section in, learned from this many reports. */
  learnedFrom?: number;
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

/** One editor: a box, save and cancel. What she types is kept while she moves to another
 *  stage and comes back. Esc cancels, Ctrl/⌘+Enter saves. */
function Editor(props: { keptKey: string; id: string; label: string; initial: string; onSave: (text: string) => Promise<void>; onCancel: () => void }) {
  const [text, setText] = useKept(props.keptKey, props.initial);
  const [busy, setBusy] = useState(false);
  const submit = () => {
    setBusy(true);
    void props.onSave(text).finally(() => setBusy(false));
  };
  return (
    <form className="stack guide-editor" onSubmit={(e) => {
      e.preventDefault();
      submit();
    }}>
      <label className="small" htmlFor={props.id}>{props.label}</label>
      <textarea id={props.id} className="input" rows={Math.min(8, Math.max(3, text.split("\n").length + 1))}
        value={text} autoFocus onChange={(e) => setText(e.target.value)}
        onKeyDown={(e) => {
          if (e.key === "Escape") {
            e.preventDefault();
            props.onCancel();
          } else if (e.key === "Enter" && (e.ctrlKey || e.metaKey) && !busy) {
            e.preventDefault();
            submit();
          }
        }} />
      <div className="row">
        <button type="submit" className="btn btn-small btn-primary" disabled={busy}>שמירה</button>
        <button type="button" className="btn btn-small" onClick={props.onCancel}>ביטול</button>
      </div>
    </form>
  );
}

/** Where the guide's open state is kept: the report page's link opens it on arrival. */
export function guideOpenKey(caseId: string, sectionKey: string): string {
  return `case:${caseId}:guide:${sectionKey}:open`;
}

export function SectionGuide(props: Props) {
  const { fail } = useApp();
  const base = `case:${props.caseId}:guide:${props.sectionKey}`;
  const [editing, setEditingKept] = useKept<"general" | "case" | null>(`${base}:editing`, null);
  // Open or closed as she left it, also across stages; opened by the report page's link.
  const [open, setOpen] = useKept(guideOpenKey(props.caseId, props.sectionKey), false);
  // Opening an editor forgets what was typed in an earlier one (saved or cancelled): it opens
  // on the text as it is now. What she types stays while she moves around the app.
  const setEditing = (next: "general" | "case" | null) => {
    if (next !== null) {
      forgetKept(`${base}:${next}`);
      setOpen(true);
    }
    setEditingKept(next);
  };
  // The open editor's text is held for the automatic lock like a paragraph, here and not in
  // the editor, so that closing it (saved or cancelled) lets go of the text (the review of
  // stage 3); leaving the stage with the editor open keeps it held.
  const typed = useKeptValue<string>(`${base}:${editing ?? "none"}`);
  const initial = editing === "case" ? props.caseGuide : props.guide;
  useHoldUnsaved(props.caseId, editing === "case" ? "תוספת לתיק הזה בלבד" : "ההנחיה לסעיף, לכל התיקים",
    editing === null || typed === undefined || typed === initial ? null : typed, props.sectionKey);
  const [resetting, setResetting] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const save = async (run: () => Promise<unknown>) => {
    setError(null);
    try {
      await run();
      setResetting(false);
      setEditing(null);
      await props.onSaved();
    } catch (e) {
      setError(fail(e as never));
    }
  };
  const empty = !props.guide.trim() && !props.caseGuide.trim();
  return (
    <details className="section-guide" open={open} onToggle={(e) => setOpen(e.currentTarget.open)}>
      <summary>איך לכתוב את הסעיף הזה</summary>
      <div className="stack guide-body">
        {editing === "general" ? (
          <Editor keptKey={`${base}:general`} id={`guide-general-${props.sectionKey}`} label="ההנחיה לסעיף, לכל התיקים" initial={props.guide}
            onSave={(t) => save(() => ipc.setSectionGuide(props.sectionKey, t))} onCancel={() => setEditing(null)} />
        ) : (
          <>
            {props.guide.trim() ? <Lines text={props.guide} /> : <p className="muted small">אין עדיין הנחיה לסעיף הזה.</p>}
            <div className="row small wrap-row">
              <span className="muted">{!props.isDefault
                ? "הנוסח שלך, לכל התיקים."
                : props.learnedFrom
                  ? `לפי הסדר שבו כתבת את הסעיף ב-${props.learnedFrom} מהדוחות שלך.`
                  : "הנוסח של המכון."}</span>
              <button type="button" className="btn btn-small" onClick={() => setEditing("general")}>עריכה</button>
              {!props.isDefault && !resetting && (
                <button type="button" className="btn btn-small" onClick={() => setResetting(true)}>
                  החזרה לנוסח המכון
                </button>
              )}
              {!props.isDefault && resetting && (
                <span className="row">
                  <span>הנוסח שלך יימחק מכל התיקים.</span>
                  <button type="button" className="btn btn-small btn-primary" onClick={() => void save(() => ipc.setSectionGuide(props.sectionKey, ""))}>להחזיר</button>
                  <button type="button" className="btn btn-small" onClick={() => setResetting(false)}>ביטול</button>
                </span>
              )}
            </div>
          </>
        )}
        {editing === "case" ? (
          <Editor keptKey={`${base}:case`} id={`guide-case-${props.sectionKey}`} label="תוספת לתיק הזה בלבד" initial={props.caseGuide}
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
