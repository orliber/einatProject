import { fireEvent, render, screen, within } from "@testing-library/react";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { mockIPC } from "@tauri-apps/api/mocks";
import type { ReactNode } from "react";
import { AppContext, type AppApi } from "../App";
import { status } from "../test/fixtures";
import { ReadinessSettings } from "./ReadinessSettings";
import { SafetyNet } from "./SafetyNet";
import { InstrumentSettings } from "./InstrumentSettings";
import { instruments } from "../web/scores";
import { UsageSettings } from "./UsageSettings";
import { IdleWarning } from "./IdleWarning";

const api = (): AppApi => ({
  status: status(),
  go: vi.fn(),
  refresh: vi.fn(async () => undefined),
  notify: vi.fn(),
  fail: (e) => e.message,
  lockNow: vi.fn(async () => undefined),
});

function wrap(children: ReactNode) {
  const qc = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  return (
    <QueryClientProvider client={qc}>
      <AppContext.Provider value={api()}>{children}</AppContext.Provider>
    </QueryClientProvider>
  );
}

describe("ready for real cases (D-108)", () => {
  it("counts what is ready, and a confirmation goes to the core", async () => {
    const calls: [string, unknown][] = [];
    const item = (key: string, done: boolean, auto = false) => ({ key, done, checked_by_program: auto, confirmed_at: done && !auto ? 1_790_000_000 : null });
    let zdr = false;
    mockIPC((cmd, args) => {
      calls.push([cmd, args]);
      if (cmd === "confirm_readiness") zdr = (args as { done: boolean }).done;
      return {
        items: [item("zdr", zdr), item("consent_form", false), item("backup", false, true), item("disk", true, true)],
        all_done: false,
      };
    });
    render(wrap(<ReadinessSettings />));
    expect(await screen.findByText("1 מתוך 4 מוכנים")).toBeInTheDocument();
    // One card per item, each with its own one line.
    const cards = screen.getAllByRole("listitem");
    expect(cards).toHaveLength(4);
    expect(within(cards[0] as HTMLElement).getByText(/נוסח הבקשה ל-Anthropic/)).toBeInTheDocument();
    // What the program checks has no button when it is fine, and a named way to fix it when not.
    expect(within(cards[3] as HTMLElement).queryByRole("button")).toBeNull();
    expect(within(cards[2] as HTMLElement).getByRole("button", { name: "לגיבוי" })).toBeInTheDocument();
    expect(screen.queryByRole("checkbox")).toBeNull();

    fireEvent.click(screen.getByRole("button", { name: /^אישרתי: .*ZDR/ }));
    await vi.waitFor(() => expect(calls.some(([c]) => c === "confirm_readiness")).toBe(true));
    expect(calls.find(([c]) => c === "confirm_readiness")?.[1]).toMatchObject({ key: "zdr", done: true });
    expect(await screen.findByText("2 מתוך 4 מוכנים")).toBeInTheDocument();
    expect(screen.getByText(/^אישרת ב-/)).toBeInTheDocument();

    // And taken back the same way.
    fireEvent.click(screen.getByRole("button", { name: /^ביטול האישור: .*ZDR/ }));
    expect(await screen.findByText("1 מתוך 4 מוכנים")).toBeInTheDocument();
    expect(calls.filter(([c]) => c === "confirm_readiness").at(-1)?.[1]).toMatchObject({ key: "zdr", done: false });
  });

  it("says so when everything is ready", async () => {
    mockIPC(() => ({ items: [{ key: "disk", done: true, checked_by_program: true, confirmed_at: null }], all_done: true }));
    render(wrap(<ReadinessSettings />));
    expect(await screen.findByText("1 מתוך 1 מוכנים")).toBeInTheDocument();
    expect(screen.getByText("הכל מוכן.")).toBeInTheDocument();
  });
});

describe("the safety net (D-107)", () => {
  const google = { available: true, on: false, needs_setup_here: false };
  const backup = { last_at: null, days_since: null, due: true, secret_changed: false, last_check_at: null, has_cases: true, auto: true };

  it("a missing part turns the row red, with its own button", async () => {
    const calls: string[] = [];
    let made = false;
    mockIPC((cmd) => {
      calls.push(cmd);
      if (cmd === "google_status") return google;
      if (cmd === "backup_status") return backup;
      if (cmd === "write_backup") { made = true; return { path: "E:\\גיבוי.vaultbak", bytes: 1, created_at: 1 }; }
      if (cmd === "safety_net") {
        return {
          google: { state: "missing", text: "כניסה עם גוגל כבויה", hint: "מפעילים אותה פעם אחת." },
          backup: made
            ? { state: "ok", text: "גיבוי מהיום", hint: null }
            : { state: "missing", text: "אין עדיין גיבוי", hint: "גיבוי מוצפן על דיסק נפרד." },
          ok: false,
        };
      }
      return null;
    });
    render(wrap(<SafetyNet />));
    const net = await screen.findByRole("region", { name: "רשת הביטחון" });
    expect(net).toHaveClass("missing");
    expect(net).toHaveTextContent("אם תשכחי את הסיסמה: כניסה עם גוגל כבויה! (חסר) · אין עדיין גיבוי! (חסר)");
    expect(within(net).getByRole("button", { name: "להפעיל כניסה עם גוגל" })).toBeInTheDocument();
    fireEvent.click(within(net).getByRole("button", { name: "לגבות עכשיו" }));
    // The backup refreshes its status, and the row is made again from it.
    expect(await within(net).findByText("גיבוי מהיום")).toBeInTheDocument();
    expect(within(net).queryByRole("button", { name: "לגבות עכשיו" })).toBeNull();
    expect(calls).toContain("write_backup");
  });

  it("all there: one green line, no buttons", async () => {
    mockIPC((cmd) => {
      if (cmd === "google_status") return { ...google, on: true };
      if (cmd === "backup_status") return { ...backup, days_since: 2, due: false };
      return { google: { state: "ok", text: "כניסה עם גוגל", hint: null }, backup: { state: "ok", text: "גיבוי מלפני יומיים", hint: null }, ok: true };
    });
    render(wrap(<SafetyNet />));
    const net = await screen.findByRole("region", { name: "רשת הביטחון" });
    expect(net).toHaveClass("ok");
    expect(net).toHaveTextContent("אם תשכחי את הסיסמה: כניסה עם גוגל✓ (תקין) · גיבוי מלפני יומיים✓ (תקין)");
    expect(within(net).queryByRole("button")).toBeNull();
  });
});

describe("her approval of the score tables (D-098)", () => {
  it("the readiness line is the program's, with a way to the tests pane", async () => {
    const go = vi.fn();
    mockIPC(() => ({ items: [{ key: "score_tables", done: false, checked_by_program: true, confirmed_at: null }], all_done: false }));
    const qc = new QueryClient({ defaultOptions: { queries: { retry: false } } });
    render(
      <QueryClientProvider client={qc}>
        <AppContext.Provider value={{ ...api(), go }}>
          <ReadinessSettings />
        </AppContext.Provider>
      </QueryClientProvider>,
    );
    expect(await screen.findByText(/ענית על הטבלאות של כל אחד מהמבחנים/)).toBeInTheDocument();
    expect(screen.queryByRole("checkbox")).toBeNull();
    fireEvent.click(screen.getByRole("button", { name: "למבחנים" }));
    expect(go).toHaveBeenCalledWith({ name: "settings", pane: "tests" });
  });

  it("each instrument gets one of her two answers, and the core gets it", async () => {
    const calls: [string, unknown][] = [];
    let answers: Record<string, string> = {};
    const list = () =>
      instruments.map((instrument) => ({ instrument, status: answers[instrument.key] ?? "awaiting", open_cases: instrument.key === "brief_p" ? 1 : 0 }));
    mockIPC((cmd, args) => {
      calls.push([cmd, args]);
      if (cmd === "set_instrument_status") {
        const a = args as { key: string; status: string };
        answers = { ...answers, [a.key]: a.status };
      }
      return list();
    });
    render(wrap(<InstrumentSettings />));
    expect(await screen.findByText(/מבחן אחד בתיק פתוח מחכה/)).toBeInTheDocument();
    // What she approves is on the screen: each instrument's ages, measures and table.
    expect(screen.getAllByText("הטבלה שבתוכנה")).toHaveLength(instruments.length);
    expect(screen.getByText(/\(INH\)/)).toBeInTheDocument();
    const group = screen.getByRole("radiogroup", { name: /BRIEF-P: מחכה לאישור/ });
    fireEvent.click(within(group).getByRole("radio", { name: "לא מעבירה את זה" }));
    await vi.waitFor(() => expect(calls.find(([c]) => c === "set_instrument_status")?.[1]).toEqual({ key: "brief_p", status: "not_used" }));
    expect(await screen.findByRole("radiogroup", { name: /BRIEF-P: לא מעבירה את זה/ })).toBeInTheDocument();
    expect(screen.queryByText(/מחכה לאישור שלך|מחכים לאישור שלך/)).toBeNull();
  });
});

describe("AI use and ceiling", () => {
  it("shows the month and stops at the ceiling", async () => {
    mockIPC(() => ({ month: "2026-10", requests: 12, input_tokens: 1, output_tokens: 1, estimated_cents: 2050, cap_usd: 20, unpriced_models: [] }));
    render(wrap(<UsageSettings />));
    expect(await screen.findByText(/12 בקשות, עלות משוערת \$20\.50/)).toBeInTheDocument();
    expect(screen.getByText(/השליחה עצורה/)).toBeInTheDocument();
  });
});

describe("idle warning", () => {
  it("keeps working with one click", () => {
    const keep = vi.fn();
    render(<IdleWarning seconds={50} onKeep={keep} />);
    expect(screen.getByRole("alert")).toHaveTextContent("50 שניות");
    fireEvent.click(screen.getByRole("button", { name: "להמשיך לעבוד" }));
    expect(keep).toHaveBeenCalled();
  });
});
