import { act, fireEvent, render, screen } from "@testing-library/react";
import { mockIPC } from "@tauri-apps/api/mocks";
import { LockScreen } from "./LockScreen";

describe("LockScreen", () => {
  it("renders the Hebrew lock screen with an accessible password field", () => {
    mockIPC(() => ({ ipc_version: 1, core_version: "0.1.0", build_commit: "test", fips_active: false, platform: "linux" }));
    render(<LockScreen />);
    expect(screen.getByRole("heading", { name: "כספת האבחון" })).toBeInTheDocument();
    expect(screen.getByLabelText("סיסמה")).toHaveAttribute("type", "password");
    expect(screen.getByRole("button", { name: "פתיחה" })).toBeInTheDocument();
  });

  it("shows that the secure core is connected after ping succeeds", async () => {
    mockIPC((cmd) => {
      if (cmd === "ping") {
        return { ipc_version: 1, core_version: "0.1.0", build_commit: "test", fips_active: false, platform: "linux" };
      }
      throw new Error(`unexpected command ${cmd}`);
    });
    render(<LockScreen />);
    expect(await screen.findByText(/התוכנה פועלת כראוי/)).toBeInTheDocument();
  });

  it("shows a clear error when the core does not answer", async () => {
    mockIPC(() => {
      throw new Error("down");
    });
    render(<LockScreen />);
    expect(await screen.findByText(/התוכנה לא מגיבה/)).toBeInTheDocument();
  });
});

describe("too many wrong tries", () => {
  it("counts the wait down and gives the button back by itself", async () => {
    vi.useFakeTimers({ toFake: ["setInterval", "clearInterval", "Date"] });
    mockIPC((cmd) => {
      if (cmd === "unlock") throw { code: "backoff", message: "יותר מדי ניסיונות. אפשר לנסות שוב בעוד 3 שניות.", details: [] };
      if (cmd === "ping") return { ipc_version: 1, core_version: "0.1.0", build_commit: "test", fips_active: false, platform: "linux" };
      if (cmd === "google_status") return { on: false };
      return null;
    });
    render(<LockScreen />);
    fireEvent.change(screen.getByLabelText("סיסמה"), { target: { value: "x" } });
    fireEvent.click(screen.getByRole("button", { name: "פתיחה" }));
    expect(await screen.findByText(/בעוד 3 שניות/)).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "פתיחה" })).toBeDisabled();
    await act(async () => {}); // the countdown's timer starts after the render
    await act(async () => { vi.advanceTimersByTime(1100); });
    expect(screen.getByText(/בעוד 2 שניות/)).toBeInTheDocument();
    await act(async () => { vi.advanceTimersByTime(2500); });
    expect(screen.queryByText(/ניסיונות שגויים/)).toBeNull();
    expect(screen.getByRole("button", { name: "פתיחה" })).toBeEnabled();
    vi.useRealTimers();
  });
});

describe("a slow Windows at the opening", () => {
  it("says it is waiting for Windows after a few seconds, then says to try again", async () => {
    vi.useFakeTimers({ toFake: ["setTimeout", "clearTimeout"] });
    let answer: (v: unknown) => void = () => undefined;
    mockIPC((cmd) => {
      if (cmd === "unlock") return new Promise((_, no) => { answer = no; });
      if (cmd === "ping") return { ipc_version: 1, core_version: "0.1.0", build_commit: "test", fips_active: false, platform: "linux" };
      if (cmd === "google_status") return { on: false };
      return null;
    });
    render(<LockScreen />);
    fireEvent.change(screen.getByLabelText("סיסמה"), { target: { value: "x" } });
    fireEvent.click(screen.getByRole("button", { name: "פתיחה" }));
    await act(async () => {});
    expect(screen.queryByText(/ממתינה ל-Windows/)).toBeNull();
    await act(async () => { vi.advanceTimersByTime(5100); });
    expect(screen.getByText(/ממתינה ל-Windows/)).toBeInTheDocument();
    await act(async () => { answer({ code: "busy", message: "Windows לא ענה בזמן. אפשר לנסות שוב בעוד רגע.", details: [] }); });
    expect(screen.getByText(/אפשר לנסות שוב בעוד רגע/)).toBeInTheDocument();
    expect(screen.queryByText(/ממתינה ל-Windows/)).toBeNull();
    expect(screen.getByLabelText("סיסמה")).toHaveAttribute("type", "password");
    vi.useRealTimers();
  });
});

describe("a version that cannot open the vault (R7)", () => {
  it("says the vault is safe and opens the page of versions; a wrong password does not", async () => {
    const calls: string[] = [];
    let code = "vault";
    mockIPC((cmd) => {
      calls.push(cmd);
      if (cmd === "unlock") throw { code, message: "תקלה טכנית.", details: [] };
      if (cmd === "ping") return { ipc_version: 1, core_version: "0.6.0", build_commit: "test", fips_active: false, platform: "linux" };
      if (cmd === "google_status") return { on: false };
      return null;
    });
    render(<LockScreen />);
    fireEvent.change(screen.getByLabelText("סיסמה"), { target: { value: "x" } });
    fireEvent.click(screen.getByRole("button", { name: "פתיחה" }));
    expect(await screen.findByText(/הכספת נשארת במחשב ולא נמחקת/)).toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: /לדף הגרסאות/ }));
    expect(calls).toContain("open_releases_page");
    code = "wrong_secret";
    fireEvent.change(screen.getByLabelText("סיסמה"), { target: { value: "y" } });
    fireEvent.click(screen.getByRole("button", { name: "פתיחה" }));
    await act(async () => {});
    expect(screen.queryByText(/הכספת נשארת במחשב/)).toBeNull();
  });
});
