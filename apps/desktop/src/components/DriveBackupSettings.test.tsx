import { fireEvent, render, screen } from "@testing-library/react";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { mockIPC } from "@tauri-apps/api/mocks";
import { AppContext, type AppApi } from "../App";
import { status } from "../test/fixtures";
import type { DriveBackupStatus } from "../ipc/client";
import { CONNECT_FIRST, DriveBackupSettings } from "./DriveBackupSettings";

const api = (over: Partial<AppApi> = {}): AppApi => ({
  status: status(),
  go: vi.fn(),
  refresh: vi.fn(async () => undefined),
  notify: vi.fn(),
  fail: (e) => e.message,
  lockNow: vi.fn(async () => undefined),
  ...over,
});

const drive = (over: Partial<DriveBackupStatus> = {}): DriveBackupStatus => ({
  available: true, connected: true, on: false, last_at: null, problem: null, ...over,
});

function show(a: AppApi = api()) {
  const qc = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  return render(
    <QueryClientProvider client={qc}>
      <AppContext.Provider value={a}>
        <DriveBackupSettings />
      </AppContext.Provider>
    </QueryClientProvider>,
  );
}

describe("the encrypted copy in Google Drive", () => {
  it("is off by default, and says in three lines what goes up", async () => {
    const calls: string[] = [];
    mockIPC((cmd) => {
      calls.push(cmd);
      return drive();
    });
    show();
    const sw = await screen.findByRole("switch", { name: /עותק מוצפן גם בגוגל דרייב/ });
    expect(sw).toHaveAttribute("aria-checked", "false");
    expect(sw).toBeEnabled();
    expect(screen.getByText(/רק קובץ הגיבוי המוצפן/)).toBeInTheDocument();
    expect(screen.getByText(/ערכת השחזור המודפסת/)).toBeInTheDocument();
    expect(screen.getByText(/שלושת העותקים האחרונים/)).toBeInTheDocument();
    expect(calls).toEqual(["drive_backup_status"]);
  });

  it("says how to connect Google instead of turning on when it is not connected", async () => {
    const calls: string[] = [];
    mockIPC((cmd) => {
      calls.push(cmd);
      return drive({ connected: false });
    });
    show();
    const sw = await screen.findByRole("switch", { name: /עותק מוצפן גם בגוגל דרייב/ });
    expect(sw).toBeDisabled();
    expect(screen.getByText(CONNECT_FIRST)).toBeInTheDocument();
    fireEvent.click(sw);
    expect(calls).not.toContain("drive_backup_turn_on");
  });

  it("turns on through Google when connected, and shows a quiet line when a copy did not go up", async () => {
    let on = false;
    const notify = vi.fn();
    mockIPC((cmd) => {
      if (cmd === "drive_backup_turn_on") on = true;
      return drive({ on, problem: on ? "offline" : null });
    });
    show(api({ notify }));
    fireEvent.click(await screen.findByRole("switch", { name: /עותק מוצפן גם בגוגל דרייב/ }));
    expect(await screen.findByRole("status")).toHaveTextContent("לא היה חיבור לאינטרנט");
    expect(screen.getByRole("switch", { name: /עותק מוצפן גם בגוגל דרייב/ })).toHaveAttribute("aria-checked", "true");
    expect(notify).toHaveBeenCalled();
  });

  it("is hidden where this version cannot talk to Google", async () => {
    mockIPC(() => drive({ available: false }));
    const { container } = show();
    await new Promise((r) => setTimeout(r, 0));
    expect(container).toBeEmptyDOMElement();
  });
});
