// A paragraph being edited when the vault locks is not lost: the core keeps it in memory while
// she types, writes it into the vault (encrypted) as it locks, and offers it back once at the
// next entry. The page itself keeps nothing: it is reloaded on lock.
import { useEffect, useRef, useState } from "react";
import { useApp } from "../App";
import { ipc, type UnsavedEdit } from "../ipc/client";
import { Dialog } from "./ui";

/** Report the open editor's text (or `null` when none is open) to the core. */
export function useHoldUnsaved(caseId: string, place: string, text: string | null) {
  const held = useRef(false);
  useEffect(() => {
    if (text === null) {
      if (held.current) void ipc.holdUnsaved(null).catch(() => undefined);
      held.current = false;
      return;
    }
    const t = window.setTimeout(() => {
      held.current = true;
      void ipc.holdUnsaved({ case_id: caseId, place, text }).catch(() => undefined);
    }, 800);
    return () => window.clearTimeout(t);
  }, [caseId, place, text]);
  useEffect(() => () => {
    if (held.current) void ipc.holdUnsaved(null).catch(() => undefined);
  }, []);
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
    <Dialog title="עריכה שלא נשמרה לפני הנעילה" subtitle={edit.place} onClose={close}
      footer={
        <div className="row">
          <button type="button" className="btn btn-primary" onClick={() => void copy()}>{copied ? "הועתק" : "העתקת הטקסט"}</button>
          <button type="button" className="btn" onClick={() => { close(); go({ name: "case", id: edit.case_id, view: "report" }); }}>מעבר לתיק</button>
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
