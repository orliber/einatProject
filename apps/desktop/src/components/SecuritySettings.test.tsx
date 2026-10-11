import { fireEvent, render, screen } from "@testing-library/react";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { mockIPC } from "@tauri-apps/api/mocks";
import { AppContext, type AppApi } from "../App";
import { status } from "../test/fixtures";
import { SecuritySettings } from "./SecuritySettings";

const api = (): AppApi => ({
  status: status(),
  go: vi.fn(),
  refresh: vi.fn(async () => undefined),
  notify: vi.fn(),
  fail: (e) => e.message,
  lockNow: vi.fn(async () => undefined),
});

const view = (stopped: boolean) => ({
  items: [
    { key: "disk", state: "warn", detail: null },
    { key: "auto_lock", state: "ok", detail: "10" },
    { key: "send", state: stopped ? "warn" : "ok", detail: null },
  ],
  send_stopped: stopped,
  daily_limit: 400,
  max_request_kb: 2048,
});

describe("security status", () => {
  it("shows a missing protection, and the emergency stop goes to the core", async () => {
    const calls: [string, unknown][] = [];
    mockIPC((cmd, args) => {
      calls.push([cmd, args]);
      return view(cmd === "set_send_stopped");
    });
    const qc = new QueryClient({ defaultOptions: { queries: { retry: false } } });
    render(
      <QueryClientProvider client={qc}>
        <AppContext.Provider value={api()}>
          <SecuritySettings />
        </AppContext.Provider>
      </QueryClientProvider>,
    );
    expect(await screen.findByText(/נעילה אוטומטית אחרי 10 דקות/)).toBeInTheDocument();
    expect(screen.getByText("לבדוק")).toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: /עצירת חירום/ }));
    expect(await screen.findByRole("button", { name: /להפעיל שליחה מחדש/ })).toBeInTheDocument();
    expect(calls.find(([c]) => c === "set_send_stopped")?.[1]).toMatchObject({ stopped: true });
  });
});
