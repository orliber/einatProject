import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { mockIPC } from "@tauri-apps/api/mocks";
import { AppContext, type AppApi } from "../App";
import { status } from "../test/fixtures";
import type { DriveBackupStatus } from "../ipc/client";
import { DRIVE_COPY_TURNS_OFF, GoogleSettings } from "./GoogleRecovery";

const api = (over: Partial<AppApi> = {}): AppApi => ({
  status: status(),
  go: vi.fn(),
  refresh: vi.fn(async () => undefined),
  notify: vi.fn(),
  fail: (e) => e.message,
  lockNow: vi.fn(async () => undefined),
  ...over,
});

function drive(on: boolean): DriveBackupStatus {
  return { available: true, connected: true, on, last_at: null, problem: null, cleanup_pending: false, copies_left: false };
}

/** Settings → disconnect Google, with the encrypted Drive copy `on` or off. */
async function disconnect(on: boolean) {
  const calls: string[] = [];
  mockIPC((cmd) => {
    calls.push(cmd);
    if (cmd === "google_status") return { available: true, on: true, needs_setup_here: false };
    if (cmd === "drive_backup_status") return drive(on);
    return null;
  });
  const qc = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  const notify = vi.fn();
  render(
    <QueryClientProvider client={qc}>
      <AppContext.Provider value={api({ notify })}><GoogleSettings /></AppContext.Provider>
    </QueryClientProvider>,
  );
  fireEvent.click(await screen.findByRole("button", { name: "ביטול…" }));
  return { calls, notify };
}

describe("disconnecting Google", () => {
  it("says the Drive copy turns off too, and its copies are deleted, when it is on", async () => {
    const { calls, notify } = await disconnect(true);
    expect(await screen.findByText(DRIVE_COPY_TURNS_OFF)).toBeInTheDocument();
    fireEvent.change(screen.getByLabelText("הסיסמה הנוכחית של הכספת"), { target: { value: "סיסמה ארוכה מספיק" } });
    fireEvent.click(screen.getByRole("button", { name: "ביטול הכניסה עם גוגל" }));
    await waitFor(() => expect(notify).toHaveBeenCalledWith(expect.stringContaining("העותק בגוגל דרייב כובה")));
    // The Drive copy's status is read again, not left saying "on".
    const after = calls.lastIndexOf("google_turn_off");
    expect(calls.slice(after)).toContain("drive_backup_status");
  });

  it("says nothing about the Drive copy when it is off", async () => {
    await disconnect(false);
    await screen.findByRole("button", { name: "ביטול הכניסה עם גוגל" });
    await new Promise((r) => setTimeout(r, 0));
    expect(screen.queryByText(DRIVE_COPY_TURNS_OFF)).toBeNull();
  });
});
