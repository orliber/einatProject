import { useEffect, useState, useSyncExternalStore } from "react";
import { useApp } from "../App";
import { ipc, type UpdateView } from "../ipc/client";
import { Dialog, ErrorLine, Spinner } from "./ui";
import "./Update.css";

/**
 * New versions (D-033). Once a day, while the vault is open, the program asks the releases
 * page whether Or published a newer version. Only a notice signed with his key counts; the
 * installer is downloaded only when she clicks, checked again, and then it runs by itself.
 */

const CHECKED_KEY = "dv.updateCheckedAt";
const AUTO_KEY = "dv.updateAuto";
const DAY_MS = 86_400_000;

let found: UpdateView | null = null;
const listeners = new Set<() => void>();
function setFound(v: UpdateView | null) {
  found = v;
  listeners.forEach((l) => l());
}
function subscribe(l: () => void) {
  listeners.add(l);
  return () => listeners.delete(l);
}

function read(key: string): string | null {
  try {
    return localStorage.getItem(key);
  } catch {
    return null;
  }
}
function write(key: string, value: string) {
  try {
    localStorage.setItem(key, value);
  } catch {
    // Private storage off: checked again next time.
  }
}

export function autoCheckOn(): boolean {
  return read(AUTO_KEY) !== "off";
}

/** Ask now. Quiet failures for the daily check; the settings button shows them. */
export async function checkNow(): Promise<UpdateView | null> {
  const v = await ipc.checkUpdate();
  write(CHECKED_KEY, String(Date.now()));
  setFound(v);
  return v;
}

/** The daily check, while the vault is open. */
export function useDailyUpdateCheck() {
  const { status } = useApp();
  useEffect(() => {
    if (!status.unlocked || !autoCheckOn()) return;
    const last = Number(read(CHECKED_KEY) ?? 0);
    if (found || Date.now() - last < DAY_MS) return;
    checkNow().catch(() => undefined);
  }, [status.unlocked]);
}

export function useFoundUpdate(): UpdateView | null {
  return useSyncExternalStore(subscribe, () => found);
}

/** "There is a new version": in the top bar, only when there is one. */
export function UpdateChip() {
  useDailyUpdateCheck();
  const update = useFoundUpdate();
  const [open, setOpen] = useState(false);
  if (!update) return null;
  return (
    <>
      <button type="button" className="update-chip" onClick={() => setOpen(true)}>
        <span aria-hidden="true">✦</span> יש גרסה חדשה
      </button>
      {open && <UpdateDialog update={update} onClose={() => setOpen(false)} />}
    </>
  );
}

export function UpdateDialog({ update, onClose }: { update: UpdateView; onClose: () => void }) {
  const { fail } = useApp();
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);

  async function install() {
    setBusy(true);
    setError(null);
    try {
      await ipc.installUpdate();
    } catch (e) {
      setError(fail(e as never));
      setBusy(false);
    }
  }

  return (
    <Dialog title={`גרסה ${update.version} מוכנה`} narrow {...(busy ? {} : { onClose })}
      subtitle={`עכשיו מותקנת ${update.current}${update.published ? ` · פורסמה ${update.published.split("-").reverse().join(".")}` : ""}`}
      footer={
        update.can_install ? (
          <>
            <button type="button" className="btn" disabled={busy} onClick={onClose}>אחר כך</button>
            <button type="button" className="btn btn-primary" disabled={busy} onClick={() => void install()}>
              {busy ? <><Spinner /> מורידה ובודקת…</> : "לעדכן עכשיו"}
            </button>
          </>
        ) : (
          <button type="button" className="btn btn-primary" onClick={onClose}>הבנתי</button>
        )
      }>
      <div className="stack update-body">
        {update.notes.length > 0 && (
          <>
            <b>מה חדש</b>
            <ul className="update-notes">{update.notes.map((n, i) => <li key={i}>{n}</li>)}</ul>
          </>
        )}
        {update.can_install ? (
          <p className="muted small">
            ההורדה ({update.size_mb} MB) נבדקת מול החתימה של אור לפני שהיא רצה. אחר כך התוכנה ננעלת, נסגרת,
            ונפתחת מחדש בגרסה החדשה. כל התיקים נשארים כמו שהם; פותחים עם אותה סיסמה.
          </p>
        ) : (
          <p className="muted small">במחשב הזה העדכון לא מותקן לבד. מתקינים את הגרסה החדשה מהקישור ששלח אור.</p>
        )}
        <ErrorLine error={error} />
      </div>
    </Dialog>
  );
}

/** Settings: the version, a check now, and whether to check by itself. */
export function UpdateSettings() {
  const { fail } = useApp();
  const update = useFoundUpdate();
  const [version, setVersion] = useState<string | null>(null);
  const [auto, setAuto] = useState(autoCheckOn);
  const [busy, setBusy] = useState(false);
  const [result, setResult] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [open, setOpen] = useState(false);

  useEffect(() => {
    ipc.ping().then((p) => setVersion(p.core_version)).catch(() => setVersion(null));
  }, []);

  async function check() {
    setBusy(true);
    setError(null);
    setResult(null);
    try {
      const v = await checkNow();
      if (v) setOpen(true);
      else setResult("זו הגרסה החדשה ביותר.");
    } catch (e) {
      setError(fail(e as never));
    } finally {
      setBusy(false);
    }
  }

  return (
    <section className="card setting" aria-labelledby="s-update">
      <h2 id="s-update">עדכונים</h2>
      <div className="row">
        <span className="grow">גרסה {version ?? "…"}{update && <b> · גרסה {update.version} מוכנה</b>}</span>
        {update ? (
          <button type="button" className="btn btn-primary btn-small" onClick={() => setOpen(true)}>לעדכון</button>
        ) : (
          <button type="button" className="btn btn-small" disabled={busy} onClick={() => void check()}>
            {busy ? <><Spinner /> בודקת…</> : "בדיקת עדכונים"}
          </button>
        )}
      </div>
      {result && <span className="small muted" role="status">{result}</span>}
      <ErrorLine error={error} />
      <label className="row small">
        <input type="checkbox" checked={auto} onChange={(e) => { setAuto(e.target.checked); write(AUTO_KEY, e.target.checked ? "on" : "off"); }} />
        לבדוק פעם ביום אם יש גרסה חדשה
      </label>
      <span className="hint">
        הבדיקה פונה רק לדף ההתקנות של אור ב-GitHub, בלי שום מידע מהכספת. גרסה מותקנת רק אחרי שלחצת, ורק אם היא חתומה
        במפתח של אור.
      </span>
      {open && update && <UpdateDialog update={update} onClose={() => setOpen(false)} />}
    </section>
  );
}
