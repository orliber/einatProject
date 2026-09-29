import { createContext, useCallback, useContext, useEffect, useRef, useState } from "react";
import { ipc, type AppStatus, type UiError } from "./ipc/client";
import { LockScreen } from "./screens/LockScreen";
import { SetupScreen } from "./screens/SetupScreen";
import { CasesScreen } from "./screens/CasesScreen";
import { CaseScreen } from "./screens/CaseScreen";
import { ConsultScreen } from "./screens/ConsultScreen";
import { SettingsScreen } from "./screens/SettingsScreen";
import { Toast } from "./components/ui";

export type Route =
  | { name: "cases" }
  | { name: "case"; id: string; view: string }
  | { name: "consult"; caseId?: string }
  | { name: "settings" };

export interface AppApi {
  status: AppStatus;
  go: (r: Route) => void;
  refresh: () => Promise<void>;
  notify: (text: string) => void;
  /** Shows the error; a "locked" error returns to the lock screen. */
  fail: (e: UiError) => string;
  lockNow: () => Promise<void>;
}

export const AppContext = createContext<AppApi | null>(null);

export function useApp(): AppApi {
  const api = useContext(AppContext);
  if (!api) throw new Error("useApp outside App");
  return api;
}

export function App() {
  const [status, setStatus] = useState<AppStatus | null>(null);
  const [route, setRoute] = useState<Route>({ name: "cases" });
  const [toast, setToast] = useState<string | null>(null);
  const [coreDown, setCoreDown] = useState(false);
  // Setup stays on screen until it is finished, even though the vault exists (and is
  // unlocked) as soon as it is created: the recovery kit must never be skipped.
  const [inSetup, setInSetup] = useState(false);

  const refresh = useCallback(async () => {
    try {
      setStatus(await ipc.status());
      setCoreDown(false);
    } catch {
      setCoreDown(true);
    }
  }, []);

  useEffect(() => {
    let alive = true;
    const tick = () =>
      ipc
        .status()
        .then((s) => {
          if (alive) {
            setStatus(s);
            setCoreDown(false);
          }
        })
        .catch(() => alive && setCoreDown(true));
    void tick();
    // The core locks itself when idle; notice it without waiting for a click.
    const t = window.setInterval(() => void tick(), 15000);
    return () => {
      alive = false;
      window.clearInterval(t);
    };
  }, []);

  const fail = useCallback(
    (e: UiError) => {
      if (e.code === "locked") void refresh();
      return e.message;
    },
    [refresh],
  );

  const lockNow = useCallback(async () => {
    await ipc.lock().catch(() => undefined);
    setRoute({ name: "cases" });
    await refresh();
  }, [refresh]);

  // Ctrl+L (⌘L on Mac) locks at once: stepping away from the desk should take one key.
  const unlocked = status?.unlocked === true;

  // Once the vault locks (by hand or when idle), reload the page: names shown on screen
  // must not stay in the page's memory. Only in the built app (tests and dev keep state).
  const wasUnlocked = useRef(false);
  useEffect(() => {
    if (wasUnlocked.current && !unlocked && import.meta.env.PROD) window.location.reload();
    wasUnlocked.current = unlocked;
  }, [unlocked]);
  useEffect(() => {
    if (!unlocked) return;
    const onKey = (e: KeyboardEvent) => {
      if ((e.ctrlKey || e.metaKey) && !e.altKey && !e.shiftKey && e.key.toLowerCase() === "l") {
        e.preventDefault();
        void lockNow();
      }
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [unlocked, lockNow]);

  if (coreDown && !status) {
    return <main className="center-note"><p className="error">אין חיבור לליבה המאובטחת. כדאי לסגור ולפתוח את התוכנה.</p></main>;
  }
  if (!status) return <main className="center-note" aria-busy="true" />;
  if (!status.vault_exists || inSetup) {
    return (
      <SetupScreen status={status}
        onCreated={() => { setInSetup(true); void refresh(); }}
        onDone={async () => { setInSetup(false); await refresh(); }} />
    );
  }
  if (!status.unlocked) {
    return <LockScreen status={status} onUnlocked={(s) => { setStatus(s); setRoute({ name: "cases" }); }} />;
  }

  const api: AppApi = { status, go: setRoute, refresh, notify: setToast, fail, lockNow };
  return (
    <AppContext.Provider value={api}>
      {route.name === "cases" && <CasesScreen />}
      {route.name === "case" && <CaseScreen key={route.id} caseId={route.id} view={route.view} />}
      {route.name === "consult" && <ConsultScreen caseId={route.caseId} />}
      {route.name === "settings" && <SettingsScreen />}
      {toast && <Toast text={toast} onDone={() => setToast(null)} />}
    </AppContext.Provider>
  );
}
