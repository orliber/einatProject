// Text being typed when the vault locks (a paragraph, a session note, a question) is not lost:
// the core keeps it in memory while she types, writes it into the vault (encrypted) as it
// locks, and offers it back once at the next entry. The page itself keeps nothing: it is reloaded on lock.
import { useCallback, useEffect, useId, useLayoutEffect, useRef, useState } from "react";
import { useApp, type Route } from "../App";
import { ipc, type UnsavedEdit } from "../ipc/client";
import { getKept } from "../kept";
import { Dialog } from "./ui";

/** The open editors' texts, by owner, the latest written last. Kept here, not in a screen: a
 *  screen she leaves keeps its text held (she went to another stage, not away from the text),
 *  and a lock sends them at once (`holdNow`). The core holds every open editor's text (up to
 *  eight, the latest last), so two cases open at the lock both keep what was typed. */
const holds = new Map<string, UnsavedEdit>();
let sent = "[]";
let timer: ReturnType<typeof setTimeout> | null = null;

/** The editors hand over their text at most every 800 ms (LocalText) or on every key (`live`),
 *  so this waits a moment after the last key: no call to the core (and its lock) on each key.
 *  A lock by hand sends at once (`holdNow`); a lock with the computer may miss this moment. */
const HOLD_WAIT_MS = 400;

/** The same editor shown again (she came back to the stage) is one text, not two: what the
 *  screen it was on held gives way to it. */
function put(owner: string, e: UnsavedEdit) {
  holds.delete(owner);
  for (const [k, h] of holds) {
    if (h.case_id === e.case_id && h.view === e.view && h.place === e.place) holds.delete(k);
  }
  holds.set(owner, e);
}

function edits(): UnsavedEdit[] {
  return [...holds.values()].filter((e) => e.text.trim()).map((e) => ({ ...e }));
}

function send(): Promise<void> {
  if (timer !== null) clearTimeout(timer);
  timer = null;
  const next = edits();
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

/** The report's paragraph editors of a case that are open now, with the text in each not saved
 *  yet: one on the report page or a section's page (also when she went to another stage with
 *  it open: it stays held, and open, until she saves or cancels it there), and the one on the
 *  finish page. The file must not go out without what she typed there, nor with it unseen
 *  (fail closed): the export lists them as what stops it. Kept texts offered back after a lock
 *  are not open editors, nor are her points, a guide or a note in the materials. */
export function openEdits(caseId: string): UnsavedEdit[] {
  return [...holds.entries()]
    .filter(([owner, e]) => !owner.startsWith(OFFERED) && e.case_id === caseId && e.view === "report" && e.place.startsWith("פסקה") && e.text.trim())
    .map(([, e]) => ({ ...e }));
}

/** Added to the place of the editor on a section's own page: the report page's editor of the
 *  same section is another editor, and both are held (and stop the file) side by side, not one
 *  in place of the other. */
export const SECTION_PAGE = ", בעמוד הסעיף";

/** The section an open editor is in, by its title ("פסקה בסעיף \"רקע\"" → "רקע"); `null` for
 *  the finish page's own editor. */
export function editSectionTitle(edit: UnsavedEdit): string | null {
  const m = /^פסקה בסעיף "(.*)"(?:, בעמוד הסעיף)?$/.exec(edit.place);
  return m ? (m[1] ?? null) : null;
}

/** Where an open paragraph editor of a section is, to take her back to it: the section's own
 *  page when the editor open is there (an edit or a paragraph of her own, kept by that page),
 *  else the report page on that section, where the report's editor is kept. */
export function editRoute(caseId: string, sectionKey: string): Route {
  const onReport = getKept<unknown>(`case:${caseId}:report:${sectionKey}:editing`, null) !== null;
  const onPage = getKept<unknown>(`case:${caseId}:section:${sectionKey}:editing`, null) !== null
    || getKept<unknown>(`case:${caseId}:section:${sectionKey}:own`, null) !== null;
  return onPage && !onReport
    ? { name: "case", id: caseId, view: sectionKey }
    : { name: "case", id: caseId, view: "report", section: sectionKey };
}

/** After a lock: the core took what it held; nothing of it stays here. */
export function forgetHold() {
  if (timer !== null) clearTimeout(timer);
  timer = null;
  holds.clear();
  sent = "[]";
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
  // A session note of the clinic: its case_id is the patient's id, and it goes to the card.
  if (edit.view === "clinic") return { name: "clinic", tab: "patients", patientId: edit.case_id };
  if (edit.view === "consult") return { name: "consult", caseId: edit.case_id };
  return { name: "case", id: edit.case_id, view: edit.view || "report" };
}

/** Owner prefix of the texts offered back after a lock, held until she closes each. */
const OFFERED = "offered:";

/** After entering: the texts that were being edited when the vault locked, one after another. */
export function UnsavedNotice() {
  const { go, notify } = useApp();
  const [kept, setKept] = useState<UnsavedEdit[]>([]);
  // How long the copy stays on the protected clipboard, once copied; `false` when it failed.
  const [copied, setCopied] = useState<number | false | null>(null);
  useEffect(() => {
    // The core keeps them in the vault while they are held here, like an open editor: a lock
    // (or a crash) before she closes one offers it again. Asked twice (StrictMode), the core
    // gives the same texts, so they replace, not add.
    ipc.takeUnsaved().then((list) => {
      const offered = list ?? [];
      offered.forEach((e, i) => holds.set(`${OFFERED}${i}`, e));
      if (offered.length) schedule();
      setKept(offered);
    }, () => undefined);
  }, []);
  const edit = kept[0];
  if (!edit) return null;
  const next = () => {
    setCopied(null);
    // Closed by her: let go of it, and the core takes it out of the vault.
    for (const [k, h] of holds) {
      if (k.startsWith(OFFERED) && h === edit) holds.delete(k);
    }
    schedule();
    setKept((k) => k.slice(1));
  };
  /** Whether the text is on the protected clipboard now. */
  async function copy(): Promise<number | false> {
    if (!edit) return false;
    // The text may hold names: only the protected clipboard (out of the clipboard history and
    // of cloud sync, cleared after a while), as for an answer about a case. No fallback.
    try {
      const secs = await ipc.copySecret(edit.text);
      setCopied(secs);
      return secs;
    } catch {
      setCopied(false);
      return false;
    }
  }
  const isCopied = typeof copied === "number";
  const toCard = edit.view === "clinic";
  /** "מעבר לתיק": the core erased the text when it gave it, and the editor there opens with the
   *  saved paragraph, so it is copied first (UIA-3). If the copy fails she stays here, with the
   *  text, and is told to copy it by hand. */
  async function toCase() {
    if (!edit) return;
    const secs = isCopied ? copied : await copy();
    if (secs === false) return;
    if (!isCopied) notify(`הטקסט הועתק, ואפשר להדביק אותו ${toCard ? "בכרטיס" : "בתיק"}. הלוח יתנקה בעוד ${secs} שניות.`);
    next();
    go(placeOf(edit));
  }
  const more = kept.length - 1;
  return (
    // Until it was copied, Escape, × and "סגירה" ask first: once closed it is gone for good.
    <Dialog key={kept.length} title="טקסט שלא נשמר לפני הנעילה" subtitle={edit.place} onClose={next} dirty={!isCopied}
      footer={(close) => (
        <div className="row">
          <button type="button" className="btn btn-primary" onClick={() => void copy()}>{isCopied ? "הועתק" : "העתקת הטקסט"}</button>
          <button type="button" className="btn" onClick={() => void toCase()}>{toCard ? (isCopied ? "מעבר לכרטיס" : "העתקה ומעבר לכרטיס") : (isCopied ? "מעבר לתיק" : "העתקה ומעבר לתיק")}</button>
          <button type="button" className="btn" onClick={close}>{more > 0 ? `הבא (עוד ${more})` : "סגירה"}</button>
        </div>
      )}>
      <p className="muted small">
        הכספת ננעלה באמצע העריכה של הטקסט הזה, והוא נשמר בתוכה, מוצפן. אפשר להעתיק אותו ולהדביק במקום. אחרי הסגירה הוא לא יוצג שוב.
      </p>
      <textarea className="textarea serif" rows={8} readOnly value={edit.text} aria-label="הטקסט שלא נשמר" />
      <p className="muted small" role="status">
        {typeof copied === "number" ? `הועתק. הלוח יתנקה בעוד ${copied} שניות, אז כדאי להדביק עכשיו.`
          : copied === false ? "ההעתקה לא הצליחה. אפשר לסמן את הטקסט ולהעתיק ביד." : ""}
      </p>
    </Dialog>
  );
}
