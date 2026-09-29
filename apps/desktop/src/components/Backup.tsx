// Encrypted backup (D-024): make one, check one (restore drill), restore one on a new computer.
// The system's own "Save as" / "Open" windows pick the file; the page never sees it.
import { useState, type FormEvent } from "react";
import { useQuery, useQueryClient } from "@tanstack/react-query";
import { useApp } from "../App";
import { ipc, type AppStatus, type BackupCheckView, type StagedBackup } from "../ipc/client";
import { Dialog, ErrorLine } from "./ui";
import "./Backup.css";

export function heDateTime(unix: number): string {
  const d = new Date(unix * 1000);
  return `${d.toLocaleDateString("he-IL")}, ${d.toLocaleTimeString("he-IL", { hour: "2-digit", minute: "2-digit" })}`;
}

export function daysAgo(days: number): string {
  if (days <= 0) return "היום";
  if (days === 1) return "אתמול";
  if (days === 2) return "שלשום";
  return `לפני ${days} ימים`;
}

export function useBackupStatus() {
  return useQuery({ queryKey: ["backup"], queryFn: ipc.backupStatus });
}

/** "Back up now": the "Save as" window, then the result in a toast. */
function useWriteBackup() {
  const { notify, fail } = useApp();
  const qc = useQueryClient();
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  async function write() {
    setBusy(true);
    setError(null);
    try {
      const done = await ipc.writeBackup();
      if (done) {
        notify(`הגיבוי נשמר מוצפן: ${done.path}`);
        await qc.invalidateQueries({ queryKey: ["backup"] });
      }
    } catch (e) {
      setError(fail(e as never));
    } finally {
      setBusy(false);
    }
  }
  return { write, busy, error };
}

/** Settings: when was the last backup, make one, check one. */
export function BackupSettings() {
  const status = useBackupStatus();
  const { write, busy, error } = useWriteBackup();
  const { fail } = useApp();
  const [staged, setStaged] = useState<StagedBackup | null>(null);
  const [chooseError, setChooseError] = useState<string | null>(null);
  const s = status.data;

  async function choose() {
    setChooseError(null);
    try {
      setStaged(await ipc.chooseBackup());
    } catch (e) {
      setChooseError(fail(e as never));
    }
  }

  return (
    <section className="card setting" aria-labelledby="s-backup">
      <h2 id="s-backup">גיבוי</h2>
      <p className="muted small">
        הגיבוי הוא קובץ אחד, מוצפן, שנפתח רק בסיסמה של הכספת (או בערכת השחזור) כפי שהיו ביום הגיבוי.
        כדאי לשמור אותו בדיסק נייד שנשמר במקום אחר, ולא בתיקייה שמסונכרנת לענן: התוכנה לא תשמור לשם.
      </p>
      {s && (
        <ul className="sec-list backup-facts">
          <li>
            <span className={s.due ? "chip chip-warn" : "chip chip-ok"}>{s.last_at === null ? "אין" : s.due ? "ישן" : "עדכני"}</span>
            {s.last_at === null ? "עוד לא נעשה גיבוי." : `הגיבוי האחרון: ${daysAgo(s.days_since ?? 0)} (${heDateTime(s.last_at)}).`}
          </li>
          <li>
            <span className={s.last_check_at === null ? "chip chip-sand" : "chip chip-ok"}>{s.last_check_at === null ? "לא נבדק" : "נבדק"}</span>
            {s.last_check_at === null
              ? "גיבוי שלא נבדק הוא הבטחה. בדיקה פותחת אותו בצד, סופרת את התיקים, ולא נוגעת בכספת."
              : `בדיקת השחזור האחרונה: ${heDateTime(s.last_check_at)}.`}
          </li>
        </ul>
      )}
      <div className="row">
        <button type="button" className="btn btn-primary" disabled={busy} onClick={() => void write()}>
          {busy ? "שומרת גיבוי מוצפן…" : "גיבוי עכשיו…"}
        </button>
        <button type="button" className="btn" onClick={() => void choose()}>בדיקת גיבוי…</button>
      </div>
      <ErrorLine error={error ?? chooseError} />
      {staged && <CheckBackupDialog staged={staged} onClose={() => { setStaged(null); void ipc.forgetBackup(); }} />}
    </section>
  );
}

/** Restore drill: the password opens the chosen backup in a scratch folder, then it is removed. */
export function CheckBackupDialog({ staged, onClose }: { staged: StagedBackup; onClose: () => void }) {
  const qc = useQueryClient();
  const [pw, setPw] = useState("");
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [result, setResult] = useState<BackupCheckView | null>(null);
  const foreign = staged.same_vault === false;

  async function check(e?: FormEvent) {
    e?.preventDefault();
    if (!pw) return;
    setBusy(true);
    setError(null);
    try {
      setResult(await ipc.checkBackup(pw));
      await qc.invalidateQueries({ queryKey: ["backup"] });
    } catch (err) {
      setError((err as { message?: string }).message ?? "הבדיקה נכשלה.");
    } finally {
      setPw("");
      setBusy(false);
    }
  }

  return (
    <Dialog narrow title="בדיקת גיבוי" subtitle={staged.file_name} onClose={onClose}
      footer={result
        ? <button type="button" className="btn btn-primary" onClick={onClose}>סגירה</button>
        : <>
            <button type="button" className="btn" onClick={onClose}>ביטול</button>
            <button type="button" className="btn btn-primary" disabled={busy || !pw || foreign} onClick={() => void check()}>{busy ? "בודקת…" : "בדיקה"}</button>
          </>}>
      {result ? (
        <div className="stack backup-result" role="status">
          <p className={result.integrity_ok ? "backup-ok" : "error"}>
            {result.integrity_ok ? "✓ הגיבוי נפתח ותקין." : "הגיבוי נפתח, אבל בדיקת השלמות מצאה בעיה. כדאי לגבות מחדש."}
          </p>
          <p>נמצאו בו {result.cases === 1 ? "תיק אחד" : `${result.cases} תיקים`} (כולל סל המחזור), מ-{heDateTime(result.created_at)}.</p>
          <p className="hint">העותק שנפתח לבדיקה כבר נמחק. הכספת עצמה לא השתנתה.</p>
        </div>
      ) : (
        <form className="stack" onSubmit={(e) => void check(e)}>
          <p>גיבוי מ-{heDateTime(staged.created_at)}.</p>
          {foreign ? (
            <p className="error">זה גיבוי של כספת אחרת, לא של הכספת הזו.</p>
          ) : (
            <div className="field">
              <label htmlFor="check-pw">הסיסמה של הכספת (זו שהייתה ביום הגיבוי)</label>
              <input id="check-pw" className="input" type="password" autoComplete="current-password" autoFocus value={pw} onChange={(e) => setPw(e.target.value)} />
            </div>
          )}
          <ErrorLine error={error} />
        </form>
      )}
    </Dialog>
  );
}

/** A quiet line on the cases screen once a week has passed without a backup. */
export function BackupReminder() {
  const { go } = useApp();
  const status = useBackupStatus();
  const { write, busy, error } = useWriteBackup();
  const [hidden, setHidden] = useState(false);
  const s = status.data;
  if (!s || !s.due || !s.has_cases || hidden) return null;
  return (
    <div className="backup-reminder" role="status">
      <span className="grow">
        {s.last_at === null ? "עוד לא נעשה גיבוי לכספת." : `הגיבוי האחרון היה ${daysAgo(s.days_since ?? 0)}.`}
        {" "}גיבוי לדיסק נייד שומר על העבודה אם המחשב יתקלקל או ייגנב.
      </span>
      {error && <span className="error small">{error}</span>}
      <button type="button" className="btn btn-small" disabled={busy} onClick={() => void write()}>{busy ? "שומרת…" : "גיבוי עכשיו…"}</button>
      <button type="button" className="link-btn small" onClick={() => go({ name: "settings" })}>עוד על הגיבוי</button>
      <button type="button" className="btn icon-btn" aria-label="לא עכשיו" onClick={() => setHidden(true)}>×</button>
    </div>
  );
}

/** First run on a new computer: restore the vault from a backup instead of starting empty. */
export function RestoreFromBackup({ onRestored, onBack }: { onRestored: (s: AppStatus) => void; onBack: () => void }) {
  const [staged, setStaged] = useState<StagedBackup | null>(null);
  const [mode, setMode] = useState<"password" | "recovery">("password");
  const [secret, setSecret] = useState("");
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);

  async function choose() {
    setError(null);
    try {
      const s = await ipc.chooseBackup();
      if (s) setStaged(s);
    } catch (e) {
      setError((e as { message?: string }).message ?? "הקובץ לא נפתח.");
    }
  }

  async function restore(e: FormEvent) {
    e.preventDefault();
    if (!secret.trim()) return;
    setBusy(true);
    setError(null);
    try {
      const status = mode === "password" ? await ipc.restoreBackup(secret, null) : await ipc.restoreBackup(null, secret);
      setSecret("");
      onRestored(status);
    } catch (err) {
      setError((err as { message?: string }).message ?? "השחזור נכשל.");
      setSecret("");
      setBusy(false);
    }
  }

  return (
    <div className="setup-col stack">
      <h1>שחזור מגיבוי</h1>
      <p className="lede">במחשב חדש, או אחרי תקלה: בוחרים את קובץ הגיבוי (בדרך כלל בדיסק הנייד), ופותחים אותו בסיסמה או בערכת השחזור שהיו ביום הגיבוי.</p>
      {!staged ? (
        <>
          <button type="button" className="btn btn-primary btn-big" onClick={() => void choose()}>בחירת קובץ הגיבוי…</button>
          <ErrorLine error={error} />
        </>
      ) : (
        <form className="stack" onSubmit={(e) => void restore(e)}>
          <div className="paper backup-file">
            <b>{staged.file_name}</b>
            <span className="muted small">גיבוי מ-{heDateTime(staged.created_at)}</span>
            <button type="button" className="link-btn small" onClick={() => { setStaged(null); setError(null); void ipc.forgetBackup(); }}>קובץ אחר</button>
          </div>
          <div className="field">
            <label htmlFor="restore-secret">{mode === "password" ? "הסיסמה של הכספת" : "ערכת השחזור (מהדף המודפס)"}</label>
            <input id="restore-secret" className={mode === "recovery" ? "input mono" : "input"} type={mode === "password" ? "password" : "text"}
              dir={mode === "recovery" ? "ltr" : undefined} autoComplete="off" autoFocus placeholder={mode === "recovery" ? "7K3M-Q9WD-…" : undefined}
              value={secret} onChange={(e) => setSecret(e.target.value)} />
          </div>
          <button type="button" className="link-btn align-start" onClick={() => { setMode(mode === "password" ? "recovery" : "password"); setSecret(""); setError(null); }}>
            {mode === "password" ? "הסיסמה נשכחה? פתיחה עם ערכת השחזור" : "פתיחה עם הסיסמה"}
          </button>
          <ErrorLine error={error} />
          <button type="submit" className="btn btn-primary btn-big" disabled={busy || !secret.trim()}>{busy ? "משחזרת…" : "שחזור ופתיחה"}</button>
        </form>
      )}
      <button type="button" className="link-btn align-start" onClick={() => { void ipc.forgetBackup(); onBack(); }}>חזרה ליצירת כספת חדשה</button>
    </div>
  );
}
