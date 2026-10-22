import { fireEvent, render, screen, within } from "@testing-library/react";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { mockIPC } from "@tauri-apps/api/mocks";
import type { ReactNode } from "react";
import { AppContext, type AppApi } from "../App";
import { status } from "../test/fixtures";
import { ReadinessSettings } from "./ReadinessSettings";
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

describe("ready for real cases", () => {
  it("says what is missing, and a confirmation goes to the core", async () => {
    const calls: [string, unknown][] = [];
    const item = (key: string, done: boolean, auto = false) => ({ key, done, checked_by_program: auto, confirmed_at: null });
    mockIPC((cmd, args) => {
      calls.push([cmd, args]);
      const zdr = cmd === "confirm_readiness";
      return {
        items: [item("zdr", zdr), item("consent_form", false), item("backup", false, true), item("disk", true, true)],
        all_done: false,
      };
    });
    render(wrap(<ReadinessSettings />));
    expect(await screen.findByText(/חסרים 3 פריטים/)).toBeInTheDocument();
    fireEvent.click(screen.getByLabelText(/ZDR/));
    await vi.waitFor(() => expect(calls.some(([c]) => c === "confirm_readiness")).toBe(true));
    expect(calls.find(([c]) => c === "confirm_readiness")?.[1]).toMatchObject({ key: "zdr", done: true });
    expect(await screen.findByText(/חסרים 2 פריטים/)).toBeInTheDocument();
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
    expect(await screen.findByText(/טבלאות המבחנים שבתיקים הפתוחים אושרו/)).toBeInTheDocument();
    expect(screen.queryByRole("checkbox")).toBeNull();
    fireEvent.click(screen.getByRole("button", { name: "לתיקון" }));
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
