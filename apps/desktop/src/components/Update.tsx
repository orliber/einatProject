import { useEffect, useState, useSyncExternalStore } from "react";
import { useApp } from "../App";
import { ipc, type UpdateView } from "../ipc/client";
import { Dialog, ErrorLine, Spinner } from "./ui";
import "./Update.css";

/**
 * New versions (D-033, D-038). Once a day, while the vault is open, the program asks the
 * releases page whether Or published a newer version. Only a notice signed with his key counts.
 * A newer version is then fetched and verified in the background, so her one click only locks,
 * restarts and installs. Nothing is installed without that click.
 */

const CHECKED_KEY = "dv.updateCheckedAt";
const AUTO_KEY = "dv.updateAuto";
const DAY_MS = 86_400_000;
/** While the program stays open for days, the daily check still comes around. */
const LOOK_AGAIN_MS = 3_600_000;

let found: UpdateView | null = null;
/** The installer of `found` is on disk, verified, waiting for her click. */
let ready = false;
let preparing = false;
const listeners = new Set<() => void>();
function notify() {
  listeners.forEach((l) => l());
}
function setFound(v: UpdateView | null) {
  if (v?.version !== found?.version) ready = false;
  found = v;
  notify();
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

/** Fetch and verify the found version in the background. Quiet: on failure, tried again later. */
function prepareInBackground() {
  if (!found?.can_install || ready || preparing) return;
  preparing = true;
  ipc
    .prepareUpdate()
    .then((r) => {
      ready = r;
    })
    .catch(() => undefined)
    .finally(() => {
      preparing = false;
      notify();
    });
}

/** Ask now. Quiet failures for the daily check; the settings button shows them. */
export async function checkNow(): Promise<UpdateView | null> {
  const v = await ipc.checkUpdate();
  write(CHECKED_KEY, String(Date.now()));
  setFound(v);
  prepareInBackground();
  return v;
}

/** The daily check, while the vault is open, and every hour after that while it stays open. */
export function useDailyUpdateCheck() {
  const { status } = useApp();
  useEffect(() => {
    if (!status.unlocked) return;
    const look = () => {
      if (!autoCheckOn()) return;
      if (found) {
        prepareInBackground();
        return;
      }
      const last = Number(read(CHECKED_KEY) ?? 0);
      if (Date.now() - last < DAY_MS) return;
      checkNow().catch(() => undefined);
    };
    look();
    const timer = window.setInterval(look, LOOK_AGAIN_MS);
    return () => window.clearInterval(timer);
  }, [status.unlocked]);
}

export function useFoundUpdate(): UpdateView | null {
  return useSyncExternalStore(subscribe, () => found);
}

export function useUpdateReady(): boolean {
  return useSyncExternalStore(subscribe, () => ready);
}

/** "There is a new version": in the top bar, only when there is one. */
export function UpdateChip() {
  useDailyUpdateCheck();
  const update = useFoundUpdate();
  const isReady = useUpdateReady();
  const [open, setOpen] = useState(false);
  if (!update) return null;
  return (
    <>
      <button type="button" className="update-chip" onClick={() => setOpen(true)}>
        <span aria-hidden="true">✦</span> {isReady ? "גרסה חדשה מוכנה" : "יש גרסה חדשה"}
      </button>
      {open && <UpdateDialog update={update} onClose={() => setOpen(false)} />}
    </>
  );
}

export function UpdateDialog({ update, onClose }: { update: UpdateView; onClose: () => void }) {
  const { fail } = useApp();
  const isReady = useUpdateReady();
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
              {busy ? <><Spinner /> {isReady ? "מתעדכנת…" : "מורידה ובודקת…"}</> : isReady ? "להפעיל מחדש ולעדכן" : "לעדכן עכשיו"}
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
            {isReady
              ? "הגרסה כבר ירדה ונבדקה מול החתימה של אור. בלחיצה התוכנה ננעלת, נסגרת"
              : `הגרסה (${update.size_mb} MB) יורדת ברקע ונבדקת מול החתימה של אור לפני שהיא רצה. בלחיצה היא מסיימת לרדת, ואז התוכנה ננעלת, נסגרת`}
            {" "}ונפתחת מחדש בגרסה החדשה. כל התיקים נשארים כמו שהם; פותחים עם אותה סיסמה.
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
        לבדוק פעם ביום אם יש גרסה חדשה, ולהוריד אותה ברקע
      </label>
      <span className="hint">
        הבדיקה וההורדה פונות רק לדף ההתקנות של אור ב-GitHub, בלי שום מידע מהכספת. גרסה מותקנת רק אחרי שלחצת, ורק אם
        היא חתומה במפתח של אור.
      </span>
      {open && update && <UpdateDialog update={update} onClose={() => setOpen(false)} />}
    </section>
  );
}
