import { fireEvent, render, screen } from "@testing-library/react";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { mockIPC } from "@tauri-apps/api/mocks";
import type { ReactNode } from "react";
import { AppContext, type AppApi } from "../App";
import { status } from "../test/fixtures";
import { ReadinessSettings } from "./ReadinessSettings";
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
    expect(await screen.findByText(/חסרים עוד 3 פריטים/)).toBeInTheDocument();
    fireEvent.click(screen.getByLabelText(/ZDR/));
    await vi.waitFor(() => expect(calls.some(([c]) => c === "confirm_readiness")).toBe(true));
    expect(calls.find(([c]) => c === "confirm_readiness")?.[1]).toMatchObject({ key: "zdr", done: true });
    expect(await screen.findByText(/חסרים עוד 2 פריטים/)).toBeInTheDocument();
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
