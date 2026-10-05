import { PREVIEW } from "../web/mode";
import { useEffect, useState, type FormEvent } from "react";
import { he } from "../i18n/he";
import { ipc, type AppStatus } from "../ipc/client";
import { CoreStatus } from "../components/CoreStatus";
import { ErrorLine, LockIcon } from "../components/ui";
import { GoogleRecoverForm } from "../components/GoogleRecovery";
import { LockScreenUpdate } from "../components/Update";
import "./LockScreen.css";

export function LockScreen({ status, onUnlocked }: { status?: AppStatus; onUnlocked?: (s: AppStatus) => void }) {
  const [mode, setMode] = useState<"password" | "recovery" | "google">("password");
  // Forgot the password: Google first when it is on for this computer (D-041).
  const [googleOn, setGoogleOn] = useState(false);
  useEffect(() => {
    ipc.googleStatus().then((g) => setGoogleOn(g.on)).catch(() => setGoogleOn(false));
  }, []);
  const [secret, setSecret] = useState("");
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [version, setVersion] = useState<string | null>(null);
  useEffect(() => {
    ipc.ping().then((p) => setVersion(p.core_version)).catch(() => setVersion(null));
  }, []);

  const offerHello = !!status?.hello_on && !!status?.hello_available && mode === "password";

  async function hello() {
    setBusy(true);
    setError(null);
    try {
      onUnlocked?.(await ipc.unlockWithHello());
    } catch {
      setError("Windows Hello לא פתח את הכספת. אפשר לנסות שוב, או להקליד את הסיסמה.");
    } finally {
      setBusy(false);
    }
  }

  async function submit(e: FormEvent) {
    e.preventDefault();
    if (!secret.trim()) return;
    setBusy(true);
    setError(null);
    try {
      const s = mode === "password" ? await ipc.unlock(secret) : await ipc.unlockWithRecovery(secret);
      setSecret("");
      onUnlocked?.(s);
    } catch (err) {
      setError((err as { message?: string }).message ?? "הפתיחה נכשלה.");
    } finally {
      setBusy(false);
    }
  }

  return (
    <main className="lock-backdrop">
      <section className="lock-card" aria-labelledby="lock-title">
        <div className="lock-icon"><LockIcon size={28} color="#fff" /></div>
        <div className="lock-heading">
          <h1 id="lock-title">{he.appName}</h1>
          {PREVIEW ? (
            <p className="preview-hint" role="note">זו הדמיה בדפדפן, עם תיקים בדויים: מקלידים כל סיסמה ולוחצים פתיחה.</p>
          ) : (
            <p>{he.lock.lockedAfterIdle(status?.lock_minutes ?? 10)}</p>
          )}
        </div>
        {offerHello && (
          <button type="button" className="btn btn-primary lock-hello" disabled={busy} onClick={() => void hello()}>
            {busy ? "פותחת…" : "פתיחה עם Windows Hello"}
          </button>
        )}
        {mode === "google" ? (
          <>
            <GoogleRecoverForm onUnlocked={onUnlocked} onBack={() => setMode("password")} />
            <button type="button" className="link-btn" onClick={() => { setMode("recovery"); setSecret(""); setError(null); }}>
              יש לי את ערכת השחזור המודפסת
            </button>
          </>
        ) : (
        <form className="lock-form" onSubmit={submit}>
          <div className="field">
            <label htmlFor="pw">{mode === "password" ? he.lock.password : "ערכת השחזור"}</label>
            <input id="pw" className="input lock-input" type={mode === "password" ? "password" : "text"}
              dir={mode === "recovery" ? "ltr" : undefined} autoComplete="current-password" autoFocus={!offerHello}
              placeholder={mode === "recovery" ? "7K3M-Q9WD-…" : undefined}
              value={secret} onChange={(e) => setSecret(e.target.value)} />
          </div>
          <ErrorLine error={error} />
          <button type="submit" className={offerHello ? "btn" : "btn btn-primary"} disabled={busy}>{busy ? "פותחת…" : he.lock.open}</button>
          <button type="button" className="link-btn" onClick={() => { setMode(mode === "password" ? (googleOn ? "google" : "recovery") : "password"); setSecret(""); setError(null); }}>
            {mode === "password" ? (googleOn ? "שכחתי את הסיסמה · כניסה עם גוגל" : "שכחתי את הסיסמה · פתיחה עם ערכת השחזור") : "חזרה לפתיחה עם סיסמה"}
          </button>
        </form>
        )}
        <div className="lock-foot">
          <p className="hint">{he.lock.footer}</p>
          <CoreStatus />
          {!PREVIEW && <LockScreenUpdate />}
        </div>
      </section>
      <span className="lock-version">{version ? `v${version}` : ""}{status?.fips_active ? " · FIPS 140-3" : ""}</span>
    </main>
  );
}
