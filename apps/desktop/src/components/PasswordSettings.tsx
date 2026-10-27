// Settings: a new password; under "מתקדם", the printed recovery kit for whoever wants one (D-103).
// Either one asks for a new backup, because older backups keep opening only with what was in
// use on their day (D-024).
import { useState, type FormEvent } from "react";
import { useQueryClient } from "@tanstack/react-query";
import { useApp } from "../App";
import { ipc } from "../ipc/client";
import { RecoveryKitPaper } from "./RecoveryKit";
import { GoogleSettings, useGoogleStatus } from "./GoogleRecovery";
import { useBackupStatus } from "./Backup";
import { Dialog, ErrorLine } from "./ui";
import { timed, WaitLine } from "./Progress";

/** In a dialog, after the password was refused because this computer is not the one it was set on. */
const OTHER_COMPUTER_HINT = "במחשב הזה הסיסמה עוד לא פותחת את הכספת, ולכן מאמתים כאן עם ערכת השחזור. אחרי שבוחרים סיסמה חדשה, היא תעבוד גם במחשב הזה.";

/** The kit's own words when the kit was the proof and it was refused. */
function refusal(e: { code?: string; message?: string }, withRecovery: boolean, fallback: string): string {
  if (withRecovery && e.code === "wrong_secret") return "ערכת השחזור לא נכונה. כדאי לבדוק שגיאות הקלדה.";
  return e.message ?? fallback;
}

/** The current password; the recovery kit only once the password was refused here because this
 *  computer is not the one it was set on (D-103: no kit offered otherwise). */
function CurrentSecret(props: { value: string; onChange: (v: string) => void; withRecovery: boolean; onToggle: () => void }) {
  return (
    <>
      <div className="field">
        <label htmlFor="current-secret">{props.withRecovery ? "ערכת השחזור (מהדף המודפס)" : "הסיסמה הנוכחית"}</label>
        <input id="current-secret" className={props.withRecovery ? "input mono" : "input"} type={props.withRecovery ? "text" : "password"}
          dir={props.withRecovery ? "ltr" : undefined} autoComplete={props.withRecovery ? "off" : "current-password"} autoFocus
          placeholder={props.withRecovery ? "7K3M-Q9WD-…" : undefined} value={props.value} onChange={(e) => props.onChange(e.target.value)} />
      </div>
      {props.withRecovery && (
        <button type="button" className="link-btn align-start" onClick={props.onToggle}>אימות עם הסיסמה</button>
      )}
    </>
  );
}

function ChangePasswordDialog({ onClose }: { onClose: () => void }) {
  const { notify } = useApp();
  const qc = useQueryClient();
  const [current, setCurrent] = useState("");
  const [withRecovery, setWithRecovery] = useState(false);
  const [pw, setPw] = useState("");
  const [pw2, setPw2] = useState("");
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);

  async function save(e?: FormEvent) {
    e?.preventDefault();
    setError(null);
    if (pw.length < 12) return setError("הסיסמה החדשה צריכה להיות באורך 12 תווים לפחות. מומלץ משפט של כמה מילים.");
    if (pw !== pw2) return setError("שתי הסיסמאות החדשות לא זהות.");
    setBusy(true);
    try {
      await timed("rekey", ipc.changePassword(current, withRecovery, pw), "password");
      await qc.invalidateQueries({ queryKey: ["backup"] });
      await qc.invalidateQueries({ queryKey: ["security"] });
      notify("הסיסמה הוחלפה. כדאי לגבות עכשיו: גיבויים קודמים נפתחים רק בסיסמה הקודמת.");
      onClose();
    } catch (err) {
      const e = err as { code?: string; message?: string };
      if (e.code === "other_computer") {
        // She is in already: here the password does not open the vault yet, the kit does.
        setWithRecovery(true);
        setError(OTHER_COMPUTER_HINT);
      } else {
        setError(refusal(e, withRecovery, "הסיסמה לא הוחלפה."));
      }
      setCurrent("");
      setPw("");
      setPw2("");
      setBusy(false);
    }
  }

  return (
    <Dialog narrow title="החלפת סיסמה" onClose={onClose}
      footer={<>
        <button type="button" className="btn" onClick={onClose}>ביטול</button>
        <button type="button" className="btn btn-primary" disabled={busy || !current || !pw} onClick={() => void save()}>{busy ? "מחליפה…" : "החלפה"}</button>
      </>}>
      <form className="stack" onSubmit={(e) => void save(e)}>
        <CurrentSecret value={current} onChange={setCurrent} withRecovery={withRecovery}
          onToggle={() => { setWithRecovery(!withRecovery); setCurrent(""); setError(null); }} />
        <div className="field">
          <label htmlFor="new-pw">סיסמה חדשה</label>
          <input id="new-pw" className="input" type="password" autoComplete="new-password" value={pw} onChange={(e) => setPw(e.target.value)} />
        </div>
        <div className="field">
          <label htmlFor="new-pw2">שוב, לאימות</label>
          <input id="new-pw2" className="input" type="password" autoComplete="new-password" value={pw2} onChange={(e) => setPw2(e.target.value)} />
        </div>
        <ul className="pw-checks" aria-label="דרישות הסיסמה">
          <li className={pw.length >= 12 ? "met" : undefined}>{pw.length >= 12 ? "✓" : "○"} 12 תווים לפחות</li>
          <li className={pw2.length > 0 && pw === pw2 ? "met" : undefined}>{pw2.length > 0 && pw === pw2 ? "✓" : "○"} שתי הסיסמאות זהות</li>
        </ul>
        <button type="submit" hidden />
        {busy && <WaitLine task="rekey" speed="password" label="מצפינה את הכספת מחדש בסיסמה החדשה" />}
        <ErrorLine error={error} />
      </form>
    </Dialog>
  );
}

function NewKitDialog({ onClose }: { onClose: () => void }) {
  const { notify } = useApp();
  const qc = useQueryClient();
  const [current, setCurrent] = useState("");
  const [withRecovery, setWithRecovery] = useState(false);
  const [kit, setKit] = useState<string | null>(null);
  const [typed, setTyped] = useState("");
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);

  async function issue(e?: FormEvent) {
    e?.preventDefault();
    setBusy(true);
    setError(null);
    try {
      const created = await timed("rekey", ipc.newRecoveryKit(current, withRecovery), "kit");
      setCurrent("");
      setKit(created.recovery_key);
      await qc.invalidateQueries({ queryKey: ["backup"] });
      await qc.invalidateQueries({ queryKey: ["security"] });
    } catch (err) {
      const e = err as { code?: string; message?: string };
      if (e.code === "other_computer") {
        setWithRecovery(true);
        setError(OTHER_COMPUTER_HINT);
      } else {
        setError(refusal(e, withRecovery, "לא נוצרה ערכה."));
      }
      setCurrent("");
    } finally {
      setBusy(false);
    }
  }

  async function confirm(e?: FormEvent) {
    e?.preventDefault();
    setError(null);
    const ok = await ipc.confirmRecoveryKey(typed).catch(() => false);
    if (!ok) return setError("הערכה שהוקלדה לא תואמת. כדאי לבדוק את הדף ולנסות שוב.");
    // Nothing secret stays in the screen's memory once it served (SEC10).
    setKit(null);
    setTyped("");
    notify("ערכת השחזור החדשה נשמרה. כדאי לגבות עכשיו.");
    onClose();
  }

  if (kit) {
    // No closing until the new kit is confirmed: the old one no longer opens the vault.
    return (
      <Dialog title="ערכת השחזור החדשה"
        footer={<button type="button" className="btn btn-primary" disabled={typed.trim().length < 20} onClick={() => void confirm()}>סיום</button>}>
        <div className="stack">
          <p>מדפיסים או מעתיקים לדף, ושומרים במקום בטוח, לא במחשב ולא בטלפון. הערכה הקודמת כבר לא פותחת את הכספת.</p>
          <RecoveryKitPaper recoveryKey={kit} />
          <form className="stack" onSubmit={(e) => void confirm(e)}>
            <div className="field">
              <label htmlFor="kit-typed">כדי לוודא שהערכה נשמרה, הקלידי אותה מהדף</label>
              <input id="kit-typed" className="input mono" dir="ltr" placeholder="XXXX-XXXX-…" value={typed} onChange={(e) => setTyped(e.target.value)} />
            </div>
            <ErrorLine error={error} />
          </form>
        </div>
      </Dialog>
    );
  }
  return (
    <Dialog narrow title="ערכת שחזור חדשה" onClose={onClose}
      footer={<>
        <button type="button" className="btn" onClick={onClose}>ביטול</button>
        <button type="button" className="btn btn-primary" disabled={busy || !current} onClick={() => void issue()}>{busy ? "יוצרת…" : "יצירת ערכה חדשה"}</button>
      </>}>
      <form className="stack" onSubmit={(e) => void issue(e)}>
        <p>דף מודפס שפותח את הכספת בלי הסיסמה. ערכה קודמת, אם הייתה, תפסיק לפתוח את הכספת, אבל גיבויים קודמים עדיין נפתחים בה.</p>
        <CurrentSecret value={current} onChange={setCurrent} withRecovery={withRecovery}
          onToggle={() => { setWithRecovery(!withRecovery); setCurrent(""); setError(null); }} />
        {busy && <WaitLine task="rekey" speed="kit" label="יוצרת ערכת שחזור חדשה ומצפינה מחדש" />}
        <ErrorLine error={error} />
      </form>
    </Dialog>
  );
}

/** Type the printed kit to see that it still opens the vault (once a year, D-076). */
function CheckKitDialog({ onClose }: { onClose: () => void }) {
  const { notify } = useApp();
  const qc = useQueryClient();
  const [typed, setTyped] = useState("");
  const [error, setError] = useState<string | null>(null);

  async function check(e?: FormEvent) {
    e?.preventDefault();
    setError(null);
    let ok: boolean;
    try {
      ok = await ipc.confirmRecoveryKey(typed);
    } catch (err) {
      // A locked vault or another failure is said as itself, not as a wrong kit.
      setTyped("");
      return setError((err as { message?: string }).message ?? "הבדיקה לא הצליחה. אפשר לנסות שוב.");
    }
    setTyped("");
    if (!ok) return setError("הערכה שהוקלדה לא פותחת את הכספת. כדאי לבדוק את הדף, ואם הוא אבד ליצור ערכה חדשה.");
    await qc.invalidateQueries({ queryKey: ["security"] });
    notify("ערכת השחזור פותחת את הכספת. כדאי לשמור את הדף במקום שלו.");
    onClose();
  }

  return (
    <Dialog narrow title="בדיקת ערכת השחזור" onClose={onClose}
      footer={<>
        <button type="button" className="btn" onClick={onClose}>ביטול</button>
        <button type="button" className="btn btn-primary" disabled={typed.trim().length < 20} onClick={() => void check()}>בדיקה</button>
      </>}>
      <form className="stack" onSubmit={(e) => void check(e)}>
        <p>מקלידים את הדף המודפס כדי לוודא שהוא עדיין פותח את הכספת.</p>
        <div className="field">
          <label htmlFor="kit-check">ערכת השחזור, מהדף המודפס</label>
          <input id="kit-check" className="input mono" dir="ltr" autoComplete="off" autoFocus placeholder="XXXX-XXXX-…" value={typed} onChange={(e) => setTyped(e.target.value)} />
        </div>
        <ErrorLine error={error} />
      </form>
    </Dialog>
  );
}

/** The kit she has stops opening the vault; nothing new is shown (D-103). */
function RetireKitDialog({ onClose }: { onClose: () => void }) {
  const { notify } = useApp();
  const qc = useQueryClient();
  const [current, setCurrent] = useState("");
  const [withRecovery, setWithRecovery] = useState(false);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);

  async function retire(e?: FormEvent) {
    e?.preventDefault();
    setBusy(true);
    setError(null);
    try {
      await timed("rekey", ipc.retireRecoveryKit(current, withRecovery), "kit");
      setCurrent("");
      await qc.invalidateQueries({ queryKey: ["backup"] });
      await qc.invalidateQueries({ queryKey: ["security"] });
      notify("ערכת השחזור כבר לא פותחת את הכספת. כדאי לגבות עכשיו.");
      onClose();
    } catch (err) {
      const e = err as { code?: string; message?: string };
      if (e.code === "other_computer") {
        setWithRecovery(true);
        setError(OTHER_COMPUTER_HINT);
      } else {
        setError(refusal(e, withRecovery, "הערכה לא בוטלה."));
      }
      setCurrent("");
    } finally {
      setBusy(false);
    }
  }

  return (
    <Dialog narrow title="ביטול ערכת השחזור" onClose={onClose}
      footer={<>
        <button type="button" className="btn" onClick={onClose}>חזרה</button>
        <button type="button" className="btn btn-primary" disabled={busy || !current} onClick={() => void retire()}>{busy ? "מבטלת…" : "ביטול הערכה"}</button>
      </>}>
      <form className="stack" onSubmit={(e) => void retire(e)}>
        <p>דף שנשכח במגירה לא יפתח יותר את הכספת עצמה.</p>
        <p><strong>אבל כל קובץ גיבוי שנשמר לפני הביטול עדיין נפתח בדף הישן,</strong> עד שהקובץ נמחק. כדאי לגבות מיד אחרי הביטול, ולמחוק מהדיסק הנייד את קובצי הגיבוי הישנים.</p>
        <p className="muted small">בלי ערכה, סיסמה שנשכחה נפתחת רק עם כניסה עם גוגל.</p>
        <CurrentSecret value={current} onChange={setCurrent} withRecovery={withRecovery}
          onToggle={() => { setWithRecovery(!withRecovery); setCurrent(""); setError(null); }} />
        {busy && <WaitLine task="rekey" speed="kit" label="מבטלת את ערכת השחזור" />}
        <ErrorLine error={error} />
      </form>
    </Dialog>
  );
}

/**
 * Why the kit cannot be retired yet, or null (D-103, the same two conditions the core checks):
 * Google sign-in on here, and a backup newer than the last change of the password or kit.
 */
export function retireBlocked(googleOn: boolean | undefined, backup: { last_at: number | null; secret_changed: boolean } | undefined): string | null {
  if (!googleOn) return "קודם מפעילים כניסה עם גוגל (למעלה): בלי ערכה ובלי גוגל, סיסמה שנשכחה = אין דרך להיכנס.";
  if (!backup || backup.last_at === null || backup.secret_changed) return "קודם גיבוי עדכני לדיסק נייד (הגדרות ← גיבוי): הוא מה שפותח את הכספת במחשב אחר.";
  return null;
}

export function PasswordSettings() {
  const [open, setOpen] = useState<"password" | "kit" | "check" | "retire" | null>(null);
  const google = useGoogleStatus();
  const backup = useBackupStatus();
  const blocked = retireBlocked(google.data?.on, backup.data);
  return (
    <>
    <section className="card setting" aria-labelledby="s-password">
      <h2 id="s-password">סיסמה</h2>
      <p className="muted small">אחרי החלפה כדאי לגבות מחדש.</p>
      <div className="row">
        <button type="button" className="btn" onClick={() => setOpen("password")}>החלפת סיסמה…</button>
      </div>
      {open === "password" && <ChangePasswordDialog onClose={() => setOpen(null)} />}
    </section>
    <GoogleSettings />
    {/* The kit is no longer part of her way in (D-103): kept here, folded, as a last resort. */}
    <details className="card setting advanced">
      <summary>מתקדם: ערכת שחזור מודפסת (לא חובה)</summary>
      <p className="muted small">דף מודפס שפותח את הכספת בלי הסיסמה. לא צריך אותו כשהכניסה עם גוגל מופעלת. שימושי רק אם גוגל לא זמינה.</p>
      <div className="row">
        <button type="button" className="btn" onClick={() => setOpen("kit")}>יצירת ערכה…</button>
        <button type="button" className="btn" onClick={() => setOpen("check")}>בדיקת ערכה…</button>
        <button type="button" className="btn" disabled={blocked !== null} aria-describedby={blocked ? "retire-blocked" : undefined} onClick={() => setOpen("retire")}>ביטול הערכה הקיימת…</button>
      </div>
      {blocked && <p id="retire-blocked" className="hint">{blocked}</p>}
      {open === "kit" && <NewKitDialog onClose={() => setOpen(null)} />}
      {open === "check" && <CheckKitDialog onClose={() => setOpen(null)} />}
      {open === "retire" && <RetireKitDialog onClose={() => setOpen(null)} />}
    </details>
    </>
  );
}
