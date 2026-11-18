import { useRef, type KeyboardEvent, type ReactNode } from "react";
import { useApp, type Route } from "../App";
import { AREA_LABEL, areaOf, homeOf, REPORT_PAGES, type Area } from "../areas";
import { setDiscreet, useDiscreet } from "../discreet";
import { he } from "../i18n/he";
import { CLINIC_TABS, type ClinicTab } from "../screens/clinic/tabs";
import { MOD } from "../shortcuts";
import { HelpIcon, LockIcon } from "./ui";
import { StatusChip } from "./Update";
import "./TopBar.css";

function Svg({ children, size = 18 }: { children: ReactNode; size?: number }) {
  return (
    <svg width={size} height={size} viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.9"
      strokeLinecap="round" strokeLinejoin="round" aria-hidden="true">{children}</svg>
  );
}
const ReportIcon = () => <Svg><path d="M14 3H7a2 2 0 0 0-2 2v14a2 2 0 0 0 2 2h10a2 2 0 0 0 2-2V8z" /><path d="M14 3v5h5M9 13h6M9 17h4" /></Svg>;
const ClinicIcon = () => <Svg><rect x="3" y="5" width="18" height="16" rx="2" /><path d="M3 10h18M8 3v4M16 3v4" /></Svg>;
const GearIcon = () => (
  <Svg size={20}>
    <circle cx="12" cy="12" r="3" />
    <path d="M19.4 15a1.7 1.7 0 0 0 .3 1.8l.1.1a2 2 0 1 1-2.8 2.8l-.1-.1a1.7 1.7 0 0 0-1.8-.3 1.7 1.7 0 0 0-1 1.5V21a2 2 0 1 1-4 0v-.1a1.7 1.7 0 0 0-1.1-1.5 1.7 1.7 0 0 0-1.8.3l-.1.1a2 2 0 1 1-2.8-2.8l.1-.1a1.7 1.7 0 0 0 .3-1.8 1.7 1.7 0 0 0-1.5-1H3a2 2 0 1 1 0-4h.1a1.7 1.7 0 0 0 1.5-1.1 1.7 1.7 0 0 0-.3-1.8l-.1-.1a2 2 0 1 1 2.8-2.8l.1.1a1.7 1.7 0 0 0 1.8.3H9a1.7 1.7 0 0 0 1-1.5V3a2 2 0 1 1 4 0v.1a1.7 1.7 0 0 0 1 1.5 1.7 1.7 0 0 0 1.8-.3l.1-.1a2 2 0 1 1 2.8 2.8l-.1.1a1.7 1.7 0 0 0-.3 1.8V9a1.7 1.7 0 0 0 1.5 1H21a2 2 0 1 1 0 4h-.1a1.7 1.7 0 0 0-1.5 1z" />
  </Svg>
);
const EyeIcon = ({ shut }: { shut: boolean }) => (
  <Svg size={20}>
    <path d="M2 12s3.6-7 10-7 10 7 10 7-3.6 7-10 7S2 12 2 12z" />
    <circle cx="12" cy="12" r="3" />
    {shut && <path d="M3 3l18 18" />}
  </Svg>
);

/**
 * The bar above every page outside a case (D-191): the program's two parts as one switch
 * ("כתיבת הדוחות", "ניהול הקליניקה"), one quiet status badge (demo mode, a new version, or
 * both: D-204), "מוצפן", the tools of the whole program in one family of 44px buttons (the
 * discreet eye, the keyboard shortcuts, the settings, the lock), and under it the row of the
 * pages of the part she is in.
 */
export function TopBar({ active }: { active: Route["name"] }) {
  const { go, lockNow, status, showShortcuts, route } = useApp();
  const discreet = useDiscreet();
  const area = areaOf(active);
  const part = (a: Area, icon: ReactNode, extra?: ReactNode) => (
    <button type="button" className={area === a ? "topbar-area topbar-area-on" : "topbar-area"}
      aria-current={area === a ? "true" : undefined} title={`${AREA_LABEL[a]} (${MOD}+${a === "reports" ? 1 : 2})`} onClick={() => go(homeOf(a))}>
      {icon}<span>{AREA_LABEL[a]}</span>{extra}
    </button>
  );
  return (
    <>
      <header className="topbar">
        <span className="topbar-brand"><LockIcon size={22} color="var(--primary)" /> <span className="topbar-brand-name">{he.appName}</span></span>
        <nav className="topbar-areas" aria-label="ניווט ראשי">
          {part("reports", <ReportIcon />)}
          {part("clinic", <ClinicIcon />, <span className="topbar-beta">בטא</span>)}
        </nav>
        <span className="grow" />
        <StatusChip demo={status.demo_mode} />
        <span className="topbar-safe"><span className="topbar-dot" aria-hidden="true" />{he.encrypted}</span>
        <div className="topbar-tools">
          {/* The eye alone (D-204): its colour, its title and the initials on the page tell the state. */}
          <button type="button" role="switch" aria-checked={discreet} aria-label="מצב דיסקרטי"
            title={discreet ? `מצב דיסקרטי פעיל: שמות מוצגים בראשי תיבות. לחיצה מחזירה את השמות (${MOD}+Shift+D).` : `מצב דיסקרטי: שמות מוצגים בראשי תיבות, למקרה שמישהו בחדר רואה את המסך (${MOD}+Shift+D).`}
            className={discreet ? "btn icon-btn topbar-discreet-on" : "btn icon-btn"} onClick={() => setDiscreet(!discreet)}>
            <EyeIcon shut={discreet} />
          </button>
          {showShortcuts && (
            <button type="button" className="btn icon-btn" aria-label="קיצורי מקלדת (F1)" title="קיצורי מקלדת (F1)" onClick={showShortcuts}>
              <HelpIcon size={20} />
            </button>
          )}
          <button type="button" className={active === "settings" ? "btn icon-btn topbar-settings-on" : "btn icon-btn"}
            aria-label="הגדרות" title="הגדרות" aria-current={active === "settings" ? "page" : undefined} onClick={() => go({ name: "settings" })}>
            <GearIcon />
          </button>
          <button type="button" className="btn topbar-lock" title={`נעילה (${MOD}+L)`} onClick={() => void lockNow()}>
            <LockIcon size={16} /> נעילה
          </button>
        </div>
      </header>
      {area === "reports" && <ReportPages active={active} />}
      {area === "clinic" && <ClinicPages route={route} />}
    </>
  );
}

/** The row of "כתיבת הדוחות": cases, consultation, writing style. */
function ReportPages({ active }: { active: Route["name"] }) {
  const { go } = useApp();
  return (
    <nav className="subbar" aria-label={AREA_LABEL.reports}>
      {REPORT_PAGES.map((p) => (
        <button key={p.name} type="button" className={active === p.name ? "subbar-link subbar-on" : "subbar-link"}
          aria-current={active === p.name ? "page" : undefined} onClick={() => go({ name: p.name })}>{p.label}</button>
      ))}
    </nav>
  );
}

/** The row of the clinic: its tabs (a tab list, with arrow keys, as the clinic had before). */
function ClinicPages({ route }: { route: Route | undefined }) {
  const { go } = useApp();
  const tabs = useRef(new Map<ClinicTab, HTMLButtonElement>());
  const clinic = route?.name === "clinic" ? route : undefined;
  // A patient's card is a page of the patients tab.
  const active: ClinicTab = clinic?.patientId !== undefined ? "patients" : (clinic?.tab ?? "today");

  /** Arrow keys walk the tabs (right to left, as they are laid out); Home and End jump to the ends. */
  function onKey(e: KeyboardEvent<HTMLDivElement>) {
    const at = CLINIC_TABS.findIndex((t) => t.id === active);
    const last = CLINIC_TABS.length - 1;
    const to = e.key === "ArrowLeft" ? (at === last ? 0 : at + 1)
      : e.key === "ArrowRight" ? (at === 0 ? last : at - 1)
      : e.key === "Home" ? 0
      : e.key === "End" ? last
      : null;
    const next = to === null ? undefined : CLINIC_TABS[to];
    if (!next) return;
    e.preventDefault();
    go({ name: "clinic", tab: next.id });
    tabs.current.get(next.id)?.focus();
  }

  return (
    <div className="subbar" role="tablist" aria-label="חלקי הקליניקה" onKeyDown={onKey}>
      {CLINIC_TABS.map((t) => (
        <button key={t.id} type="button" role="tab" id={`clinic-tab-${t.id}`} aria-selected={t.id === active}
          aria-controls={t.id === active ? `clinic-panel-${t.id}` : undefined}
          tabIndex={t.id === active ? 0 : -1} className={t.id === active ? "subbar-link subbar-on" : "subbar-link"}
          ref={(el) => { if (el) tabs.current.set(t.id, el); else tabs.current.delete(t.id); }}
          onClick={() => go({ name: "clinic", tab: t.id })}>
          {t.label}
        </button>
      ))}
    </div>
  );
}
