// Round 48: the report page as the document, one section in focus. The four states, the brief
// that goes with the first draft, and Enter that approves and moves on. Every name is invented.
import { act, configure, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { mockIPC } from "@tauri-apps/api/mocks";
import { App, queryClient } from "../../App";
import { FakeCore } from "../../web/fakeCore";
import type { CaseDetail } from "../../ipc/client";
import type { CaseSummary } from "../../ipc/generated/CaseSummary";
import { draftRequest, firstFocus, nextWaiting, scrollShift } from "./ReportView";
import { DRAFT_FROM_APPROVED, DRAFT_FROM_SOURCES } from "./instructions";
import pageCss from "./ReportView.css?raw";

configure({ asyncUtilTimeout: 6000 });
vi.setConfig({ testTimeout: 30_000 });

type Section = CaseDetail["sections"][number];

function section(key: string, paragraphs: Partial<Section["paragraphs"][number]>[] = [], extra: Partial<Section> = {}): Section {
  return {
    key,
    title: key,
    part: "א",
    paragraphs: paragraphs.map((p, i) => ({ id: `${key}-${i}`, text: "טקסט בדוי", status: "approved", by_ai: true, sources: [], warnings: [], polish: [], notes: [], replaces: null, has_versions: false, style_note: null, style: null, ...p })),
    approved: paragraphs.some((p) => (p.status ?? "approved") === "approved"),
    source_count: 1,
    sortable: true,
    mode: "ai",
    ...extra,
  } as Section;
}

describe("a paragraph clicked out of focus stays where it was", () => {
  it("scrolls by what it moved, and not for less than a pixel", () => {
    expect(scrollShift(300, 352)).toBe(52);
    expect(scrollShift(300, 260)).toBe(-40);
    expect(scrollShift(300, 300.4)).toBe(0);
  });
});

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

async function openReport(reshape?: (d: CaseDetail) => void, prep?: (core: FakeCore) => Promise<void>) {
  const core = new FakeCore();
  await core.handle("unlock", { password: "x" });
  await core.handle("set_review_always", { on: false });
  await prep?.(core);
  const calls: { cmd: string; args: Record<string, unknown> }[] = [];
  mockIPC(async (cmd, args) => {
    calls.push({ cmd, args: (args ?? {}) as Record<string, unknown> });
    const out = await core.handle(cmd, args);
    if (cmd !== "case_detail") return out;
    const d = structuredClone(out as CaseDetail);
    // These tests give their own notes: the demo case's own ones are taken off first.
    for (const x of d.sections.flatMap((s) => s.paragraphs)) Object.assign(x, { notes: [], polish: [] });
    reshape?.(d);
    return d;
  });
  const user = userEvent.setup();
  render(<App />);
  await user.click(await screen.findByRole("button", { name: /בתיק של נועם$/ }));
  const page = await screen.findByRole("article", { name: "הדוח, כמו בקובץ" });
  return { calls, user, page };
}

const focused = (page: HTMLElement) => page.querySelector<HTMLElement>(".a4-section.is-focus");

describe("proofreading under a paragraph (D-167)", () => {
  it("folds the notes into one quiet line that opens on a click, apart from the warnings", async () => {
    const { page, user } = await openReport((d) => {
      const p = d.sections.flatMap((s) => s.paragraphs).find((x) => x.status === "approved");
      if (p) {
        p.polish = ["מילה כפולה: \"של של\"", "הפסקה לא מסתיימת בסימן פיסוק (אחרי \"בגן\")"];
        p.warnings = [];
      }
    });
    const note = await within(page).findByRole("group", { name: "הגהה" });
    expect(note.tagName).toBe("DETAILS");
    expect(note).toHaveClass("note-polish");
    expect(note).not.toHaveClass("note-warn");
    // Folded: one line with the count, the notes themselves not shown.
    expect(note).not.toHaveAttribute("open");
    const line = within(note).getByText("הגהה · 2");
    expect(line.tagName).toBe("SUMMARY");
    expect(within(note).queryByText("מילה כפולה: \"של של\"")).not.toBeVisible();
    await user.click(line);
    expect(note).toHaveAttribute("open");
    expect(within(note).getAllByRole("listitem").map((li) => li.textContent)).toEqual([
      "מילה כפולה: \"של של\"",
      "הפסקה לא מסתיימת בסימן פיסוק (אחרי \"בגן\")",
    ]);
    expect(within(note).getByText("מילה כפולה: \"של של\"")).toBeVisible();
    // Plain text: nothing in a note is read as markup.
    expect(note.querySelector("b, i, a, script")).toBeNull();
  });

  it("a paragraph with no notes has no proofreading block", async () => {
    const { page } = await openReport();
    await within(page).findAllByText(/./);
    expect(within(page).queryByRole("group", { name: "הגהה" })).toBeNull();
  });
});

describe("the report page, one section in focus", () => {
  it("only the section in focus has buttons; the brief goes with its first draft", async () => {
    const { calls, user, page } = await openReport();
    const first = focused(page);
    if (!first) throw new Error("no section in focus");
    expect(page.querySelectorAll(".a4-section.is-focus")).toHaveLength(1);
    // Before the draft: what matters to her here, behind its quiet line (D-204), then "✦ לנסח".
    await user.click(await within(first).findByRole("button", { name: "איך לנסח? (לא חובה)" }));
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
    await user.click(await within(first).findByRole("button", { name: "✦ לנסח את הסעיף" }));
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
    await user.click(await within(first).findByRole("button", { name: "איך לנסח? (לא חובה)" }));
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
    await user.click(await within(first).findByRole("button", { name: "✦ לנסח את הסעיף" }));
    await within(first).findByRole("group", { name: /^הטיוטה של/ });
    const line = page.querySelector<HTMLElement>(".a4-gap-line");
    if (!line) throw new Error("no empty section left");
    const second = line.closest<HTMLElement>(".a4-section");
    await user.click(within(line).getByRole("button", { name: "✦ לנסח" }));
    // Out of focus it brings the section into focus (D-140); its "✦ לנסח את הסעיף" sends.
    await user.click(await within(line.closest<HTMLElement>(".a4-section") as HTMLElement).findByRole("button", { name: "✦ לנסח את הסעיף" }));
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
    await user.click(await within(first).findByRole("button", { name: "✦ לנסח את הסעיף" }));
    await within(first).findByRole("group", { name: /^הטיוטה של/ });
    const line = page.querySelector<HTMLElement>(".a4-gap-line");
    const second = line?.closest<HTMLElement>(".a4-section");
    if (!line || !second) throw new Error("no empty section left");
    await user.click(within(line).getByRole("button", { name: "✦ לנסח" }));
    // Out of focus it brings the section into focus (D-140); its "✦ לנסח את הסעיף" sends.
    await user.click(await within(line.closest<HTMLElement>(".a4-section") as HTMLElement).findByRole("button", { name: "✦ לנסח את הסעיף" }));
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

describe("the brief behind a quiet line (D-204)", () => {
  const BOX = { name: /^איך לנסח את הסעיף/ };

  it("the brief starts folded behind “איך לנסח? (לא חובה)”, aria-expanded false", async () => {
    const { page, user } = await openReport();
    const first = focused(page);
    if (!first) throw new Error("no section in focus");
    const toggle = await within(first).findByRole("button", { name: "איך לנסח? (לא חובה)" });
    expect(toggle).toHaveAttribute("aria-expanded", "false");
    expect(within(first).queryByRole("textbox", BOX)).toBeNull();
    // One row: the draft, her own writing, and the quiet line; nothing else was added.
    const row = toggle.closest(".a4-brief-closed") as HTMLElement;
    expect(within(row).getAllByRole("button").map((b) => b.textContent)).toEqual(["✦ לנסח את הסעיף", "לכתוב בעצמי", "איך לנסח? (לא חובה)"]);
    expect(within(row).getByRole("button", { name: "✦ לנסח את הסעיף" })).toHaveClass("btn", "btn-primary");
    expect(page.querySelectorAll(".a4-brief")).toHaveLength(1);
    // Folded, the section's guide is in its "⋯".
    expect(within(first).queryByRole("button", { name: "איך לכתוב את הסעיף ←" })).toBeNull();
    await user.click(within(first).getByRole("button", { name: /^עוד על הסעיף/ }));
    expect(await screen.findByRole("menuitem", { name: "איך לכתוב את הסעיף" })).toBeInTheDocument();
    await user.keyboard("{Escape}");
    await waitFor(() => expect(screen.queryByRole("menuitem", { name: "איך לכתוב את הסעיף" })).toBeNull());
    // Open: the box, with the keyboard in it, and the guide's link under it, as before.
    await user.click(toggle);
    expect(within(first).getByRole("textbox", BOX)).toHaveFocus();
    expect(within(first).getByRole("button", { name: "איך לכתוב את הסעיף ←" })).toBeInTheDocument();
    expect(within(first).queryByRole("button", { name: "איך לנסח? (לא חובה)" })).toBeNull();
    expect(page.querySelectorAll(".a4-brief")).toHaveLength(1);
  });

  it("Escape or “ביטול” folds it back and the focus returns to that line", async () => {
    const { calls, page, user } = await openReport();
    const first = focused(page);
    if (!first) throw new Error("no section in focus");
    await user.click(await within(first).findByRole("button", { name: "איך לנסח? (לא חובה)" }));
    expect(within(first).getByRole("textbox", BOX)).toHaveFocus();
    await user.keyboard("{Escape}");
    const toggle = await within(first).findByRole("button", { name: "איך לנסח? (לא חובה)" });
    await waitFor(() => expect(toggle).toHaveFocus());
    expect(within(first).queryByRole("textbox", BOX)).toBeNull();
    await user.click(toggle);
    await user.click(within(first).getByRole("button", { name: "ביטול" }));
    const again = await within(first).findByRole("button", { name: "איך לנסח? (לא חובה)" });
    await waitFor(() => expect(again).toHaveFocus());
    expect(within(first).queryByRole("textbox", BOX)).toBeNull();
    // Nothing went out, and nothing was kept, on the way.
    expect(calls.some((c) => c.cmd === "prepare_section" || c.cmd === "keep_draft_brief")).toBe(false);
  });

  it("a wish kept for the section opens the box by itself, with its text", async () => {
    const { calls, page, user } = await openReport(undefined, async (core) => {
      const noam = ((await core.handle("list_cases", {})) as CaseSummary[]).find((c) => c.child_name === "נועם");
      if (!noam) throw new Error("no case");
      const d = (await core.handle("case_detail", { caseId: noam.id })) as CaseDetail;
      await core.handle("keep_draft_brief", { sectionKey: firstFocus(d.sections), text: "להתחיל מהתצפית בגן", keep: true });
    });
    const first = focused(page);
    if (!first) throw new Error("no section in focus");
    const box = await within(first).findByRole("textbox", BOX);
    expect(box).toHaveValue("להתחיל מהתצפית בגן");
    expect(within(first).getByRole("checkbox", { name: /לזכור לסעיף הזה/ })).toBeChecked();
    expect(within(first).queryByRole("button", { name: "איך לנסח? (לא חובה)" })).toBeNull();
    expect(within(first).queryByRole("button", { name: "✦ לנסח את הסעיף" })).toBeNull();
    // "ביטול" leaves a kept wish in sight: the keyboard goes to the heading, the box stays.
    await user.click(within(first).getByRole("button", { name: "ביטול" }));
    expect(within(first).getByRole("heading", { level: 3 })).toHaveFocus();
    expect(within(first).getByRole("textbox", BOX)).toHaveValue("להתחיל מהתצפית בגן");
    // Sent from the box, it goes with the request, as she sees it.
    await user.click(within(first).getByRole("button", { name: "✦ לנסח" }));
    await waitFor(() => expect(calls.some((c) => c.cmd === "prepare_section")).toBe(true));
    const asked = calls.find((c) => c.cmd === "prepare_section")?.args as { instruction: string };
    expect(asked.instruction).toContain("מה שחשוב לי שייכתב בסעיף: להתחיל מהתצפית בגן");
  });

  it("“✦ לנסח את הסעיף” prepares the section with no wish and keeps nothing (no keep_draft_brief)", async () => {
    const { calls, page, user } = await openReport();
    const first = focused(page);
    if (!first) throw new Error("no section in focus");
    await user.click(await within(first).findByRole("button", { name: "✦ לנסח את הסעיף" }));
    await waitFor(() => expect(calls.some((c) => c.cmd === "prepare_section")).toBe(true));
    const asked = calls.find((c) => c.cmd === "prepare_section")?.args as { sectionKey: string; instruction: string };
    expect(`sec-${asked.sectionKey}`).toBe(first.id);
    // The usual request, exactly what an empty box sends.
    expect(asked.instruction).toBe(DRAFT_FROM_SOURCES);
    await within(first).findByRole("group", { name: /^הטיוטה של/ });
    expect(calls.some((c) => c.cmd === "keep_draft_brief")).toBe(false);
  });

  it("Enter on an empty section's heading focuses “✦ לנסח את הסעיף”, and a second Enter drafts", async () => {
    const { calls, page, user } = await openReport();
    const first = focused(page);
    if (!first) throw new Error("no section in focus");
    const draft = await within(first).findByRole("button", { name: "✦ לנסח את הסעיף" });
    within(first).getByRole("heading", { level: 3 }).focus();
    await user.keyboard("{Enter}");
    expect(draft).toHaveFocus();
    expect(calls.some((c) => c.cmd === "prepare_section")).toBe(false);
    await user.keyboard("{Enter}");
    await waitFor(() => expect(calls.some((c) => c.cmd === "prepare_section")).toBe(true));
    await within(first).findByRole("group", { name: /^הטיוטה של/ });
  });
});

describe("the keyboard with the brief folded (D-204)", () => {
  const BOX = { name: /^איך לנסח את הסעיף/ };
  /** The stylesheet's selector that shows the sources line with the brief folded. */
  const sourcesShown = ".a4-section.is-focus:has(.a4-brief-closed) .a4-sources";

  it("a case opened from the cases list puts the keyboard on the heading of the empty section in focus, not on the page", async () => {
    const scrolled: { el: Element; arg: unknown }[] = [];
    const spy = vi.spyOn(Element.prototype, "scrollIntoView").mockImplementation(function (this: Element, arg?: unknown) {
      scrolled.push({ el: this, arg });
    });
    try {
      const { page } = await openReport();
      const first = focused(page);
      if (!first) throw new Error("no section in focus");
      await within(first).findByRole("button", { name: "✦ לנסח את הסעיף" });
      await waitFor(() => expect(within(first).getByRole("heading", { level: 3 })).toHaveFocus());
      // The section comes into view first, as a draft waiting does: on a laptop it may be below
      // the window, and "nearest" moves nothing when it is already in view.
      expect(scrolled.filter((x) => x.el === first).map((x) => x.arg)).toEqual([{ block: "nearest" }]);
      // The box stays folded, and nothing is typed into.
      expect(within(first).queryByRole("textbox", BOX)).toBeNull();
      // What it will be written from is said with the brief folded (shown by the stylesheet, below).
      expect(first.querySelector(".a4-sources")).toHaveTextContent(/^מתוך/);
      expect(page.querySelectorAll(sourcesShown)).toEqual(first.querySelectorAll(".a4-sources"));
    } finally {
      spy.mockRestore();
    }
  });

  it("the “✦ לנסח” line of a section out of focus brings it into focus with the keyboard on its heading", async () => {
    const { page, user } = await openReport();
    const first = focused(page);
    if (!first) throw new Error("no section in focus");
    await within(first).findByRole("button", { name: "✦ לנסח את הסעיף" });
    const line = within(page).getAllByRole("button", { name: "✦ לנסח" }).find((b) => b.closest(".a4-gap-line"));
    const other = line?.closest<HTMLElement>(".a4-section");
    if (!line || !other) throw new Error("no empty section out of focus");
    await user.click(line);
    await waitFor(() => expect(other).toHaveClass("is-focus"));
    // The line went away under the click; the keyboard is on the section, not lost on the page.
    await waitFor(() => expect(within(other).getByRole("heading", { level: 3 })).toHaveFocus());
    expect(document.activeElement).not.toBe(document.body);
    expect(within(other).getByRole("button", { name: "✦ לנסח את הסעיף" })).toBeInTheDocument();
  });

  it("a paragraph reached by the keyboard out of focus keeps it: the heading never takes it from where she is", async () => {
    const { page } = await openReport();
    const first = focused(page);
    if (!first) throw new Error("no section in focus");
    await within(first).findByRole("button", { name: "✦ לנסח את הסעיף" });
    const para = page.querySelector<HTMLElement>(".a4-section:not(.is-focus) .a4-para:not(.is-proposed) .a4-p");
    const other = para?.closest<HTMLElement>(".a4-section");
    if (!para || !other) throw new Error("no approved paragraph out of focus");
    // Tab arriving on a paragraph brings its section into focus (round 48); she stays on it.
    act(() => para.focus());
    await waitFor(() => expect(other).toHaveClass("is-focus"));
    await new Promise((r) => setTimeout(r, 50));
    expect(para).toHaveFocus();
    expect(within(other).getByRole("heading", { level: 3 })).not.toHaveFocus();
  });

  it("the stylesheet shows the sources line with the brief folded, with nothing focused in it, and only then", () => {
    const rule = pageCss.split("}").find((r) => r.split("{")[0]?.split(",").some((sel) => sel.trim() === sourcesShown));
    expect(rule).toMatch(/opacity:\s*1;/);
    // Not for every empty section in focus: one with no material says so once, in its own line.
    expect(pageCss).not.toContain(".a4-section.is-focus.a4-st-empty .a4-sources");
    // The heading that takes the keyboard has a ring of its own.
    expect(pageCss).toMatch(/\.a4-heading:focus-visible \{[^}]*outline: 2px solid/);
  });

  it("an empty section in focus with no material keeps one line that says so: its sources line stays hidden", async () => {
    let key: string | null = null;
    const { page, user } = await openReport((d) => {
      // The first section to write, with nothing sorted to it.
      key ??= firstFocus(d.sections);
      const bare = d.sections.find((x) => x.key === key);
      if (!bare) throw new Error("no section to write");
      bare.source_count = 0;
      d.routing = d.routing.map((r) => ({ ...r, feeds: r.feeds.filter((k) => k !== key) }));
    });
    const first = page.querySelector<HTMLElement>(`#sec-${key}`);
    if (!first) throw new Error("no bare section");
    await user.click(within(first).getByRole("heading", { level: 3 }));
    await waitFor(() => expect(first).toHaveClass("is-focus", "a4-st-empty"));
    expect(await within(first).findByText("אין חומר לסעיף.")).toBeInTheDocument();
    expect(first.querySelector(".a4-sources")).toHaveTextContent("אין חומרים לסעיף.");
    // Not shown by the folded-brief rule: the gap line is the one that says it.
    expect(within(first).queryByRole("button", { name: "✦ לנסח את הסעיף" })).toBeNull();
    expect(page.querySelectorAll(sourcesShown)).toHaveLength(0);
    // D-210: it is still written with the AI, from her points only; the box opens on request.
    expect(within(first).queryByText("מה לכתוב בסעיף?")).toBeNull();
    await user.click(within(first).getByRole("button", { name: "✦ לנסח מנקודות" }));
    expect(await within(first).findByText("מה לכתוב בסעיף?")).toBeInTheDocument();
    expect(within(first).getByRole("button", { name: "✦ לנסח מהנקודות" })).toBeInTheDocument();
  });

  it("Enter on “✦ לנסח את הסעיף” keeps the keyboard on the section while it is written, and the next Enter approves the draft", async () => {
    const { calls, page, user } = await openReport();
    const first = focused(page);
    if (!first) throw new Error("no section in focus");
    const head = within(first).getByRole("heading", { level: 3 });
    const draft = await within(first).findByRole("button", { name: "✦ לנסח את הסעיף" });
    await waitFor(() => expect(head).toHaveFocus());
    await user.keyboard("{Enter}");
    expect(draft).toHaveFocus();
    await user.keyboard("{Enter}");
    // The button goes away while the request is prepared; the keyboard is not lost on the page.
    await waitFor(() => expect(calls.some((c) => c.cmd === "prepare_section")).toBe(true));
    await within(first).findByRole("group", { name: /^הטיוטה של/ });
    await waitFor(() => expect(head).toHaveFocus());
    expect(document.activeElement).not.toBe(document.body);
    // On the heading, Enter approves the draft that came in.
    await user.keyboard("{Enter}");
    await waitFor(() => expect(calls.some((c) => c.cmd === "approve_paragraphs")).toBe(true));
  });

  it("the box opens with her newest text, whatever folded it: never an older one, and empty when she emptied it", async () => {
    const { calls, page, user } = await openReport();
    const first = focused(page);
    if (!first) throw new Error("no section in focus");
    const other = page.querySelector<HTMLElement>(".a4-section:not(.is-focus)");
    if (!other) throw new Error("no other section");
    const away = async () => {
      await user.click(within(other).getByRole("heading", { level: 3 }));
      await waitFor(() => expect(other).toHaveClass("is-focus"));
      expect(within(first).queryByRole("textbox", BOX)).toBeNull();
    };
    const back = async () => {
      await user.click(within(first).getByRole("heading", { level: 3 }));
      await waitFor(() => expect(first).toHaveClass("is-focus"));
      return within(first).findByRole("textbox", BOX);
    };
    await user.click(await within(first).findByRole("button", { name: "איך לנסח? (לא חובה)" }));
    await user.type(within(first).getByRole("textbox", BOX), "להתחיל מהתצפית בגן");
    await user.keyboard("{Escape}");
    await user.click(await within(first).findByRole("button", { name: "איך לנסח? (לא חובה)" }));
    // She writes something else instead, and leaves the section without folding the box.
    const box = within(first).getByRole("textbox", BOX);
    expect(box).toHaveValue("להתחיל מהתצפית בגן");
    await user.clear(box);
    await user.type(box, "בלי מונחים מקצועיים");
    await away();
    expect(await back()).toHaveValue("בלי מונחים מקצועיים");
    // Emptied, it comes back empty: the older text never returns.
    await user.clear(within(first).getByRole("textbox", BOX));
    await away();
    expect(await back()).toHaveValue("");
    // Nothing went out, and nothing was kept, on the way.
    expect(calls.some((c) => c.cmd === "prepare_section" || c.cmd === "keep_draft_brief")).toBe(false);
  });

  it("her newest text also wins over a wish kept for the section, and over one whose request failed", async () => {
    let key = "";
    const real = FakeCore.prototype.handle;
    const spy = vi.spyOn(FakeCore.prototype, "handle").mockImplementation(function (this: FakeCore, cmd: string, args?: unknown) {
      if (cmd === "prepare_section") return Promise.reject(new Error("אין חיבור"));
      return real.call(this, cmd, args as never);
    });
    try {
      const { calls, page, user } = await openReport(undefined, async (core) => {
        const noam = ((await core.handle("list_cases", {})) as CaseSummary[]).find((c) => c.child_name === "נועם");
        if (!noam) throw new Error("no case");
        const d = (await core.handle("case_detail", { caseId: noam.id })) as CaseDetail;
        key = firstFocus(d.sections) ?? "";
        await core.handle("keep_draft_brief", { sectionKey: key, text: "נשמר לסעיף", keep: true });
      });
      const first = page.querySelector<HTMLElement>(`#sec-${key}`);
      const other = page.querySelector<HTMLElement>(".a4-section:not(.is-focus)");
      if (!first || !other) throw new Error("no sections");
      const away = async () => {
        await user.click(within(other).getByRole("heading", { level: 3 }));
        await waitFor(() => expect(other).toHaveClass("is-focus"));
      };
      const back = async () => {
        await user.click(within(first).getByRole("heading", { level: 3 }));
        await waitFor(() => expect(first).toHaveClass("is-focus"));
        return within(first).findByRole("textbox", BOX);
      };
      // The kept wish, edited and left: the edit comes back, not the kept text.
      const box = await within(first).findByRole("textbox", BOX);
      expect(box).toHaveValue("נשמר לסעיף");
      await user.clear(box);
      await user.type(box, "ישן");
      await away();
      expect(await back()).toHaveValue("ישן");
      // Sent, and the request fails: the box shows what was sent.
      await user.click(within(first).getByRole("button", { name: "✦ לנסח" }));
      await waitFor(() => expect(calls.some((c) => c.cmd === "prepare_section")).toBe(true));
      expect(await within(first).findByRole("textbox", BOX)).toHaveValue("ישן");
      // She writes something else and leaves: the newest text comes back, not the one sent.
      await user.clear(within(first).getByRole("textbox", BOX));
      await user.type(within(first).getByRole("textbox", BOX), "חדש");
      await away();
      expect(await back()).toHaveValue("חדש");
      // Emptied, it comes back empty.
      await user.clear(within(first).getByRole("textbox", BOX));
      await away();
      expect(await back()).toHaveValue("");
    } finally {
      spy.mockRestore();
    }
  });

  /** The report over a case whose section in focus has a wish kept for it ("נשמר לסעיף"). */
  async function withKeptWish() {
    let key = "";
    const open = await openReport(undefined, async (core) => {
      const noam = ((await core.handle("list_cases", {})) as CaseSummary[]).find((c) => c.child_name === "נועם");
      if (!noam) throw new Error("no case");
      const d = (await core.handle("case_detail", { caseId: noam.id })) as CaseDetail;
      key = firstFocus(d.sections) ?? "";
      await core.handle("keep_draft_brief", { sectionKey: key, text: "נשמר לסעיף", keep: true });
    });
    const first = open.page.querySelector<HTMLElement>(`#sec-${key}`);
    if (!first) throw new Error("no section");
    return { ...open, first, key };
  }
  const KEEP = { name: /לזכור לסעיף הזה/ };
  const SHORT = { name: "קצר וממוקד" };

  it("the box comes back as she left it: her text, the style she pressed and “לזכור” unticked", async () => {
    const { calls, page, user, first } = await withKeptWish();
    const other = page.querySelector<HTMLElement>(".a4-section:not(.is-focus)");
    if (!other) throw new Error("no other section");
    const box = await within(first).findByRole("textbox", BOX);
    expect(box).toHaveValue("נשמר לסעיף");
    expect(within(first).getByRole("checkbox", KEEP)).toBeChecked();
    // For this child only: not kept for the section, a style, and words of her own.
    await user.click(within(first).getByRole("checkbox", KEEP));
    await user.click(within(first).getByRole("button", SHORT));
    await user.clear(box);
    await user.type(box, "רק לילד הזה");
    await user.click(within(other).getByRole("heading", { level: 3 }));
    await waitFor(() => expect(other).toHaveClass("is-focus"));
    await user.click(within(first).getByRole("heading", { level: 3 }));
    await waitFor(() => expect(first).toHaveClass("is-focus"));
    const again = await within(first).findByRole("textbox", BOX);
    expect(again).toHaveValue("רק לילד הזה");
    expect(within(first).getByRole("checkbox", KEEP)).not.toBeChecked();
    expect(within(first).getByRole("button", SHORT)).toHaveAttribute("aria-pressed", "true");
    // Enter sends it as she left it: with the style, and not kept for the other children.
    again.focus();
    await user.keyboard("{Enter}");
    await waitFor(() => expect(calls.some((c) => c.cmd === "keep_draft_brief")).toBe(true));
    expect(calls.find((c) => c.cmd === "keep_draft_brief")?.args).toMatchObject({ text: "רק לילד הזה", keep: false });
    await waitFor(() => expect(calls.some((c) => c.cmd === "prepare_section")).toBe(true));
    const asked = calls.find((c) => c.cmd === "prepare_section")?.args as { instruction: string };
    expect(asked.instruction).toContain("כתבי בקצרה ובממוקד. רק לילד הזה");
  });

  it("leaving the report page and coming back finds the box as she left it, not the older kept wish", async () => {
    const { calls, user, first, key } = await withKeptWish();
    const box = await within(first).findByRole("textbox", BOX);
    await user.click(within(first).getByRole("checkbox", KEEP));
    await user.click(within(first).getByRole("button", SHORT));
    await user.clear(box);
    await user.type(box, "החדש שלי");
    const stages = screen.getByRole("navigation", { name: "שלבי התיק" });
    await user.click(within(stages).getByRole("button", { name: /חומרים/ }));
    await waitFor(() => expect(screen.queryByRole("article", { name: "הדוח, כמו בקובץ" })).toBeNull());
    await user.click(within(stages).getByRole("button", { name: /כתיבה/ }));
    const page = await screen.findByRole("article", { name: "הדוח, כמו בקובץ" });
    const section = page.querySelector<HTMLElement>(`#sec-${key}`);
    if (!section) throw new Error("no section");
    if (!section.classList.contains("is-focus")) await user.click(within(section).getByRole("heading", { level: 3 }));
    expect(await within(section).findByRole("textbox", BOX)).toHaveValue("החדש שלי");
    expect(within(section).getByRole("checkbox", KEEP)).not.toBeChecked();
    expect(within(section).getByRole("button", SHORT)).toHaveAttribute("aria-pressed", "true");
    // Nothing went out, and nothing was kept, on the way.
    expect(calls.some((c) => c.cmd === "prepare_section" || c.cmd === "keep_draft_brief")).toBe(false);
  });

  it("what the box held is gone once the vault locks, and never reached the core", async () => {
    const { calls, user, first, key } = await withKeptWish();
    const box = await within(first).findByRole("textbox", BOX);
    await user.click(within(first).getByRole("checkbox", KEEP));
    await user.clear(box);
    await user.type(box, "טיוטה שלא נשלחה");
    fireEvent.keyDown(window, { key: "l", code: "KeyL", ctrlKey: true });
    await user.type(await screen.findByLabelText("סיסמה"), "x");
    await user.click(screen.getByRole("button", { name: "פתיחה" }));
    await user.click(await screen.findByRole("button", { name: /בתיק של נועם$/ }));
    const page = await screen.findByRole("article", { name: "הדוח, כמו בקובץ" });
    const section = page.querySelector<HTMLElement>(`#sec-${key}`);
    if (!section) throw new Error("no section");
    if (!section.classList.contains("is-focus")) await user.click(within(section).getByRole("heading", { level: 3 }));
    expect(await within(section).findByRole("textbox", BOX)).toHaveValue("נשמר לסעיף");
    expect(within(section).getByRole("checkbox", KEEP)).toBeChecked();
    expect(JSON.stringify(calls)).not.toContain("טיוטה שלא נשלחה");
  });

  it("the box opened and left for another stage is open again on the way back, with her text", async () => {
    const { user, page } = await openReport();
    const first = focused(page);
    if (!first) throw new Error("no section in focus");
    const key = first.id;
    await user.click(await within(first).findByRole("button", { name: "איך לנסח? (לא חובה)" }));
    await user.type(within(first).getByRole("textbox", BOX), "להתחיל מהתצפית");
    const stages = screen.getByRole("navigation", { name: "שלבי התיק" });
    await user.click(within(stages).getByRole("button", { name: /חומרים/ }));
    await waitFor(() => expect(screen.queryByRole("article", { name: "הדוח, כמו בקובץ" })).toBeNull());
    await user.click(within(stages).getByRole("button", { name: /כתיבה/ }));
    const back = await screen.findByRole("article", { name: "הדוח, כמו בקובץ" });
    const section = back.querySelector<HTMLElement>(`#${key}`);
    if (!section) throw new Error("no section");
    if (!section.classList.contains("is-focus")) await user.click(within(section).getByRole("heading", { level: 3 }));
    expect(await within(section).findByRole("textbox", BOX)).toHaveValue("להתחיל מהתצפית");
  });

  it("a draft left before a wish was kept for the section in another report never drops that newer wish unseen", async () => {
    const core = new FakeCore();
    await core.handle("unlock", { password: "x" });
    await core.handle("set_review_always", { on: false });
    const noam = ((await core.handle("list_cases", {})) as CaseSummary[]).find((c) => c.child_name === "נועם");
    if (!noam) throw new Error("no case");
    const key = firstFocus(((await core.handle("case_detail", { caseId: noam.id })) as CaseDetail).sections) ?? "";
    const calls: { cmd: string; args: Record<string, unknown> }[] = [];
    mockIPC(async (cmd, args) => {
      calls.push({ cmd, args: (args ?? {}) as Record<string, unknown> });
      return core.handle(cmd, args);
    });
    const user = userEvent.setup();
    render(<App />);
    await user.click(await screen.findByRole("button", { name: /בתיק של נועם$/ }));
    const section = async () => {
      const page = await screen.findByRole("article", { name: "הדוח, כמו בקובץ" });
      const sec = page.querySelector<HTMLElement>(`#sec-${key}`);
      if (!sec) throw new Error("no section");
      return sec;
    };
    // No wish yet: she opens the box, types, and leaves the writing page.
    let sec = await section();
    await user.click(await within(sec).findByRole("button", { name: "איך לנסח? (לא חובה)" }));
    await user.type(within(sec).getByRole("textbox", BOX), "טיוטה ישנה");
    const stages = screen.getByRole("navigation", { name: "שלבי התיק" });
    await user.click(within(stages).getByRole("button", { name: /חומרים/ }));
    await waitFor(() => expect(screen.queryByRole("article", { name: "הדוח, כמו בקובץ" })).toBeNull());
    // Meanwhile, in another child's report, she keeps a wish for this section (as sendBrief does).
    queryClient.setQueryData(["draftBriefs"], await core.handle("keep_draft_brief", { sectionKey: key, text: "משאלה חדשה שנשמרה", keep: true }));
    // Back: the newer kept wish is in the box, ticked, not the older draft; Enter keeps it.
    await user.click(within(stages).getByRole("button", { name: /כתיבה/ }));
    sec = await section();
    if (!sec.classList.contains("is-focus")) await user.click(within(sec).getByRole("heading", { level: 3 }));
    const box = await within(sec).findByRole("textbox", BOX);
    expect(box).toHaveValue("משאלה חדשה שנשמרה");
    expect(within(sec).getByRole("checkbox", { name: /לזכור לסעיף הזה/ })).toBeChecked();
    box.focus();
    await user.keyboard("{Enter}");
    await waitFor(() => expect(calls.filter((c) => c.cmd === "keep_draft_brief")).toHaveLength(1));
    const kept = ((await core.handle("draft_briefs", {})) as { sections: Record<string, string> }).sections[key];
    expect(kept).toBe("משאלה חדשה שנשמרה");
  });

  it("a draft behind a newer wish kept in another report is offered back (“להחזיר”), as she left it", async () => {
    const core = new FakeCore();
    await core.handle("unlock", { password: "x" });
    await core.handle("set_review_always", { on: false });
    const noam = ((await core.handle("list_cases", {})) as CaseSummary[]).find((c) => c.child_name === "נועם");
    if (!noam) throw new Error("no case");
    const key = firstFocus(((await core.handle("case_detail", { caseId: noam.id })) as CaseDetail).sections) ?? "";
    mockIPC(async (cmd, args) => core.handle(cmd, args));
    const user = userEvent.setup();
    render(<App />);
    await user.click(await screen.findByRole("button", { name: /בתיק של נועם$/ }));
    const section = async () => {
      const sec = (await screen.findByRole("article", { name: "הדוח, כמו בקובץ" })).querySelector<HTMLElement>(`#sec-${key}`);
      if (!sec) throw new Error("no section");
      if (!sec.classList.contains("is-focus")) await user.click(within(sec).getByRole("heading", { level: 3 }));
      return sec;
    };
    let sec = await section();
    await user.click(await within(sec).findByRole("button", { name: "איך לנסח? (לא חובה)" }));
    await user.type(within(sec).getByRole("textbox", BOX), "רק לילד הזה");
    await user.click(within(sec).getByRole("button", { name: "קצר וממוקד" }));
    const stages = screen.getByRole("navigation", { name: "שלבי התיק" });
    await user.click(within(stages).getByRole("button", { name: /חומרים/ }));
    await waitFor(() => expect(screen.queryByRole("article", { name: "הדוח, כמו בקובץ" })).toBeNull());
    queryClient.setQueryData(["draftBriefs"], await core.handle("keep_draft_brief", { sectionKey: key, text: "משאלה חדשה שנשמרה", keep: true }));
    await user.click(within(stages).getByRole("button", { name: /כתיבה/ }));
    sec = await section();
    // The newer wish is in the box; hers is one line under it, not gone.
    expect(await within(sec).findByRole("textbox", BOX)).toHaveValue("משאלה חדשה שנשמרה");
    // Her words and the style she pressed with them.
    expect(within(sec).getByText("מה שכתבת כאן ולא נשלח: ״רק לילד הזה״, בסגנון: קצר וממוקד")).toBeInTheDocument();
    await user.click(within(sec).getByRole("button", { name: "להחזיר" }));
    // Back as she left it: her text, her style, "לזכור" unticked; the line is gone.
    await waitFor(() => expect(within(sec).getByRole("textbox", BOX)).toHaveValue("רק לילד הזה"));
    expect(within(sec).getByRole("button", { name: "קצר וממוקד" })).toHaveAttribute("aria-pressed", "true");
    expect(within(sec).getByRole("checkbox", { name: /לזכור לסעיף הזה/ })).not.toBeChecked();
    expect(within(sec).queryByRole("button", { name: "להחזיר" })).toBeNull();
    // Left again and back: still hers (the box that held the kept wish did not count as left).
    await user.click(within(stages).getByRole("button", { name: /חומרים/ }));
    await waitFor(() => expect(screen.queryByRole("article", { name: "הדוח, כמו בקובץ" })).toBeNull());
    await user.click(within(stages).getByRole("button", { name: /כתיבה/ }));
    sec = await section();
    expect(await within(sec).findByRole("textbox", BOX)).toHaveValue("רק לילד הזה");
  });

  /** The demo case open on its report, a wish perhaps kept for `key` first, and a way back to a
   *  section (in focus) after leaving the writing page or moving to another section. */
  async function reportAt(key: (sections: CaseDetail["sections"]) => string, kept?: string, reviewAlways = false) {
    const core = new FakeCore();
    await core.handle("unlock", { password: "x" });
    await core.handle("set_review_always", { on: reviewAlways });
    const noam = ((await core.handle("list_cases", {})) as CaseSummary[]).find((c) => c.child_name === "נועם");
    if (!noam) throw new Error("no case");
    const k = key(((await core.handle("case_detail", { caseId: noam.id })) as CaseDetail).sections);
    if (kept) await core.handle("keep_draft_brief", { sectionKey: k, text: kept, keep: true });
    const calls: string[] = [];
    mockIPC(async (cmd, args) => {
      calls.push(cmd);
      return core.handle(cmd, args);
    });
    const user = userEvent.setup();
    render(<App />);
    await user.click(await screen.findByRole("button", { name: /בתיק של נועם$/ }));
    const section = async (which = k) => {
      const sec = (await screen.findByRole("article", { name: "הדוח, כמו בקובץ" })).querySelector<HTMLElement>(`#sec-${which}`);
      if (!sec) throw new Error("no section");
      if (!sec.classList.contains("is-focus")) await user.click(within(sec).getByRole("heading", { level: 3 }));
      return sec;
    };
    const stages = () => screen.getByRole("navigation", { name: "שלבי התיק" });
    const away = async () => {
      await user.click(within(stages()).getByRole("button", { name: /חומרים/ }));
      await waitFor(() => expect(screen.queryByRole("article", { name: "הדוח, כמו בקובץ" })).toBeNull());
    };
    const back = async () => {
      await user.click(within(stages()).getByRole("button", { name: /כתיבה/ }));
      return section();
    };
    const keepElsewhere = async (text: string, keep = true) =>
      queryClient.setQueryData(["draftBriefs"], await core.handle("keep_draft_brief", { sectionKey: k, text, keep }));
    return { core, calls, user, key: k, section, away, back, keepElsewhere };
  }

  it("a kept wish she only looked at is not offered back as hers when a newer one is kept elsewhere", async () => {
    const r = await reportAt((ss) => firstFocus(ss) ?? "", "המשאלה הישנה");
    let sec = await r.section();
    expect(await within(sec).findByRole("textbox", BOX)).toHaveValue("המשאלה הישנה");
    await r.away();
    await r.keepElsewhere("המשאלה החדשה");
    sec = await r.back();
    expect(await within(sec).findByRole("textbox", BOX)).toHaveValue("המשאלה החדשה");
    expect(within(sec).queryByRole("button", { name: "להחזיר" })).toBeNull();
    expect(within(sec).queryByText(/ולא נשלח/)).toBeNull();
  });

  it("a kept wish she only looked at, removed elsewhere, does not come back ticked", async () => {
    const r = await reportAt((ss) => firstFocus(ss) ?? "", "המשאלה הישנה");
    let sec = await r.section();
    expect(await within(sec).findByRole("textbox", BOX)).toHaveValue("המשאלה הישנה");
    await r.away();
    await r.keepElsewhere("משהו אחר", false);
    sec = await r.back();
    const toggle = within(sec).queryByRole("button", { name: "איך לנסח? (לא חובה)" });
    if (toggle) await r.user.click(toggle);
    expect(within(sec).getByRole("checkbox", { name: /לזכור לסעיף הזה/ })).not.toBeChecked();
    expect(within(sec).getByRole("textbox", BOX)).not.toHaveValue("המשאלה הישנה");
  });

  it("her draft stays offered under a newer kept wish across another section, the page left, and the review gone back from", async () => {
    const r = await reportAt((ss) => firstFocus(ss) ?? "", undefined, true);
    let sec = await r.section();
    await r.user.click(await within(sec).findByRole("button", { name: "איך לנסח? (לא חובה)" }));
    await r.user.type(within(sec).getByRole("textbox", BOX), "רק לילד הזה");
    await r.away();
    await r.keepElsewhere("המשאלה החדשה");
    sec = await r.back();
    const line = "מה שכתבת כאן ולא נשלח: ״רק לילד הזה״";
    expect(within(sec).getByText(line)).toBeInTheDocument();
    // Said with the box, too: the box is described by the line.
    const box = await within(sec).findByRole("textbox", BOX);
    expect(box).toHaveValue("המשאלה החדשה");
    expect(box).toHaveAccessibleDescription(line);
    // Another section, and back.
    const page = await screen.findByRole("article", { name: "הדוח, כמו בקובץ" });
    const other = Array.from(page.querySelectorAll<HTMLElement>(".a4-section")).find((x) => x.id !== `sec-${r.key}` && x.querySelector("h3"));
    if (!other) throw new Error("no other section");
    await r.user.click(within(other).getByRole("heading", { level: 3 }));
    await waitFor(() => expect(other).toHaveClass("is-focus"));
    sec = await r.section();
    expect(within(sec).getByText(line)).toBeInTheDocument();
    // The page left, and back.
    await r.away();
    sec = await r.back();
    expect(within(sec).getByText(line)).toBeInTheDocument();
    // Enter sends the newer wish; back from the review, hers is still offered.
    const again = await within(sec).findByRole("textbox", BOX);
    again.focus();
    await r.user.keyboard("{Enter}");
    const review = await screen.findByRole("dialog", { name: /לפני שליחה ל-/ });
    await r.user.click(within(review).getByRole("button", { name: "חזרה לעריכה" }));
    await waitFor(() => expect(screen.queryByRole("dialog", { name: /לפני שליחה ל-/ })).toBeNull());
    expect(r.calls).not.toContain("send_section");
    expect(await within(sec).findByText(line)).toBeInTheDocument();
    expect(within(sec).getByRole("textbox", BOX)).toHaveValue("המשאלה החדשה");
    // Taken back: the keyboard is in the box, with her words.
    await r.user.click(within(sec).getByRole("button", { name: "להחזיר" }));
    await waitFor(() => expect(within(sec).getByRole("textbox", BOX)).toHaveValue("רק לילד הזה"));
    await waitFor(() => expect(within(sec).getByRole("textbox", BOX)).toHaveFocus());
  });

  it("“להחזיר” leaves the keyboard in the box, also in the diagnoses, whose box never takes it by itself", async () => {
    const r = await reportAt(() => "diagnoses", "המשאלה הישנה");
    let sec = await r.section();
    const box = await within(sec).findByRole("textbox", BOX);
    expect(box).toHaveValue("המשאלה הישנה");
    await r.user.clear(box);
    await r.user.type(box, "רק לילד הזה");
    await r.away();
    await r.keepElsewhere("המשאלה החדשה");
    sec = await r.back();
    expect(await within(sec).findByRole("textbox", BOX)).toHaveValue("המשאלה החדשה");
    await r.user.click(within(sec).getByRole("button", { name: "להחזיר" }));
    await waitFor(() => expect(within(sec).getByRole("textbox", BOX)).toHaveValue("רק לילד הזה"));
    await waitFor(() => expect(within(sec).getByRole("textbox", BOX)).toHaveFocus());
  });

  it("a draft whose section's kept wish was only removed in another report opens as she left it", async () => {
    const core = new FakeCore();
    await core.handle("unlock", { password: "x" });
    await core.handle("set_review_always", { on: false });
    const noam = ((await core.handle("list_cases", {})) as CaseSummary[]).find((c) => c.child_name === "נועם");
    if (!noam) throw new Error("no case");
    const key = firstFocus(((await core.handle("case_detail", { caseId: noam.id })) as CaseDetail).sections) ?? "";
    await core.handle("keep_draft_brief", { sectionKey: key, text: "נשמר לסעיף", keep: true });
    mockIPC(async (cmd, args) => core.handle(cmd, args));
    const user = userEvent.setup();
    render(<App />);
    await user.click(await screen.findByRole("button", { name: /בתיק של נועם$/ }));
    const section = async () => {
      const sec = (await screen.findByRole("article", { name: "הדוח, כמו בקובץ" })).querySelector<HTMLElement>(`#sec-${key}`);
      if (!sec) throw new Error("no section");
      if (!sec.classList.contains("is-focus")) await user.click(within(sec).getByRole("heading", { level: 3 }));
      return sec;
    };
    let sec = await section();
    const box = await within(sec).findByRole("textbox", BOX);
    expect(box).toHaveValue("נשמר לסעיף");
    await user.click(within(sec).getByRole("checkbox", { name: /לזכור לסעיף הזה/ }));
    await user.click(within(sec).getByRole("button", { name: "קצר וממוקד" }));
    await user.clear(box);
    await user.type(box, "רק לילד הזה");
    const stages = screen.getByRole("navigation", { name: "שלבי התיק" });
    await user.click(within(stages).getByRole("button", { name: /חומרים/ }));
    await waitFor(() => expect(screen.queryByRole("article", { name: "הדוח, כמו בקובץ" })).toBeNull());
    // Another report sends with "לזכור" unticked: the kept wish is removed.
    queryClient.setQueryData(["draftBriefs"], await core.handle("keep_draft_brief", { sectionKey: key, text: "משהו אחר", keep: false }));
    await user.click(within(stages).getByRole("button", { name: /כתיבה/ }));
    sec = await section();
    const toggle = within(sec).queryByRole("button", { name: "איך לנסח? (לא חובה)" });
    if (toggle) await user.click(toggle);
    expect(await within(sec).findByRole("textbox", BOX)).toHaveValue("רק לילד הזה");
    expect(within(sec).getByRole("button", { name: "קצר וממוקד" })).toHaveAttribute("aria-pressed", "true");
    expect(within(sec).getByRole("checkbox", { name: /לזכור לסעיף הזה/ })).not.toBeChecked();
    expect(within(sec).queryByRole("button", { name: "להחזיר" })).toBeNull();
  });

  it("back from the review, the box opens as she sent it from: text, style and “לזכור”", async () => {
    const core = new FakeCore();
    await core.handle("unlock", { password: "x" });
    await core.handle("set_review_always", { on: true });
    const noam = ((await core.handle("list_cases", {})) as CaseSummary[]).find((c) => c.child_name === "נועם");
    if (!noam) throw new Error("no case");
    const key = firstFocus(((await core.handle("case_detail", { caseId: noam.id })) as CaseDetail).sections) ?? "";
    const calls: string[] = [];
    mockIPC(async (cmd, args) => {
      calls.push(cmd);
      return core.handle(cmd, args);
    });
    const user = userEvent.setup();
    render(<App />);
    await user.click(await screen.findByRole("button", { name: /בתיק של נועם$/ }));
    const sec = (await screen.findByRole("article", { name: "הדוח, כמו בקובץ" })).querySelector<HTMLElement>(`#sec-${key}`);
    if (!sec) throw new Error("no section");
    await user.click(await within(sec).findByRole("button", { name: "איך לנסח? (לא חובה)" }));
    await user.click(within(sec).getByRole("button", { name: "קצר וממוקד" }));
    const box = within(sec).getByRole("textbox", BOX);
    await user.type(box, "להתחיל מהתצפית");
    box.focus();
    await user.keyboard("{Enter}");
    const review = await screen.findByRole("dialog", { name: /לפני שליחה ל-/ });
    await user.click(within(review).getByRole("button", { name: "חזרה לעריכה" }));
    await waitFor(() => expect(screen.queryByRole("dialog", { name: /לפני שליחה ל-/ })).toBeNull());
    expect(calls).not.toContain("send_section");
    const again = await within(sec).findByRole("textbox", BOX);
    expect(again).toHaveValue("להתחיל מהתצפית");
    expect(within(sec).getByRole("button", { name: "קצר וממוקד" })).toHaveAttribute("aria-pressed", "true");
    expect(within(sec).getByRole("checkbox", { name: /לזכור לסעיף הזה/ })).not.toBeChecked();
  });

  it("a draft sent unchanged with “לזכור” ticked, back from the review, opens as she sent it: style too, and no line", async () => {
    const r = await reportAt((ss) => firstFocus(ss) ?? "", undefined, true);
    let sec = await r.section();
    await r.user.click(await within(sec).findByRole("button", { name: "איך לנסח? (לא חובה)" }));
    await r.user.type(within(sec).getByRole("textbox", BOX), "רק לילד הזה");
    await r.user.click(within(sec).getByRole("button", { name: "קצר וממוקד" }));
    await r.user.click(within(sec).getByRole("checkbox", { name: /לזכור לסעיף הזה/ }));
    // Another section and back: her draft is in the box.
    const page = await screen.findByRole("article", { name: "הדוח, כמו בקובץ" });
    const other = Array.from(page.querySelectorAll<HTMLElement>(".a4-section")).find((x) => x.id !== `sec-${r.key}` && x.querySelector("h3"));
    if (!other) throw new Error("no other section");
    await r.user.click(within(other).getByRole("heading", { level: 3 }));
    await waitFor(() => expect(other).toHaveClass("is-focus"));
    sec = await r.section();
    const box = await within(sec).findByRole("textbox", BOX);
    expect(box).toHaveValue("רק לילד הזה");
    // Sent as it is (its wish is kept for the section as she sends), then back from the review.
    box.focus();
    await r.user.keyboard("{Enter}");
    const review = await screen.findByRole("dialog", { name: /לפני שליחה ל-/ });
    await r.user.click(within(review).getByRole("button", { name: "חזרה לעריכה" }));
    await waitFor(() => expect(screen.queryByRole("dialog", { name: /לפני שליחה ל-/ })).toBeNull());
    expect(((await r.core.handle("draft_briefs", {})) as { sections: Record<string, string> }).sections[r.key]).toBe("רק לילד הזה");
    expect(await within(sec).findByRole("textbox", BOX)).toHaveValue("רק לילד הזה");
    await waitFor(() => expect(within(sec).getByRole("button", { name: "קצר וממוקד" })).toHaveAttribute("aria-pressed", "true"));
    expect(within(sec).getByRole("checkbox", { name: /לזכור לסעיף הזה/ })).toBeChecked();
    expect(within(sec).queryByText(/ולא נשלח/)).toBeNull();
    // And so it stays when she leaves the page and comes back.
    await r.away();
    sec = await r.back();
    expect(await within(sec).findByRole("textbox", BOX)).toHaveValue("רק לילד הזה");
    expect(within(sec).getByRole("button", { name: "קצר וממוקד" })).toHaveAttribute("aria-pressed", "true");
    expect(within(sec).queryByText(/ולא נשלח/)).toBeNull();
  });

  it("her untick, taken back over a newer wish kept in another report, never forgets that newer wish", async () => {
    const r = await reportAt((ss) => firstFocus(ss) ?? "", "המשאלה הישנה");
    let sec = await r.section();
    const box = await within(sec).findByRole("textbox", BOX);
    expect(box).toHaveValue("המשאלה הישנה");
    await r.user.click(within(sec).getByRole("checkbox", { name: /לזכור לסעיף הזה/ }));
    await r.user.clear(box);
    await r.user.type(box, "רק לילד הזה");
    await r.away();
    await r.keepElsewhere("המשאלה החדשה");
    sec = await r.back();
    expect(await within(sec).findByRole("textbox", BOX)).toHaveValue("המשאלה החדשה");
    await r.user.click(within(sec).getByRole("button", { name: "להחזיר" }));
    await waitFor(() => expect(within(sec).getByRole("textbox", BOX)).toHaveValue("רק לילד הזה"));
    expect(within(sec).getByRole("checkbox", { name: /לזכור לסעיף הזה/ })).not.toBeChecked();
    // Enter: her words go, for this child only; the wish kept since stays the section's.
    within(sec).getByRole("textbox", BOX).focus();
    await r.user.keyboard("{Enter}");
    await waitFor(() => expect(r.calls).toContain("prepare_section"));
    await waitFor(() => expect(r.calls).toContain("keep_draft_brief"));
    const briefs = (await r.core.handle("draft_briefs", {})) as { recent: string[]; sections: Record<string, string> };
    expect(briefs.sections[r.key]).toBe("המשאלה החדשה");
    expect(briefs.recent[0]).toBe("רק לילד הזה");
  });

  it("an untick in the box as it opens still forgets the wish kept for the section", async () => {
    const r = await reportAt((ss) => firstFocus(ss) ?? "", "המשאלה הישנה");
    const sec = await r.section();
    const box = await within(sec).findByRole("textbox", BOX);
    await r.user.click(within(sec).getByRole("checkbox", { name: /לזכור לסעיף הזה/ }));
    box.focus();
    await r.user.keyboard("{Enter}");
    await waitFor(() => expect(r.calls).toContain("keep_draft_brief"));
    await waitFor(async () => expect(((await r.core.handle("draft_briefs", {})) as { sections: Record<string, string> }).sections[r.key]).toBeUndefined());
  });

  it("a style pressed on a kept wish she only looked at is offered back as the style, with the newer wish", async () => {
    const r = await reportAt((ss) => firstFocus(ss) ?? "", "ישנה");
    let sec = await r.section();
    expect(await within(sec).findByRole("textbox", BOX)).toHaveValue("ישנה");
    await r.user.click(within(sec).getByRole("button", { name: "קצר וממוקד" }));
    await r.away();
    await r.keepElsewhere("חדשה");
    sec = await r.back();
    expect(await within(sec).findByRole("textbox", BOX)).toHaveValue("חדשה");
    // Not "what you wrote": the old kept wish was never her words here.
    expect(within(sec).getByText("הסגנון שבחרת כאן ולא נשלח: קצר וממוקד")).toBeInTheDocument();
    expect(within(sec).queryByText(/מה שכתבת/)).toBeNull();
    await r.user.click(within(sec).getByRole("button", { name: "להחזיר" }));
    await waitFor(() => expect(within(sec).getByRole("button", { name: "קצר וממוקד" })).toHaveAttribute("aria-pressed", "true"));
    expect(within(sec).getByRole("textbox", BOX)).toHaveValue("חדשה");
    expect(within(sec).getByRole("checkbox", { name: /לזכור לסעיף הזה/ })).toBeChecked();
    expect(within(sec).queryByRole("button", { name: "להחזיר" })).toBeNull();
  });

  it("styles left on an emptied or unticked box come back with the newer kept wish, ticked, as the line says", async () => {
    const r = await reportAt((ss) => firstFocus(ss) ?? "", "ישנה");
    let sec = await r.section();
    const box = await within(sec).findByRole("textbox", BOX);
    expect(box).toHaveValue("ישנה");
    await r.user.clear(box);
    await r.user.click(within(sec).getByRole("button", { name: "קצר וממוקד" }));
    await r.user.click(within(sec).getByRole("checkbox", { name: /לזכור לסעיף הזה/ }));
    await r.away();
    await r.keepElsewhere("חדשה");
    sec = await r.back();
    expect(await within(sec).findByRole("textbox", BOX)).toHaveValue("חדשה");
    expect(within(sec).getByText("הסגנון שבחרת כאן ולא נשלח: קצר וממוקד")).toBeInTheDocument();
    await r.user.click(within(sec).getByRole("button", { name: "להחזיר" }));
    await waitFor(() => expect(within(sec).getByRole("button", { name: "קצר וממוקד" })).toHaveAttribute("aria-pressed", "true"));
    // The box shows the section's kept wish, so "לזכור" is ticked, as in any box opened with it.
    expect(within(sec).getByRole("textbox", BOX)).toHaveValue("חדשה");
    expect(within(sec).getByRole("checkbox", { name: /לזכור לסעיף הזה/ })).toBeChecked();
  });

  it("a tick unticked by a removal elsewhere does not come back when the same wish is kept again", async () => {
    const r = await reportAt((ss) => firstFocus(ss) ?? "", "ק");
    let sec = await r.section();
    const box = await within(sec).findByRole("textbox", BOX);
    expect(box).toHaveValue("ק");
    await r.user.clear(box);
    await r.user.type(box, "שלי");
    await r.away();
    await r.keepElsewhere("משהו", false);
    // The wish that opened the box is gone: the box she worked in is still open, unticked.
    sec = await r.back();
    expect(within(sec).queryByRole("button", { name: "איך לנסח? (לא חובה)" })).toBeNull();
    expect(await within(sec).findByRole("textbox", BOX)).toHaveValue("שלי");
    expect(within(sec).getByRole("checkbox", { name: /לזכור לסעיף הזה/ })).not.toBeChecked();
    await r.away();
    await r.keepElsewhere("ק");
    // Kept again: a newer wish, in the box; hers is offered under it, and Enter keeps "ק".
    sec = await r.back();
    expect(await within(sec).findByRole("textbox", BOX)).toHaveValue("ק");
    expect(within(sec).getByRole("checkbox", { name: /לזכור לסעיף הזה/ })).toBeChecked();
    expect(within(sec).getByText("מה שכתבת כאן ולא נשלח: ״שלי״")).toBeInTheDocument();
  });

  it("toggling “לזכור” on her draft taken back never makes it about the newer wish it never showed", async () => {
    const r = await reportAt((ss) => firstFocus(ss) ?? "", "המשאלה הישנה");
    let sec = await r.section();
    const box = await within(sec).findByRole("textbox", BOX);
    await r.user.click(within(sec).getByRole("checkbox", { name: /לזכור לסעיף הזה/ }));
    await r.user.clear(box);
    await r.user.type(box, "רק לילד הזה");
    await r.away();
    await r.keepElsewhere("המשאלה החדשה");
    sec = await r.back();
    await r.user.click(within(sec).getByRole("button", { name: "להחזיר" }));
    await waitFor(() => expect(within(sec).getByRole("textbox", BOX)).toHaveValue("רק לילד הזה"));
    // Ticked, another section, back, unticked again, Enter.
    await r.user.click(within(sec).getByRole("checkbox", { name: /לזכור לסעיף הזה/ }));
    const page = await screen.findByRole("article", { name: "הדוח, כמו בקובץ" });
    const other = Array.from(page.querySelectorAll<HTMLElement>(".a4-section")).find((x) => x.id !== `sec-${r.key}` && x.querySelector("h3"));
    if (!other) throw new Error("no other section");
    await r.user.click(within(other).getByRole("heading", { level: 3 }));
    await waitFor(() => expect(other).toHaveClass("is-focus"));
    sec = await r.section();
    expect(await within(sec).findByRole("checkbox", { name: /לזכור לסעיף הזה/ })).toBeChecked();
    await r.user.click(within(sec).getByRole("checkbox", { name: /לזכור לסעיף הזה/ }));
    within(sec).getByRole("textbox", BOX).focus();
    await r.user.keyboard("{Enter}");
    await waitFor(() => expect(r.calls).toContain("keep_draft_brief"));
    const briefs = (await r.core.handle("draft_briefs", {})) as { recent: string[]; sections: Record<string, string> };
    expect(briefs.sections[r.key]).toBe("המשאלה החדשה");
  });

  it("her words, later kept as they are in another report, are not offered back again", async () => {
    const r = await reportAt((ss) => firstFocus(ss) ?? "");
    let sec = await r.section();
    await r.user.click(await within(sec).findByRole("button", { name: "איך לנסח? (לא חובה)" }));
    await r.user.type(within(sec).getByRole("textbox", BOX), "אותו נוסח");
    await r.away();
    await r.keepElsewhere("אותו נוסח");
    sec = await r.back();
    expect(await within(sec).findByRole("textbox", BOX)).toHaveValue("אותו נוסח");
    expect(within(sec).queryByText(/ולא נשלח/)).toBeNull();
    expect(within(sec).queryByRole("button", { name: "להחזיר" })).toBeNull();
  });

  it("a request that failed before her wish was kept never opens the box with what it held before", async () => {
    let release = () => {};
    const kept = new Promise<void>((done) => (release = done));
    const real = FakeCore.prototype.handle;
    const spy = vi.spyOn(FakeCore.prototype, "handle").mockImplementation(async function (this: FakeCore, cmd: string, args?: unknown) {
      if (cmd === "prepare_section") throw new Error("אין חיבור");
      if (cmd === "keep_draft_brief" && (args as { text?: string }).text === "שלי") await kept;
      return real.call(this, cmd, args as never);
    });
    try {
      const r = await reportAt((ss) => firstFocus(ss) ?? "", "ישנה");
      const sec = await r.section();
      const box = await within(sec).findByRole("textbox", BOX);
      await r.user.clear(box);
      await r.user.type(box, "שלי");
      await r.user.click(within(sec).getByRole("button", { name: "קצר וממוקד" }));
      box.focus();
      await r.user.keyboard("{Enter}");
      await waitFor(() => expect(r.calls).toContain("prepare_section"));
      // Failed, and her wish not kept yet: the box waits instead of opening with "ישנה".
      await waitFor(() => expect(within(sec).queryByRole("textbox", BOX)).toBeNull());
      expect(within(sec).queryByText(/ישנה/)).toBeNull();
      release();
      const again = await within(sec).findByRole("textbox", BOX);
      expect(again).toHaveValue("שלי");
      expect(within(sec).getByRole("button", { name: "קצר וממוקד" })).toHaveAttribute("aria-pressed", "true");
      expect(within(sec).getByRole("checkbox", { name: /לזכור לסעיף הזה/ })).toBeChecked();
      expect(within(sec).queryByText(/ולא נשלח/)).toBeNull();
    } finally {
      spy.mockRestore();
    }
  });

  it("“ביטול” and Escape fold the box but keep what she typed; it is there again when she opens it", async () => {
    const { calls, page, user } = await openReport();
    const first = focused(page);
    if (!first) throw new Error("no section in focus");
    await user.click(await within(first).findByRole("button", { name: "איך לנסח? (לא חובה)" }));
    await user.type(within(first).getByRole("textbox", BOX), "להתחיל מהתצפית בגן");
    await user.click(within(first).getByRole("button", { name: "ביטול" }));
    const toggle = await within(first).findByRole("button", { name: "איך לנסח? (לא חובה)" });
    await waitFor(() => expect(toggle).toHaveFocus());
    expect(within(first).queryByRole("textbox", BOX)).toBeNull();
    await user.click(toggle);
    const box = within(first).getByRole("textbox", BOX);
    expect(box).toHaveValue("להתחיל מהתצפית בגן");
    expect(box).toHaveFocus();
    // Escape folds it the same way, with what she added since.
    await user.type(box, ", בלי מונחים");
    await user.keyboard("{Escape}");
    await user.click(await within(first).findByRole("button", { name: "איך לנסח? (לא חובה)" }));
    expect(within(first).getByRole("textbox", BOX)).toHaveValue("להתחיל מהתצפית בגן, בלי מונחים");
    await user.keyboard("{Escape}");
    // Folded, "✦ לנסח את הסעיף" sends the usual request: what she folded away is never sent unseen.
    await user.click(await within(first).findByRole("button", { name: "✦ לנסח את הסעיף" }));
    await waitFor(() => expect(calls.some((c) => c.cmd === "prepare_section")).toBe(true));
    const asked = calls.find((c) => c.cmd === "prepare_section")?.args as { instruction: string };
    expect(asked.instruction).toBe(DRAFT_FROM_SOURCES);
    expect(calls.some((c) => c.cmd === "keep_draft_brief")).toBe(false);
  });
});
