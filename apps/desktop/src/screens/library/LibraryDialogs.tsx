// Small dialogs of the case library (D-023): a folder's name, where to move, confirm, erase.
import { useState } from "react";
import type { Folder } from "../../ipc/client";
import { Dialog, ErrorLine } from "../../components/ui";

export function FolderNameDialog(props: { title: string; initial?: string; onClose: () => void; onSave: (name: string) => Promise<void> }) {
  const [name, setName] = useState(props.initial ?? "");
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  async function save() {
    if (!name.trim()) return setError("צריך לתת שם לתיקייה.");
    setBusy(true);
    try {
      await props.onSave(name.trim());
    } catch (e) {
      setError((e as { message?: string }).message ?? "לא נשמר.");
      setBusy(false);
    }
  }
  return (
    <Dialog narrow title={props.title} onClose={props.onClose}
      footer={<>
        <button type="button" className="btn" onClick={props.onClose}>ביטול</button>
        <button type="button" className="btn btn-primary" disabled={busy} onClick={() => void save()}>שמירה</button>
      </>}>
      <form className="stack" onSubmit={(e) => { e.preventDefault(); void save(); }}>
        <div className="field">
          <label htmlFor="folder-name">שם התיקייה</label>
          <input id="folder-name" className="input" autoFocus maxLength={60} value={name} onChange={(e) => setName(e.target.value)} />
        </div>
        <p className="hint">השם נשמר מוצפן במחשב הזה ולא נשלח לשום מקום, גם אם הוא כולל שם משפחה.</p>
        <ErrorLine error={error} />
      </form>
    </Dialog>
  );
}

/** Folder ids under `id`, including itself (a folder cannot move into these). */
export function descendants(folders: Folder[], id: string): Set<string> {
  const out = new Set([id]);
  let grew = true;
  while (grew) {
    grew = false;
    for (const f of folders) {
      if (f.parent_id && out.has(f.parent_id) && !out.has(f.id)) {
        out.add(f.id);
        grew = true;
      }
    }
  }
  return out;
}

export function MoveDialog(props: { what: string; folders: Folder[]; current: string | null; exclude?: Set<string>; onClose: () => void; onMove: (target: string | null) => Promise<void> }) {
  const [target, setTarget] = useState<string | null>(props.current);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const children = (parent: string | null) =>
    props.folders.filter((f) => f.parent_id === parent && !props.exclude?.has(f.id)).sort((a, b) => a.name.localeCompare(b.name, "he"));
  const tree = (parent: string | null, depth: number): React.ReactNode[] =>
    children(parent).flatMap((f) => [
      <label key={f.id} className="check-row move-row" style={{ paddingInlineStart: 10 + depth * 22 }}>
        <input type="radio" name="move-target" checked={target === f.id} onChange={() => setTarget(f.id)} />
        <FolderIcon /> <span className="grow">{f.name}</span>
      </label>,
      ...tree(f.id, depth + 1),
    ]);
  async function move() {
    setBusy(true);
    try {
      await props.onMove(target);
    } catch (e) {
      setError((e as { message?: string }).message ?? "לא הועבר.");
      setBusy(false);
    }
  }
  return (
    <Dialog narrow title={`העברת ${props.what}`} subtitle="לאן?" onClose={props.onClose}
      footer={<>
        <button type="button" className="btn" onClick={props.onClose}>ביטול</button>
        <button type="button" className="btn btn-primary" disabled={busy || target === props.current} onClick={() => void move()}>העברה</button>
      </>}>
      <div className="move-tree" role="radiogroup" aria-label="תיקיית היעד">
        <label className="check-row move-row">
          <input type="radio" name="move-target" checked={target === null} onChange={() => setTarget(null)} />
          <span className="grow"><b>כל התיקים</b> (הרמה העליונה)</span>
        </label>
        {tree(null, 1)}
      </div>
      <ErrorLine error={error} />
    </Dialog>
  );
}

export function ConfirmDialog(props: { title: string; text: string; action: string; danger?: boolean; onClose: () => void; onConfirm: () => Promise<void> }) {
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  return (
    <Dialog narrow title={props.title} onClose={props.onClose}
      footer={<>
        <button type="button" className="btn" onClick={props.onClose}>ביטול</button>
        <button type="button" className={props.danger ? "btn btn-danger" : "btn btn-primary"} disabled={busy}
          onClick={() => { setBusy(true); props.onConfirm().catch((e: { message?: string }) => { setError(e.message ?? "לא בוצע."); setBusy(false); }); }}>
          {props.action}
        </button>
      </>}>
      <p>{props.text}</p>
      <ErrorLine error={error} />
    </Dialog>
  );
}

/** Erasing from the recycle bin cannot be undone, so the password is asked again. */
export function PurgeDialog(props: { name: string; onClose: () => void; onPurge: (password: string) => Promise<void> }) {
  const [pw, setPw] = useState("");
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  async function purge() {
    setBusy(true);
    try {
      await props.onPurge(pw);
    } catch (e) {
      setError((e as { message?: string }).message ?? "לא נמחק.");
      setPw("");
      setBusy(false);
    }
  }
  return (
    <Dialog narrow title="מחיקה לצמיתות" subtitle={props.name} onClose={props.onClose}
      footer={<>
        <button type="button" className="btn" onClick={props.onClose}>ביטול</button>
        <button type="button" className="btn btn-danger" disabled={busy || !pw} onClick={() => void purge()}>מחיקה לצמיתות</button>
      </>}>
      <form className="stack" onSubmit={(e) => { e.preventDefault(); if (pw) void purge(); }}>
        <p>כל מה שבתיק יימחק מהכספת ולא יהיה אפשר לשחזר אותו ממנה. <b>גיבויים שנעשו קודם עדיין מכילים את התיק</b>: אם צריך למחוק אותו לגמרי (למשל לבקשת ההורים), מוחקים גם אותם ועושים גיבוי חדש. זה גם לא מוחק דוחות Word שכבר הופקו.</p>
        <div className="field">
          <label htmlFor="purge-pw">הסיסמה של הכספת, לאישור</label>
          <input id="purge-pw" className="input" type="password" autoComplete="current-password" autoFocus value={pw} onChange={(e) => setPw(e.target.value)} />
        </div>
        <ErrorLine error={error} />
      </form>
    </Dialog>
  );
}

export function FolderIcon({ size = 20 }: { size?: number }) {
  return (
    <svg width={size} height={size} viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.8" strokeLinejoin="round" aria-hidden="true">
      <path d="M3 7a2 2 0 0 1 2-2h4l2 2h8a2 2 0 0 1 2 2v8a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2z" />
    </svg>
  );
}
