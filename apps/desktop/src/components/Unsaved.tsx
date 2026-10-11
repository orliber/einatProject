// Text being typed when the vault locks (a paragraph, a session note, a question) is not lost:
// the core keeps it in memory while she types, writes it into the vault (encrypted) as it
// locks, and offers it back once at the next entry. The page itself keeps nothing: it is reloaded on lock.
import { useEffect, useRef, useState } from "react";
import { useApp, type Route } from "../App";
import { ipc, type UnsavedEdit } from "../ipc/client";
import { Dialog } from "./ui";

/** Report the open editor's text (or `null` when none is open) to the core. `view` is where
 *  in the case it is written ("report", "materials", "consult"), to return there after entering. */
export function useHoldUnsaved(caseId: string | null, place: string, text: string | null, view = "report") {
  const held = useRef(false);
  useEffect(() => {
    if (text === null || caseId === null) {
      if (held.current) void ipc.holdUnsaved(null).catch(() => undefined);
      held.current = false;
      return;
    }
    const t = window.setTimeout(() => {
      held.current = true;
      void ipc.holdUnsaved({ case_id: caseId, place, view, text }).catch(() => undefined);
    }, 800);
    return () => window.clearTimeout(t);
  }, [caseId, place, text, view]);
  useEffect(() => () => {
    if (held.current) void ipc.holdUnsaved(null).catch(() => undefined);
  }, []);
}

/** Where the kept text was being written. Edits kept by earlier versions have no view. */
function placeOf(edit: UnsavedEdit): Route {
  if (edit.view === "consult") return { name: "consult", caseId: edit.case_id };
  return { name: "case", id: edit.case_id, view: edit.view || "report" };
}

/** After entering: the text that was being edited when the vault locked. */
export function UnsavedNotice() {
  const { go } = useApp();
  const [edit, setEdit] = useState<UnsavedEdit | null>(null);
  const [copied, setCopied] = useState(false);
  useEffect(() => {
    ipc.takeUnsaved().then(setEdit, () => undefined);
  }, []);
  if (!edit) return null;
  const close = () => setEdit(null);
  async function copy() {
    if (!edit) return;
    try {
      await navigator.clipboard.writeText(edit.text);
      setCopied(true);
    } catch {
      setCopied(false);
    }
  }
  return (
    <Dialog title="טקסט שלא נשמר לפני הנעילה" subtitle={edit.place} onClose={close}
      footer={
        <div className="row">
          <button type="button" className="btn btn-primary" onClick={() => void copy()}>{copied ? "הועתק" : "העתקת הטקסט"}</button>
          <button type="button" className="btn" onClick={() => { close(); go(placeOf(edit)); }}>מעבר לתיק</button>
          <button type="button" className="btn" onClick={close}>סגירה</button>
        </div>
      }>
      <p className="muted small">
        הכספת ננעלה באמצע העריכה של הטקסט הזה, והוא נשמר בתוכה, מוצפן. אפשר להעתיק אותו ולהדביק במקום. אחרי הסגירה הוא לא יוצג שוב.
      </p>
      <textarea className="textarea serif" rows={8} readOnly value={edit.text} aria-label="הטקסט שלא נשמר" />
    </Dialog>
  );
}
