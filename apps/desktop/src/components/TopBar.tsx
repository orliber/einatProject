import { useApp, type Route } from "../App";
import { he } from "../i18n/he";
import { LockIcon } from "./ui";
import "./TopBar.css";

/** The dark bar of the screens outside a case: cases, consultation, settings. */
export function TopBar({ active }: { active: Route["name"] }) {
  const { go, lockNow, status } = useApp();
  const tab = (name: Route["name"], label: string, route: Route) => (
    <button type="button" className={active === name ? "topbar-link topbar-active" : "topbar-link"}
      aria-current={active === name ? "page" : undefined} onClick={() => go(route)}>{label}</button>
  );
  return (
    <header className="topbar">
      <LockIcon size={24} color="#9cc9c3" />
      <span className="topbar-brand">{he.appName}</span>
      <span className="topbar-note">{he.encrypted}</span>
      {status.demo_mode && <span className="topbar-demo" title="לא הוגדר מפתח API. התשובות הן דוגמאות מקומיות, ושום דבר לא נשלח.">מצב הדגמה</span>}
      <nav className="topbar-nav" aria-label="ניווט ראשי">
        {tab("cases", "תיקים", { name: "cases" })}
        {tab("consult", "התייעצות", { name: "consult" })}
        {tab("settings", "הגדרות", { name: "settings" })}
        <button type="button" className="topbar-link topbar-lock" onClick={() => void lockNow()}>
          <LockIcon size={16} /> נעילה
        </button>
      </nav>
    </header>
  );
}
