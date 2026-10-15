// Text from documents and from the model is shown as text on every screen, never as HTML,
// and copying an answer goes only through the protected clipboard. The whole app runs over the
// in-memory demo core; every name and detail is invented.
import { configure, render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { mockIPC } from "@tauri-apps/api/mocks";
import { App } from "../App";
import { FakeCore } from "../web/fakeCore";
import { a11yViolations } from "../test/a11y";

configure({ asyncUtilTimeout: 5000 });
vi.setConfig({ testTimeout: 30_000 });

const PAYLOAD = '<img src=x onerror="window.__xss=1"><script>window.__xss=2</script><b class="injected">מודגש</b>';

/** Answers a command instead of the demo core; `undefined` lets the core answer. */
type Override = (cmd: string, args: Record<string, unknown>, core: FakeCore) => unknown;

async function openApp(over: Override = () => undefined, prepare?: (core: FakeCore, caseId: string) => Promise<void>) {
  const core = new FakeCore();
  await core.handle("unlock", { password: "x" });
  // These tests read every screen a request passes through, the review screen included, so
  // they run with "show me every request before sending" on (off by default; set here for the tests that read it).
  await core.handle("set_review_always", { on: true });
  const cases = (await core.handle("list_cases", {})) as { id: string; child_name: string | null }[];
  const caseId = cases.find((c) => c.child_name === "נועם")?.id ?? "";
  if (prepare) await prepare(core, caseId);
  const calls: { cmd: string; args: Record<string, unknown> }[] = [];
  mockIPC(async (cmd, args) => {
    const a = (args ?? {}) as Record<string, unknown>;
    calls.push({ cmd, args: a });
    const out = await over(cmd, a, core);
    return out === undefined ? core.handle(cmd, a) : out;
  });
  const user = userEvent.setup();
  const view = render(<App />);
  return { core, caseId, calls, user, view };
}

function expectNoMarkup(root: ParentNode = document) {
  expect(root.querySelector("img, script, .injected")).toBeNull();
  expect((window as unknown as { __xss?: number }).__xss).toBeUndefined();
}

afterEach(() => {
  delete (window as unknown as { __xss?: number }).__xss;
});

describe("consultation", () => {
  async function ask(user: ReturnType<typeof userEvent.setup>, question: string) {
    await user.click(await screen.findByRole("button", { name: "התייעצות" }));
    await user.type(await screen.findByLabelText("שאלה ל-Claude"), question);
    await user.click(screen.getByRole("button", { name: "שליחה" }));
    const review = await screen.findByRole("dialog", { name: /לפני שליחה/ });
    await user.click(within(review).getByRole("button", { name: /^שליחה/ }));
  }

  it("the model's answer is shown as text, in paragraphs, and the screen stays accessible", async () => {
    const answer = `פסקה ראשונה של תשובה. ${PAYLOAD}\n\nפסקה שנייה.`;
    const { user, view } = await openApp((cmd) =>
      cmd === "send_consult" ? { answer, demo: false, conversation_id: "conv-test" } : undefined,
    );
    await ask(user, "מה כדאי לבדוק בתצפית?");
    const chat = screen.getByRole("region", { name: "השיחה" });
    const bubble = (await within(chat).findByText(/פסקה ראשונה של תשובה/)).closest(".chat-bubble") as HTMLElement;
    expect(bubble.querySelectorAll("p")).toHaveLength(2);
    expect(bubble).toHaveTextContent(PAYLOAD);
    expectNoMarkup();
    expect(await a11yViolations(view.container)).toEqual([]);
  });

  it("the question she typed is echoed as text too", async () => {
    const { user } = await openApp();
    await ask(user, `שאלה עם תגיות ${PAYLOAD}`);
    const chat = screen.getByRole("region", { name: "השיחה" });
    // The question is the bubble and, shortened, the conversation's name above it: both text.
    const bubble = (await within(chat).findAllByText(/שאלה עם תגיות/)).find((el) => el.classList.contains("chat-bubble-me"));
    expect(bubble).toHaveTextContent(PAYLOAD);
    expectNoMarkup();
  });

  it("copying an answer uses only the protected clipboard (never the ordinary one)", async () => {
    const writeText = vi.fn(async () => undefined);
    Object.defineProperty(navigator, "clipboard", { value: { writeText }, configurable: true });
    const { user, calls } = await openApp((cmd) => {
      if (cmd === "send_consult") return { answer: "תשובה בדויה לשאלה.", demo: false, conversation_id: "conv-copy" };
      if (cmd === "copy_secret") return 60;
      return undefined;
    });
    await ask(user, "שאלה קצרה");
    await user.click(await screen.findByRole("button", { name: "העתקה" }));
    expect(calls.find((c) => c.cmd === "copy_secret")?.args).toEqual({ text: "תשובה בדויה לשאלה." });
    expect(await screen.findByRole("button", { name: "הועתק ✓" })).toBeInTheDocument();
    expect(writeText).not.toHaveBeenCalled();
  });

  it("if the protected clipboard fails, nothing is copied another way and she is told", async () => {
    const writeText = vi.fn(async () => undefined);
    Object.defineProperty(navigator, "clipboard", { value: { writeText }, configurable: true });
    const { user } = await openApp((cmd) => {
      if (cmd === "send_consult") return { answer: "תשובה בדויה.", demo: false, conversation_id: "conv-copy" };
      if (cmd === "copy_secret") throw { code: "internal", message: "ההעתקה לא הצליחה.", details: [] };
      return undefined;
    });
    await ask(user, "שאלה קצרה");
    await user.click(await screen.findByRole("button", { name: "העתקה" }));
    expect(await screen.findByRole("alert")).toHaveTextContent("ההעתקה לא הצליחה.");
    expect(writeText).not.toHaveBeenCalled();
  });

  it("a failed preparation keeps her question in the box and says why", async () => {
    const { user } = await openApp((cmd) => {
      if (cmd === "prepare_consult") throw { code: "internal", message: "הבדיקה לפני השליחה לא הצליחה.", details: [] };
      return undefined;
    });
    await user.click(await screen.findByRole("button", { name: "התייעצות" }));
    const box = await screen.findByLabelText("שאלה ל-Claude");
    await user.type(box, "שאלה שלא נשלחה");
    await user.keyboard("{Enter}");
    expect(await screen.findByRole("alert")).toHaveTextContent("הבדיקה לפני השליחה לא הצליחה.");
    expect(box).toHaveValue("שאלה שלא נשלחה");
    // The question is not left in the conversation as if it were on its way.
    expect(within(document.querySelector(".consult-body") as HTMLElement).queryByText("שאלה שלא נשלחה")).toBeNull();
  });

  it("a failed send drops the pending question and shows the error", async () => {
    const { user } = await openApp((cmd) => {
      if (cmd === "send_consult") throw { code: "egress", message: "השליחה לא הצליחה. אפשר לנסות שוב.", details: [] };
      return undefined;
    });
    await ask(user, "שאלה שנכשלה");
    expect(await screen.findByRole("alert")).toHaveTextContent("השליחה לא הצליחה");
    await waitFor(() => expect(screen.queryByText("ממתינה לשליחה…")).toBeNull());
    expect(within(document.querySelector(".consult-body") as HTMLElement).queryByText("שאלה שנכשלה")).toBeNull();
  });
});

describe("case materials and the report page", () => {
  it("a material with markup opens as text, and its review before sending is text too", async () => {
    const { user, view } = await openApp(undefined, async (core, caseId) => {
      await core.handle("add_input", { caseId, kind: "free_text", title: "הערה עם תגיות", content: `נועם בונה מגדלים. ${PAYLOAD}` });
    });
    await user.click(await screen.findByRole("button", { name: /בתיק של נועם$/ }));
    await screen.findByRole("article", { name: "הדוח, כמו בקובץ" });
    await user.click(screen.getAllByRole("button", { name: /חומרים/ })[0] as HTMLElement);
    await user.click(await within(screen.getByRole("region", { name: "רשימת החומרים" })).findByRole("button", { name: /הערה עם תגיות/ }));
    const dialog = await screen.findByRole("dialog", { name: "הערה עם תגיות" });
    await waitFor(() => expect(dialog.querySelector("mark.mark-real")).not.toBeNull());
    expect(dialog).toHaveTextContent(PAYLOAD);
    expectNoMarkup();
    expect(await a11yViolations(view.container)).toEqual([]);
  });

  it("her own paragraph with markup is shown on the A4 page as text", async () => {
    const { user } = await openApp(undefined, async (core, caseId) => {
      await core.handle("add_own_paragraph", { caseId, sectionKey: "referral", text: `פסקה שלה. ${PAYLOAD}`, first: false, after: null });
    });
    await user.click(await screen.findByRole("button", { name: /בתיק של נועם$/ }));
    const page = await screen.findByRole("article", { name: "הדוח, כמו בקובץ" });
    expect(await within(page).findByText(/פסקה שלה\./)).toHaveTextContent(PAYLOAD);
    expectNoMarkup(page);
  });

  it("a draft from the model with markup (text, sources, warnings, style note) is shown as text", async () => {
    const injected = {
      id: "d-model", text: `ניסוח מוצע של המודל. ${PAYLOAD}`, status: "proposed", by_ai: true,
      sources: [`S1 · אינטייק · ${PAYLOAD}`], warnings: [`בדיקה: ${PAYLOAD}`], replaces: null, has_versions: false,
      style_note: `הערת סגנון ${PAYLOAD}`,
    };
    const { user } = await openApp(async (cmd, args, core) => {
      if (cmd !== "case_detail") return undefined;
      const detail = (await core.handle(cmd, args)) as { sections: { key: string; paragraphs: unknown[] }[] };
      return { ...detail, sections: detail.sections.map((s) => (s.key === "background" ? { ...s, paragraphs: [injected] } : s)) };
    });
    await user.click(await screen.findByRole("button", { name: /בתיק של נועם$/ }));
    const page = await screen.findByRole("article", { name: "הדוח, כמו בקובץ" });
    expect(await within(page).findByText(/ניסוח מוצע של המודל\./)).toHaveTextContent(PAYLOAD);
    // It waits for her: a draft is never approved by itself.
    expect(within(page).getAllByRole("button", { name: "✓ לאשר" }).length).toBeGreaterThan(0);
    expectNoMarkup();
  });
});

