import { act, fireEvent, render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { mockIPC } from "@tauri-apps/api/mocks";
import { App } from "./App";
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
});
