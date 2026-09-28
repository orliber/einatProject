import { useState, type FormEvent } from "react";
import { he } from "../i18n/he";
import { ipc, type AppStatus } from "../ipc/client";
import { CoreStatus } from "../components/CoreStatus";
import { ErrorLine, LockIcon } from "../components/ui";
import "./LockScreen.css";

export function LockScreen({ status, onUnlocked }: { status?: AppStatus; onUnlocked?: (s: AppStatus) => void }) {
  const [mode, setMode] = useState<"password" | "recovery">("password");
  const [secret, setSecret] = useState("");
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);

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
          <p>{he.lock.lockedAfterIdle(status?.lock_minutes ?? 10)}</p>
        </div>
        <form className="lock-form" onSubmit={submit}>
          <div className="field">
            <label htmlFor="pw">{mode === "password" ? he.lock.password : "ערכת השחזור"}</label>
            <input id="pw" className="input lock-input" type={mode === "password" ? "password" : "text"}
              dir={mode === "recovery" ? "ltr" : undefined} autoComplete="current-password" autoFocus
              placeholder={mode === "recovery" ? "7K3M-Q9WD-…" : undefined}
              value={secret} onChange={(e) => setSecret(e.target.value)} />
          </div>
          <ErrorLine error={error} />
          <button type="submit" className="btn btn-primary" disabled={busy}>{busy ? "פותחת…" : he.lock.open}</button>
          <button type="button" className="link-btn" onClick={() => { setMode(mode === "password" ? "recovery" : "password"); setSecret(""); setError(null); }}>
            {mode === "password" ? "שכחתי את הסיסמה · פתיחה עם ערכת השחזור" : "חזרה לפתיחה עם סיסמה"}
          </button>
        </form>
        <div className="lock-foot">
          <p className="hint">{he.lock.footer}</p>
          <CoreStatus />
        </div>
      </section>
      <span className="lock-version">v0.1{status?.fips_active ? " · FIPS 140-3" : ""}</span>
    </main>
  );
}
