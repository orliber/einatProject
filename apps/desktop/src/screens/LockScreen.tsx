import { he } from "../i18n/he";
import { CoreStatus } from "../components/CoreStatus";
import "./LockScreen.css";

export function LockScreen() {
  return (
    <main className="lock-backdrop">
      <section className="lock-card" aria-labelledby="lock-title">
        <div className="lock-icon" aria-hidden="true">
          <svg width="26" height="26" viewBox="0 0 24 24" fill="none" stroke="currentColor"
            strokeWidth="2" strokeLinecap="round" strokeLinejoin="round">
            <rect x="5" y="11" width="14" height="10" rx="2" />
            <path d="M8 11V7a4 4 0 0 1 8 0v4" />
          </svg>
        </div>
        <div className="lock-heading">
          <h1 id="lock-title">{he.appName}</h1>
          <p>{he.lock.lockedAfterIdle}</p>
        </div>
        <form className="lock-form" onSubmit={(e) => e.preventDefault()}>
          <div className="field">
            <label htmlFor="pw">{he.lock.password}</label>
            <input id="pw" className="input" type="password" autoComplete="current-password" />
          </div>
          <button type="submit" className="btn btn-primary">{he.lock.open}</button>
        </form>
        <p className="hint">{he.lock.footer}</p>
        <CoreStatus />
      </section>
    </main>
  );
}
