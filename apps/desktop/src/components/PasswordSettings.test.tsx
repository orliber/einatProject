import { fireEvent, render, screen } from "@testing-library/react";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { mockIPC } from "@tauri-apps/api/mocks";
import { AppContext, type AppApi } from "../App";
import { status } from "../test/fixtures";
import { PasswordSettings } from "./PasswordSettings";

const api = (over: Partial<AppApi> = {}): AppApi => ({
  status: status(),
  go: vi.fn(),
  refresh: vi.fn(async () => undefined),
  notify: vi.fn(),
  fail: (e) => e.message,
  lockNow: vi.fn(async () => undefined),
  ...over,
});

function show(a: AppApi = api()) {
  const qc = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  return render(
    <QueryClientProvider client={qc}>
      <AppContext.Provider value={a}>
        <PasswordSettings />
      </AppContext.Provider>
    </QueryClientProvider>,
  );
}

describe("password settings: the printed kit is a folded last resort (D-103)", () => {
  it("shows only the password up front; the kit waits under 'מתקדם'", () => {
    mockIPC(() => null);
    const { container } = show();
    expect(screen.getByRole("heading", { name: "סיסמה" })).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "החלפת סיסמה…" })).toBeVisible();
    const advanced = container.querySelector("details.advanced");
    expect(advanced).not.toBeNull();
    expect(advanced).not.toHaveAttribute("open");
    expect(advanced?.querySelector("summary")).toHaveTextContent("מתקדם");
  });

  it("the password dialog offers no kit unless the password was refused for this computer", () => {
    mockIPC(() => null);
    show();
    fireEvent.click(screen.getByRole("button", { name: "החלפת סיסמה…" }));
    expect(screen.getByLabelText("הסיסמה הנוכחית")).toBeInTheDocument();
    expect(screen.queryByText(/ערכת השחזור/)).toBeNull();
  });

  it("retiring the kit asks for the password, and nothing secret comes back", async () => {
    const calls: [string, unknown][] = [];
    mockIPC((cmd, args) => {
      calls.push([cmd, args]);
      return null;
    });
    const notify = vi.fn();
    show(api({ notify }));
    fireEvent.click(screen.getByRole("button", { name: "ביטול הערכה הקיימת…" }));
    fireEvent.change(screen.getByLabelText("הסיסמה הנוכחית"), { target: { value: "סיסמה בדויה לבדיקה" } });
    fireEvent.click(screen.getByRole("button", { name: "ביטול הערכה" }));
    await vi.waitFor(() => expect(notify).toHaveBeenCalledWith("ערכת השחזור כבר לא פותחת את הכספת. כדאי לגבות עכשיו."));
    expect(calls.find(([c]) => c === "retire_recovery_kit")?.[1]).toMatchObject({ current: "סיסמה בדויה לבדיקה", withRecovery: false });
    expect(calls.some(([c]) => c === "new_recovery_kit")).toBe(false);
  });
});
