// The base styles come first, before any component's stylesheet, whatever chunk the bundler
// puts them in: a component rule overrides a base rule by coming later.
import "./styles/tokens.css";
import "./styles/base.css";
import "./styles/motion.css";
import { createContext, lazy, Suspense, useCallback, useContext, useEffect, useMemo, useRef, useState } from "react";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { ipc, onCoreEvent, type AppStatus, type UiError } from "./ipc/client";
import { LockScreen } from "./screens/LockScreen";
import { CasesScreen } from "./screens/CasesScreen";
import { CaseScreen } from "./screens/CaseScreen";
import { ErrorLine, Toast, type ToastAction } from "./components/ui";
import { IdleWarning } from "./components/IdleWarning";
import { forgetHold, holdNow, UnsavedNotice } from "./components/Unsaved";
import { applyTextSize, readTextSize, stepTextSize } from "./textSize";
import { ShortcutsSheet, useShortcutsSheet } from "./components/ShortcutsSheet";
import { isCombo, KEYS } from "./shortcuts";
import { forgetFailures, rememberFailure } from "./errors";
import { resetRuns } from "./screens/case/reportRun";
import { clearKept } from "./kept";
import type { SettingsPane } from "./screens/settingsPanes";

// The pages she opens rarely load on their own, so the first paint and the case pages carry
// only what they need; they are fetched in the background right after the vault opens.
const SetupScreen = lazy(() => import("./screens/SetupScreen").then((m) => ({ default: m.SetupScreen })));
const ConsultScreen = lazy(() => import("./screens/ConsultScreen").then((m) => ({ default: m.ConsultScreen })));
const SettingsScreen = lazy(() => import("./screens/SettingsScreen").then((m) => ({ default: m.SettingsScreen })));
const StyleScreen = lazy(() => import("./screens/StyleScreen").then((m) => ({ default: m.StyleScreen })));
function warmRarePages() {
  const load = () => { void import("./screens/SettingsScreen"); void import("./screens/ConsultScreen"); void import("./screens/StyleScreen"); };
  // WebKit (the Mac) has no requestIdleCallback.
  if (typeof window.requestIdleCallback === "function") window.requestIdleCallback(load);
  else window.setTimeout(load, 1500);
}
/** While a page's code is fetched: an empty page in its place, never a flash of another page. */
const PageLoading = () => <main className="center-note" aria-busy="true" />;

export type Route =
  | { name: "cases" }
  | { name: "case"; id: string; view: string }
  | { name: "consult"; caseId?: string }
  | { name: "style" }
  | { name: "settings"; pane?: SettingsPane };

export interface AppApi {
  status: AppStatus;
  /** Where she is now; lets an error offer the next step on this case. */
  route?: Route;
  go: (r: Route) => void;
  refresh: () => Promise<void>;
  /** A short notice; with an action (such as "ביטול") it stays longer. */
  notify: (text: string, action?: ToastAction) => void;
  /** Shows the error; a "locked" error returns to the lock screen. */
  fail: (e: UiError) => string;
  lockNow: () => Promise<void>;
  /** The keyboard shortcuts sheet (also F1). */
  showShortcuts?: () => void;
}

export const AppContext = createContext<AppApi | null>(null);

/** Data from the core, cached while the vault is open (D-023). Cleared on every lock. */
export const queryClient = new QueryClient({
  defaultOptions: { queries: { retry: false, refetchOnWindowFocus: false, staleTime: 5_000 } },
});

/** The idle countdown changes on every read; everything else in the status changes rarely.
 *  Two statuses that differ only in the countdown are the same for the screens (F3). */
export function sameStatus(a: AppStatus | null, b: AppStatus): boolean {
  if (a === null) return false;
  const without = (s: AppStatus) => JSON.stringify({ ...s, idle_lock_in: null });
  return without(a) === without(b);
}

/** With events, the poll is a safety net only: the core announces a lock or the last minute.
 *  Without them (a refused grant, the preview) the poll is all there is, so it runs often. */
export const STATUS_POLL_MS = 60_000;
export const STATUS_POLL_NO_EVENTS_MS = 15_000;

export function useApp(): AppApi {
  const api = useContext(AppContext);
  if (!api) throw new Error("useApp outside App");
  return api;
}

export function App() {
  const [status, setStatus] = useState<AppStatus | null>(null);
  const [route, setRoute] = useState<Route>({ name: "cases" });
  const [toast, setToast] = useState<{ text: string; action?: ToastAction | undefined } | null>(null);
  const [coreDown, setCoreDown] = useState(false);
  // Setup stays on screen until it is finished, even though the vault exists (and is
  // unlocked) as soon as it is created: the recovery kit must never be skipped.
  const [inSetup, setInSetup] = useState(false);

  // The seconds before the idle lock live apart from the status: a changing countdown must
  // not redraw every screen, and a field she is typing in must never lose the keyboard.
  const [idleIn, setIdleIn] = useState<number | null>(null);
  const apply = useCallback((s: AppStatus) => {
    setIdleIn(s.idle_lock_in);
    setStatus((prev) => (sameStatus(prev, s) ? prev : s));
    setCoreDown(false);
  }, []);

  const refresh = useCallback(async () => {
    try {
      apply(await ipc.status());
    } catch {
      setCoreDown(true);
    }
  }, [apply]);

  useEffect(() => {
    let alive = true;
    const tick = () =>
      ipc
        .status()
        .then((s) => alive && apply(s))
        .catch(() => alive && setCoreDown(true));
    void tick();
    // Poll often until events are known to arrive; then the poll only backs them up.
    let t = window.setInterval(() => void tick(), STATUS_POLL_NO_EVENTS_MS);
    let stop: (() => void) | null = null;
    void onCoreEvent((e) => {
      if (!alive) return;
      if (e.kind === "idle_soon") setIdleIn(e.seconds);
      void tick();
    }).then((unlisten) => {
      if (unlisten === null) return;
      if (!alive) {
        unlisten();
        return;
      }
      stop = unlisten;
      window.clearInterval(t);
      t = window.setInterval(() => void tick(), STATUS_POLL_MS);
    });
    return () => {
      alive = false;
      window.clearInterval(t);
      stop?.();
    };
  }, [apply]);

  const fail = useCallback(
    (e: UiError) => {
      rememberFailure(e);
      if (e.code === "locked") void refresh();
      return e.message;
    },
    [refresh],
  );

  const lockNow = useCallback(async () => {
    // What she was typing a moment ago reaches the core first, which keeps it as it locks.
    await holdNow();
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
    // Names must not stay in memory once the vault is locked.
    if (!unlocked) {
      queryClient.clear();
      clearKept();
      // And the failures kept for "לנסות שוב": they were for the vault just closed.
      forgetFailures();
      forgetHold();
      // And "write the whole report": its line was for the vault just closed (the next step
      // offers what is still unwritten once it is open again).
      resetRuns();
    }
    wasUnlocked.current = unlocked;
  }, [unlocked]);
  useEffect(() => {
    if (unlocked) warmRarePages();
  }, [unlocked]);
  useEffect(() => {
    if (!unlocked) return;
    const onKey = (e: KeyboardEvent) => {
      // By the physical key: with the Hebrew layout on, Ctrl+L is "ך" (UX-5).
      if (isCombo(e, KEYS.lock)) {
        e.preventDefault();
        void lockNow();
      }
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [unlocked, lockNow]);

  // Typing a long paragraph never calls the core, so the window reports activity itself
  // (at most every 30 seconds): the idle lock must not throw away work in progress.
  useEffect(() => {
    if (!unlocked) return;
    let last = 0;
    const onActivity = () => {
      const now = Date.now();
      if (now - last < 30_000) return;
      last = now;
      void ipc.touch().catch(() => undefined);
    };
    const events = ["keydown", "pointerdown", "wheel", "input"] as const;
    events.forEach((e) => window.addEventListener(e, onActivity, { passive: true }));
    return () => events.forEach((e) => window.removeEventListener(e, onActivity));
  }, [unlocked]);

  // Ctrl + / Ctrl − / Ctrl 0 change the text size, as in a browser.
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (!(e.ctrlKey || e.metaKey) || e.altKey) return;
      const now = readTextSize();
      let next: number | null = null;
      if (e.key === "+" || e.key === "=" || e.code === "Equal" || e.code === "NumpadAdd") next = stepTextSize(now, 1);
      else if (e.key === "-" || e.code === "Minus" || e.code === "NumpadSubtract") next = stepTextSize(now, -1);
      else if (e.key === "0" || e.code === "Digit0" || e.code === "Numpad0") next = 100;
      if (next === null) return;
      e.preventDefault();
      applyTextSize(next);
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, []);

  const sheet = useShortcutsSheet(unlocked);

  const keepWorking = useCallback(async () => {
    await ipc.touch().catch(() => undefined);
    await refresh();
  }, [refresh]);

  if (coreDown && !status) {
    return <main className="center-note"><ErrorLine error="אין חיבור לליבה המאובטחת. אפשר לנסות שוב, ואם זה חוזר, לסגור ולפתוח את התוכנה." retry={() => window.location.reload()} report /></main>;
  }
  if (!status) return <main className="center-note" aria-busy="true" />;
  if (!status.vault_exists || inSetup) {
    return (
      <Suspense fallback={<PageLoading />}>
        <SetupScreen status={status}
          onCreated={() => { setInSetup(true); void refresh(); }}
          onDone={async () => { setInSetup(false); await refresh(); }}
          onRestored={(s) => { setInSetup(false); setStatus(s); setRoute({ name: "cases" }); }} />
      </Suspense>
    );
  }
  if (!status.unlocked) {
    return <LockScreen status={status} onUnlocked={(s) => { setStatus(s); setRoute({ name: "cases" }); }} />;
  }

  return (
    <Shell status={status} route={route} setRoute={setRoute} refresh={refresh} setToast={setToast} fail={fail} lockNow={lockNow}
      showShortcuts={sheet.show} toast={toast} sheet={sheet} idleIn={idleIn} keepWorking={keepWorking} />
  );
}

/** The unlocked app. The context value is kept while nothing in it changed, so a toast or a
 *  countdown does not redraw the case she is working in. */
function Shell(props: {
  status: AppStatus;
  route: Route;
  setRoute: (r: Route) => void;
  refresh: () => Promise<void>;
  setToast: (t: { text: string; action?: ToastAction | undefined } | null) => void;
  fail: (e: UiError) => string;
  lockNow: () => Promise<void>;
  showShortcuts: () => void;
  toast: { text: string; action?: ToastAction | undefined } | null;
  sheet: { open: boolean; close: () => void };
  idleIn: number | null;
  keepWorking: () => Promise<void>;
}) {
  const { status, route, setRoute, refresh, setToast, fail, lockNow, showShortcuts, toast, sheet, idleIn, keepWorking } = props;
  const notify = useCallback((text: string, action?: ToastAction) => setToast({ text, action }), [setToast]);
  const api = useMemo<AppApi>(
    () => ({ status, route, go: setRoute, refresh, notify, fail, lockNow, showShortcuts }),
    [status, route, setRoute, refresh, notify, fail, lockNow, showShortcuts],
  );
  return (
    <QueryClientProvider client={queryClient}>
    <AppContext.Provider value={api}>
      {route.name === "cases" && <CasesScreen />}
      {route.name === "case" && <CaseScreen key={route.id} caseId={route.id} view={route.view} />}
      <Suspense fallback={<PageLoading />}>
        {route.name === "consult" && <ConsultScreen caseId={route.caseId} />}
        {route.name === "style" && <StyleScreen />}
        {route.name === "settings" && <SettingsScreen key={route.pane ?? ""} pane={route.pane} />}
      </Suspense>
      {toast && <Toast text={toast.text} action={toast.action} onDone={() => setToast(null)} />}
      <UnsavedNotice />
      {sheet.open && <ShortcutsSheet onClose={sheet.close} />}
      {idleIn != null && idleIn <= 75 && <IdleWarning seconds={idleIn} onKeep={() => void keepWorking()} />}
    </AppContext.Provider>
    </QueryClientProvider>
  );
}
