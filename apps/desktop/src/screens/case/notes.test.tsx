// D-161: no note gets lost and the page stays clean. A paragraph's notes are one folded line
// that opens on a click; what the last answer said about the section shows for the section in
// focus only; "approve all" says how many it held back and goes to the first one. Over the
// demo core; every name and detail is invented.
import { configure, render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { mockIPC } from "@tauri-apps/api/mocks";
import { App } from "../../App";
import { FakeCore } from "../../web/fakeCore";
import type { CaseDetail } from "../../ipc/client";
import { approvedLine } from "../CaseScreen";
import { heldNotes, isClean, noteLine, sectionNotesLine } from "./notes";

configure({ asyncUtilTimeout: 6000 });
vi.setConfig({ testTimeout: 30_000 });

type Paragraph = CaseDetail["sections"][number]["paragraphs"][number];

async function openReport(reshape: (d: CaseDetail, core: FakeCore) => void, answer?: (cmd: string, args: Record<string, unknown>) => unknown) {
  const core = new FakeCore();
  await core.handle("unlock", { password: "x" });
  await core.handle("set_review_always", { on: false });
  const calls: { cmd: string; args: Record<string, unknown> }[] = [];
  mockIPC(async (cmd, args) => {
    calls.push({ cmd, args: (args ?? {}) as Record<string, unknown> });
    // The reshaped case holds drafts the demo core does not know: their answer is given here.
    const given = answer?.(cmd, (args ?? {}) as Record<string, unknown>);
    if (given !== undefined) return given;
    const out = await core.handle(cmd, args);
    if (cmd !== "case_detail") return out;
    const d = structuredClone(out as CaseDetail);
    // These tests give their own notes: the demo case's own ones are taken off first.
    for (const x of d.sections.flatMap((s) => s.paragraphs)) Object.assign(x, { notes: [], polish: [] });
    reshape(d, core);
    return d;
  });
  const user = userEvent.setup();
  render(<App />);
  await user.click(await screen.findByRole("button", { name: /בתיק של נועם$/ }));
  const page = await screen.findByRole("article", { name: "הדוח, כמו בקובץ" });
  return { calls, user, page, core };
}

/** A draft of `texts` waiting in the section, each with its notes. */
function drafted(d: CaseDetail, key: string, drafts: { text: string; notes?: Paragraph["notes"] }[]) {
  const s = d.sections.find((x) => x.key === key);
  if (!s) throw new Error(key);
  s.paragraphs = drafts.map((x, i) => ({
    id: `${key}-${i}`, text: x.text, status: "proposed", by_ai: true, sources: [], warnings: [], polish: (x.notes ?? []).filter((n) => n.kind === "polish").map((n) => n.text), notes: x.notes ?? [],
    replaces: null, has_versions: false, style_note: null, style: null,
  }));
}

describe("what holds a paragraph for her eye", () => {
  it("is every note but the proofreading, and a warning the core sent without a kind", () => {
    expect(isClean({ notes: [{ kind: "polish", text: "מילה כפולה" }], warnings: [] })).toBe(true);
    expect(isClean({ notes: [{ kind: "verify_partial", text: "חלקי" }], warnings: [] })).toBe(false);
    expect(heldNotes({ notes: [{ kind: "age", text: "גיל אחר" }], warnings: ["גיל אחר", "עוד אזהרה"] })).toEqual([
      { kind: "age", text: "גיל אחר" },
      { kind: null, text: "עוד אזהרה" },
    ]);
    expect(noteLine({ kind: "verify_contradicted", text: "במקור כתוב אחרת" })).toBe("בבדיקה מול המקורות, סתירה: במקור כתוב אחרת");
    expect(noteLine({ kind: "no_source", text: "הפסקה לא מציינת מקור" })).toBe("הפסקה לא מציינת מקור");
  });

  it("the section's line counts in words, and says nothing when there is nothing", () => {
    const none = { questions: [], missing: [], contradictions: [], unanswered: [] };
    expect(sectionNotesLine(none, "Claude")).toBeNull();
    expect(sectionNotesLine({ ...none, questions: ["א", "ב"], missing: ["ג"] }, "Claude")).toBe("Claude שאל 2 שאלות · חסר מידע בנושא אחד");
    expect(sectionNotesLine({ ...none, contradictions: ["א"], unanswered: ["ב", "ג"] }, "Claude")).toBe("סתירה אחת בין החומרים · 2 שורות בהנחיה בלי פסקה");
  });

  it("the line after approve all says how many went in and how many wait", () => {
    expect(approvedLine(7, 2)).toBe("אושרו 7 פסקאות · 2 נשארו לבדיקה שלך");
    expect(approvedLine(1, 1)).toBe("אושרה פסקה אחת · אחת נשארה לבדיקה שלך");
    expect(approvedLine(3, 0)).toBe("אושרו 3 פסקאות");
  });
});

describe("notes on the report page (D-161)", () => {
  it("a paragraph's notes are one folded line; a contradiction keeps a small mark on it", async () => {
    const { page, user } = await openReport((d) => {
      drafted(d, "kindergarten", [
        { text: "פסקה עם שתי הערות.", notes: [
          { kind: "no_source", text: "הפסקה לא מציינת מקור" },
          { kind: "verify_contradicted", text: "במקור כתוב שהוא נפרד בבכי" },
          { kind: "polish", text: "מילה כפולה" },
        ] },
        { text: "פסקה עם הערה קלה.", notes: [{ kind: "term", text: "כתוב «גן»; בשאר הדוח «מסגרת»" }] },
        { text: "פסקה נקייה." },
      ]);
    });
    const folds = await within(page).findAllByRole("note", { name: "הערות לבדיקה" });
    expect(folds).toHaveLength(2);
    const [marked, light] = folds as [HTMLElement, HTMLElement];
    // Folded: the count, and the mark of the blocking kind; the notes themselves not shown.
    expect(marked.tagName).toBe("DETAILS");
    expect(marked).not.toHaveAttribute("open");
    expect(marked).toHaveClass("is-blocking");
    expect(within(marked).getByText("2 הערות לבדיקה · סותר את החומרים")).toBeInTheDocument();
    expect(within(marked).getByText(/במקור כתוב שהוא נפרד בבכי/)).not.toBeVisible();
    expect(light).not.toHaveClass("is-blocking");
    expect(within(light).getByText("הערה אחת לבדיקה")).toBeInTheDocument();
    // No always-open warning box is left on the page.
    expect(page.querySelector(".note-warn")).toBeNull();
    await user.click(within(marked).getByText(/2 הערות לבדיקה/));
    expect(within(marked).getAllByRole("listitem").map((li) => li.textContent)).toEqual([
      "הפסקה לא מציינת מקור",
      "בבדיקה מול המקורות, סתירה: במקור כתוב שהוא נפרד בבכי",
    ]);
    // Proofreading stays in its own fold.
    expect(within(page).getByRole("note", { name: "הגהה" })).toBeInTheDocument();
    expect(marked.querySelector("b, i, a, script")).toBeNull();
  });

  it("what the answer said about a section shows only for the section in focus", async () => {
    const said = { questions: ["מתי התחיל ללכת?", "האם יש אחים?"], missing: ["אין מידע על הלידה"], contradictions: [], unanswered: [] };
    const { page, user } = await openReport((d) => {
      drafted(d, "kindergarten", [{ text: "טיוטה ראשונה." }]);
      drafted(d, "parents_view", [{ text: "טיוטה שנייה." }]);
      for (const key of ["kindergarten", "parents_view"]) {
        const s = d.sections.find((x) => x.key === key);
        if (s) s.notes = said;
      }
    });
    // One section is in focus: one line, though two sections have something to say.
    const fold = await within(page).findByRole("note", { name: "מה עלה בניסוח הסעיף" });
    expect(within(page).getAllByRole("note", { name: "מה עלה בניסוח הסעיף" })).toHaveLength(1);
    expect(fold).not.toHaveAttribute("open");
    const line = within(fold).getByText(/שאל 2 שאלות · חסר מידע בנושא אחד$/);
    expect(within(fold).getByText("מתי התחיל ללכת?")).not.toBeVisible();
    await user.click(line);
    expect(within(fold).getByText("מתי התחיל ללכת?")).toBeVisible();
    expect(within(fold).getByText("אין מידע על הלידה")).toBeVisible();
    expect(fold.closest(".a4-section")).toHaveClass("is-focus");
  });

  it("a section with nothing to say has no line at all", async () => {
    const { page } = await openReport((d) => drafted(d, "kindergarten", [{ text: "טיוטה נקייה." }]));
    await within(page).findByText("טיוטה נקייה.");
    expect(within(page).queryByRole("note", { name: "מה עלה בניסוח הסעיף" })).toBeNull();
    expect(within(page).queryByRole("note", { name: "הערות לבדיקה" })).toBeNull();
  });

  it("approve all takes the clean drafts, says how many wait, and goes to the first one held", async () => {
    const approved = new Set<string>();
    const { calls, user } = await openReport((d) => {
      drafted(d, "kindergarten", [{ text: "נקייה בגן." }]);
      drafted(d, "parents_view", [{ text: "נקייה בבית." }, { text: "עם הערה.", notes: [{ kind: "verify_unsupported", text: "לא נמצא לפסקה בסיס בחומרים" }] }]);
      for (const s of d.sections) for (const p of s.paragraphs) if (approved.has(p.id)) p.status = "approved";
    }, (cmd, args) => {
      if (cmd !== "approve_clean_paragraphs") return undefined;
      const ids = args.draftIds as string[];
      ids.forEach((id) => approved.add(id));
      return ids.length;
    });
    const contents = await screen.findByRole("complementary", { name: "תוכן הדוח" });
    await user.click(await within(contents).findByRole("button", { name: /לאשר את כל הטיוטות \(2\)/ }));
    const sent = calls.find((c) => c.cmd === "approve_clean_paragraphs");
    // Only the clean ones are offered; the core decides again anyway.
    expect(sent?.args.draftIds).toEqual(["parents_view-0", "kindergarten-0"]);
    const line = await within(contents).findByRole("button", { name: /אושרו 2 פסקאות · אחת נשארה לבדיקה שלך/ });
    await user.click(line);
    await waitFor(() => expect(document.querySelector(".a4-section.is-focus")?.id).toBe("sec-parents_view"));
    expect(document.getElementById("para-parents_view-1")).not.toBeNull();
  });
});
