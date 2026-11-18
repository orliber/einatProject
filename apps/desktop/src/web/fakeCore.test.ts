// The preview's demo core must show the review screen the way the real core builds it: nothing
// declared in the case appears on the "what Claude will receive" side, labels included.
// All names are invented.
import { FakeCore } from "./fakeCore";
import type { Prepared } from "../ipc/generated/Prepared";
import type { UiError } from "../ipc/generated/UiError";

const NAMES = ["יונתן", "מיכל", "אלון"];

async function newCase(core: FakeCore, consent = true): Promise<string> {
  await core.handle("unlock", { password: "x" });
  return (await core.handle("create_case", {
    meta: {
      code: "תיק-בדיקה",
      age: { years: 5, months: 2 },
      child_gender: "male",
      current_section: null,
      retention_until: null,
      consent: consent ? { given_on: "2026-01-05", form_version: "v1", given_by: "שני ההורים" } : null,
    },
    identities: [
      { id: null, role: "child", value: "יונתן", aliases: [] },
      { id: null, role: "mother", value: "מיכל", aliases: [] },
      { id: null, role: "father", value: "אלון", aliases: [] },
    ],
  })) as string;
}

/** Everything on the outgoing side of the review, the part labels included. */
function outgoing(p: Prepared): string {
  return p.parts.map((part) => `${part.label}\n${part.outgoing.map((s) => s.text).join("")}`).join("\n");
}

describe("FakeCore review screens", () => {
  it("a material's title with a name is hidden on the outgoing side when sorting", async () => {
    const core = new FakeCore();
    const caseId = await newCase(core);
    await core.handle("add_input", {
      caseId, kind: "intake", title: "שיחה עם מיכל",
      content: "יונתן בן חמש. מיכל מספרת שהוא בונה מגדלים. אלון מוסיף שהשינה רגועה.\n\nבגן הוא משחק לבד בחצר.",
    });
    const p = (await core.handle("prepare_sort", { caseId })) as Prepared;
    const out = outgoing(p);
    for (const name of NAMES) expect(out).not.toContain(name);
    // The title is its own part of the review, with the name marked on the case side.
    const title = p.parts.find((part) => part.label.endsWith("כותרת"));
    expect(title?.original.map((s) => s.text).join("")).toBe("שיחה עם מיכל");
    expect(title?.original.some((s) => s.text === "מיכל" && s.mark)).toBe(true);
  });

  it("a title with nothing to hide stays the part's label", async () => {
    const core = new FakeCore();
    const caseId = await newCase(core);
    await core.handle("add_input", { caseId, kind: "intake", title: "אינטייק ראשון", content: "יונתן אוהב פאזלים.\n\nהאם מספרת על שינה רגועה." });
    const p = (await core.handle("prepare_sort", { caseId })) as Prepared;
    expect(p.parts.map((part) => part.label)).toEqual(["S1 · אינטייק · אינטייק ראשון"]);
  });

  it("a section request hides names in titles and texts alike", async () => {
    const core = new FakeCore();
    const caseId = await newCase(core);
    await core.handle("add_input", { caseId, kind: "intake", title: "אינטייק אצל אלון", content: "יונתן הופנה לאבחון על ידי מיכל ואלון." });
    const p = (await core.handle("prepare_section", { caseId, sectionKey: "background", instruction: "" })) as Prepared;
    expect(p.approval_id).not.toBeNull();
    const out = outgoing(p);
    expect(out).toContain("[ילד]");
    for (const name of NAMES) expect(out).not.toContain(name);
  });

  it("a material edited after the review cancels the request, and the send says why", async () => {
    const core = new FakeCore();
    const caseId = await newCase(core);
    const input = (await core.handle("add_input", { caseId, kind: "intake", title: "אינטייק", content: "יונתן הופנה לאבחון." })) as { id: string };
    const p = (await core.handle("prepare_section", { caseId, sectionKey: "background", instruction: "" })) as Prepared;
    await core.handle("update_input", { caseId, inputId: input.id, title: "אינטייק", content: "הופנה לאבחון." });
    const err = await core.handle("send_section", { approvalId: p.approval_id }).then(() => null, (e: UiError) => e);
    expect(err?.message).toContain("החומר השתנה");
  });

  it("without recorded consent nothing is prepared (fail-closed)", async () => {
    const core = new FakeCore();
    const caseId = await newCase(core, false);
    await core.handle("add_input", { caseId, kind: "intake", title: "", content: "יונתן משחק.\n\nעוד פסקה." });
    const err = await core.handle("prepare_sort", { caseId }).then(() => null, (e: UiError) => e);
    expect(err?.code).toBe("consent_missing");
  });

  it("an approval is good for one send only", async () => {
    vi.useFakeTimers({ shouldAdvanceTime: true });
    try {
      const core = new FakeCore();
      const caseId = await newCase(core);
      await core.handle("add_input", { caseId, kind: "intake", title: "", content: "יונתן משחק בקוביות.\n\nהשינה רגועה." });
      const p = (await core.handle("prepare_sort", { caseId })) as Prepared;
      const first = core.handle("send_sort", { approvalId: p.approval_id });
      await vi.runAllTimersAsync();
      await first;
      const again = await core.handle("send_sort", { approvalId: p.approval_id }).then(() => null, (e: UiError) => e);
      expect(again?.code).toBe("refused");
    } finally {
      vi.useRealTimers();
    }
  });
});

describe("the case list's next section (D-204)", () => {
  it("names the first section with nothing approved, as the core does, and none once all are approved", async () => {
    const core = new FakeCore();
    const caseId = await newCase(core);
    const row = async () => ((await core.handle("list_cases", {})) as import("../ipc/generated/CaseSummary").CaseSummary[]).find((c) => c.id === caseId);
    expect((await row())?.next_section).toEqual({ key: "referral", title: "סיבת הפניה" });
    await core.handle("add_own_paragraph", { caseId, sectionKey: "referral", text: "פסקה בדויה." });
    expect((await row())?.next_section).toEqual({ key: "background", title: "רקע: הריון, לידה והתפתחות" });
    const detail = (await core.handle("case_detail", { caseId })) as import("../ipc/generated/CaseDetail").CaseDetail;
    for (const s of detail.sections) {
      if (s.key !== "referral" && s.key !== "signature") await core.handle("add_own_paragraph", { caseId, sectionKey: s.key, text: "פסקה בדויה." });
    }
    const all = await row();
    expect(all && "next_section" in all).toBe(false);
    expect(all?.total_sections).toBe(15);
  });
});

describe("the demo case's notes (D-161)", () => {
  it("sit on approved paragraphs only, are counted for the finish page, and go once their words are changed", async () => {
    const core = new FakeCore();
    await core.handle("unlock", { password: "x" });
    const cases = (await core.handle("list_cases", {})) as { id: string; child_name: string | null }[];
    const caseId = cases.find((c) => c.child_name === "נועם")?.id ?? "";
    const detail = async () => (await core.handle("case_detail", { caseId })) as import("../ipc/generated/CaseDetail").CaseDetail;
    const noted = (await detail()).sections.flatMap((s) => s.paragraphs).filter((p) => p.notes.length > 0);
    expect(noted.every((p) => p.status === "approved")).toBe(true);
    expect(noted.flatMap((p) => p.notes.map((n) => n.kind)).sort()).toEqual(["past_report_overlap", "polish", "polish", "score_mismatch", "verify_unsupported"]);
    const check = (await core.handle("check_export", { caseId })) as import("../ipc/generated/ExportCheck").ExportCheck;
    expect(check.polish_notes).toBe(2);
    expect(check.blocking).toEqual([]);
    // Every note is about words that are in its paragraph.
    const scores = noted.find((p) => p.notes.some((n) => n.kind === "score_mismatch"));
    expect(scores?.text).toContain("115");
    await core.handle("edit_paragraph", { caseId, draftId: scores?.id, text: scores?.text.replace("115", "112").replace("של של", "של"), openedWith: null });
    const after = (await detail()).sections.flatMap((s) => s.paragraphs).find((p) => p.id === scores?.id);
    expect(after?.notes.map((n) => n.kind)).toEqual(["polish"]);
    expect(after?.polish).toHaveLength(1);
  });
});
