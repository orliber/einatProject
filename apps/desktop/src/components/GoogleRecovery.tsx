// Forgot the password → sign in with Google (D-041).
// The vault opens with Google only together with a key that stays on this computer, so a
// broken-into Google account alone opens nothing. Sign-in happens in her own browser.
import { useState, type FormEvent } from "react";
import { useQuery, useQueryClient } from "@tanstack/react-query";
import { useApp } from "../App";
import { ipc, type AppStatus } from "../ipc/client";
import { Dialog, ErrorLine } from "./ui";

function message(err: unknown, fallback: string): string {
  return (err as { message?: string }).message ?? fallback;
}

export function useGoogleStatus() {
  return useQuery({ queryKey: ["google"], queryFn: ipc.googleStatus, retry: false });
}

/** While her browser is open: what to do there, and a way to stop. */
function WaitingForBrowser() {
  return (
    <div className="stack" role="status">
      <p>נפתח חלון של גוגל בדפדפן. בוחרים שם את החשבון ומאשרים, ואז חוזרים לכאן.</p>
      <button type="button" className="btn align-start" onClick={() => void ipc.googleCancel()}>ביטול</button>
    </div>
  );
}

const TWO_STEP =
  "חשוב: בחשבון הגוגל צריך להיות מופעל אימות דו-שלבי (בהגדרות החשבון ← אבטחה ← אימות דו-שלבי). כך מי שמגלה את סיסמת הגוגל לא נכנס לחשבון.";

/**
 * Turning it on: her vault password, then Google in her browser. Needs the vault open (the
 * settings, or first-run setup right after the kit). What follows success is the caller's.
 */
export function TurnOnDialog({ onClose, onTurnedOn }: { onClose: () => void; onTurnedOn: () => Promise<void> | void }) {
  const [password, setPassword] = useState("");
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);

  async function turnOn(e?: FormEvent) {
    e?.preventDefault();
    if (!password) return;
    setBusy(true);
    setError(null);
    try {
      await ipc.googleTurnOn(password);
      await onTurnedOn();
      onClose();
    } catch (err) {
      setError(message(err, "החיבור לגוגל לא הושלם."));
      setPassword("");
      setBusy(false);
    }
  }

  return (
    <Dialog narrow title="כניסה עם גוגל אם שוכחים את הסיסמה" {...(busy ? {} : { onClose })}
      footer={busy ? null : <>
        <button type="button" className="btn" onClick={onClose}>ביטול</button>
        <button type="button" className="btn btn-primary" disabled={!password} onClick={() => void turnOn()}>המשך לגוגל</button>
      </>}>
      {busy ? <WaitingForBrowser /> : (
        <form className="stack" onSubmit={(e) => void turnOn(e)}>
          <p>אם הסיסמה תישכח, אפשר יהיה להיכנס עם חשבון הגוגל ולבחור סיסמה חדשה.</p>
          <ul className="small">
            <li>זה עובד רק במחשב הזה: חלק מהמפתח נשמר בגוגל, וחלק נשמר במחשב, קשור למשתמש שלך בווינדוס. גוגל לבדה לא פותחת את הכספת, וגם לא מי שפורץ לחשבון הגוגל.</li>
            <li>לגוגל נשמר רק מפתח, בתיקייה נסתרת של התוכנה. שום תיק, שם או טקסט לא יוצא.</li>
            <li>במחשב חדש משחזרים את הגיבוי עם הסיסמה.</li>
          </ul>
          <p className="hint">{TWO_STEP}</p>
          <div className="field">
            <label htmlFor="g-pw">הסיסמה הנוכחית של הכספת</label>
            <input id="g-pw" className="input" type="password" autoComplete="current-password" autoFocus
              value={password} onChange={(e) => setPassword(e.target.value)} />
          </div>
          <button type="submit" hidden />
          <ErrorLine error={error} />
        </form>
      )}
    </Dialog>
  );
}

/** Said when disconnecting Google while the encrypted copy in Drive is on (D-099). */
export const DRIVE_COPY_TURNS_OFF =
  "גם העותק המוצפן בגוגל דרייב יכובה: העותקים שם יימחקו, וההרשאה שניתנה לתוכנה בגוגל תבוטל.";

function TurnOffDialog({ onClose }: { onClose: () => void }) {
  const { notify } = useApp();
  const qc = useQueryClient();
  const { data: drive } = useQuery({ queryKey: ["drive-backup"], queryFn: ipc.driveBackupStatus, retry: false });
  const copyOn = drive?.on === true;
  const [password, setPassword] = useState("");
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);

  async function turnOff(e?: FormEvent) {
    e?.preventDefault();
    if (!password) return;
    setBusy(true);
    setError(null);
    try {
      await ipc.googleTurnOff(password);
      await Promise.all([
        qc.invalidateQueries({ queryKey: ["google"] }),
        qc.invalidateQueries({ queryKey: ["drive-backup"] }),
      ]);
      notify(copyOn
        ? "הכניסה עם גוגל בוטלה, והעותק בגוגל דרייב כובה. המפתח שנשאר בגוגל כבר לא פותח כלום."
        : "הכניסה עם גוגל בוטלה. המפתח שנשאר בגוגל כבר לא פותח כלום.");
      onClose();
    } catch (err) {
      setError(message(err, "הביטול לא הושלם."));
      setPassword("");
      setBusy(false);
    }
  }

  return (
    <Dialog narrow title="ביטול הכניסה עם גוגל" onClose={onClose}
      footer={<>
        <button type="button" className="btn" onClick={onClose}>חזרה</button>
        <button type="button" className="btn btn-danger" disabled={busy || !password} onClick={() => void turnOff()}>ביטול הכניסה עם גוגל</button>
      </>}>
      <form className="stack" onSubmit={(e) => void turnOff(e)}>
        <p>אחרי הביטול, אם הסיסמה תישכח, אין דרך להיכנס (אלא אם יצרת ערכת שחזור תחת "מתקדם").</p>
        {copyOn && <p>{DRIVE_COPY_TURNS_OFF}</p>}
        <div className="field">
          <label htmlFor="g-off-pw">הסיסמה הנוכחית של הכספת</label>
          <input id="g-off-pw" className="input" type="password" autoComplete="current-password" autoFocus
            value={password} onChange={(e) => setPassword(e.target.value)} />
        </div>
        <button type="submit" hidden />
        <ErrorLine error={error} />
      </form>
    </Dialog>
  );
}

/** Settings: turn it on or off. Hidden where it cannot work (not Windows, or not set up). */
export function GoogleSettings() {
  const { notify } = useApp();
  const qc = useQueryClient();
  const { data: status } = useGoogleStatus();
  const [open, setOpen] = useState<"on" | "off" | null>(null);
  if (!status?.available) return null;
  return (
    <section className="card setting" aria-labelledby="s-google">
      <h2 id="s-google">כניסה עם גוגל כששוכחים סיסמה</h2>
      {status.on ? (
        <p className="muted small"><span className="chip chip-ok">פעיל</span> במסך הכניסה: "שכחתי את הסיסמה".</p>
      ) : status.needs_setup_here ? (
        <p className="muted small">הופעלה במחשב אחר. כדי שתעבוד כאן, מחברים שוב.</p>
      ) : (
        <p className="muted small">אם הסיסמה תישכח, נכנסים עם גוגל ובוחרים סיסמה חדשה. רק מהמחשב הזה.</p>
      )}
      <div className="row">
        {status.on ? (
          <>
            <button type="button" className="btn" onClick={() => setOpen("on")}>חיבור לחשבון אחר…</button>
            <button type="button" className="btn" onClick={() => setOpen("off")}>ביטול…</button>
          </>
        ) : (
          <button type="button" className="btn" onClick={() => setOpen("on")}>הפעלה…</button>
        )}
      </div>
      {open === "on" && <TurnOnDialog onClose={() => setOpen(null)} onTurnedOn={async () => {
        await qc.invalidateQueries({ queryKey: ["google"] });
        notify("הכניסה עם גוגל הופעלה. אם הסיסמה תישכח, נכנסים מהמחשב הזה עם חשבון הגוגל.");
      }} />}
      {open === "off" && <TurnOffDialog onClose={() => setOpen(null)} />}
    </section>
  );
}

/** Lock screen, password forgotten: a new password, then Google in the browser. */
export function GoogleRecoverForm({ onUnlocked, onBack }: { onUnlocked?: ((s: AppStatus) => void) | undefined; onBack: () => void }) {
  const [pw, setPw] = useState("");
  const [pw2, setPw2] = useState("");
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);

  async function recover(e?: FormEvent) {
    e?.preventDefault();
    setError(null);
    if (pw.length < 12) return setError("הסיסמה החדשה צריכה להיות באורך 12 תווים לפחות. מומלץ משפט של כמה מילים.");
    if (pw !== pw2) return setError("שתי הסיסמאות החדשות לא זהות.");
    setBusy(true);
    try {
      const s = await ipc.googleRecover(pw);
      setPw("");
      setPw2("");
      onUnlocked?.(s);
    } catch (err) {
      setError(message(err, "הכניסה עם גוגל לא הושלמה."));
      setBusy(false);
    }
  }

  if (busy) return <WaitingForBrowser />;
  return (
    <form className="lock-form" onSubmit={(e) => void recover(e)}>
      <p className="hint">בוחרים סיסמה חדשה, ואז נכנסים עם חשבון הגוגל שחובר לכספת. הסיסמה הישנה תפסיק לעבוד.</p>
      <div className="field">
        <label htmlFor="g-new">סיסמה חדשה</label>
        <input id="g-new" className="input lock-input" type="password" autoComplete="new-password" autoFocus
          value={pw} onChange={(e) => setPw(e.target.value)} />
      </div>
      <div className="field">
        <label htmlFor="g-new2">שוב, לאימות</label>
        <input id="g-new2" className="input lock-input" type="password" autoComplete="new-password"
          value={pw2} onChange={(e) => setPw2(e.target.value)} />
      </div>
      <ErrorLine error={error} />
      <button type="submit" className="btn btn-primary" disabled={!pw || !pw2}>כניסה עם גוגל</button>
      <button type="button" className="link-btn" onClick={onBack}>חזרה לפתיחה עם סיסמה</button>
    </form>
  );
}
