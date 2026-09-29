import { useState, type FormEvent } from "react";
import { he } from "../i18n/he";
import { ipc, type AppStatus } from "../ipc/client";
import { ErrorLine, LockIcon } from "../components/ui";
import "./SetupScreen.css";

type Step = "password" | "recovery" | "me";

/** First run: a password, the printed recovery kit, and the practitioner's own names. */
export function SetupScreen({ status, onCreated, onDone }: { status: AppStatus; onCreated?: () => void; onDone: () => Promise<void> }) {
  const [step, setStep] = useState<Step>("password");
  const [pw, setPw] = useState("");
  const [pw2, setPw2] = useState("");
  const [key, setKey] = useState("");
  const [typed, setTyped] = useState("");
  const [names, setNames] = useState("");
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const groups = key.split("-").filter(Boolean);

  async function create(e: FormEvent) {
    e.preventDefault();
    setError(null);
    if (pw.length < 12) return setError("הסיסמה צריכה להיות באורך 12 תווים לפחות. מומלץ משפט של כמה מילים.");
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
    if (!ok) return setError("הערכה שהוקלדה לא תואמת. כדאי לבדוק את הדף ולנסות שוב.");
    setKey("");
    setStep("me");
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
    const order: Step[] = ["password", "recovery", "me"];
    const i = order.indexOf(s);
    const cur = order.indexOf(step);
    return i < cur ? "done" : i === cur ? "current" : "next";
  };

  return (
    <div className="setup">
      <header className="setup-head">
        <LockIcon size={24} color="var(--primary)" />
        <span className="brand-name">{he.appName}</span>
        <ol className="steps" aria-label="שלבי ההגדרה">
          {(["password", "recovery", "me"] as Step[]).map((s, i) => (
            <li key={s} className={`step step-${stepState(s)}`} aria-current={stepState(s) === "current" ? "step" : undefined}>
              <span className="step-dot">{stepState(s) === "done" ? "✓" : i + 1}</span>
              {s === "password" ? "סיסמה" : s === "recovery" ? "ערכת שחזור" : "הפרטים שלך"}
            </li>
          ))}
        </ol>
      </header>

      <main className="setup-main">
        {step === "password" && (
          <form className="setup-col stack" onSubmit={create}>
            <h1>ברוכה הבאה לכספת האבחון</h1>
            <p className="lede">כל התיקים יישמרו כאן, במחשב הזה בלבד, מוצפנים. בוחרים סיסמה לפתיחה: משפט קצר שקל לזכור, 12 תווים לפחות.</p>
            {status.cloud_synced_folder && (
              <p className="error">תיקיית הכספת נמצאת בתוך תיקייה שמסונכרנת לענן ({status.cloud_synced_folder}). אי אפשר ליצור בה כספת.</p>
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
              {status.fips_active ? "ההצפנה נעשית ברכיב הצפנה מאושר ומבוקר (FIPS 140-3)." : "הכל נשמר מוצפן במחשב הזה."} את הסיסמה אף אחד לא יודע ולא שומר, גם לא אנחנו.
            </p>
          </form>
        )}

        {step === "recovery" && (
          <div className="setup-two">
            <div className="stack grow">
              <h1>ערכת השחזור שלך</h1>
              <p className="lede">זו הדרך היחידה לפתוח את הכספת אם הסיסמה תישכח. מדפיסים או מעתיקים לדף, ושומרים אותו במקום בטוח, לא במחשב ולא בטלפון.</p>
              <div className="paper kit">
                <div className="kit-head"><span>ערכת שחזור · כספת האבחון</span><span>{new Date().toLocaleDateString("he-IL")}</span></div>
                <div className="kit-grid" dir="ltr">
                  {groups.map((g, i) => (
                    <span key={i} className={i === groups.length - 1 ? "kit-check" : undefined}>{g}</span>
                  ))}
                </div>
                <p className="hint">אין הבדל בין אותיות גדולות וקטנות. הקבוצה האחרונה בודקת טעויות הקלדה.</p>
                <div className="row">
                  <button type="button" className="btn btn-primary" onClick={() => window.print()}>הדפסה</button>
                </div>
              </div>
              <form className="stack" onSubmit={confirm}>
                <div className="field">
                  <label htmlFor="typed">כדי לוודא שהערכה נשמרה, הקלידי אותה מהדף</label>
                  <div className="row">
                    <input id="typed" className="input grow mono" dir="ltr" placeholder="XXXX-XXXX-…" value={typed} onChange={(e) => setTyped(e.target.value)} />
                    <button type="submit" className="btn btn-primary" disabled={typed.trim().length < 20}>המשך</button>
                  </div>
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

        {step === "me" && (
          <form className="setup-col stack" onSubmit={finish}>
            <h1>הפרטים שלך</h1>
            <p className="lede">השם שלך ושם הקליניקה יוסתרו תמיד לפני שליחה ל-Claude, בכל התיקים, ויוחלפו ב"המאבחנת".</p>
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
