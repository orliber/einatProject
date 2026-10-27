import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { mockIPC } from "@tauri-apps/api/mocks";
import { AppContext, type AppApi } from "../App";
import { status } from "../test/fixtures";
import type { DriveBackupStatus, DriveCleanupResult } from "../ipc/client";
import { CLEANUP_PENDING, CLEANUP_WITHDRAWN, CONNECT_FIRST, COPIES_LEFT, DriveBackupSettings } from "./DriveBackupSettings";

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
  available: true, connected: true, on: false, last_at: null, problem: null, cleanup_pending: false, copies_left: false, ...over,
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
  it("is off by default, and says in a few lines what goes up and what permission it keeps", async () => {
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
    expect(screen.getByText(/בלי הסיסמה שלך, הקובץ הזה לא שימושי/)).toBeInTheDocument();
    expect(screen.getByText(/שלושת העותקים האחרונים/)).toBeInTheDocument();
    expect(screen.getByText(/שומרת בתוך הכספת הרשאה לתיקייה הנסתרת בלבד, עד שתכבי/)).toBeInTheDocument();
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

  /** Turn off from "on", with Google answering `result`; returns the switch and the calls. */
  async function turnOffWith(result: DriveCleanupResult, notify = vi.fn()) {
    let on = true;
    let release = () => {};
    const calls: string[] = [];
    mockIPC(async (cmd) => {
      calls.push(cmd);
      if (cmd === "drive_backup_turn_off") {
        await new Promise<void>((r) => { release = r; });
        on = false;
        return result;
      }
      return drive({ on, cleanup_pending: !on && result === "pending" });
    });
    show(api({ notify }));
    const sw = await screen.findByRole("switch", { name: /עותק מוצפן גם בגוגל דרייב/ });
    fireEvent.click(sw);
    // While Google is asked, a second click cannot start it again.
    await waitFor(() => expect(sw).toBeDisabled());
    fireEvent.click(sw);
    release();
    await waitFor(() => expect(sw).toHaveAttribute("aria-checked", "false"));
    expect(calls.filter((c) => c === "drive_backup_turn_off")).toHaveLength(1);
    return { notify };
  }

  it("says the copies were deleted and the permission withdrawn only when Google confirmed it", async () => {
    const { notify } = await turnOffWith("cleaned");
    await waitFor(() => expect(notify).toHaveBeenCalledWith(expect.stringContaining("ההרשאה שניתנה לתוכנה בוטלה")));
    expect(screen.queryByText(CLEANUP_PENDING)).toBeNull();
  });

  it("says what is still owed at Google when it could not finish, without blaming the internet", async () => {
    const { notify } = await turnOffWith("pending");
    expect(await screen.findByText(CLEANUP_PENDING)).toBeInTheDocument();
    expect(screen.queryByText(/אין חיבור/)).toBeNull();
    expect(notify).not.toHaveBeenCalled();
  });

  it("tells her where to delete the copies herself when the permission was gone, and what that also deletes", async () => {
    await turnOffWith("withdrawn");
    expect(await screen.findByText(CLEANUP_WITHDRAWN)).toBeInTheDocument();
    expect(screen.getByText(/זה מוחק גם את המפתח של הכניסה עם גוגל/)).toBeInTheDocument();
  });

  it("shows a cleanup still owed from before, while off", async () => {
    mockIPC(() => drive({ cleanup_pending: true }));
    show();
    expect(await screen.findByText(CLEANUP_PENDING)).toBeInTheDocument();
  });

  it("lets the owed cleanup's line go once the program finished it by itself", async () => {
    let pending = true;
    mockIPC(() => drive({ cleanup_pending: pending }));
    const qc = new QueryClient({ defaultOptions: { queries: { retry: false } } });
    render(
      <QueryClientProvider client={qc}>
        <AppContext.Provider value={api()}><DriveBackupSettings /></AppContext.Provider>
      </QueryClientProvider>,
    );
    expect(await screen.findByText(CLEANUP_PENDING)).toBeInTheDocument();
    pending = false;
    await qc.refetchQueries({ queryKey: ["drive-backup"] });
    await waitFor(() => expect(screen.queryByText(CLEANUP_PENDING)).toBeNull());
  });

  it("after a restore, says copies from before may be in Drive and how to delete them", async () => {
    mockIPC(() => drive({ copies_left: true }));
    show();
    expect(await screen.findByText(COPIES_LEFT)).toBeInTheDocument();
  });

  it("is hidden where this version cannot talk to Google", async () => {
    mockIPC(() => drive({ available: false }));
    const { container } = show();
    await new Promise((r) => setTimeout(r, 0));
    expect(container).toBeEmptyDOMElement();
  });
});
