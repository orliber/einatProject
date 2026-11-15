// D-168: everything on the main page, only for the section in focus. The section's guide, the
// DSM level and the lines of her past reports are each one quiet line that opens; on the
// diagnoses section her list leads, open, before "✦ לנסח"; on the DSM section the level is
// open until she sets one. Over the demo core; every name and detail is invented.
import { configure, render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { mockIPC } from "@tauri-apps/api/mocks";
import { App } from "../../App";
import { FakeCore } from "../../web/fakeCore";
import type { CaseDetail } from "../../ipc/client";
import { severityWords } from "./ReportView";

configure({ asyncUtilTimeout: 6000 });
vi.setConfig({ testTimeout: 30_000 });

const LINES = [
  { text: "מומלץ על הדרכת הורים.", reports: 5, in_similar: 0, similar: 0 },
  { text: "מומלץ על מעקב התפתחותי בעוד שנה.", reports: 3, in_similar: 0, similar: 0 },
];

async function openReport(reshape?: (d: CaseDetail) => void) {
  const core = new FakeCore();
  await core.handle("unlock", { password: "x" });
  await core.handle("set_review_always", { on: false });
  const calls: { cmd: string; args: Record<string, unknown> }[] = [];
  mockIPC(async (cmd, args) => {
    const a = (args ?? {}) as Record<string, unknown>;
    calls.push({ cmd, args: a });
    if (cmd === "section_library") return { lines: a.sectionKey === "diagnoses" || a.sectionKey === "recommendations" ? LINES : [], similar: [] };
    const out = await core.handle(cmd, args);
    if (cmd !== "case_detail" || !reshape) return out;
    const d = structuredClone(out as CaseDetail);
    reshape(d);
    return d;
  });
  const user = userEvent.setup();
  render(<App />);
  await user.click(await screen.findByRole("button", { name: /בתיק של נועם$/ }));
  const page = await screen.findByRole("article", { name: "הדוח, כמו בקובץ" });
  const focus = async (title: RegExp, key: string) => {
    await user.click(within(screen.getByRole("complementary", { name: "תוכן הדוח" })).getByRole("button", { name: title }));
    await waitFor(() => expect(document.querySelector(".a4-section.is-focus")?.id).toBe(`sec-${key}`));
    return document.getElementById(`sec-${key}`) as HTMLElement;
  };
  return { calls, user, page, focus };
}

describe("the section's panels on the report page (D-168)", () => {
  it("the guide opens on the page, under the section in focus, and closes again", async () => {
    const { page, user } = await openReport();
    const section = page.querySelector<HTMLElement>(".a4-section.is-focus");
    if (!section) throw new Error("no section in focus");
    // Until asked for, nothing of it is on the page.
    expect(within(page).queryByText("איך לכתוב את הסעיף הזה")).toBeNull();
    await user.click(await within(section).findByRole("button", { name: "איך לכתוב את הסעיף ←" }));
    // Still the report page: the guide is in the section, with its editor one click away.
    expect(screen.getByRole("article", { name: "הדוח, כמו בקובץ" })).toBeInTheDocument();
    expect(within(section).getByText("איך לכתוב את הסעיף הזה")).toBeInTheDocument();
    expect(within(section).getByRole("button", { name: "עריכה" })).toBeVisible();
    // Only the section in focus has it.
    expect(within(page).getAllByText("איך לכתוב את הסעיף הזה")).toHaveLength(1);
    await user.click(within(section).getByText("איך לכתוב את הסעיף הזה"));
    await waitFor(() => expect(within(page).queryByText("איך לכתוב את הסעיף הזה")).toBeNull());
  });

  it("the DSM level is open until she sets one, and one folded line after", async () => {
    const { focus, user, calls } = await openReport();
    const dsm = await focus(/^אבחנה על-פי DSM/, "dsm");
    const line = within(dsm).getByText("רמת החומרה: לא נקבעה");
    expect(line.tagName).toBe("SUMMARY");
    expect(line.closest("details")).toHaveAttribute("open");
    expect(within(dsm).getByText("עד שתקבעי דרגה, הטיוטה של הסעיף נכתבת בלי דרגה.")).toBeVisible();
    await user.selectOptions(within(dsm).getByLabelText("תקשורת חברתית ואינטראקציה חברתית"), "2");
    await waitFor(() => expect(calls.some((c) => c.cmd === "set_dsm_severity")).toBe(true));
    // It stays open while she chooses the second one; the line says what is set.
    expect(await within(dsm).findByText("רמת החומרה: תקשורת חברתית: דרגה 2 · התנהגות חוזרת: לא נקבעה")).toBeInTheDocument();
    expect(within(dsm).queryByText(/נכתבת בלי דרגה/)).toBeNull();
    expect(severityWords(null)).toBe("לא נקבעה");
    expect(severityWords({ social: 1, restricted: 3 })).toBe("תקשורת חברתית: דרגה 1 · התנהגות חוזרת: דרגה 3");
  });

  it("a level already set is one folded line", async () => {
    const { focus } = await openReport((d) => { d.meta.dsm_severity = { social: 2, restricted: 1 }; });
    const dsm = await focus(/^אבחנה על-פי DSM/, "dsm");
    const line = within(dsm).getByText("רמת החומרה: תקשורת חברתית: דרגה 2 · התנהגות חוזרת: דרגה 1");
    expect(line.closest("details")).not.toHaveAttribute("open");
    // No other section shows it.
    expect(screen.getAllByText(/^רמת החומרה:/)).toHaveLength(1);
  });

  it("on the diagnoses section her list leads, open, before the drafting; what she ticks is added once", async () => {
    const { focus, user, calls } = await openReport();
    const diagnoses = await focus(/^אבחנות/, "diagnoses");
    const line = await within(diagnoses).findByText("מהדוחות שלך: 2 אבחנות שחוזרות");
    expect(line.closest("details")).toHaveAttribute("open");
    const draft = await within(diagnoses).findByRole("button", { name: "✦ לנסח" });
    // Her choice comes before the AI's draft on the page.
    expect(line.compareDocumentPosition(draft) & Node.DOCUMENT_POSITION_FOLLOWING).toBeTruthy();
    await user.click(within(diagnoses).getByRole("checkbox", { name: /מעקב התפתחותי/ }));
    await user.click(within(diagnoses).getByRole("checkbox", { name: /הדרכת הורים/ }));
    await user.click(within(diagnoses).getByRole("button", { name: "להוסיף 2" }));
    await waitFor(() => expect(calls.filter((c) => c.cmd === "add_own_paragraph").map((c) => c.args.text)).toEqual(["מומלץ על הדרכת הורים.", "מומלץ על מעקב התפתחותי בעוד שנה."]));
    // They are hers and approved, in the list's order; the list now shows them as added.
    await waitFor(() => expect(Array.from(diagnoses.querySelectorAll(".a4-p")).map((p) => p.textContent)).toEqual(["מומלץ על הדרכת הורים.", "מומלץ על מעקב התפתחותי בעוד שנה."]));
    await waitFor(() => expect(within(diagnoses).getByRole("checkbox", { name: /הדרכת הורים/ })).toBeDisabled());
    expect(within(diagnoses).getByRole("checkbox", { name: /מעקב התפתחותי/ })).toBeDisabled();
    expect(within(diagnoses).getByRole("button", { name: "להוסיף" })).toBeDisabled();
  });

  it("on the recommendations section the list is one folded line, and only the section in focus has one", async () => {
    const { focus, page } = await openReport();
    const recommendations = await focus(/^המלצות/, "recommendations");
    const line = await within(recommendations).findByText("מהדוחות שלך: 2 המלצות שחוזרות");
    expect(line.closest("details")).not.toHaveAttribute("open");
    expect(within(page).getAllByText(/^מהדוחות שלך:/)).toHaveLength(1);
  });
});
