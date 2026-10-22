import { PREVIEW } from "../web/mode";
import { useEffect, useState, type FormEvent } from "react";
import { he } from "../i18n/he";
import { ipc, type AppStatus } from "../ipc/client";
import { CoreStatus } from "../components/CoreStatus";
import { ErrorLine, LockIcon } from "../components/ui";
import { GoogleRecoverForm } from "../components/GoogleRecovery";
import { LockScreenUpdate } from "../components/Update";
import { timed, WaitLine } from "../components/Progress";
import "./LockScreen.css";

export function LockScreen({ status, onUnlocked }: { status?: AppStatus; onUnlocked?: (s: AppStatus) => void }) {
  const [mode, setMode] = useState<"password" | "recovery" | "google" | "forgot">("password");
  // Forgot the password: Google first when it is on for this computer (D-041). When it is off,
  // a short note says to use the kit now, and where Google is turned on for next time (D-096).
  const [google, setGoogle] = useState<{ on: boolean; available: boolean }>({ on: false, available: false });
  useEffect(() => {
    ipc.googleStatus()
      .then((g) => setGoogle({ on: g?.on === true, available: g?.available === true }))
      .catch(() => setGoogle({ on: false, available: false }));
  }, []);
  const [secret, setSecret] = useState("");
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const show = (m: typeof mode) => { setMode(m); setSecret(""); setError(null); };
  // The vault did not open for a reason that is not the password (R7): often a new version.
  const [broken, setBroken] = useState(false);
  const [version, setVersion] = useState<string | null>(null);
  useEffect(() => {
    ipc.ping().then((p) => setVersion(p.core_version)).catch(() => setVersion(null));
  }, []);

  // Too many wrong tries: the core says how long to wait. The wait counts down here and the
  // button comes back by itself, instead of a fixed number that is already wrong a second later.
  const [waitUntil, setWaitUntil] = useState<number | null>(null);
  const [now, setNow] = useState(() => Date.now());
  useEffect(() => {
    if (waitUntil === null) return;
    const t = window.setInterval(() => {
      const n = Date.now();
      setNow(n);
      if (n >= waitUntil) setWaitUntil(null);
    }, 250);
    return () => window.clearInterval(t);
  }, [waitUntil]);
  const waitSeconds = waitUntil === null ? 0 : Math.max(1, Math.ceil((waitUntil - now) / 1000));

  // An opening still going after a few seconds is most likely waiting for Windows to unseal
  // the half of the password kept on this computer (a cold start): it says so.
  const [slow, setSlow] = useState(false);
  useEffect(() => {
    if (!busy) return;
    const t = window.setTimeout(() => setSlow(true), 5000);
    return () => {
      window.clearTimeout(t);
      setSlow(false);
    };
  }, [busy]);

  async function submit(e: FormEvent) {
    e.preventDefault();
    if (!secret.trim()) return;
    setBusy(true);
    setError(null);
    setBroken(false);
    try {
      const s = await timed("unlock", mode === "password" ? ipc.unlock(secret) : ipc.unlockWithRecovery(secret), mode);
      setSecret("");
      onUnlocked?.(s);
    } catch (err) {
      const e = err as { code?: string; message?: string };
      const seconds = e.code === "backoff" ? Number(/\d+/.exec(e.message ?? "")?.[0]) : NaN;
      if (seconds > 0) {
        setNow(Date.now());
        setWaitUntil(Date.now() + seconds * 1000);
      } else if (e.code === "other_computer" && mode === "password") {
        // Not a forgotten password: here only the kit opens it. The form turns to the kit,
        // and the message says why (review of stage 5, U3).
        setMode("recovery");
        setSecret("");
        setError(e.message ?? "במחשב הזה נכנסים עם ערכת השחזור.");
      } else {
        setError(e.message ?? "הפתיחה נכשלה.");
        setBroken(e.code === "vault" || e.code === "internal");
      }
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
        {mode === "google" ? (
          <>
            <GoogleRecoverForm onUnlocked={onUnlocked} onBack={() => show("password")} />
            <button type="button" className="link-btn" onClick={() => show("recovery")}>
              יש לי את ערכת השחזור המודפסת
            </button>
          </>
        ) : mode === "forgot" ? (
          <ForgotPassword googleAvailable={google.available} onKit={() => show("recovery")} onBack={() => show("password")} />
        ) : (
        <form className="lock-form" onSubmit={submit}>
          <div className="field">
            <label htmlFor="pw">{mode === "password" ? he.lock.password : "ערכת השחזור"}</label>
            <input id="pw" className="input lock-input" type={mode === "password" ? "password" : "text"}
              dir={mode === "recovery" ? "ltr" : undefined} autoComplete="current-password" autoFocus
              placeholder={mode === "recovery" ? "7K3M-Q9WD-…" : undefined}
              value={secret} onChange={(e) => setSecret(e.target.value)} />
          </div>
          {waitUntil !== null ? (
            <p className="lock-wait" role="status">ניסיונות שגויים. אפשר לנסות שוב בעוד {waitSeconds} {waitSeconds === 1 ? "שנייה" : "שניות"}.</p>
          ) : (
            <ErrorLine error={error} />
          )}
          {broken && <VersionTrouble version={version} />}
          {busy && <WaitLine key={mode} task="unlock" speed={mode} delay={1000} label="פותחת את הכספת המוצפנת" />}
          {busy && slow && mode === "password" && <p className="hint" role="status">ממתינה למחשב… זה קורה לפעמים מיד אחרי הדלקת המחשב.</p>}
          <button type="submit" className="btn btn-primary" disabled={busy || waitUntil !== null}>{busy ? "פותחת…" : he.lock.open}</button>
          <button type="button" className="link-btn" onClick={() => show(mode === "password" ? (google.on ? "google" : "forgot") : "password")}>
            {mode === "password" ? "שכחתי את הסיסמה" : "חזרה לפתיחה עם סיסמה"}
          </button>
        </form>
        )}
        <div className="lock-foot">
          <p className="lock-safe"><LockIcon size={14} color="currentColor" />{he.lock.footer}</p>
          <CoreStatus />
          {!PREVIEW && <LockScreenUpdate />}
        </div>
      </section>
      <span className="lock-version">{version ? `v${version}` : ""}{status?.fips_active ? " · FIPS 140-3" : ""}</span>
    </main>
  );
}

/**
 * Forgot the password, Google sign-in not on: two lines. The printed kit opens it now; Google
 * is turned on in the settings, once inside, for next time (D-096).
 */
function ForgotPassword({ googleAvailable, onKit, onBack }: { googleAvailable: boolean; onKit: () => void; onBack: () => void }) {
  return (
    <div className="lock-form">
      <div className="lock-note" role="note">
        <p><strong>עכשיו:</strong> פותחים עם ערכת השחזור המודפסת, ובוחרים סיסמה חדשה בפנים.</p>
        {googleAvailable && (
          <p><strong>לפעם הבאה:</strong> אחרי הכניסה מפעילים כניסה עם גוגל, בהגדרות ← סיסמה ואבטחה.</p>
        )}
      </div>
      <button type="button" className="btn btn-primary" autoFocus onClick={onKit}>פתיחה עם ערכת השחזור</button>
      <button type="button" className="link-btn" onClick={onBack}>חזרה לפתיחה עם סיסמה</button>
    </div>
  );
}

/**
 * R7: a version that cannot open the vault never leaves her stuck. The vault stays on the
 * computer untouched by a reinstall, and the version before can be installed over this one.
 */
function VersionTrouble({ version }: { version: string | null }) {
  const [problem, setProblem] = useState<string | null>(null);
  return (
    <div className="lock-note" role="note">
      <p><strong>הכספת לא נפתחה בגרסה הזו{version ? ` (${version})` : ""}.</strong></p>
      <p>הכספת נשארת במחשב ולא נמחקת. אם זה התחיל אחרי עדכון, מתקינים את הגרסה הקודמת ומספרים לאור.</p>
      <button type="button" className="btn btn-small" onClick={() => {
        setProblem(null);
        ipc.openReleasesPage().catch((e: { message?: string }) => setProblem(e.message ?? "הדף לא נפתח."));
      }}>לדף הגרסאות (נפתח בדפדפן)</button>
      <ErrorLine error={problem} />
    </div>
  );
}
