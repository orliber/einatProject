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
