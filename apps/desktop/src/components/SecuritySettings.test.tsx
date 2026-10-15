import { fireEvent, render, screen } from "@testing-library/react";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { mockIPC } from "@tauri-apps/api/mocks";
import { AppContext, type AppApi } from "../App";
import { status } from "../test/fixtures";
import { SecuritySettings } from "./SecuritySettings";

const api = (over: Partial<AppApi> = {}): AppApi => ({
  status: status(),
  go: vi.fn(),
  refresh: vi.fn(async () => undefined),
  notify: vi.fn(),
  fail: (e) => e.message,
  lockNow: vi.fn(async () => undefined),
  ...over,
});

const layer = (key: string, title: string, state: string, now: string) => ({
  key, title, state, now, what: `${title} עושה כך.`, against: "מי שמנסה לקרוא.",
});

const view = (stopped: boolean) => ({
  items: [],
  send_stopped: stopped,
  daily_limit: 400,
  max_request_kb: 2048,
  layers: [
    layer("encryption", "הצפנת הכספת", "ok", "הכספת מוצפנת."),
    layer("privacy_gate", "הסתרת פרטים לפני שליחה", "ok", "נבדק עכשיו: השער חסם שם גלוי של ילד בדוי."),
    layer("send_rules", "שליחה רק לפי הכללים", stopped ? "info" : "ok", stopped ? "השליחה כבויה עכשיו (עצירת חירום)." : "נבדק עכשיו."),
  ],
  yours: [layer("disk", "הצפנת הדיסק (BitLocker)", "warn", "לא נמצאה. כדאי להפעיל אותה ב-Windows.")],
});

function show(a: AppApi = api()) {
  const qc = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  render(
    <QueryClientProvider client={qc}>
      <AppContext.Provider value={a}>
        <SecuritySettings />
      </AppContext.Provider>
    </QueryClientProvider>,
  );
}

describe("the protection layers", () => {
  it("shows each layer numbered, what was found now, and what it protects against", async () => {
    mockIPC(() => view(false));
    show();
    expect(await screen.findByText("1. הצפנת הכספת")).toBeInTheDocument();
    expect(screen.getByText("נבדק עכשיו: השער חסם שם גלוי של ילד בדוי.")).toBeInTheDocument();
    expect(screen.getAllByText("מגינה מפני: מי שמנסה לקרוא.")).toHaveLength(4);
    // What depends on her, with the missing protection shown plainly.
    expect(screen.getByText("מה שתלוי בך")).toBeInTheDocument();
    expect(screen.getByText("לבדוק")).toBeInTheDocument();
  });

  it("the emergency stop goes to the core, and the layer says sending is off", async () => {
    const calls: [string, unknown][] = [];
    mockIPC((cmd, args) => {
      calls.push([cmd, args]);
      return view(cmd === "set_send_stopped");
    });
    show();
    fireEvent.click(await screen.findByRole("button", { name: /עצירת חירום/ }));
    expect(await screen.findByRole("button", { name: /להפעיל שליחה מחדש/ })).toBeInTheDocument();
    expect(calls.find(([c]) => c === "set_send_stopped")?.[1]).toMatchObject({ stopped: true });
    expect(screen.getByText("השליחה כבויה עכשיו (עצירת חירום).")).toBeInTheDocument();
  });

  it("saves the same page as a PDF and says where", async () => {
    mockIPC((cmd) => (cmd === "export_security_pdf" ? "C:\\Downloads\\שכבות ההגנה.pdf" : view(false)));
    const notify = vi.fn();
    show(api({ notify }));
    fireEvent.click(await screen.findByRole("button", { name: "לשמור כ-PDF" }));
    await vi.waitFor(() => expect(notify).toHaveBeenCalledWith("נשמר: C:\\Downloads\\שכבות ההגנה.pdf"));
  });
});
