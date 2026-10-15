// Round 36: her diagnoses and recommendations to choose from, and the reports most like the case.
import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { mockIPC, clearMocks } from "@tauri-apps/api/mocks";
import { afterEach, describe, expect, it, vi } from "vitest";
import { AppContext } from "../App";
import { SectionLibraryPanel, lineDetail } from "./SectionLibrary";

afterEach(() => clearMocks());

const api = () => ({ fail: () => "שגיאה", notify: vi.fn() }) as never;

function wrap(ui: React.ReactNode) {
  const qc = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  return render(
    <QueryClientProvider client={qc}>
      <AppContext.Provider value={api()}>{ui}</AppContext.Provider>
    </QueryClientProvider>,
  );
}

describe("SectionLibraryPanel", () => {
  it("offers her recommendations with how often she gave them, and adds one on her click", async () => {
    const calls: [string, unknown][] = [];
    mockIPC((cmd, args) => {
      if (cmd === "section_library") {
        return {
          lines: [{ text: "מומלץ על הדרכת הורים.", reports: 5, in_similar: 3, similar: 4 }],
          similar: [{ report: "דוח 2024", reasons: ["WPPSI-IV", "גיל שלוש עד שש"], text: "מומלץ ש[ילד] ימשיך במעקב." }],
        };
      }
      calls.push([cmd, args]);
      return null;
    });
    const onAdded = vi.fn();
    const user = userEvent.setup();
    wrap(<SectionLibraryPanel caseId="c1" sectionKey="recommendations" onAdded={onAdded} />);
    await user.click(await screen.findByText("מהדוחות הקודמים שלך"));
    expect(screen.getByText("ההמלצות שלך")).toBeInTheDocument();
    expect(screen.getByText("(ב-3 מ-4 דוחות דומים)")).toBeInTheDocument();
    expect(screen.getByText("דוח 2024 · WPPSI-IV · גיל שלוש עד שש")).toBeInTheDocument();
    expect(screen.getByText("מומלץ שילד ימשיך במעקב.")).toBeInTheDocument();
    await user.click(screen.getByRole("button", { name: "להוסיף" }));
    await vi.waitFor(() => expect(calls).toContainEqual(["add_own_paragraph", expect.objectContaining({ caseId: "c1", sectionKey: "recommendations", text: "מומלץ על הדרכת הורים." })]));
    expect(onAdded).toHaveBeenCalled();
    expect(screen.getByRole("button", { name: "נוסף" })).toBeDisabled();
  });

  it("shows nothing when her reports offer nothing", async () => {
    mockIPC((cmd) => (cmd === "section_library" ? { lines: [], similar: [] } : null));
    const { container } = wrap(<SectionLibraryPanel caseId="c1" sectionKey="cognitive" onAdded={vi.fn()} />);
    await new Promise((r) => setTimeout(r, 20));
    expect(container).toBeEmptyDOMElement();
    expect(lineDetail({ text: "x", reports: 2, in_similar: 0, similar: 0 })).toBe("ב-2 דוחות");
  });
});
