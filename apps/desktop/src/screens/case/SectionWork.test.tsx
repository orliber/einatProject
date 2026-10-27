// One section at work, over the demo core: an empty section and its first draft, approving a
// paragraph or the whole draft (also with Ctrl+Shift+Enter), editing, removing, a paragraph of
// her own, the conversation about the section, choosing its materials, the sections written
// from approved ones, and what she sees when a step fails. Every name and detail is invented.
import { configure, render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { clearKept } from "../../kept";
import { a11yViolations } from "../../test/a11y";
import { appApi, CaseHost, Host, serve, type Override } from "../../test/host";
import type { FakeCore } from "../../web/fakeCore";
import { DRAFT_FROM_APPROVED, DRAFT_FROM_SOURCES } from "./instructions";
import { SectionWork } from "./SectionWork";

configure({ asyncUtilTimeout: 8000 });
vi.setConfig({ testTimeout: 30_000 });

afterEach(() => clearKept());

/** A draft written by the demo core before the screen opens (its wait skipped). */
async function draftIn(core: FakeCore, caseId: string, sectionKey: string) {
  const prepared = (await core.handle("prepare_section", { caseId, sectionKey, instruction: "" })) as { approval_id: string };
  vi.useFakeTimers({ toFake: ["setTimeout"] });
  try {
    const sent = core.handle("send_section", { approvalId: prepared.approval_id });
    await vi.advanceTimersByTimeAsync(3000);
    await sent;
  } finally {
    vi.useRealTimers();
  }
}

async function openSection(key: string, over?: Override, setup?: (core: FakeCore, caseId: string) => Promise<void>) {
  const served = await serve(over);
  await setup?.(served.core, served.caseId);
  const app = appApi();
  const user = userEvent.setup();
  const view = render(
    <Host app={app}>
      <CaseHost caseId={served.caseId}>
        {(api) => {
          const section = api.detail.sections.find((s) => s.key === key);
          return section ? <SectionWork api={api} section={section} /> : null;
        }}
      </CaseHost>
    </Host>,
  );
  const draft = await screen.findByRole("region", { name: "טיוטת הסעיף" });
  return { ...served, app, user, view, draft };
}

const asked = (calls: { cmd: string; args: Record<string, unknown> }[]) => calls.filter((c) => c.cmd === "prepare_section").map((c) => c.args);
const proposedCount = (draft: HTMLElement) => draft.querySelectorAll("article.para.pending").length;

describe("an empty section", () => {
  it("says what it is written from and offers a first draft or her own paragraph; passes the accessibility check", async () => {
    const { draft, view } = await openSection("kindergarten");
    expect(screen.getByRole("heading", { level: 1, name: "מסגרת חינוכית" })).toBeInTheDocument();
    expect(screen.getByText("Claude כותב מתוך:")).toBeInTheDocument();
    expect(screen.getByText(/שיחה עם הגננת/)).toBeInTheDocument();
    expect(within(draft).getByText("עוד אין טיוטה לסעיף הזה.")).toBeInTheDocument();
    expect(within(draft).getByRole("button", { name: "כתיבת טיוטה עם Claude" })).toBeEnabled();
    expect(within(draft).getByRole("button", { name: "כתיבה בעצמי" })).toBeEnabled();
    // With no draft yet, the quick requests and "שליחה" wait.
    const chat = screen.getByRole("region", { name: "שיחה על הסעיף" });
    for (const label of ["להרחיב", "לקצר", "לבדוק מול החומרים", "להציע המלצות"]) expect(within(chat).getByRole("button", { name: label })).toBeDisabled();
    expect(within(chat).getByRole("button", { name: /שליחה/ })).toBeDisabled();
    expect(await a11yViolations(view.container)).toEqual([]);
  });

  it("the first draft is asked for from the sources, shows while it is written, and waits for her approval", async () => {
    const { draft, calls } = await openSection("kindergarten");
    const user = userEvent.setup();
    await user.click(within(draft).getByRole("button", { name: "כתיבת טיוטה עם Claude" }));
    // While it is written: one request at a time.
    const chat = screen.getByRole("region", { name: "שיחה על הסעיף" });
    expect(await within(chat).findByText("כותב…")).toBeInTheDocument();
    expect(within(draft).getByText("הבקשה נשלחה (בלי שמות). אפשר לעבוד בינתיים בסעיפים אחרים.")).toBeInTheDocument();
    expect(asked(calls)[0]).toMatchObject({ sectionKey: "kindergarten", instruction: DRAFT_FROM_SOURCES });
    expect(await within(draft).findByText("טיוטה של Claude מחכה לאישורך")).toBeInTheDocument();
    expect(proposedCount(draft)).toBeGreaterThan(0);
    expect(within(draft).getByRole("button", { name: `אישור כל הטיוטה (${proposedCount(draft)})` })).toBeInTheDocument();
    expect(within(chat).queryByText("כותב…")).toBeNull();
  });

  it("a paragraph of her own: “הוספה לדוח” waits for text, and the paragraph goes in approved", async () => {
    const { draft, user, calls } = await openSection("kindergarten");
    await user.click(within(draft).getByRole("button", { name: "כתיבה בעצמי" }));
    const box = within(draft).getByRole("textbox", { name: "פסקה חדשה" });
    expect(box).toHaveFocus();
    const add = within(draft).getByRole("button", { name: "הוספה לדוח" });
    expect(add).toBeDisabled();
    await user.type(box, "בגן הוא נעזר בהכנה מראש לפני כל מעבר.");
    expect(add).toBeEnabled();
    await user.click(add);
    expect(await within(draft).findByText("בגן הוא נעזר בהכנה מראש לפני כל מעבר.")).toBeInTheDocument();
    expect(within(draft).getByText("✓ אושר על ידך")).toBeInTheDocument();
    expect(calls.find((c) => c.cmd === "add_own_paragraph")?.args).toMatchObject({ sectionKey: "kindergarten", text: "בגן הוא נעזר בהכנה מראש לפני כל מעבר." });
    expect(within(draft).getByText("✓ הסעיף אושר.")).toBeInTheDocument();
  });

  it("a section written from the approved ones says so and asks for its draft from them", async () => {
    const { draft, user, calls } = await openSection("summary", (cmd) => (cmd === "send_section" ? { paragraphs: [], questions: [], missing: [], contradictions: [], demo: true } : undefined));
    expect(screen.getByText("הסעיף נכתב מתוך הסעיפים שכבר אישרת.")).toBeInTheDocument();
    expect(screen.queryByRole("button", { name: /קישור חומרים|שינוי החומרים/ })).toBeNull();
    await user.click(within(draft).getByRole("button", { name: "כתיבת טיוטה מהסעיפים שאושרו" }));
    await waitFor(() => expect(asked(calls)[0]).toMatchObject({ sectionKey: "summary", instruction: DRAFT_FROM_APPROVED }));
  });
});

describe("a draft waiting for her", () => {
  const withDraft = (over?: Override) => openSection("kindergarten", over, (core, caseId) => draftIn(core, caseId, "kindergarten"));

  it("passes the accessibility check, with the paragraph's own buttons", async () => {
    const { draft, view } = await withDraft();
    expect(within(draft).getByText("טיוטה של Claude מחכה לאישורך")).toBeInTheDocument();
    const first = draft.querySelector("article.para.pending") as HTMLElement;
    expect(within(first).getByText("הצעה של Claude · ממתינה לאישור")).toBeInTheDocument();
    for (const name of ["אישור", "ניסוח מחדש", "עריכה", "הסרה"]) expect(within(first).getByRole("button", { name })).toBeEnabled();
    expect(await a11yViolations(view.container)).toEqual([]);
  });

  it("one paragraph is approved on its own; the rest still wait", async () => {
    const { draft, user, calls } = await withDraft();
    const before = proposedCount(draft);
    const first = draft.querySelector("article.para.pending") as HTMLElement;
    await user.click(within(first).getByRole("button", { name: "אישור" }));
    await waitFor(() => expect(proposedCount(draft)).toBe(before - 1));
    expect(calls.some((c) => c.cmd === "approve_paragraph")).toBe(true);
    expect(within(draft).getAllByText("✓ אושר על ידך").length).toBeGreaterThan(0);
  });

  it("Ctrl+Shift+Enter approves the whole draft, and the next section is offered", async () => {
    const { draft, user, calls, app, caseId } = await withDraft();
    await user.keyboard("{Control>}{Shift>}{Enter}{/Shift}{/Control}");
    expect(await within(draft).findByText("✓ הסעיף אושר.")).toBeInTheDocument();
    expect(calls.find((c) => c.cmd === "approve_section")?.args).toMatchObject({ caseId, sectionKey: "kindergarten" });
    expect(proposedCount(draft)).toBe(0);
    const next = within(draft).getByRole("button", { name: /^לסעיף הבא: / });
    await user.click(next);
    expect(app.go).toHaveBeenCalledWith(expect.objectContaining({ name: "case", id: caseId }));
  });

  it("“אישור כל הטיוטה” approves it too", async () => {
    const { draft, user } = await withDraft();
    await user.click(within(draft).getByRole("button", { name: /^אישור כל הטיוטה/ }));
    expect(await within(draft).findByText("✓ הסעיף אושר.")).toBeInTheDocument();
  });

  it("a paragraph is edited, then saved and approved together; Cancel changes nothing", async () => {
    const { draft, user, calls } = await withDraft();
    const first = draft.querySelector("article.para.pending") as HTMLElement;
    await user.click(within(first).getByRole("button", { name: "עריכה" }));
    const box = within(first).getByRole("textbox", { name: "עריכת הפסקה" });
    expect(box).toHaveFocus();
    // While editing, Ctrl+Shift+Enter does not approve the draft behind her back.
    await user.keyboard("{Control>}{Shift>}{Enter}{/Shift}{/Control}");
    expect(calls.some((c) => c.cmd === "approve_section")).toBe(false);
    await user.click(within(first).getByRole("button", { name: "ביטול" }));
    expect(calls.some((c) => c.cmd === "edit_paragraph")).toBe(false);

    await user.click(within(first).getByRole("button", { name: "עריכה" }));
    const again = within(first).getByRole("textbox", { name: "עריכת הפסקה" });
    await user.clear(again);
    await user.type(again, "פסקה מתוקנת על המסגרת.");
    await user.click(within(first).getByRole("button", { name: "שמירה ואישור" }));
    expect(await within(draft).findByText("פסקה מתוקנת על המסגרת.")).toBeInTheDocument();
    const cmds = calls.map((c) => c.cmd);
    expect(cmds.indexOf("edit_paragraph")).toBeLessThan(cmds.lastIndexOf("approve_paragraph"));
  });

  it("“הסרה” takes a paragraph out of the draft", async () => {
    const { draft, user, calls } = await withDraft();
    const before = proposedCount(draft);
    await user.click(within(draft.querySelector("article.para.pending") as HTMLElement).getByRole("button", { name: "הסרה" }));
    await waitFor(() => expect(proposedCount(draft)).toBe(before - 1));
    expect(calls.some((c) => c.cmd === "reject_paragraph")).toBe(true);
  });

  it("a request in the conversation goes with Ctrl+Enter, and the answer is in the conversation", async () => {
    const { user, calls } = await withDraft();
    const chat = screen.getByRole("region", { name: "שיחה על הסעיף" });
    const box = within(chat).getByLabelText("הודעה ל-Claude");
    const send = within(chat).getByRole("button", { name: /שליחה/ });
    expect(send).toBeDisabled();
    await user.type(box, "להדגיש את הקושי במעברים");
    expect(send).toBeEnabled();
    await user.keyboard("{Control>}{Enter}{/Control}");
    await waitFor(() => expect(asked(calls).at(-1)).toMatchObject({ instruction: "להדגיש את הקושי במעברים" }));
    // One request at a time: everything that sends waits for this one.
    expect(within(chat).getByRole("button", { name: "לקצר" })).toBeDisabled();
    // The draft written before had its answer here already; this request adds a second one.
    await waitFor(() => expect(within(chat).getAllByText(/^מצב הדגמה: ניסחתי/)).toHaveLength(2));
    expect(within(chat).getByText("להדגיש את הקושי במעברים")).toBeInTheDocument();
    expect(box).toHaveValue("");
  });

  it("a request answered at once (as in the demo) is still cleared from the box", async () => {
    const { user, calls } = await withDraft((cmd) => (cmd === "send_section" ? { paragraphs: [], questions: [], missing: [], contradictions: [], demo: true } : undefined));
    const chat = screen.getByRole("region", { name: "שיחה על הסעיף" });
    const box = within(chat).getByLabelText("הודעה ל-Claude");
    await user.type(box, "לקצר את הפסקה השנייה");
    await user.keyboard("{Control>}{Enter}{/Control}");
    await waitFor(() => expect(calls.some((c) => c.cmd === "send_section")).toBe(true));
    await waitFor(() => expect(within(chat).queryByText("כותב…")).toBeNull());
    // Past the pause after typing, too: the sent request does not come back.
    await new Promise((r) => setTimeout(r, 1000));
    expect(box).toHaveValue("");
  });

  it("a quick request sends its fixed wording", async () => {
    const { user, calls } = await withDraft();
    await user.click(within(screen.getByRole("region", { name: "שיחה על הסעיף" })).getByRole("button", { name: "לקצר" }));
    await waitFor(() => expect(asked(calls).at(-1)).toMatchObject({ instruction: "קצרי את הטיוטה של הסעיף ושמרי על כל הממצאים העיקריים." }));
  });
});

describe("choosing the materials", () => {
  it("the panel opens from the keyboard, and a material marked or unmarked is saved", async () => {
    const { user, calls } = await openSection("kindergarten");
    const toggle = screen.getByRole("button", { name: "✎ שינוי החומרים" });
    expect(toggle).toHaveAttribute("aria-expanded", "false");
    toggle.focus();
    await user.keyboard("{Enter}");
    expect(toggle).toHaveAttribute("aria-expanded", "true");
    const panel = screen.getByRole("region", { name: "החומרים שהסעיף נכתב מהם" });
    const box = within(panel).getByRole("checkbox", { name: /מפגש שני/ });
    const was = (box as HTMLInputElement).checked;
    await user.click(box);
    await waitFor(() => expect(calls.some((c) => c.cmd === "set_input_sections")).toBe(true));
    const feeds = calls.find((c) => c.cmd === "set_input_sections")?.args.sections as string[];
    expect(feeds.includes("kindergarten")).toBe(!was);
  });
});

describe("when a step fails", () => {
  it("a request that cannot be prepared says why in Hebrew, and the buttons are free again", async () => {
    const { draft, user } = await openSection("kindergarten", (cmd) => {
      if (cmd === "prepare_section") throw { code: "consent_missing", message: "לפני שליחה ל-Claude צריך לרשום בתיק את הסכמת ההורים.", details: [], action: { kind: "dismiss" } };
      return undefined;
    });
    await user.click(within(draft).getByRole("button", { name: "כתיבת טיוטה עם Claude" }));
    expect(await within(draft).findByRole("alert")).toHaveTextContent("לפני שליחה ל-Claude צריך לרשום בתיק את הסכמת ההורים.");
    expect(within(draft).getByRole("button", { name: "כתיבת טיוטה עם Claude" })).toBeEnabled();
  });

  it("a failed send offers “לנסות שוב”, which sends the same approved request again", async () => {
    let failOnce = true;
    const { draft, user, calls } = await openSection("kindergarten", (cmd) => {
      if (cmd === "send_section" && failOnce) {
        failOnce = false;
        throw { code: "egress", message: "השליחה לא הצליחה. אפשר לנסות שוב.", details: [], action: { kind: "retry", after_seconds: 0 } };
      }
      return undefined;
    });
    await user.click(within(draft).getByRole("button", { name: "כתיבת טיוטה עם Claude" }));
    const alert = await within(draft).findByRole("alert");
    expect(alert).toHaveTextContent("השליחה לא הצליחה. אפשר לנסות שוב.");
    await user.click(within(alert.parentElement as HTMLElement).getByRole("button", { name: "לנסות שוב" }));
    expect(await within(draft).findByText("טיוטה של Claude מחכה לאישורך")).toBeInTheDocument();
    const sends = calls.filter((c) => c.cmd === "send_section").map((c) => c.args.approvalId);
    expect(sends).toHaveLength(2);
    expect(sends[1]).toBe(sends[0]);
    expect(asked(calls)).toHaveLength(1);
  });

  it("an approval that fails says why, and the paragraph still waits", async () => {
    const { draft, user } = await openSection("kindergarten", (cmd) => {
      if (cmd === "approve_paragraph") throw { code: "internal", message: "האישור לא נשמר.", details: [] };
      return undefined;
    }, (core, caseId) => draftIn(core, caseId, "kindergarten"));
    const before = proposedCount(draft);
    await user.click(within(draft.querySelector("article.para.pending") as HTMLElement).getByRole("button", { name: "אישור" }));
    expect(await within(draft).findByRole("alert")).toHaveTextContent("האישור לא נשמר.");
    expect(proposedCount(draft)).toBe(before);
  });

  it("a conversation that cannot be read says why", async () => {
    const { draft } = await openSection("kindergarten", (cmd) => {
      if (cmd === "chat") throw { code: "internal", message: "לא הצלחנו לקרוא את השיחה על הסעיף.", details: [] };
      return undefined;
    });
    expect(await within(draft).findByRole("alert")).toHaveTextContent("לא הצלחנו לקרוא את השיחה על הסעיף.");
  });
});
