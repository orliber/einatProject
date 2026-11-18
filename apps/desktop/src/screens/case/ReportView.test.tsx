// Round 48: the report page as the document, one section in focus. The four states, the brief
// that goes with the first draft, and Enter that approves and moves on. Every name is invented.
import { configure, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { mockIPC } from "@tauri-apps/api/mocks";
import { App } from "../../App";
import { FakeCore } from "../../web/fakeCore";
import type { CaseDetail } from "../../ipc/client";
import type { CaseSummary } from "../../ipc/generated/CaseSummary";
import { draftRequest, firstFocus, nextWaiting, scrollShift } from "./ReportView";
import { DRAFT_FROM_APPROVED, DRAFT_FROM_SOURCES } from "./instructions";

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
