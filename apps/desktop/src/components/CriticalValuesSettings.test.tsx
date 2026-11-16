import { fireEvent, render, screen } from "@testing-library/react";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { mockIPC } from "@tauri-apps/api/mocks";
import type { ReactNode } from "react";
import { AppContext, type AppApi } from "../App";
import { status } from "../test/fixtures";
import { instruments } from "../web/scores";
import { CriticalValuesSettings, parseCritical } from "./CriticalValuesSettings";

function wrap(children: ReactNode) {
  const qc = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  const api: AppApi = { status: status(), go: vi.fn(), refresh: vi.fn(async () => undefined), notify: vi.fn(), fail: (e) => e.message, lockNow: vi.fn(async () => undefined) };
  return (
    <QueryClientProvider client={qc}>
      <AppContext.Provider value={api}>{children}</AppContext.Provider>
    </QueryClientProvider>
  );
}

const sets = [{ key: "wppsi_iii_2_3", instrument: "wppsi_iii", title_he: "WPPSI-III, גילאי 2:6–3:11", measures: ["receptive_vocabulary", "information", "block_design", "object_assembly"] }];

describe("critical values from the manual (D-190)", () => {
  it("reads a typed value, and refuses what is not one", () => {
    expect(parseCritical("")).toBeNull();
    expect(parseCritical(" 2.5 ")).toBe(2.5);
    expect(parseCritical("3,12")).toBe(3.12);
    expect(parseCritical("3.125")).toBeNaN();
    expect(parseCritical("abc")).toBeNaN();
  });

  it("saves the values of one set, and says how many are entered (invented values)", async () => {
    const calls: [string, unknown][] = [];
    let values: { set: string; measure: string; value: number }[] = [];
    mockIPC((cmd, args) => {
      calls.push([cmd, args]);
      if (cmd === "instrument_approvals") return instruments.map((instrument) => ({ instrument, status: "awaiting", open_cases: 0 }));
      if (cmd === "set_critical_values") values = (args as { values: typeof values }).values;
      return { sets, values };
    });
    render(wrap(<CriticalValuesSettings />));
    expect(await screen.findByText(/לא הוזנו ערכים/)).toBeInTheDocument();
    fireEvent.click(screen.getByText("WPPSI-III, גילאי 2:6–3:11"));
    fireEvent.change(await screen.findByLabelText(/מידע \(IN\)/), { target: { value: "2.5" } });
    fireEvent.change(screen.getByLabelText(/קוביות \(BD\)/), { target: { value: "12" } });
    fireEvent.click(screen.getByRole("button", { name: "שמירה" }));
    expect(await screen.findByRole("alert")).toHaveTextContent(/בין 0 ל-9/);
    expect(calls.some(([c]) => c === "set_critical_values")).toBe(false);
    fireEvent.change(screen.getByLabelText(/קוביות \(BD\)/), { target: { value: "2.75" } });
    fireEvent.click(screen.getByRole("button", { name: "שמירה" }));
    await vi.waitFor(() => expect(calls.find(([c]) => c === "set_critical_values")?.[1]).toEqual({
      values: [{ set: "wppsi_iii_2_3", measure: "information", value: 2.5 }, { set: "wppsi_iii_2_3", measure: "block_design", value: 2.75 }],
    }));
    expect(await screen.findByText(/הוזנו 2 מתוך 4/)).toBeInTheDocument();
  });
});
