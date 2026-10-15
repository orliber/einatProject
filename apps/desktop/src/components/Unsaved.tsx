// Text being typed when the vault locks (a paragraph, a session note, a question) is not lost:
// the core keeps it in memory while she types, writes it into the vault (encrypted) as it
// locks, and offers it back once at the next entry. The page itself keeps nothing: it is reloaded on lock.
import { useCallback, useEffect, useId, useLayoutEffect, useRef, useState } from "react";
import { useApp, type Route } from "../App";
import { ipc, type UnsavedEdit } from "../ipc/client";
import { Dialog } from "./ui";

/** The open editors' texts, by owner, the latest written last. Kept here, not in a screen: a
 *  screen she leaves keeps its text held (she went to another stage, not away from the text),
 *  and a lock sends the latest text first (`holdNow`). The core holds one text: the latest
 *  editor still open, so closing one falls back to the one before it. */
const holds = new Map<string, UnsavedEdit>();
let sent = "null";
let timer: ReturnType<typeof setTimeout> | null = null;

/** The editors hand over their text at most every 800 ms (LocalText) or on every key (`live`),
 *  so this waits only a moment: no call to the core on each key. */
const HOLD_WAIT_MS = 100;

/** The same editor shown again (she came back to the stage) is one text, not two: what the
 *  screen it was on held gives way to it. */
function put(owner: string, e: UnsavedEdit) {
  holds.delete(owner);
  for (const [k, h] of holds) {
    if (h.case_id === e.case_id && h.view === e.view && h.place === e.place) holds.delete(k);
  }
  holds.set(owner, e);
}

function edit(): UnsavedEdit | null {
  const latest = [...holds.values()].reverse().find((e) => e.text.trim());
  return latest ? { ...latest } : null;
}

function send(): Promise<void> {
  if (timer !== null) clearTimeout(timer);
  timer = null;
  const next = edit();
  const key = JSON.stringify(next);
  if (key === sent) return Promise.resolve();
  sent = key;
  return ipc.holdUnsaved(next).catch(() => undefined);
}

function schedule() {
  if (timer !== null) clearTimeout(timer);
  timer = setTimeout(() => void send(), HOLD_WAIT_MS);
}

/** Before a lock by hand: the text as it is this moment reaches the core, which keeps it in
 *  the vault as it locks. A text typed without a pause is held too. */
export function holdNow(): Promise<void> {
  return send();
}

/** After a lock: the core took what it held; nothing of it stays here. */
export function forgetHold() {
  if (timer !== null) clearTimeout(timer);
  timer = null;
  holds.clear();
  sent = "null";
}

/** Report the open editor's text (or `null` when none is open) to the core. `view` is where
 *  in the case it is written ("report", "materials", "consult"), to return there after entering.
 *  Returns `live`: give it to the editor's `onLive`, so the text held is the one in the box this
 *  moment, not its debounced copy. Closing the editor (save, cancel) lets go of the text;
 *  leaving the screen with the editor open keeps it held. */
export function useHoldUnsaved(caseId: string | null, place: string, text: string | null, view = "report"): (text: string) => void {
  const owner = useId();
  const open = text !== null && caseId !== null;
  const meta = useRef({ caseId, place, view, open });
  /** Once the box reports on every key, its debounced copy is older than what is held. */
  const fromBox = useRef(false);
  useLayoutEffect(() => {
    meta.current = { caseId, place, view, open };
  });
  useEffect(() => {
    if (!open || caseId === null) {
      if (holds.delete(owner)) {
        fromBox.current = false;
        schedule();
      }
      return;
    }
    const held = holds.get(owner);
    if (fromBox.current && held) {
      // The box's text is fresher; only where it is written may have changed.
      holds.set(owner, { ...held, case_id: caseId, place, view });
    } else {
      put(owner, { case_id: caseId, place, view, text: text ?? "" });
    }
    schedule();
  }, [owner, open, caseId, place, view, text]);
  return useCallback((live: string) => {
    const m = meta.current;
    if (!m.open || m.caseId === null) return;
    fromBox.current = true;
    put(owner, { case_id: m.caseId, place: m.place, view: m.view, text: live });
    schedule();
  }, [owner]);
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
