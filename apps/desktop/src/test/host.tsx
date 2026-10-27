// One screen or case view on its own, over the in-memory demo core: the app's context, a fresh
// query cache, and (for a case view) a case whose review sends at once, as with "show me every
// request before sending" off. Every name and detail comes from the demo core, all invented.
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { mockIPC } from "@tauri-apps/api/mocks";
import { useCallback, useEffect, useState, type ReactNode } from "react";
import { AppContext, type AppApi } from "../App";
import { rememberFailure } from "../errors";
import { ipc, type CaseDetail } from "../ipc/client";
import type { CaseApi, Job } from "../screens/CaseScreen";
import { FakeCore } from "../web/fakeCore";
import { status } from "./fixtures";

/** Answers a command instead of the demo core; `undefined` lets the core answer. */
export type Override = (cmd: string, args: Record<string, unknown>, core: FakeCore) => unknown;

export interface Served {
  core: FakeCore;
  /** The demo case of נועם. */
  caseId: string;
  calls: { cmd: string; args: Record<string, unknown> }[];
}

/** The demo core, unlocked, behind the IPC. */
export async function serve(over: Override = () => undefined): Promise<Served> {
  const core = new FakeCore();
  await core.handle("unlock", { password: "x" });
  await core.handle("set_review_always", { on: false });
  const cases = (await core.handle("list_cases", {})) as { id: string; child_name: string | null }[];
  const caseId = cases.find((c) => c.child_name === "נועם")?.id ?? "";
  const calls: Served["calls"] = [];
  mockIPC(async (cmd, args) => {
    const a = (args ?? {}) as Record<string, unknown>;
    calls.push({ cmd, args: a });
    const out = await over(cmd, a, core);
    return out === undefined ? core.handle(cmd, a) : out;
  });
  return { core, caseId, calls };
}

/** The app's context as the screens see it; `fail` keeps the failure for its error line, as the app does. */
export function appApi(over: Partial<AppApi> = {}): AppApi {
  return {
    status: status(),
    go: vi.fn(),
    refresh: vi.fn(async () => undefined),
    notify: vi.fn(),
    fail: (e) => {
      rememberFailure(e);
      return e.message;
    },
    lockNow: vi.fn(async () => undefined),
    ...over,
  };
}

export function Host({ app, children }: { app: AppApi; children: ReactNode }) {
  const [qc] = useState(() => new QueryClient({ defaultOptions: { queries: { retry: false } } }));
  return (
    <QueryClientProvider client={qc}>
      <AppContext.Provider value={app}>{children}</AppContext.Provider>
    </QueryClientProvider>
  );
}

let jobSeq = 0;

/** A case as the case screen gives it to a view: its detail (reloaded on `reload`), the
 *  requests on their way, and a review that sends what the gate cleared. */
export function CaseHost({ caseId, children }: { caseId: string; children: (api: CaseApi) => ReactNode }) {
  const [detail, setDetail] = useState<CaseDetail | null>(null);
  const [jobs, setJobs] = useState<Job[]>([]);
  const reload = useCallback(async () => setDetail(await ipc.caseDetail(caseId)), [caseId]);
  useEffect(() => {
    let alive = true;
    void ipc.caseDetail(caseId).then((d) => alive && setDetail(d));
    return () => {
      alive = false;
    };
  }, [caseId]);
  const track = useCallback<CaseApi["track"]>(async (job, run, then) => {
    const j: Job = { ...job, id: ++jobSeq, started: Date.now(), estimate: 1000 };
    setJobs((js) => [...js, j]);
    try {
      const out = await run();
      await then?.(out);
      return out;
    } finally {
      setJobs((js) => js.filter((x) => x.id !== j.id));
    }
  }, []);
  if (!detail) return null;
  const api: CaseApi = {
    caseId,
    detail,
    reload,
    review: async (r) => {
      if (!r.prepared.approval_id) return false;
      await r.onSend(r.prepared.approval_id);
      return true;
    },
    sort: async () => undefined,
    jobs,
    track,
    draft: async () => false,
  };
  return <>{children(api)}</>;
}
