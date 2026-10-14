import { act, fireEvent, render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { mockIPC } from "@tauri-apps/api/mocks";
import { emit } from "@tauri-apps/api/event";
import { App, sameStatus } from "./App";
import { status } from "./test/fixtures";

const KEY = "7K3M-Q9WD-2HXR-PB6T-N4VC-8FJY-D3KA-W7ZE-5RGH-M2QX-T6BN-9CYP-4HDV-K8XA";

describe("App", () => {
  it("first run: password, then the recovery kit that must be typed back", async () => {
    let created = false;
    mockIPC((cmd, args) => {
      if (cmd === "app_status") return status({ vault_exists: created, unlocked: created });
      if (cmd === "create_vault") {
        created = true;
        return { recovery_key: KEY };
      }
      if (cmd === "confirm_recovery_key") return (args as { typed: string }).typed === KEY;
      if (cmd === "ping") return { ipc_version: 1, core_version: "0.1.0", build_commit: "t", fips_active: false, platform: "linux" };
      return null;
    });
    const user = userEvent.setup();
    render(<App />);
    await user.type(await screen.findByLabelText("סיסמה"), "כלב ירוק רץ מהר");
    await user.type(screen.getByLabelText("שוב, לאימות"), "כלב ירוק רץ מהר");
    await user.click(screen.getByRole("button", { name: "יצירת הכספת" }));
    expect(await screen.findByText("7K3M")).toBeInTheDocument();
    expect(screen.getByText("K8XA")).toBeInTheDocument();

    const typed = screen.getByLabelText(/הקלידי אותה מהדף/);
    // A wrong kit of the full length (a shorter one leaves "המשך" disabled).
    await user.type(typed, KEY.replace(/[0-9A-Z]/g, "X"));
    await user.click(screen.getByRole("button", { name: "המשך" }));
    expect(await screen.findByRole("alert")).toHaveTextContent("לא תואמת");

    await user.clear(typed);
    await user.type(typed, KEY);
    await user.click(screen.getByRole("button", { name: "המשך" }));
    expect(await screen.findByRole("heading", { name: "הפרטים שלך" })).toBeInTheDocument();
  });

  it("the recovery kit stays on screen although the vault now exists (regression)", async () => {
    vi.useFakeTimers({ shouldAdvanceTime: true });
    let created = false;
    mockIPC((cmd) => {
      if (cmd === "app_status") return status({ vault_exists: created, unlocked: created });
      if (cmd === "create_vault") {
        created = true;
        return { recovery_key: KEY };
      }
      if (cmd === "list_cases") return [];
      return null;
    });
    const user = userEvent.setup({ advanceTimers: vi.advanceTimersByTime });
    render(<App />);
    await user.type(await screen.findByLabelText("סיסמה"), "כלב ירוק רץ מהר");
    await user.type(screen.getByLabelText("שוב, לאימות"), "כלב ירוק רץ מהר");
    await user.click(screen.getByRole("button", { name: "יצירת הכספת" }));
    expect(await screen.findByRole("heading", { name: "ערכת השחזור שלך" })).toBeInTheDocument();
    // The periodic status check now reports an existing, unlocked vault.
    await act(async () => {
      await vi.advanceTimersByTimeAsync(16000);
    });
    expect(screen.getByRole("heading", { name: "ערכת השחזור שלך" })).toBeInTheDocument();
    expect(screen.queryByRole("heading", { name: "התיק הראשון" })).not.toBeInTheDocument();
    vi.useRealTimers();
  });

  it("a short password is refused before anything is created", async () => {
    let calls = 0;
    mockIPC((cmd) => {
      if (cmd === "app_status") return status({ vault_exists: false, unlocked: false });
      if (cmd === "create_vault") calls += 1;
      return null;
    });
    const user = userEvent.setup();
    render(<App />);
    await user.type(await screen.findByLabelText("סיסמה"), "קצר");
    await user.type(screen.getByLabelText("שוב, לאימות"), "קצר");
    await user.click(screen.getByRole("button", { name: "יצירת הכספת" }));
    expect(await screen.findByRole("alert")).toHaveTextContent("12 תווים");
    expect(calls).toBe(0);
  });

  it("locked: a wrong password shows the core's message", async () => {
    mockIPC((cmd) => {
      if (cmd === "app_status") return status({ unlocked: false });
      if (cmd === "unlock") throw { code: "wrong_secret", message: "הסיסמה או ערכת השחזור לא נכונות.", details: [] };
      if (cmd === "ping") return { ipc_version: 1, core_version: "0.1.0", build_commit: "t", fips_active: false, platform: "linux" };
      return null;
    });
    const user = userEvent.setup();
    render(<App />);
    await user.type(await screen.findByLabelText("סיסמה"), "לא נכונה בכלל");
    await user.click(screen.getByRole("button", { name: "פתיחה" }));
    expect(await screen.findByText("הסיסמה או ערכת השחזור לא נכונות.")).toBeInTheDocument();
  });

  it("unlocked with no cases: a clear first step", async () => {
    mockIPC((cmd) => {
      if (cmd === "app_status") return status();
      if (cmd === "list_cases") return [];
      return null;
    });
    render(<App />);
    expect(await screen.findByRole("heading", { name: "התיק הראשון" })).toBeInTheDocument();
    expect(screen.getByText("מצב הדגמה")).toBeInTheDocument();
  });

  it("Ctrl+L locks at once", async () => {
    let locked = false;
    vi.useRealTimers();
    mockIPC((cmd) => {
      if (cmd === "app_status") return status({ vault_exists: true, unlocked: !locked });
      if (cmd === "lock") {
        locked = true;
        return null;
      }
      if (cmd === "list_cases") return [];
      if (cmd === "ping") return { ipc_version: 1, core_version: "0.1.0", build_commit: "t", fips_active: false, platform: "linux" };
      return null;
    });
    render(<App />);
    await screen.findByText("עוד אין תיקים");
    fireEvent.keyDown(window, { key: "l", ctrlKey: true });
    expect(await screen.findByRole("button", { name: "פתיחה" })).toBeInTheDocument();
    expect(locked).toBe(true);
  });

  it("a status that differs only in the idle countdown is the same status (F3)", () => {
    const a = status({ idle_lock_in: 600 });
    expect(sameStatus(a, status({ idle_lock_in: 70 }))).toBe(true);
    expect(sameStatus(a, status({ idle_lock_in: 600, demo_mode: false }))).toBe(false);
    expect(sameStatus(null, a)).toBe(false);
  });

  it("the countdown reaches the screen without redrawing it (F3)", async () => {
    vi.useFakeTimers({ shouldAdvanceTime: true });
    let idle = 600;
    let listed = 0;
    mockIPC((cmd) => {
      if (cmd === "app_status") return status({ idle_lock_in: idle });
      if (cmd === "list_cases") {
        listed += 1;
        return [];
      }
      return null;
    });
    render(<App />);
    expect(await screen.findByRole("heading", { name: "התיק הראשון" })).toBeInTheDocument();
    expect(screen.queryByRole("alert")).not.toBeInTheDocument();
    idle = 70;
    await act(async () => {
      await vi.advanceTimersByTimeAsync(61_000);
    });
    // The warning shows, and the cases screen was not mounted again (it lists once).
    expect(await screen.findByRole("alert")).toHaveTextContent("70");
    expect(listed).toBe(1);
    vi.useRealTimers();
  });

  it("a lock reaches the window by the core's event, without waiting for the poll (D-060)", async () => {
    let unlocked = true;
    mockIPC(
      (cmd) => {
        if (cmd === "app_status") return status({ unlocked });
        if (cmd === "list_cases") return [];
      if (cmd === "ping") return { ipc_version: 1, core_version: "0.1.0", build_commit: "t", fips_active: false, platform: "linux" };
        if (cmd === "ping") return { ipc_version: 1, core_version: "0.1.0", build_commit: "t", fips_active: false, platform: "linux" };
        return null;
      },
      { shouldMockEvents: true },
    );
    render(<App />);
    expect(await screen.findByRole("heading", { name: "התיק הראשון" })).toBeInTheDocument();
    unlocked = false;
    await act(async () => {
      await emit("core", { kind: "locked" });
    });
    expect(await screen.findByLabelText("סיסמה")).toBeInTheDocument();
  });

  it("when events cannot reach the window, a lock still shows within 15 seconds (D-060)", async () => {
    vi.useFakeTimers({ shouldAdvanceTime: true });
    let unlocked = true;
    mockIPC((cmd) => {
      if (cmd === "plugin:event|listen") throw new Error("event.listen not allowed");
      if (cmd === "app_status") return status({ unlocked });
      if (cmd === "list_cases") return [];
      if (cmd === "ping") return { ipc_version: 1, core_version: "0.1.0", build_commit: "t", fips_active: false, platform: "linux" };
      return null;
    });
    render(<App />);
    expect(await screen.findByRole("heading", { name: "התיק הראשון" })).toBeInTheDocument();
    unlocked = false;
    await act(async () => {
      await vi.advanceTimersByTimeAsync(15_500);
    });
    expect(await screen.findByLabelText("סיסמה")).toBeInTheDocument();
    vi.useRealTimers();
  });
});
