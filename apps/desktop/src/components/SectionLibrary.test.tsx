// Round 36: her diagnoses and recommendations to choose from, and the reports most like the
// case. D-168: several lines ticked and added by one button, in the list's order, and a line
// the section already holds is shown as added and cannot be added twice.
import { render, screen, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { mockIPC, clearMocks } from "@tauri-apps/api/mocks";
import { afterEach, describe, expect, it, vi } from "vitest";
import { AppContext } from "../App";
import { SectionLibraryPanel, librarySummary, lineDetail, sameLine } from "./SectionLibrary";

afterEach(() => clearMocks());

const api = () => ({ fail: () => "שגיאה", notify: vi.fn() }) as never;

function wrap(ui: React.ReactNode) {
  const qc = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  const tree = (node: React.ReactNode) => (
    <QueryClientProvider client={qc}>
      <AppContext.Provider value={api()}>{node}</AppContext.Provider>
    </QueryClientProvider>
  );
  const view = render(tree(ui));
  return { ...view, again: (node: React.ReactNode) => view.rerender(tree(node)) };
}

const LINES = [
  { text: "מומלץ על הדרכת הורים.", reports: 5, in_similar: 3, similar: 4 },
  { text: "מומלץ על טיפול בריפוי בעיסוק.", reports: 4, in_similar: 0, similar: 0 },
  { text: "מומלץ על מעקב התפתחותי בעוד שנה.", reports: 3, in_similar: 0, similar: 0 },
];

describe("SectionLibraryPanel", () => {
  it("is one folded line with the count; the lines she ticks are added by one button, in the list's order", async () => {
    const added: string[] = [];
    mockIPC((cmd, args) => {
      if (cmd === "section_library") {
        return { lines: LINES, similar: [{ report: "דוח 2024", reasons: ["WPPSI-IV", "גיל שלוש עד שש"], text: "מומלץ ש[ילד] ימשיך במעקב." }] };
      }
      if (cmd === "add_own_paragraph") added.push(String((args as { text: string }).text));
      return null;
    });
    const onAdded = vi.fn();
    const user = userEvent.setup();
    const { again } = wrap(<SectionLibraryPanel caseId="c1" sectionKey="recommendations" present={[]} onAdded={onAdded} />);
    const line = await screen.findByText("מהדוחות שלך: 3 המלצות שחוזרות");
    expect(line.tagName).toBe("SUMMARY");
    expect(line.closest("details")).not.toHaveAttribute("open");
    await user.click(line);
    expect(screen.getByText("ההמלצות שלך")).toBeInTheDocument();
    expect(screen.getByText("(ב-3 מ-4 דוחות דומים)")).toBeInTheDocument();
    expect(screen.getByText("דוח 2024 · WPPSI-IV · גיל שלוש עד שש")).toBeInTheDocument();
    expect(screen.getByText("מומלץ שילד ימשיך במעקב.")).toBeInTheDocument();
    // Nothing ticked: nothing to add.
    expect(screen.getByRole("button", { name: "להוסיף" })).toBeDisabled();
    // Ticked last to first; they are added in the list's order.
    await user.click(screen.getByRole("checkbox", { name: /מעקב התפתחותי/ }));
    await user.click(screen.getByRole("checkbox", { name: /הדרכת הורים/ }));
    await user.click(screen.getByRole("button", { name: "להוסיף 2" }));
    await vi.waitFor(() => expect(onAdded).toHaveBeenCalledTimes(1));
    expect(added).toEqual(["מומלץ על הדרכת הורים.", "מומלץ על מעקב התפתחותי בעוד שנה."]);

    // The section now holds them: shown as added, and not to be ticked again.
    again(<SectionLibraryPanel caseId="c1" sectionKey="recommendations" present={["מומלץ על הדרכת הורים", "  מומלץ על מעקב  התפתחותי בעוד שנה. "]} onAdded={onAdded} />);
    const there = screen.getByRole("checkbox", { name: /הדרכת הורים/ });
    expect(there).toBeChecked();
    expect(there).toBeDisabled();
    expect(within(there.closest("li") as HTMLElement).getByText("נוסף")).toBeInTheDocument();
    expect(screen.getByRole("checkbox", { name: /מעקב התפתחותי/ })).toBeDisabled();
    expect(screen.getByRole("checkbox", { name: /ריפוי בעיסוק/ })).toBeEnabled();
    expect(screen.getByRole("button", { name: "להוסיף" })).toBeDisabled();
  });

  it("a line the section already holds is never added again, even when it was ticked before", async () => {
    const added: string[] = [];
    mockIPC((cmd, args) => {
      if (cmd === "section_library") return { lines: LINES, similar: [] };
      if (cmd === "add_own_paragraph") added.push(String((args as { text: string }).text));
      return null;
    });
    const user = userEvent.setup();
    const { again } = wrap(<SectionLibraryPanel caseId="c1" sectionKey="recommendations" present={[]} lead onAdded={vi.fn()} />);
    // `lead`: open from the start.
    await user.click(await screen.findByRole("checkbox", { name: /הדרכת הורים/ }));
    await user.click(screen.getByRole("checkbox", { name: /ריפוי בעיסוק/ }));
    // Meanwhile she wrote the first one herself.
    again(<SectionLibraryPanel caseId="c1" sectionKey="recommendations" present={["מומלץ על הדרכת הורים."]} lead onAdded={vi.fn()} />);
    await user.click(screen.getByRole("button", { name: "להוסיף 1" }));
    await vi.waitFor(() => expect(added).toEqual(["מומלץ על טיפול בריפוי בעיסוק."]));
  });

  it("adds once on a double click, and what failed stays ticked to try again", async () => {
    let adds = 0;
    let failNext = true;
    mockIPC(async (cmd) => {
      if (cmd === "section_library") return { lines: LINES.slice(0, 1), similar: [] };
      if (cmd === "add_own_paragraph") {
        adds += 1;
        await new Promise((r) => setTimeout(r, 30));
        if (failNext) throw new Error("x");
      }
      return null;
    });
    const onAdded = vi.fn();
    const user = userEvent.setup();
    wrap(<SectionLibraryPanel caseId="c1" sectionKey="diagnoses" present={[]} lead onAdded={onAdded} />);
    expect(await screen.findByText("מהדוחות שלך: אבחנה אחת שחוזרת")).toBeInTheDocument();
    await user.click(screen.getByRole("checkbox"));
    await user.dblClick(screen.getByRole("button", { name: "להוסיף 1" }));
    expect(await screen.findByText("שגיאה")).toBeInTheDocument();
    expect(adds).toBe(1);
    expect(onAdded).not.toHaveBeenCalled();
    failNext = false;
    await user.click(screen.getByRole("button", { name: "להוסיף 1" }));
    await vi.waitFor(() => expect(onAdded).toHaveBeenCalledTimes(1));
    expect(adds).toBe(2);
  });

  it("shows nothing when her reports offer nothing", async () => {
    mockIPC((cmd) => (cmd === "section_library" ? { lines: [], similar: [] } : null));
    const { container } = wrap(<SectionLibraryPanel caseId="c1" sectionKey="cognitive" present={[]} onAdded={vi.fn()} />);
    await new Promise((r) => setTimeout(r, 20));
    expect(container).toBeEmptyDOMElement();
    expect(lineDetail({ text: "x", reports: 2, in_similar: 0, similar: 0 })).toBe("ב-2 דוחות");
    expect(librarySummary("recommendations", 8)).toBe("מהדוחות שלך: 8 המלצות שחוזרות");
    expect(librarySummary("cognitive", 0)).toBe("מהדוחות שלך: מה כתבת בסעיף הזה בדוחות דומים");
    expect(sameLine("  מומלץ   על מעקב. ")).toBe(sameLine("מומלץ על מעקב"));
    expect(sameLine("מומלץ על מעקב")).not.toBe(sameLine("מומלץ על מעקב נוסף"));
  });
});
