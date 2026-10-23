import { act, fireEvent, render, screen } from "@testing-library/react";
import { mockIPC } from "@tauri-apps/api/mocks";
import { LockScreen } from "./LockScreen";
import { a11yViolations } from "../test/a11y";

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
    expect(screen.queryByText(/ממתינה למחשב/)).toBeNull();
    await act(async () => { vi.advanceTimersByTime(5100); });
    expect(screen.getByText(/ממתינה למחשב/)).toBeInTheDocument();
    await act(async () => { answer({ code: "busy", message: "המחשב לא ענה בזמן. אפשר לנסות שוב בעוד רגע.", details: [] }); });
    expect(screen.getByText(/אפשר לנסות שוב בעוד רגע/)).toBeInTheDocument();
    expect(screen.queryByText(/ממתינה למחשב/)).toBeNull();
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

describe("forgot the password (D-096)", () => {
  const ping = { ipc_version: 1, core_version: "0.1.0", build_commit: "test", fips_active: false, platform: "linux" };
  function core(google: { available: boolean; on: boolean }) {
    mockIPC((cmd) => {
      if (cmd === "ping") return ping;
      if (cmd === "google_status") return { ...google, needs_setup_here: false };
      return null;
    });
  }

  it("is one field, one green button, one short line, and the link is always there", async () => {
    core({ available: true, on: false });
    const { container } = render(<LockScreen />);
    expect(screen.getByText("מוצפן ונשמר במחשב הזה")).toBeInTheDocument();
    expect(container.querySelectorAll("input")).toHaveLength(1);
    expect(container.querySelectorAll(".btn-primary")).toHaveLength(1);
    expect(screen.getByRole("button", { name: "שכחתי את הסיסמה" })).toBeInTheDocument();
    expect(await screen.findByText(/התוכנה פועלת כראוי/)).toBeInTheDocument();
  });

  it("with Google on, goes to the Google sign-in", async () => {
    core({ available: true, on: true });
    render(<LockScreen />);
    await act(async () => {}); // the Google status arrives
    fireEvent.click(screen.getByRole("button", { name: "שכחתי את הסיסמה" }));
    expect(screen.getByLabelText("סיסמה חדשה")).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "כניסה עם גוגל" })).toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "יש לי את ערכת השחזור המודפסת" }));
    expect(screen.getByLabelText("ערכת השחזור")).toBeInTheDocument();
  });

  it("with Google off, says to use the kit now and where to turn Google on for next time", async () => {
    core({ available: true, on: false });
    const { container } = render(<LockScreen />);
    await act(async () => {});
    fireEvent.click(screen.getByRole("button", { name: "שכחתי את הסיסמה" }));
    expect(screen.queryByLabelText("סיסמה חדשה")).toBeNull();
    expect(screen.getByText(/פותחים עם ערכת השחזור המודפסת/)).toBeInTheDocument();
    expect(screen.getByText(/בהגדרות ← סיסמה ואבטחה/)).toBeInTheDocument();
    expect(await a11yViolations(container)).toEqual([]);
    fireEvent.click(screen.getByRole("button", { name: "פתיחה עם ערכת השחזור" }));
    expect(screen.getByLabelText("ערכת השחזור")).toHaveAttribute("type", "text");
    fireEvent.click(screen.getByRole("button", { name: "חזרה לפתיחה עם סיסמה" }));
    expect(screen.getByLabelText("סיסמה")).toHaveAttribute("type", "password");
  });

  it("where this version has no Google sign-in, does not offer it", async () => {
    core({ available: false, on: false });
    render(<LockScreen />);
    await act(async () => {});
    fireEvent.click(screen.getByRole("button", { name: "שכחתי את הסיסמה" }));
    expect(screen.getByText(/פותחים עם ערכת השחזור המודפסת/)).toBeInTheDocument();
    expect(screen.queryByText(/גוגל/)).toBeNull();
  });
});
