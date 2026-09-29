import { useApp, type Route } from "../App";
import { he } from "../i18n/he";
import { LockIcon } from "./ui";
import "./TopBar.css";

/** The bar of the screens outside a case: cases, consultation, settings. */
export function TopBar({ active }: { active: Route["name"] }) {
  const { go, lockNow, status } = useApp();
  const tab = (name: Route["name"], label: string, route: Route) => (
    <button type="button" className={active === name ? "topbar-link topbar-active" : "topbar-link"}
      aria-current={active === name ? "page" : undefined} onClick={() => go(route)}>{label}</button>
  );
  return (
    <header className="topbar">
      <span className="topbar-brand"><LockIcon size={22} color="var(--primary)" /> {he.appName}</span>
      <nav className="topbar-nav" aria-label="ניווט ראשי">
        {tab("cases", "תיקים", { name: "cases" })}
        {tab("consult", "התייעצות", { name: "consult" })}
        {tab("settings", "הגדרות", { name: "settings" })}
      </nav>
      <span className="grow" />
      {status.demo_mode && <span className="topbar-demo" title="לא הוגדר מפתח API. התשובות הן דוגמאות מקומיות, ושום דבר לא נשלח.">מצב הדגמה</span>}
      <span className="topbar-safe"><span className="topbar-dot" aria-hidden="true" />{he.encrypted}</span>
      <button type="button" className="btn btn-small" title="נעילה (Ctrl+L)" onClick={() => void lockNow()}>
        <LockIcon size={15} /> נעילה
      </button>
    </header>
  );
}
