import { useEffect, useState, type FormEvent } from "react";
import { he } from "../i18n/he";
import { ipc, type AppStatus } from "../ipc/client";
import { ErrorLine, LockIcon } from "../components/ui";
import { RestoreFromBackup } from "../components/Backup";
import { TurnOnDialog } from "../components/GoogleRecovery";
import { timed, WaitLine } from "../components/Progress";
import "./SetupScreen.css";

type Step = "password" | "google" | "backup" | "me" | "restore";

/** First run: a password, Google for a forgotten password (where this version has it), a first
 *  backup to a removable drive, and the practitioner's own names. No printed kit (D-103): the vault still gets a kit slot, but its key
 *  is never shown; a kit can be made later under "מתקדם" in the settings. */
export function SetupScreen({ status, onCreated, onDone, onRestored }: { status: AppStatus; onCreated?: () => void; onDone: () => Promise<void>; onRestored?: (s: AppStatus) => void }) {
  const [step, setStep] = useState<Step>("password");
  const [pw, setPw] = useState("");
  const [pw2, setPw2] = useState("");
  const [names, setNames] = useState("");
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  // Sign in with Google when the password is forgotten (D-041): offered once, right after the
  // password, while the vault is open; skipped where this version cannot do it (D-096, D-103).
  const [googleAvailable, setGoogleAvailable] = useState(false);
  // The steps are shown once it is known whether Google is one of them: no 3 turning into 4.
  const [googleKnown, setGoogleKnown] = useState(false);
  const [googleOpen, setGoogleOpen] = useState(false);
  const [googleOn, setGoogleOn] = useState(false);
  useEffect(() => {
    ipc.googleStatus()
      .then((g) => setGoogleAvailable(g?.available === true && !g.on))
      .catch(() => setGoogleAvailable(false))
      .finally(() => setGoogleKnown(true));
  }, []);
    // Without the printed kit, Google and the backup are the ways back in (D-103): both offered,
  // both skippable, each with its cost said in one line.
  const order: Step[] = googleAvailable ? ["password", "google", "backup", "me"] : ["password", "backup", "me"];
  const [backupDone, setBackupDone] = useState(false);
  async function firstBackup() {
    setError(null);
    setBusy(true);
    try {
      // The system's "Save as" window; cancelled = stay on this step.
      const done = await ipc.writeBackup();
      if (done) {
        setBackupDone(true);
        setStep("me");
      }
    } catch (err) {
      setError((err as { message: string }).message);
    } finally {
      setBusy(false);
    }
  }

  async function create(e: FormEvent) {
    e.preventDefault();
    setError(null);
    if (pw.length < 12) return setError("לפחות 12 תווים. מומלץ משפט קצר.");
    if (pw !== pw2) return setError("שתי הסיסמאות לא זהות.");
    setBusy(true);
    try {
      // The kit's key comes back too; it is not kept or shown (D-103).
      await timed("create", ipc.createVault(pw));
      setPw("");
      setPw2("");
      setStep(googleAvailable ? "google" : "backup");
      onCreated?.();
    } catch (err) {
      setError((err as { message: string }).message);
    } finally {
      setBusy(false);
    }
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
        {step !== "restore" && googleKnown && <ol className="steps" aria-label="שלבי ההגדרה">
          {order.map((s, i) => (
            <li key={s} className={`step step-${stepState(s)}`} aria-current={stepState(s) === "current" ? "step" : undefined}>
              <span className="step-dot">{stepState(s) === "done" ? "✓" : i + 1}</span>
              {s === "password" ? "סיסמה" : s === "google" ? "כניסה עם גוגל" : s === "backup" ? "גיבוי" : "הפרטים שלך"}
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
            {busy && <WaitLine task="create" label="יוצרת את הכספת ומצפינה אותה" />}
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

        {step === "google" && (
          <div className="setup-col stack">
            <h1>להפעיל כניסה עם גוגל כששוכחים סיסמה</h1>
            <p className="lede">מומלץ מאוד. אם הסיסמה תישכח, נכנסים מהמחשב הזה עם חשבון הגוגל ובוחרים סיסמה חדשה. בלי זה, אין דרך להיכנס אם הסיסמה נשכחה.</p>
            <div className="row">
              <button type="button" className="btn btn-primary btn-big" autoFocus onClick={() => setGoogleOpen(true)}>הפעלה…</button>
              <button type="button" className="btn btn-big" onClick={() => setStep("backup")}>אחר כך</button>
            </div>
            <p className="hint">אפשר גם בכל זמן אחר, בהגדרות ← סיסמה ואבטחה.</p>
            {googleOpen && (
              <TurnOnDialog onClose={() => setGoogleOpen(false)} onTurnedOn={() => { setGoogleOn(true); setStep("backup"); }} />
            )}
          </div>
        )}

        {step === "backup" && (
          <div className="setup-col stack">
            {googleOn && <p className="setup-done" role="status">✓ הכניסה עם גוגל הופעלה.</p>}
            <h1>גיבוי לדיסק נייד</h1>
            <p className="lede">אם המחשב יתקלקל או יאבד, הגיבוי הוא מה שפותח את התיקים במחשב אחר, עם הסיסמה. מחברים דיסק נייד ובוחרים בו מקום. מכאן הגיבוי נעשה לבד בכל פעם שהדיסק מחובר.</p>
            <div className="row">
              <button type="button" className="btn btn-primary btn-big" autoFocus disabled={busy} onClick={() => void firstBackup()}>{busy ? "שומרת גיבוי מוצפן…" : "בחירת מקום וגיבוי ראשון…"}</button>
              <button type="button" className="btn btn-big" disabled={busy} onClick={() => { setError(null); setStep("me"); }}>אחר כך</button>
            </div>
            <ErrorLine error={error} />
            <p className="hint">בלי גיבוי, מחשב שהתקלקל = אין דרך לפתוח את התיקים. אפשר גם בכל זמן אחר, בהגדרות ← גיבוי.</p>
          </div>
        )}

        {step === "me" && (
          <form className="setup-col stack" onSubmit={finish}>
            {backupDone && <p className="setup-done" role="status">✓ הגיבוי הראשון נשמר.</p>}
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
