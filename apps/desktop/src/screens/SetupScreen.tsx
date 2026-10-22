import { useEffect, useState, type FormEvent } from "react";
import { he } from "../i18n/he";
import { ipc, type AppStatus } from "../ipc/client";
import { ErrorLine, LockIcon } from "../components/ui";
import { RestoreFromBackup } from "../components/Backup";
import { RecoveryKitPaper } from "../components/RecoveryKit";
import { TurnOnDialog } from "../components/GoogleRecovery";
import "./SetupScreen.css";

type Step = "password" | "recovery" | "google" | "me" | "restore";

/** First run: a password, the printed recovery kit, Google for a forgotten password (optional,
 *  where this version has it), and the practitioner's own names. */
export function SetupScreen({ status, onCreated, onDone, onRestored }: { status: AppStatus; onCreated?: () => void; onDone: () => Promise<void>; onRestored?: (s: AppStatus) => void }) {
  const [step, setStep] = useState<Step>("password");
  const [pw, setPw] = useState("");
  const [pw2, setPw2] = useState("");
  const [key, setKey] = useState("");
  const [typed, setTyped] = useState("");
  // How far the typed-back kit has come (groups of four letters or digits).
  const kitGroups = key.split("-").filter(Boolean).length;
  const typedGroups = Math.floor(typed.replace(/[^0-9a-z]/gi, "").length / 4);
  const [names, setNames] = useState("");
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  // Sign in with Google when the password is forgotten (D-041): offered once, right after the
  // kit, while the vault is open; skipped where this version cannot do it (D-096).
  const [googleAvailable, setGoogleAvailable] = useState(false);
  const [googleOpen, setGoogleOpen] = useState(false);
  const [googleOn, setGoogleOn] = useState(false);
  useEffect(() => {
    ipc.googleStatus()
      .then((g) => setGoogleAvailable(g?.available === true && !g.on))
      .catch(() => setGoogleAvailable(false));
  }, []);
  const order: Step[] = googleAvailable ? ["password", "recovery", "google", "me"] : ["password", "recovery", "me"];

  async function create(e: FormEvent) {
    e.preventDefault();
    setError(null);
    if (pw.length < 12) return setError("לפחות 12 תווים. מומלץ משפט קצר.");
    if (pw !== pw2) return setError("שתי הסיסמאות לא זהות.");
    setBusy(true);
    try {
      const created = await ipc.createVault(pw);
      setKey(created.recovery_key);
      setPw("");
      setPw2("");
      setStep("recovery");
      onCreated?.();
    } catch (err) {
      setError((err as { message: string }).message);
    } finally {
      setBusy(false);
    }
  }

  async function confirm(e: FormEvent) {
    e.preventDefault();
    setError(null);
    const ok = await ipc.confirmRecoveryKey(typed).catch(() => false);
    if (!ok) return setError("הערכה לא תואמת. בודקים את הדף ומנסים שוב.");
    // Nothing secret stays in the screen's memory once it served (SEC10).
    setKey("");
    setTyped("");
    setStep(googleAvailable ? "google" : "me");
  }

  async function finish(e: FormEvent) {
    e.preventDefault();
    setBusy(true);
    try {
      const list = names.split(/[,\n]/).map((n) => n.trim()).filter(Boolean);
      if (list.length) await ipc.setPractitioner(list);
      await onDone();
    } catch (err) {
      setError((err as { message: string }).message);
    } finally {
      setBusy(false);
    }
  }

  const stepState = (s: Step) => {
    const i = order.indexOf(s);
    const cur = order.indexOf(step);
    return i < cur ? "done" : i === cur ? "current" : "next";
  };

  return (
    <div className="setup">
      <header className="setup-head">
        <LockIcon size={24} color="var(--primary)" />
        <span className="brand-name">{he.appName}</span>
        {step !== "restore" && <ol className="steps" aria-label="שלבי ההגדרה">
          {order.map((s, i) => (
            <li key={s} className={`step step-${stepState(s)}`} aria-current={stepState(s) === "current" ? "step" : undefined}>
              <span className="step-dot">{stepState(s) === "done" ? "✓" : i + 1}</span>
              {s === "password" ? "סיסמה" : s === "recovery" ? "ערכת שחזור" : s === "google" ? "כניסה עם גוגל" : "הפרטים שלך"}
            </li>
          ))}
        </ol>}
      </header>

      <main className="setup-main">
        {step === "password" && (
          <form className="setup-col stack" onSubmit={create}>
            <h1>ברוכה הבאה לכספת האבחון</h1>
            <p className="lede">התיקים נשמרים מוצפנים במחשב הזה בלבד. בוחרים סיסמה: משפט קצר, 12 תווים לפחות.</p>
            {status.cloud_synced_folder && (
              <ErrorLine error={`תיקיית הכספת נמצאת בתוך תיקייה שמסונכרנת לענן (${status.cloud_synced_folder}). אי אפשר ליצור בה כספת. אור יכול לעזור להעביר אותה.`} report />
            )}
            <div className="field">
              <label htmlFor="pw1">סיסמה</label>
              <input id="pw1" className="input" type="password" autoComplete="new-password" value={pw} onChange={(e) => setPw(e.target.value)} autoFocus />
            </div>
            <div className="field">
              <label htmlFor="pw2">שוב, לאימות</label>
              <input id="pw2" className="input" type="password" autoComplete="new-password" value={pw2} onChange={(e) => setPw2(e.target.value)} />
            </div>
            <ul className="pw-checks" aria-label="דרישות הסיסמה">
              <li className={pw.length >= 12 ? "met" : undefined}>{pw.length >= 12 ? "✓" : "○"} 12 תווים לפחות{pw.length > 0 && pw.length < 12 ? ` (עוד ${12 - pw.length})` : ""}</li>
              <li className={pw2.length > 0 && pw === pw2 ? "met" : undefined}>{pw2.length > 0 && pw === pw2 ? "✓" : "○"} שתי הסיסמאות זהות</li>
            </ul>
            <ErrorLine error={error} />
            <button type="submit" className="btn btn-primary btn-big" disabled={busy}>{busy ? "יוצרת כספת מוצפנת…" : "יצירת הכספת"}</button>
            <p className="hint">
              {status.fips_active ? "ההצפנה נעשית ברכיב הצפנה מאושר ומבוקר (FIPS 140-3)." : "הכל נשמר מוצפן במחשב הזה."} אף אחד לא שומר את הסיסמה.
            </p>
            {onRestored && (
              <button type="button" className="link-btn restore-link" onClick={() => { setError(null); setStep("restore"); }}>
                מחשב חדש? יש לי גיבוי של הכספת
              </button>
            )}
          </form>
        )}

        {step === "restore" && onRestored && (
          <RestoreFromBackup onRestored={onRestored} onBack={() => setStep("password")} />
        )}

        {step === "recovery" && (
          <div className="setup-two">
            <div className="stack grow">
              <h1>ערכת השחזור שלך</h1>
              <p className="lede">הדרך היחידה לפתוח אם הסיסמה תישכח. מדפיסים ושומרים במקום בטוח, לא במחשב ולא בטלפון.</p>
              <RecoveryKitPaper recoveryKey={key} />
              <form className="stack" onSubmit={confirm}>
                <div className="field">
                  <label htmlFor="typed">כדי לוודא שהערכה נשמרה, הקלידי אותה מהדף (לא להעתיק מהמסך)</label>
                  <div className="row">
                    <input id="typed" className="input grow mono" dir="ltr" placeholder="XXXX-XXXX-…" autoComplete="off" spellCheck={false}
                      value={typed} onChange={(e) => setTyped(e.target.value)} aria-describedby="typed-count" />
                    <button type="submit" className="btn btn-primary" disabled={typedGroups < kitGroups}>המשך</button>
                  </div>
                  <span id="typed-count" className="hint">
                    {typedGroups < kitGroups
                      ? `הוקלדו ${typedGroups} מתוך ${kitGroups} קבוצות.`
                      : "כל הקבוצות הוקלדו."}
                  </span>
                </div>
                <ErrorLine error={error} />
              </form>
            </div>
            <aside className="why">
              <h2>למה זה חשוב</h2>
              <p>הכספת מוצפנת כך שרק מי שמחזיקה בסיסמה או בערכה יכולה לפתוח אותה. אין לנו עותק, וזו בדיוק ההגנה על המטופלים.</p>
              <ul>
                <li>לא מצלמים את הדף ולא שומרים אותו במחשב.</li>
                <li>מחשב חדש? מתקינים, פותחים עם הערכה, ובוחרים סיסמה חדשה.</li>
              </ul>
            </aside>
          </div>
        )}

        {step === "google" && (
          <div className="setup-col stack">
            <h1>להפעיל כניסה עם גוגל כששוכחים סיסמה</h1>
            <p className="lede">לא חובה. אם הסיסמה תישכח, נכנסים מהמחשב הזה עם חשבון הגוגל ובוחרים סיסמה חדשה. ערכת השחזור עדיין עובדת.</p>
            <div className="row">
              <button type="button" className="btn btn-primary btn-big" autoFocus onClick={() => setGoogleOpen(true)}>הפעלה…</button>
              <button type="button" className="btn btn-big" onClick={() => setStep("me")}>אחר כך</button>
            </div>
            <p className="hint">אפשר גם בכל זמן אחר, בהגדרות ← סיסמה ואבטחה.</p>
            {googleOpen && (
              <TurnOnDialog onClose={() => setGoogleOpen(false)} onTurnedOn={() => { setGoogleOn(true); setStep("me"); }} />
            )}
          </div>
        )}

        {step === "me" && (
          <form className="setup-col stack" onSubmit={finish}>
            {googleOn && <p className="setup-done" role="status">✓ הכניסה עם גוגל הופעלה.</p>}
            <h1>הפרטים שלך</h1>
            <p className="lede">השם שלך ושם הקליניקה יוחלפו תמיד ב"המאבחנת" לפני שליחה.</p>
            <div className="field">
              <label htmlFor="names">השם שלך, ושמות נוספים (קליניקה, כינוי), מופרדים בפסיק</label>
              <textarea id="names" className="textarea" rows={3} value={names} onChange={(e) => setNames(e.target.value)} />
            </div>
            <ErrorLine error={error} />
            <button type="submit" className="btn btn-primary btn-big" disabled={busy}>סיום וכניסה</button>
          </form>
        )}
      </main>
    </div>
  );
}
