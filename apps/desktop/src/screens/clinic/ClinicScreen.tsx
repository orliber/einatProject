// The clinic's page (beta, D-170): the bar, a quiet line saying it is a beta and that nothing
// leaves the computer, discreet mode, and the tabs. The tab and the open patient live in the
// app's route, so a link from one tab to another (or to a patient's card) is one `go`.
import { useMemo, useRef, useState, type KeyboardEvent } from "react";
import { useApp } from "../../App";
import { Switch } from "../../components/Switch";
import { TopBar } from "../../components/TopBar";
import { CalendarTab } from "./CalendarTab";
import { ClinicSettingsTab } from "./ClinicSettingsTab";
import { MoneyTab } from "./MoneyTab";
import { ClinicNavContext, DiscreetContext, type ClinicNav } from "./parts";
import { PatientCardView } from "./PatientCardView";
import { PatientsTab } from "./PatientsTab";
import { CLINIC_TABS, type ClinicTab } from "./tabs";
import { TasksTab } from "./TasksTab";
import { TodayTab } from "./TodayTab";
import { WaitingTab } from "./WaitingTab";
import "./ClinicScreen.css";

// Discreet mode is remembered while the program is open, and nowhere else: in memory, never
// in storage. Locking the vault reloads the page, so it starts off again.
let discreetNow = false;

/** For the tests: each starts with discreet mode off. */
export function forgetDiscreet() {
  discreetNow = false;
}

export function ClinicScreen({ tab = "today", patientId }: { tab?: ClinicTab | undefined; patientId?: string | undefined }) {
  const { go } = useApp();
  const [discreet, setDiscreet] = useState(discreetNow);
  const tabs = useRef(new Map<ClinicTab, HTMLButtonElement>());
  const nav = useMemo<ClinicNav>(() => ({
    openPatient: (id) => go({ name: "clinic", tab: "patients", patientId: id }),
    openTab: (next) => go({ name: "clinic", tab: next }),
  }), [go]);
  // A patient's card is a page of the patients tab.
  const active: ClinicTab = patientId !== undefined ? "patients" : tab;

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
    nav.openTab(next.id);
    tabs.current.get(next.id)?.focus();
  }

  return (
    <div className="page">
      <TopBar active="clinic" />
      <main className="page-main clinic">
        <p className="clinic-beta" role="note">
          <span className="chip">בטא</span>
          <span>ניהול הקליניקה בגרסת בטא. הכל נשמר מוצפן בכספת שבמחשב, ושום דבר מכאן לא נשלח לשום מקום.</span>
        </p>
        <header className="page-head clinic-head">
          <h1>קליניקה</h1>
          <div className="clinic-discreet">
            <Switch id="clinic-discreet" label="מצב דיסקרטי" hint="שמות המטופלים מוצגים בראשי תיבות בלבד, למקרה שמישהו בחדר רואה את המסך."
              checked={discreet} onChange={(on) => { discreetNow = on; setDiscreet(on); }} />
          </div>
        </header>

        <div className="clinic-tabs" role="tablist" aria-label="חלקי הקליניקה" onKeyDown={onKey}>
          {CLINIC_TABS.map((t) => (
            <button key={t.id} type="button" role="tab" id={`clinic-tab-${t.id}`} aria-selected={t.id === active} aria-controls={t.id === active ? `clinic-panel-${t.id}` : undefined}
              tabIndex={t.id === active ? 0 : -1} className={t.id === active ? "clinic-tab clinic-tab-on" : "clinic-tab"}
              ref={(el) => { if (el) tabs.current.set(t.id, el); else tabs.current.delete(t.id); }}
              onClick={() => nav.openTab(t.id)}>
              {t.label}
            </button>
          ))}
        </div>

        <DiscreetContext.Provider value={discreet}>
          <ClinicNavContext.Provider value={nav}>
            <div className="clinic-panel" role="tabpanel" id={`clinic-panel-${active}`} aria-labelledby={`clinic-tab-${active}`} tabIndex={0}>
              {patientId !== undefined ? <PatientCardView key={patientId} patientId={patientId} onBack={() => nav.openTab("patients")} />
                : active === "today" ? <TodayTab />
                : active === "calendar" ? <CalendarTab />
                : active === "patients" ? <PatientsTab />
                : active === "waiting" ? <WaitingTab />
                : active === "tasks" ? <TasksTab />
                : active === "money" ? <MoneyTab />
                : <ClinicSettingsTab />}
            </div>
          </ClinicNavContext.Provider>
        </DiscreetContext.Provider>
      </main>
    </div>
  );
}
