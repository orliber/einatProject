import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { mockIPC } from "@tauri-apps/api/mocks";
import { AppContext, type AppApi } from "../App";
import { status } from "../test/fixtures";
import { DsmSeverityPanel } from "./DsmSeverity";

function api(): AppApi {
  return { status: status(), go: vi.fn(), refresh: vi.fn(async () => undefined), notify: vi.fn(), fail: (e) => e.message, lockNow: vi.fn(async () => undefined) };
}

describe("DsmSeverityPanel", () => {
  it("keeps the level she picks for each domain, and none is the default", async () => {
    const calls: [string, unknown][] = [];
    mockIPC((cmd, args) => {
      calls.push([cmd, args]);
      return null;
    });
    const saved = vi.fn(async () => undefined);
    const user = userEvent.setup();
    const { rerender } = render(
      <AppContext.Provider value={api()}>
        <DsmSeverityPanel caseId="c1" onSaved={saved} />
      </AppContext.Provider>,
    );
    const social = screen.getByLabelText("תקשורת חברתית ואינטראקציה חברתית");
    expect(social).toHaveValue("");
    await user.selectOptions(social, "2");
    expect(calls).toContainEqual(["set_dsm_severity", { caseId: "c1", social: 2, restricted: null }]);
    expect(saved).toHaveBeenCalledTimes(1);

    rerender(
      <AppContext.Provider value={api()}>
        <DsmSeverityPanel caseId="c1" severity={{ social: 2, restricted: null }} onSaved={saved} />
      </AppContext.Provider>,
    );
    await user.selectOptions(screen.getByLabelText("דפוסי התנהגות, תחומי עניין ופעילויות מוגבלים וחוזרים"), "1");
    expect(calls).toContainEqual(["set_dsm_severity", { caseId: "c1", social: 2, restricted: 1 }]);
    expect(screen.getAllByRole("option", { name: "דרגה 3: נדרשת תמיכה משמעותית מאוד" })).toHaveLength(2);
  });
});
