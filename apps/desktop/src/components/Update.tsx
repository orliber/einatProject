import { useContext, useEffect, useState, useSyncExternalStore } from "react";
import { AppContext, useApp } from "../App";
import type { UiError } from "../ipc/client";
import { ipc, type UpdateView } from "../ipc/client";
import { Dialog, ErrorLine, Spinner, SparkIcon } from "./ui";
import { Switch } from "./Switch";
import { timed, WaitLine } from "./Progress";
import { holdNow } from "./Unsaved";
import "./Update.css";

/**
 * New versions (D-033, D-038). Once a day, while the vault is open, the program asks the
 * releases page whether Or published a newer version. Only a notice signed with his key counts.
 * A newer version is then fetched and verified in the background, so her one click only locks,
 * restarts and installs. Nothing is installed without that click.
 */

const CHECKED_KEY = "dv.updateCheckedAt";
const AUTO_KEY = "dv.updateAuto";
/** The version found, as the releases page described it (nothing from the vault): the chip
 *  comes back after the reload at a lock and after a restart, until a check finds nothing
 *  newer or the running version has caught up. */
const FOUND_KEY = "dv.updateFound";
const DAY_MS = 86_400_000;
/** While the program stays open for days, the daily check still comes around. */
const LOOK_AGAIN_MS = 3_600_000;

function read(key: string): string | null {
  try {
    return localStorage.getItem(key);
  } catch {
    return null;
  }
}
function write(key: string, value: string | null) {
  try {
    if (value === null) localStorage.removeItem(key);
    else localStorage.setItem(key, value);
  } catch {
    // Private storage off: checked again next time.
  }
}

/** The stored version, only when it has the shape the core gives (a stale or edited entry is dropped). */
function restoreFound(): UpdateView | null {
  const raw = read(FOUND_KEY);
  if (!raw) return null;
  try {
    const v = JSON.parse(raw) as Partial<UpdateView> | null;
    if (
      v && typeof v === "object" && typeof v.version === "string" && typeof v.current === "string" &&
      typeof v.published === "string" && typeof v.size_mb === "number" && typeof v.can_install === "boolean" &&
      Array.isArray(v.notes) && v.notes.every((n) => typeof n === "string")
    ) {
      return { version: v.version, current: v.current, published: v.published, notes: v.notes, size_mb: v.size_mb, can_install: v.can_install };
    }
  } catch {
    // Not what was written: forgotten.
  }
  write(FOUND_KEY, null);
  return null;
}

/** "0.9.3" against "0.9.10", by number; what follows a dash is ignored. */
export function versionAtLeast(running: string, wanted: string): boolean {
  const parts = (v: string) => v.replace(/^v/, "").split("-")[0]?.split(".").map((n) => Number.parseInt(n, 10) || 0) ?? [];
  const a = parts(running);
  const b = parts(wanted);
  for (let i = 0; i < Math.max(a.length, b.length); i++) {
    const x = a[i] ?? 0;
    const y = b[i] ?? 0;
    if (x !== y) return x > y;
  }
  return true;
}

let found: UpdateView | null = restoreFound();
/** A version remembered from before the reload: the running version is asked once, in case it is already this one. */
let unverified = found !== null;
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
  unverified = false;
  write(FOUND_KEY, v ? JSON.stringify(v) : null);
  notify();
}
function subscribe(l: () => void) {
  listeners.add(l);
  return () => listeners.delete(l);
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
  // Nothing newer than what runs: nothing to offer (and nothing remembered).
  const newer = v && !versionAtLeast(v.current, v.version) ? v : null;
  setFound(newer);
  prepareInBackground();
  return newer;
}

/** A version remembered from before the reload is dropped when this is already it (the update
 *  was installed, or she installed it by hand); otherwise its installer is fetched again. */
async function verifyRemembered(): Promise<void> {
  if (!unverified || !found) return;
  unverified = false;
  const remembered = found;
  try {
    const { core_version } = await ipc.ping();
    if (found === remembered && versionAtLeast(core_version, remembered.version)) {
      setFound(null);
      return;
    }
  } catch {
    // The version could not be asked: the remembered one is offered as it was.
  }
  prepareInBackground();
}

/**
 * The daily check, and every hour after that while the program stays open. It runs on the
 * lock screen too (D-038): a version that cannot open the vault must still be able to receive
 * the version that fixes it. Nothing from the vault is involved either way.
 */
export function useDailyUpdateCheck() {
  useEffect(() => {
    const look = () => {
      if (unverified) {
        // First the remembered version; the daily check follows once that is settled.
        void verifyRemembered().then(look);
        return;
      }
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
  }, []);
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
      {/* In a narrow bar (a laptop, or larger text) the words shorten; the name stays whole. */}
      <button type="button" className="update-chip" aria-label={isReady ? "גרסה חדשה מוכנה" : "יש גרסה חדשה"} onClick={() => setOpen(true)}>
        <SparkIcon /> <span className="update-chip-long">{isReady ? "גרסה חדשה מוכנה" : "יש גרסה חדשה"}</span>
        <span className="update-chip-short" aria-hidden="true">גרסה חדשה</span>
      </button>
      {open && <UpdateDialog update={update} onClose={() => setOpen(false)} />}
    </>
  );
}

export function UpdateDialog({ update, onClose }: { update: UpdateView; onClose: () => void }) {
  // Also shown on the lock screen, where there is no open vault to report errors to.
  const app = useContext(AppContext);
  const fail = app?.fail ?? ((e: UiError) => e.message);
  const isReady = useUpdateReady();
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  /** A Mac whose Applications folder cannot be written: the verified image was opened in
   *  Finder, and she finishes by hand. Guidance, not a failure: nothing to try again. */
  const [byHand, setByHand] = useState<string | null>(null);

  async function install() {
    setBusy(true);
    setError(null);
    setByHand(null);
    try {
      // What she was typing reaches the core first, as before a lock by hand: the install locks.
      await holdNow();
      await ipc.installUpdate();
    } catch (e) {
      const u = e as UiError;
      if (u.code === "update_by_hand") setByHand(u.message);
      else setError(fail(u));
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
        {/* Never measured: a finished install closes the program (it reopens on the new version),
            so the guess comes from the installer's size. */}
        {busy && !isReady && <WaitLine task="update" delay={0} guess={(10 + update.size_mb * 2) * 1000} label="מורידה את הגרסה ובודקת את החתימה שלה" />}
        {update.notes.length > 0 && (
          <>
            <b>מה חדש</b>
            <ul className="update-notes">{update.notes.map((n, i) => <li key={i}>{n}</li>)}</ul>
          </>
        )}
        {update.can_install ? (
          <p className="muted small">
            {isReady
              ? "הגרסה ירדה ונבדקה. בלחיצה התוכנה נסגרת"
              : `הגרסה (${update.size_mb} MB) יורדת ונבדקת ברקע. בלחיצה התוכנה נסגרת`}
            {" "}ונפתחת בגרסה החדשה. התיקים והסיסמה נשארים.
          </p>
        ) : (
          <p className="muted small">במחשב הזה העדכון לא מותקן לבד. מתקינים את הגרסה החדשה מהקישור ששלח אור.</p>
        )}
        {byHand && <p className="hint" role="status">{byHand}</p>}
        <ErrorLine error={error} />
      </div>
    </Dialog>
  );
}

/** On the lock screen: the same daily check, and the update when one is waiting. */
export function LockScreenUpdate() {
  useDailyUpdateCheck();
  const update = useFoundUpdate();
  const isReady = useUpdateReady();
  const [open, setOpen] = useState(false);
  if (!update?.can_install) return null;
  return (
    <>
      <button type="button" className="update-chip" onClick={() => setOpen(true)}>
        <SparkIcon /> {isReady ? `גרסה ${update.version} מוכנה` : `יש גרסה ${update.version}`}
      </button>
      {open && <UpdateDialog update={update} onClose={() => setOpen(false)} />}
    </>
  );
}

/**
 * D-104: a newer version opened the vault, so this one does not open it and changes nothing in
 * it. Shown on the lock screen instead of the password: what to do, and the new version itself
 * (the same signed update as everywhere, or the releases page where this computer does not
 * install by itself). Never the kit: nothing is wrong with the password or the vault.
 */
export function NewerVaultNotice({ version }: { version: string }) {
  const update = useFoundUpdate();
  const [open, setOpen] = useState(false);
  const [busy, setBusy] = useState(false);
  const [none, setNone] = useState(false);
  const [error, setError] = useState<string | null>(null);

  async function install() {
    if (update?.can_install) {
      setOpen(true);
      return;
    }
    setBusy(true);
    setNone(false);
    setError(null);
    try {
      const v = await timed("check", checkNow(), "update");
      if (v?.can_install) setOpen(true);
      else setNone(true);
    } catch (e) {
      setError((e as UiError).message ?? "הבדיקה לא הצליחה. אפשר לנסות שוב, או להוריד מדף הגרסאות.");
    } finally {
      setBusy(false);
    }
  }

  function releases() {
    setError(null);
    ipc.openReleasesPage().catch((e: { message?: string }) => setError(e.message ?? "הדף לא נפתח."));
  }

  return (
    <div className="lock-form newer-vault" role="alert">
      <div className="lock-note">
        <p><strong>הכספת נפתחה בגרסה חדשה יותר{version ? ` (${version})` : ""}. צריך להתקין את הגרסה החדשה.</strong></p>
        <p>הכספת והתיקים לא נפגעו, והגרסה הזו לא משנה בהם כלום. לא צריך את ערכת השחזור.</p>
      </div>
      {busy && <WaitLine task="check" speed="update" label="בודקת ב-GitHub מה הגרסה החדשה" />}
      {none && <p className="hint" role="status">כאן הגרסה החדשה לא מותקנת לבד. מורידים אותה מדף הגרסאות ומתקינים.</p>}
      <ErrorLine error={error} />
      <button type="button" className="btn btn-primary" autoFocus disabled={busy} onClick={() => void install()}>
        {busy ? <><Spinner /> בודקת…</> : update?.can_install ? `להתקנת גרסה ${update.version}` : "להתקנת הגרסה החדשה"}
      </button>
      <button type="button" className="link-btn" onClick={releases}>לדף הגרסאות (נפתח בדפדפן)</button>
      {open && update && <UpdateDialog update={update} onClose={() => setOpen(false)} />}
    </div>
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
      const v = await timed("check", checkNow(), "update");
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
      {busy && <WaitLine task="check" speed="update" label="בודקת ב-GitHub אם יש גרסה חדשה" />}
      {result && <span className="small muted" role="status">{result}</span>}
      <ErrorLine error={error} />
      <Switch id="update-auto" label="לבדוק פעם ביום אם יש גרסה חדשה" checked={auto}
        hint="בלי שום מידע מהכספת. מתקינים רק בלחיצה, ורק גרסה חתומה."
        onChange={(v) => { setAuto(v); write(AUTO_KEY, v ? "on" : "off"); }} />
      {open && update && <UpdateDialog update={update} onClose={() => setOpen(false)} />}
    </section>
  );
}
