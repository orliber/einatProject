// "למה כתבת את זה?" and earlier wordings: text from documents and from the model, shown as text.
// Loading, empty and error states, and bringing a wording back. All texts are invented.
import type { ReactNode } from "react";
import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { mockIPC } from "@tauri-apps/api/mocks";
import { AppContext, type AppApi } from "../App";
import { status } from "../test/fixtures";
import { a11yViolations } from "../test/a11y";
import { ParagraphVersions } from "./ParagraphVersions";
import { Why } from "./Why";

const PAYLOAD = '<img src=x onerror="window.__xss=1"><script>window.__xss=2</script>';

function api(): AppApi {
  return { status: status(), go: vi.fn(), refresh: vi.fn(async () => undefined), notify: vi.fn(), fail: (e) => e.message, lockNow: vi.fn(async () => undefined) };
}

function inApp(node: ReactNode) {
  return render(<AppContext.Provider value={api()}>{node}</AppContext.Provider>);
}

afterEach(() => {
  delete (window as unknown as { __xss?: number }).__xss;
});

describe("Why", () => {
  it("shows each source passage as plain text, best match first", async () => {
    mockIPC((cmd, args) => {
      expect(cmd).toBe("paragraph_sources");
      expect(args).toEqual({ caseId: "c1", draftId: "d1" });
      return [
        { label: "אינטייק · <i>הורים</i>", text: `ההורים מספרים על שינה רגועה. ${PAYLOAD}` },
        { label: "תצפית בגן", text: "משחק לבד בחצר." },
      ];
    });
    const { container } = inApp(<Why caseId="c1" draftId="d1" />);
    const quotes = await screen.findAllByRole("blockquote");
    expect(quotes.map((q) => q.querySelector(".why-label")?.textContent)).toEqual(["אינטייק · <i>הורים</i>", "תצפית בגן"]);
    expect(quotes[0]).toHaveTextContent(PAYLOAD);
    expect(container.querySelector("img, script, i")).toBeNull();
    expect((window as unknown as { __xss?: number }).__xss).toBeUndefined();
    expect(await a11yViolations(container)).toEqual([]);
  });

  it("a paragraph with no source says so in her words", async () => {
    mockIPC(() => []);
    inApp(<Why caseId="c1" draftId="d1" />);
    expect(await screen.findByText(/אין מקור מסומן/)).toBeInTheDocument();
  });

  it("a failure is shown as the core's message", async () => {
    mockIPC(() => {
      throw { code: "not_found", message: "הפסקה לא נמצאה.", details: [] };
    });
    inApp(<Why caseId="c1" draftId="d1" />);
    expect(await screen.findByRole("alert")).toHaveTextContent("הפסקה לא נמצאה.");
  });
});

describe("ParagraphVersions", () => {
  it("earlier wordings are plain text, marked as AI or hers, and one can be brought back", async () => {
    const calls: { cmd: string; args: unknown }[] = [];
    mockIPC((cmd, args) => {
      calls.push({ cmd, args });
      if (cmd === "paragraph_versions") {
        return [
          { id: "v2", saved_at: 1_700_000_000, by_ai: true, text: `ניסוח של המודל. ${PAYLOAD}` },
          { id: "v1", saved_at: 0, by_ai: false, text: "הניסוח הראשון שלה." },
        ];
      }
      return null;
    });
    const onRestored = vi.fn();
    const user = userEvent.setup();
    const { container } = inApp(<ParagraphVersions caseId="c1" draftId="d1" onRestored={onRestored} />);
    const items = await screen.findAllByRole("listitem");
    expect(items).toHaveLength(2);
    expect(items[0]).toHaveTextContent("✦ ניסוח של AI");
    expect(items[0]).toHaveTextContent(PAYLOAD);
    expect(items[1]).toHaveTextContent("הניסוח שלך");
    expect(container.querySelector("img, script")).toBeNull();
    expect(await a11yViolations(container)).toEqual([]);

    await user.click(screen.getAllByRole("button", { name: "להחזיר את הניסוח הזה" })[1] as HTMLElement);
    expect(calls).toContainEqual({ cmd: "restore_paragraph_version", args: { caseId: "c1", draftId: "d1", versionId: "v1" } });
    expect(onRestored).toHaveBeenCalledTimes(1);
  });

  it("a failed restore keeps the list and shows why; the caller is not told it worked", async () => {
    mockIPC((cmd) => {
      if (cmd === "paragraph_versions") return [{ id: "v1", saved_at: 0, by_ai: false, text: "נוסח קודם." }];
      throw { code: "locked", message: "הכספת ננעלה.", details: [] };
    });
    const onRestored = vi.fn();
    const user = userEvent.setup();
    inApp(<ParagraphVersions caseId="c1" draftId="d1" onRestored={onRestored} />);
    await user.click(await screen.findByRole("button", { name: "להחזיר את הניסוח הזה" }));
    expect(await screen.findByRole("alert")).toHaveTextContent("הכספת ננעלה.");
    expect(onRestored).not.toHaveBeenCalled();
    expect(screen.getByRole("button", { name: "להחזיר את הניסוח הזה" })).toBeEnabled();
  });

  it("no earlier wordings: says so", async () => {
    mockIPC(() => []);
    inApp(<ParagraphVersions caseId="c1" draftId="d1" onRestored={vi.fn()} />);
    expect(await screen.findByText("אין גרסאות קודמות.")).toBeInTheDocument();
  });
});
