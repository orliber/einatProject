// The clinic's page (beta, D-170): the bar with the clinic's tabs under it (D-191), and the open
// tab. "בטא" is on the bar's switch ("ניהול הקליניקה", TopBar's .topbar-beta), which shows on
// every clinic page at every width; the page has no line of its own about it (D-204). The tab
// and the open patient live in the app's route, so a link from one tab to another (or to a
// patient's card) is one `go`. The clinic's settings are a part of the program's settings
// (D-205), not a tab here.
import { useMemo } from "react";
import { useApp } from "../../App";
import { TopBar } from "../../components/TopBar";
import { CalendarTab } from "./CalendarTab";
import { InsightsTab } from "./InsightsTab";
import { MoneyTab } from "./MoneyTab";
import { ClinicNavContext, type ClinicNav } from "./parts";
import { PatientCardView } from "./PatientCardView";
import { PatientsTab } from "./PatientsTab";
import { CLINIC_TABS, type ClinicTab } from "./tabs";
import { TasksTab } from "./TasksTab";
import { TodayTab } from "./TodayTab";
import { WaitingTab } from "./WaitingTab";
import "./ClinicScreen.css";

/** For the tests: each starts with discreet mode off (the switch is the program's, D-192). */
export { forgetDiscreet } from "../../discreet";

const TAB_TITLE = Object.fromEntries(CLINIC_TABS.map((t) => [t.id, t.label])) as Record<ClinicTab, string>;

export function ClinicScreen({ tab = "today", patientId }: { tab?: ClinicTab | undefined; patientId?: string | undefined }) {
  const { go } = useApp();
  const nav = useMemo<ClinicNav>(() => ({
    openPatient: (id) => go({ name: "clinic", tab: "patients", patientId: id }),
    openTab: (next) => go({ name: "clinic", tab: next }),
  }), [go]);
  // A patient's card is a page of the patients tab.
  const active: ClinicTab = patientId !== undefined ? "patients" : tab;

  // The tabs are in the row under the top bar (D-191); this page holds the open tab.
  return (
    <div className="page">
      <TopBar active="clinic" />
      <main className="page-main clinic">
        <h1 className="visually-hidden">ניהול הקליניקה: {TAB_TITLE[active]}</h1>
        <ClinicNavContext.Provider value={nav}>
          <div className="clinic-panel" role="tabpanel" id={`clinic-panel-${active}`} aria-labelledby={`clinic-tab-${active}`} tabIndex={0}>
            {patientId !== undefined ? <PatientCardView key={patientId} patientId={patientId} onBack={() => nav.openTab("patients")} />
              : active === "today" ? <TodayTab />
              : active === "calendar" ? <CalendarTab />
              : active === "patients" ? <PatientsTab />
              : active === "waiting" ? <WaitingTab />
              : active === "tasks" ? <TasksTab />
              : active === "money" ? <MoneyTab />
              : <InsightsTab />}
          </div>
        </ClinicNavContext.Provider>
      </main>
    </div>
  );
}
