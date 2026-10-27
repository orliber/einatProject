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

  it("with Google on, goes to the Google sign-in; the kit is only a small link (D-103)", async () => {
    core({ available: true, on: true });
    render(<LockScreen />);
    await act(async () => {}); // the Google status arrives
    fireEvent.click(screen.getByRole("button", { name: "שכחתי את הסיסמה" }));
    expect(screen.getByLabelText("סיסמה חדשה")).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "כניסה עם גוגל" })).toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "יש לי ערכת שחזור מודפסת" }));
    expect(screen.getByLabelText("ערכת השחזור")).toBeInTheDocument();
  });

  it("with Google off, says so plainly, where to turn it on, and offers no kit button", async () => {
    core({ available: true, on: false });
    const { container } = render(<LockScreen />);
    await act(async () => {});
    fireEvent.click(screen.getByRole("button", { name: "שכחתי את הסיסמה" }));
    expect(screen.queryByLabelText("סיסמה חדשה")).toBeNull();
    expect(screen.getByText(/הכניסה עם גוגל לא מופעלת במחשב הזה/)).toBeInTheDocument();
    expect(screen.getByText(/בהגדרות ← סיסמה ואבטחה/)).toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "פתיחה עם ערכת השחזור" })).toBeNull();
    expect(await a11yViolations(container)).toEqual([]);
    fireEvent.click(screen.getByRole("button", { name: "יש לי ערכת שחזור מודפסת" }));
    expect(screen.getByLabelText("ערכת השחזור")).toHaveAttribute("type", "text");
    fireEvent.click(screen.getByRole("button", { name: "חזרה לפתיחה עם סיסמה" }));
    expect(screen.getByLabelText("סיסמה")).toHaveAttribute("type", "password");
  });

  it("where this version has no Google sign-in, does not offer it", async () => {
    core({ available: false, on: false });
    render(<LockScreen />);
    await act(async () => {});
    fireEvent.click(screen.getByRole("button", { name: "שכחתי את הסיסמה" }));
    expect(screen.getByText(/אי אפשר לבחור סיסמה חדשה מכאן/)).toBeInTheDocument();
    expect(screen.queryByText(/גוגל/)).toBeNull();
  });

  it("another computer: says what opens it, stays on the password, and keeps the kit as a small link", async () => {
    mockIPC((cmd) => {
      if (cmd === "ping") return ping;
      if (cmd === "google_status") return { available: true, on: false, needs_setup_here: false };
      if (cmd === "unlock") throw { code: "other_computer", message: "הסיסמה פותחת את הכספת רק במחשב ובחשבון המשתמש שבהם היא נקבעה.", details: [] };
      return null;
    });
    render(<LockScreen />);
    await act(async () => {});
    expect(screen.queryByRole("button", { name: "יש לי ערכת שחזור מודפסת" })).toBeNull();
    fireEvent.change(screen.getByLabelText("סיסמה"), { target: { value: "סיסמה בדויה לבדיקה" } });
    fireEvent.click(screen.getByRole("button", { name: "פתיחה" }));
    expect(await screen.findByText(/רק במחשב ובחשבון המשתמש/)).toBeInTheDocument();
    expect(screen.getByLabelText("סיסמה")).toHaveAttribute("type", "password");
    expect(screen.getByRole("button", { name: "יש לי ערכת שחזור מודפסת" })).toBeInTheDocument();
  });
});
