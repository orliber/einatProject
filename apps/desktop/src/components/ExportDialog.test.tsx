// The export dialog's versions (round 55, D-095): the full report, or a version for the
// parents with no numbers unless she turns them on. All names and data are invented.
import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { mockIPC } from "@tauri-apps/api/mocks";
import { AppContext, type AppApi } from "../App";
import type { CaseDetail, ExportCheck, ReportSettings } from "../ipc/client";
import type { CaseApi } from "../screens/CaseScreen";
import { status } from "../test/fixtures";
import { ExportDialog, parentsFileName } from "./ExportDialog";

function app(): AppApi {
  return { status: status(), go: vi.fn(), refresh: vi.fn(async () => undefined), notify: vi.fn(), fail: (e) => e.message, lockNow: vi.fn(async () => undefined) };
}

function caseApi(): CaseApi {
  const detail = {
    meta: { code: "TEST-0002" },
    identities: [{ role: "child", value: "אלון בדוי" }],
    sections: [{ key: "referral", title: "סיבת הפניה", approved: true }],
  } as unknown as CaseDetail;
  return {
    caseId: "c1", detail, jobs: [],
    reload: vi.fn(async () => undefined),
    review: vi.fn(async () => true),
    sort: vi.fn(async () => undefined),
    track: (_job, run) => run(),
    draft: vi.fn(async () => true),
  };
}

const check: ExportCheck = {
  blocking: [], to_complete: [], empty_sections: [], included_sections: 3, score_tables: 1,
  file_name: "דוח אבחון TEST-0002.docx", parents_left_out: 2, parents_chart_rows: 4,
};

const settings: ReportSettings = {
  title: "דוח אבחון פסיכולוגי", font: "David", confidentiality: "חסוי", signature: [], supervisor: [],
  parents_opening: "להורים שלום, זה הסיכום בשפה פשוטה.",
};

function setup() {
  const calls: { cmd: string; args: Record<string, unknown> }[] = [];
  mockIPC((cmd, args) => {
    calls.push({ cmd, args: args as Record<string, unknown> });
    if (cmd === "check_export") return check;
    if (cmd === "report_settings") return settings;
    if (cmd === "export_report" || cmd === "export_pdf") return "/בדוי/הורדות/קובץ";
    return null;
  });
  render(<AppContext.Provider value={app()}><ExportDialog api={caseApi()} onClose={vi.fn()} /></AppContext.Provider>);
  return { calls, user: userEvent.setup() };
}

describe("ExportDialog", () => {
  it("exports the full report unless she picks the parents' version", async () => {
    const { calls, user } = setup();
    await user.click(await screen.findByRole("button", { name: "הפקת הדוח" }));
    await screen.findByText("הדוח מוכן");
    expect(calls.find((c) => c.cmd === "export_report")?.args).toMatchObject({ caseId: "c1", parents: null });
  });

  it("the parents' version goes without numbers by default, with her opening sentence and its own file name", async () => {
    const { calls, user } = setup();
    await user.click(await screen.findByRole("radio", { name: "גרסה להורים" }));
    expect(await screen.findByText("להורים שלום, זה הסיכום בשפה פשוטה.")).toBeInTheDocument();
    expect(screen.getByRole("switch", { name: "להציג ציונים בגרסה להורים" })).toHaveAttribute("aria-checked", "false");
    expect(screen.getByText(/חזק \/ בטווח הגיל \/ קשה יותר/, { selector: "li span" })).toBeInTheDocument();
    // What the version leaves out is said, not done in silence.
    expect(screen.getByText(/2 משפטים הושמטו כי היה בהם מספר שנראה כמו ציון/)).toBeInTheDocument();
    expect(screen.getByText(/דוח אבחון TEST-0002 - להורים\.docx/)).toBeInTheDocument();
    expect(screen.queryByText(/טבלת הציונים תצורף כנספח/)).toBeNull();
    await user.click(screen.getByRole("button", { name: "הפקת הגרסה להורים" }));
    await screen.findByText("הגרסה להורים מוכנה");
    expect(calls.find((c) => c.cmd === "export_report")?.args).toMatchObject({ parents: { show_numbers: false } });
  });

  it("numbers go only when she turns them on, also in the PDF", async () => {
    const { calls, user } = setup();
    await user.click(await screen.findByRole("radio", { name: "גרסה להורים" }));
    await user.click(screen.getByRole("switch", { name: "להציג ציונים בגרסה להורים" }));
    expect(screen.queryByText(/משפטים הושמטו/)).toBeNull();
    await user.click(screen.getByRole("radio", { name: /PDF/ }));
    await user.click(screen.getByRole("button", { name: "הפקת הגרסה להורים" }));
    await screen.findByText("הגרסה להורים מוכנה");
    expect(calls.find((c) => c.cmd === "export_pdf")?.args).toMatchObject({ parents: { show_numbers: true } });
  });

  it("names the parents' file as the core does", () => {
    expect(parentsFileName("דוח אבחון TEST-0002.docx")).toBe("דוח אבחון TEST-0002 - להורים.docx");
    expect(parentsFileName("דוח")).toBe("דוח - להורים");
  });
});
