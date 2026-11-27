// The export dialog's versions (round 55, D-095): the full report, or a version for the
// parents with no numbers unless she turns them on. All names and data are invented.
import { render, renderHook, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { mockIPC } from "@tauri-apps/api/mocks";
import { AppContext, type AppApi } from "../App";
import type { CaseDetail, ExportCheck, ReportSettings } from "../ipc/client";
import type { CaseApi } from "../screens/CaseScreen";
import { status } from "../test/fixtures";
import { ExportDialog, parentsFileName, shownFileName } from "./ExportDialog";
import { forgetHold, useHoldUnsaved } from "./Unsaved";
import { clearKept, setKept } from "../kept";

/** The file name as said in the dialog. */
const fileName = () => document.querySelector(".export-file-name")?.textContent;

function app(): AppApi {
  return { status: status(), go: vi.fn(), refresh: vi.fn(async () => undefined), notify: vi.fn(), fail: (e) => e.message, lockNow: vi.fn(async () => undefined) };
}

function caseApi(sections: unknown[] = [{ key: "referral", title: "סיבת הפניה", approved: true }]): CaseApi {
  const detail = {
    meta: { code: "TEST-0002" },
    identities: [{ role: "child", value: "אלון בדוי" }],
    sections,
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
  file_name: "דוח אבחון TEST-0002.docx", parents_left_out: 2, parents_chart_rows: 4, polish_notes: 0,
};

const settings: ReportSettings = {
  title: "דוח אבחון פסיכולוגי", font: "David", confidentiality: "חסוי", signature: [], supervisor: [],
  parents_opening: "להורים שלום, זה הסיכום בשפה פשוטה.",
};

function setup(over: Partial<ExportCheck> = {}) {
  const calls: { cmd: string; args: Record<string, unknown> }[] = [];
  mockIPC((cmd, args) => {
    calls.push({ cmd, args: args as Record<string, unknown> });
    if (cmd === "check_export") return { ...check, ...over };
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

  it("a [חסר: …] mark left in the report is said, and the green line is not; no folder is promised", async () => {
    setup({ to_complete: ["רקע התפתחותי: [חסר: גיל ההליכה]"] });
    expect(await screen.findByText(/נשאר סימון אחד של \[חסר: …\] בדוח/)).toHaveTextContent("רקע התפתחותי: [חסר: גיל ההליכה]");
    expect(screen.queryByText(/ולא נשארו תפקידים/)).toBeNull();
    // Where the file goes is said after it is saved (it is not always the downloads folder).
    expect(screen.queryByText(/תיקיית ההורדות/)).toBeNull();
  });

  it("with nothing left to complete the green line stays", async () => {
    setup();
    expect(await screen.findByText(/ולא נשארו תפקידים/)).toBeInTheDocument();
  });

  it("the parents' version goes without numbers by default, with her opening sentence and its own file name", async () => {
    const { calls, user } = setup();
    await user.click(await screen.findByRole("radio", { name: "גרסה להורים" }));
    expect(await screen.findByText("להורים שלום, זה הסיכום בשפה פשוטה.")).toBeInTheDocument();
    expect(screen.getByRole("switch", { name: "להציג ציונים בגרסה להורים" })).toHaveAttribute("aria-checked", "false");
    expect(screen.getByText(/חזק \/ בטווח הגיל \/ קשה יותר/, { selector: "li span" })).toBeInTheDocument();
    // What the version leaves out is said, not done in silence.
    expect(screen.getByText(/2 משפטים הושמטו כי היה בהם מספר שנראה כמו ציון/)).toBeInTheDocument();
    expect(fileName()).toBe("דוח אבחון TEST-0002 - להורים.docx");
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
    // The name said is the one the file gets: the PDF's, not the Word's.
    expect(fileName()).toBe("דוח אבחון TEST-0002 - להורים.pdf");
    expect(screen.getByRole("dialog", { name: "הפקת דוח PDF" })).toBeInTheDocument();
    await user.click(screen.getByRole("button", { name: "הפקת הגרסה להורים" }));
    await screen.findByText("הגרסה להורים מוכנה");
    expect(calls.find((c) => c.cmd === "export_pdf")?.args).toMatchObject({ parents: { show_numbers: true } });
  });

  it("when the check fails, the spinner stops and 'try again' runs the check again (regression)", async () => {
    let fails = 1;
    const calls: string[] = [];
    mockIPC((cmd) => {
      calls.push(cmd);
      if (cmd === "check_export") {
        if (fails-- > 0) return Promise.reject({ code: "io", message: "הקובץ לא נפתח", details: [], action: { kind: "retry" } });
        return check;
      }
      if (cmd === "report_settings") return settings;
      return null;
    });
    const user = userEvent.setup();
    render(<AppContext.Provider value={app()}><ExportDialog api={caseApi()} onClose={vi.fn()} /></AppContext.Provider>);
    expect(await screen.findByText("הקובץ לא נפתח")).toBeInTheDocument();
    expect(screen.queryByText("בדיקת הדוח")).toBeNull();
    await user.click(screen.getByRole("button", { name: /לנסות שוב/ }));
    expect(await screen.findByText(/שם הקובץ:/)).toBeInTheDocument();
    expect(calls.filter((c) => c === "check_export")).toHaveLength(2);
  });

  it("a PDF is named as the core saves it: the Word name's stem, .pdf", () => {
    expect(shownFileName("דוח אבחון – תיק-1024.docx", false)).toBe("דוח אבחון – תיק-1024.docx");
    expect(shownFileName("דוח אבחון – תיק-1024.docx", true)).toBe("דוח אבחון – תיק-1024.pdf");
    expect(shownFileName("דוח", true)).toBe("דוח.pdf");
  });

  it("names the parents' file as the core does", () => {
    expect(parentsFileName("דוח אבחון TEST-0002.docx")).toBe("דוח אבחון TEST-0002 - להורים.docx");
    expect(parentsFileName("דוח")).toBe("דוח - להורים");
  });

  it("a paragraph edit left open and unsaved stops the file, and leads back to it", async () => {
    const hold = renderHook(() => useHoldUnsaved("c1", 'פסקה בסעיף "סיבת הפניה"', "טקסט בדוי עם תיקון שלא נשמר"));
    try {
      const go = vi.fn();
      mockIPC((cmd) => (cmd === "check_export" ? check : cmd === "report_settings" ? settings : null));
      const user = userEvent.setup();
      const onClose = vi.fn();
      render(<AppContext.Provider value={{ ...app(), go }}><ExportDialog api={caseApi()} onClose={onClose} /></AppContext.Provider>);
      expect(await screen.findByText(/יש עריכה שלא נשמרה בסעיף "סיבת הפניה"/)).toBeInTheDocument();
      expect(screen.getByRole("button", { name: "הפקת הדוח" })).toBeDisabled();
      expect(screen.queryByText(/ולא נשארו תפקידים/)).toBeInTheDocument();
      await user.click(screen.getByRole("button", { name: "חזרה לעריכה" }));
      expect(onClose).toHaveBeenCalled();
      // The report page, on that section: where the report's editor is kept and opens again
      // (the section's key alone would open the section's own page, without it).
      expect(go).toHaveBeenCalledWith({ name: "case", id: "c1", view: "report", section: "referral" });
    } finally {
      hold.unmount();
      forgetHold();
    }
  });

  it("an edit left open on the section's own page leads back to that page", async () => {
    const hold = renderHook(() => useHoldUnsaved("c1", 'פסקה בסעיף "סיבת הפניה"', "פסקה בדויה משלה שלא נשמרה"));
    setKept<string | null>("case:c1:section:referral:own", null)("פסקה בדויה משלה שלא נשמרה");
    try {
      const go = vi.fn();
      mockIPC((cmd) => (cmd === "check_export" ? check : cmd === "report_settings" ? settings : null));
      const user = userEvent.setup();
      render(<AppContext.Provider value={{ ...app(), go }}><ExportDialog api={caseApi()} onClose={vi.fn()} /></AppContext.Provider>);
      await user.click(await screen.findByRole("button", { name: "חזרה לעריכה" }));
      expect(go).toHaveBeenCalledWith({ name: "case", id: "c1", view: "referral" });
    } finally {
      hold.unmount();
      forgetHold();
      clearKept();
    }
  });

  it("UIC-8: while the file is made nothing can be changed or closed, and the title says what was made", async () => {
    let finish: (p: string) => void = () => undefined;
    mockIPC((cmd) => {
      if (cmd === "check_export") return check;
      if (cmd === "report_settings") return settings;
      if (cmd === "export_report") return new Promise<string>((r) => { finish = r; });
      return null;
    });
    const user = userEvent.setup();
    render(<AppContext.Provider value={app()}><ExportDialog api={caseApi()} onClose={vi.fn()} /></AppContext.Provider>);
    // One group for the file type: the arrows move between Word and PDF.
    const types = screen.getAllByRole("radio").filter((r) => r.getAttribute("name") === "file-type");
    expect(types).toHaveLength(2);
    await user.click(await screen.findByRole("button", { name: "הפקת הדוח" }));
    expect(screen.getByRole("radio", { name: "גרסה להורים" })).toBeDisabled();
    expect(screen.getByRole("radio", { name: /PDF/ })).toBeDisabled();
    expect(screen.getByRole("button", { name: "ביטול" })).toBeDisabled();
    expect(screen.queryByRole("button", { name: "סגירה" })).toBeNull();
    finish("/בדוי/הורדות/דוח");
    expect(await screen.findByText("הדוח מוכן")).toBeInTheDocument();
    expect(screen.queryByText("הגרסה להורים מוכנה")).toBeNull();
  });

  it("a PDF with [חסר: …] left in asks once before it is made", async () => {
    const { calls, user } = setup({ to_complete: ["רקע התפתחותי: [חסר: גיל ההליכה]"] });
    await user.click(await screen.findByRole("radio", { name: /PDF/ }));
    expect(screen.getByText(/ב-PDF אי אפשר להשלים אחר כך/)).toBeInTheDocument();
    await user.click(screen.getByRole("button", { name: "הפקת הדוח" }));
    expect(calls.some((c) => c.cmd === "export_pdf")).toBe(false);
    expect(screen.getByRole("alert")).toHaveTextContent("להפיק בכל זאת?");
    await user.click(screen.getByRole("button", { name: "כן, להפיק PDF עם הסימונים" }));
    await screen.findByText("הדוח מוכן");
    expect(calls.filter((c) => c.cmd === "export_pdf")).toHaveLength(1);
  });

  it("a Word file with [חסר: …] left in goes without the question", async () => {
    const { calls, user } = setup({ to_complete: ["רקע התפתחותי: [חסר: גיל ההליכה]"] });
    await user.click(await screen.findByRole("button", { name: "הפקת הדוח" }));
    await screen.findByText("הדוח מוכן");
    expect(calls.filter((c) => c.cmd === "export_report")).toHaveLength(1);
  });

  it("the file name keeps its extension at the end in a right-to-left line", async () => {
    setup();
    await screen.findByText(/שם הקובץ:/);
    const name = document.querySelector(".export-file-name") as HTMLElement;
    const parts = name.querySelectorAll("bdi");
    expect(parts).toHaveLength(2);
    expect(parts[0]).toHaveTextContent("דוח אבחון TEST-0002");
    expect(parts[1]).toHaveTextContent(".docx");
    expect(parts[1]).toHaveAttribute("dir", "ltr");
  });

  it("a number unlike the score table still open is named before the file is made", async () => {
    mockIPC((cmd) => (cmd === "check_export" ? check : cmd === "report_settings" ? settings : null));
    const sections = [{
      key: "cognitive", title: "פרופיל קוגניטיבי", approved: true,
      paragraphs: [{ id: "p1", status: "approved", text: "טקסט בדוי 115", notes: [{ kind: "score_mismatch", text: "המספר 115 לא תואם" }], warnings: [] }],
    }];
    render(<AppContext.Provider value={app()}><ExportDialog api={caseApi(sections)} onClose={vi.fn()} /></AppContext.Provider>);
    await waitFor(() => expect(screen.getByText(/מספר שלא תואם את טבלת הציונים: פרופיל קוגניטיבי/)).toBeInTheDocument());
    // Advice: the file can still be made.
    expect(screen.getByRole("button", { name: "הפקת הדוח" })).toBeEnabled();
  });
});
