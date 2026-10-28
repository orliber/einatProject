// Round 48: the report page as the document, one section in focus. The four states, the brief
// that goes with the first draft, and Enter that approves and moves on. Every name is invented.
import { configure, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { mockIPC } from "@tauri-apps/api/mocks";
import { App } from "../../App";
import { FakeCore } from "../../web/fakeCore";
import type { CaseDetail } from "../../ipc/client";
import { draftRequest, firstFocus, nextWaiting } from "./ReportView";
import { DRAFT_FROM_APPROVED, DRAFT_FROM_SOURCES } from "./instructions";

configure({ asyncUtilTimeout: 6000 });
vi.setConfig({ testTimeout: 30_000 });

type Section = CaseDetail["sections"][number];

function section(key: string, paragraphs: Partial<Section["paragraphs"][number]>[] = [], extra: Partial<Section> = {}): Section {
  return {
    key,
    title: key,
    part: "א",
    paragraphs: paragraphs.map((p, i) => ({ id: `${key}-${i}`, text: "טקסט בדוי", status: "approved", by_ai: true, sources: [], warnings: [], replaces: null, has_versions: false, style_note: null, style: null, ...p })),
    approved: paragraphs.some((p) => (p.status ?? "approved") === "approved"),
    source_count: 1,
    sortable: true,
    mode: "ai",
    ...extra,
  } as Section;
}

describe("where the focus goes", () => {
  const sections = [
    section("a", [{}]),
    section("b", [{ status: "proposed" }]),
    section("c"),
    section("d", [{ status: "proposed" }]),
    section("signature"),
  ];

  it("starts on the first draft waiting, else on the first section to write", () => {
    expect(firstFocus(sections)).toBe("b");
    expect(firstFocus([section("a", [{}]), section("mine", [], { mode: "self" }), section("bare", [], { source_count: 0 }), section("c")])).toBe("c");
    expect(firstFocus([section("a", [{}])])).toBe("a");
    expect(firstFocus([])).toBeNull();
  });

  it("Enter moves on to the next draft waiting, round to the top, never to an empty section", () => {
    expect(nextWaiting(sections, "b")).toBe("d");
    expect(nextWaiting(sections, "d")).toBe("b");
    expect(nextWaiting(sections, "a")).toBe("b");
    expect(nextWaiting([section("a", [{}]), section("c")], "a")).toBeNull();
  });

  it("what matters to her joins the usual request; nothing typed sends the usual one", () => {
    expect(draftRequest("kindergarten", "  ")).toBeUndefined();
    expect(draftRequest("kindergarten", "להתחיל מהקשר עם הצוות")).toBe(`${DRAFT_FROM_SOURCES} מה שחשוב לי שייכתב בסעיף: להתחיל מהקשר עם הצוות`);
    expect(draftRequest("summary", "קצר")).toBe(`${DRAFT_FROM_APPROVED} מה שחשוב לי שייכתב בסעיף: קצר`);
  });
});

async function openReport() {
  const core = new FakeCore();
  await core.handle("unlock", { password: "x" });
  await core.handle("set_review_always", { on: false });
  const calls: { cmd: string; args: Record<string, unknown> }[] = [];
  mockIPC((cmd, args) => {
    calls.push({ cmd, args: (args ?? {}) as Record<string, unknown> });
    return core.handle(cmd, args);
  });
  const user = userEvent.setup();
  render(<App />);
  await user.click(await screen.findByRole("button", { name: /בתיק של נועם$/ }));
  const page = await screen.findByRole("article", { name: "הדוח, כמו בקובץ" });
  return { calls, user, page };
}

const focused = (page: HTMLElement) => page.querySelector<HTMLElement>(".a4-section.is-focus");

describe("the report page, one section in focus", () => {
  it("only the section in focus has buttons; the brief goes with its first draft", async () => {
    const { calls, user, page } = await openReport();
    const first = focused(page);
    if (!first) throw new Error("no section in focus");
    expect(page.querySelectorAll(".a4-section.is-focus")).toHaveLength(1);
    // Before the draft: what matters to her here, then "✦ לנסח".
    const brief = await within(first).findByRole("textbox", { name: /^איך לנסח את הסעיף/ });
    // Out of focus, an empty section is one line, with no box of its own.
    expect(page.querySelectorAll(".a4-brief")).toHaveLength(1);
    await user.type(brief, "להתחיל מהקשר עם הצוות{Enter}");
    await waitFor(() => expect(calls.some((c) => c.cmd === "prepare_section")).toBe(true));
    const asked = calls.find((c) => c.cmd === "prepare_section")?.args as { instruction: string };
    expect(asked.instruction).toContain("מה שחשוב לי שייכתב בסעיף: להתחיל מהקשר עם הצוות");
    // The draft: one row of buttons, under it.
    const row = await within(first).findByRole("group", { name: /^הטיוטה של/ });
    expect(within(row).getAllByRole("button").map((b) => b.textContent)).toEqual(["✓ לאשר", "✦ לשנות", "לערוך"]);
    expect(screen.getAllByRole("button", { name: "✓ לאשר" })).toHaveLength(1);
    // "✦ לשנות" shows what she asked for before the draft.
    await user.click(within(row).getByRole("button", { name: "✦ לשנות" }));
    const change = within(first).getByRole("group", { name: "מה לשנות בטיוטה" });
    expect(within(change).getByText("ביקשת לפני הניסוח: להתחיל מהקשר עם הצוות")).toBeInTheDocument();
    fireEvent.keyDown(within(change).getAllByRole("button")[0] as HTMLElement, { key: "Escape", code: "Escape" });
    expect(within(first).queryByRole("group", { name: "מה לשנות בטיוטה" })).toBeNull();
  });

  it("a change with nothing chosen says what is missing instead of doing nothing", async () => {
    const { calls, user, page } = await openReport();
    const first = focused(page);
    if (!first) throw new Error("no section in focus");
    await user.click(await within(first).findByRole("button", { name: "✦ לנסח" }));
    const row = await within(first).findByRole("group", { name: /^הטיוטה של/ });
    await user.click(within(row).getByRole("button", { name: "✦ לשנות" }));
    const change = within(first).getByRole("group", { name: "מה לשנות בטיוטה" });
    // The foot does not promise that an empty box sends.
    expect(within(change).queryByText(/גם בלי לכתוב כלום/)).toBeNull();
    const sent = () => calls.filter((c) => c.cmd === "prepare_section").length;
    const before = sent();
    await user.click(within(change).getByRole("button", { name: "✦ לנסח מחדש" }));
    expect(within(change).getByRole("alert")).toHaveTextContent("בחרי שינוי מהיר, או כתבי מה לשנות.");
    const box = within(change).getByRole("textbox");
    expect(box).toHaveFocus();
    expect(box).toHaveAccessibleDescription("בחרי שינוי מהיר, או כתבי מה לשנות.");
    await user.keyboard("{Enter}");
    expect(sent()).toBe(before);
    // A quick change is enough; the sentence goes as soon as she chooses.
    await user.click(within(change).getByRole("button", { name: "קצר יותר" }));
    expect(within(change).queryByRole("alert")).toBeNull();
    await user.click(within(change).getByRole("button", { name: "✦ לנסח מחדש" }));
    await waitFor(() => expect(sent()).toBe(before + 1));
  });

  it("the box sends a style with her words, and keeps the wish for the section when she asks (D-140)", async () => {
    const { calls, user, page } = await openReport();
    const first = focused(page);
    if (!first) throw new Error("no section in focus");
    const box = await within(first).findByRole("textbox", { name: /^איך לנסח את הסעיף/ });
    await user.click(within(first).getByRole("button", { name: "קצר וממוקד" }));
    expect(within(first).getByRole("button", { name: "קצר וממוקד" })).toHaveAttribute("aria-pressed", "true");
    await user.click(within(first).getByRole("checkbox", { name: /לזכור לסעיף הזה/ }));
    await user.type(box, "להתחיל מהתצפית{Enter}");
    await waitFor(() => expect(calls.some((c) => c.cmd === "prepare_section")).toBe(true));
    const asked = calls.find((c) => c.cmd === "prepare_section")?.args as { instruction: string };
    expect(asked.instruction).toContain("מה שחשוב לי שייכתב בסעיף: כתבי בקצרה ובממוקד. להתחיל מהתצפית");
    const kept = calls.find((c) => c.cmd === "keep_draft_brief")?.args as { text: string; keep: boolean };
    expect(kept).toMatchObject({ text: "להתחיל מהתצפית", keep: true });
  });

  it("Enter approves the draft in focus and moves on to the next one; Ctrl+Enter opens the change", async () => {
    const { calls, user, page } = await openReport();
    // Two drafts: the section in focus, then another one written from its single line.
    const first = focused(page);
    if (!first) throw new Error("no section in focus");
    await user.click(await within(first).findByRole("button", { name: "✦ לנסח" }));
    await within(first).findByRole("group", { name: /^הטיוטה של/ });
    const line = page.querySelector<HTMLElement>(".a4-gap-line");
    if (!line) throw new Error("no empty section left");
    const second = line.closest<HTMLElement>(".a4-section");
    await user.click(within(line).getByRole("button", { name: "✦ לנסח" }));
    // Out of focus it opens the section's box (D-140); the box's "✦ לנסח" sends.
    await user.click(await within(line.closest<HTMLElement>(".a4-section") as HTMLElement).findByRole("button", { name: "✦ לנסח" }));
    await waitFor(() => expect(second).toHaveClass("is-focus"));
    await within(second as HTMLElement).findByRole("group", { name: /^הטיוטה של/ });
    // The first draft, out of focus, waits its turn, with no buttons.
    expect(first).not.toHaveClass("is-focus");
    expect(within(first).getByText("טיוטה, מחכה לתורה")).toBeInTheDocument();
    expect(within(first).queryByRole("button", { name: "✓ לאשר" })).toBeNull();

    const head = within(second as HTMLElement).getByRole("heading", { level: 3 });
    head.focus();
    fireEvent.keyDown(head, { key: "Enter", code: "Enter", ctrlKey: true });
    expect(within(second as HTMLElement).getByRole("group", { name: "מה לשנות בטיוטה" })).toBeInTheDocument();
    fireEvent.keyDown(head, { key: "Escape", code: "Escape" });
    expect(within(second as HTMLElement).queryByRole("group", { name: "מה לשנות בטיוטה" })).toBeNull();

    fireEvent.keyDown(head, { key: "Enter", code: "Enter" });
    await waitFor(() => expect(first).toHaveClass("is-focus"));
    const firstHead = within(first).getByRole("heading", { level: 3 });
    await waitFor(() => expect(document.activeElement).toBe(firstHead));
    fireEvent.keyDown(firstHead, { key: "Enter", code: "Enter" });
    await waitFor(() => expect(screen.queryByRole("button", { name: "✓ לאשר" })).toBeNull());
    expect(calls.filter((c) => c.cmd === "approve_paragraphs").length).toBeGreaterThanOrEqual(2);
  });

  it("a held Enter approves one draft, not the whole report", async () => {
    const { calls, user, page } = await openReport();
    const first = focused(page);
    if (!first) throw new Error("no section in focus");
    await user.click(await within(first).findByRole("button", { name: "✦ לנסח" }));
    await within(first).findByRole("group", { name: /^הטיוטה של/ });
    const line = page.querySelector<HTMLElement>(".a4-gap-line");
    const second = line?.closest<HTMLElement>(".a4-section");
    if (!line || !second) throw new Error("no empty section left");
    await user.click(within(line).getByRole("button", { name: "✦ לנסח" }));
    // Out of focus it opens the section's box (D-140); the box's "✦ לנסח" sends.
    await user.click(await within(line.closest<HTMLElement>(".a4-section") as HTMLElement).findByRole("button", { name: "✦ לנסח" }));
    await within(second).findByRole("group", { name: /^הטיוטה של/ });
    const head = within(second).getByRole("heading", { level: 3 });
    head.focus();
    // The key held down: the first press, then the keyboard's repeats.
    fireEvent.keyDown(head, { key: "Enter", code: "Enter" });
    for (let i = 0; i < 5; i++) fireEvent.keyDown(document.activeElement ?? head, { key: "Enter", code: "Enter", repeat: true });
    // The focus moves on to the first draft; the key is still held.
    const firstHead = within(first).getByRole("heading", { level: 3 });
    await waitFor(() => expect(document.activeElement).toBe(firstHead));
    for (let i = 0; i < 5; i++) fireEvent.keyDown(firstHead, { key: "Enter", code: "Enter", repeat: true });
    await new Promise((r) => setTimeout(r, 500));
    expect(calls.filter((c) => c.cmd === "approve_paragraphs").length).toBeGreaterThan(0);
    // The first draft is still waiting for its own press.
    expect(within(first).getByRole("button", { name: "✓ לאשר" })).toBeInTheDocument();
  });

  it("in full page, with the contents hidden, the next step comes back under the page", async () => {
    const { user } = await openReport();
    expect(screen.queryByRole("contentinfo", { name: "הצעד הבא" })).toBeNull();
    await user.click(screen.getByRole("button", { name: "דף מלא" }));
    expect(screen.queryByRole("complementary", { name: "תוכן הדוח" })).toBeNull();
    expect(screen.getByRole("contentinfo", { name: "הצעד הבא" })).toBeInTheDocument();
    await user.click(screen.getByRole("button", { name: "דף מלא" }));
  });

  it("the contents say how far the report is, and the two keys", async () => {
    await openReport();
    const contents = screen.getByRole("complementary", { name: "תוכן הדוח" });
    expect(within(contents).getByText(/מאושרים$/)).toBeInTheDocument();
    expect(within(contents).getByText("לאשר ולעבור הלאה")).toBeInTheDocument();
    // The section in focus is marked where she is, in the contents too.
    expect(within(contents).getAllByRole("button").filter((b) => b.getAttribute("aria-current") === "location")).toHaveLength(1);
    // The next step is beside the report now, not in a bar under it.
    expect(screen.queryByRole("contentinfo", { name: "הצעד הבא" })).toBeNull();
  });
});
