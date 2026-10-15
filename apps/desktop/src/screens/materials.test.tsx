// The materials screen over the in-memory demo core (round 49): what is missing, one list,
// the empty-text refusal, deleting with undo, and approving a draft from the keyboard.
// Every name and detail is invented.
import { configure, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { mockIPC } from "@tauri-apps/api/mocks";
import { App } from "../App";
import { FakeCore } from "../web/fakeCore";
import { a11yViolations } from "../test/a11y";

configure({ asyncUtilTimeout: 5000 });
vi.setConfig({ testTimeout: 30_000 });

async function openCase(prepare?: (core: FakeCore) => Promise<void>) {
  const core = new FakeCore();
  await core.handle("unlock", { password: "x" });
  // These tests walk every screen a request passes through, the review screen included, so
  // they run with "show me every request before sending" on (off by default; set here for the tests that read it).
  await core.handle("set_review_always", { on: true });
  await prepare?.(core);
  const calls: { cmd: string; args: Record<string, unknown> }[] = [];
  mockIPC((cmd, args) => {
    calls.push({ cmd, args: (args ?? {}) as Record<string, unknown> });
    return core.handle(cmd, args);
  });
  const user = userEvent.setup();
  const view = render(<App />);
  await user.click(await screen.findByRole("button", { name: /בתיק של נועם$/ }));
  const page = await screen.findByRole("article", { name: "הדוח, כמו בקובץ" });
  return { core, calls, user, view, page };
}

async function toMaterials(user: ReturnType<typeof userEvent.setup>) {
  await user.click(within(screen.getByRole("navigation", { name: "שלבי התיק" })).getByRole("button", { name: /חומרים/ }));
  await screen.findByRole("heading", { name: "מה יש בתיק" });
  return screen.getByRole("region", { name: "רשימת החומרים" });
}

describe("materials", () => {
  it("the materials screen passes the accessibility check", async () => {
    const { user, view } = await openCase();
    await toMaterials(user);
    expect(await a11yViolations(view.container)).toEqual([]);
  });

  it("the page says what the report can be written from, and what is still missing", async () => {
    const { user } = await openCase();
    await toMaterials(user);
    expect(screen.getByText(/^יש חומר ל-\d+ מתוך \d+ סעיפים\. הכול נשמר מוצפן\.$/)).toBeInTheDocument();
    const needs = screen.getByRole("list", { name: "מה עוד חסר" });
    const cells = within(needs).getAllByRole("listitem");
    expect(cells.map((c) => c.textContent?.replace(/\s+/g, " ").trim())).toEqual([
      "✓יש: אינטייק עם ההורים", "✓יש: דיווח מהמסגרת", "✓יש: תצפית ומפגשים", "✓יש: ציוני מבחנים", "○אבחונים קודמים (לא חובה) (העלאת מסמך)",
    ]);
    // Going on to writing is at the top of the page now, and the bar at the bottom is gone.
    expect(screen.getByRole("button", { name: "לכתיבת הדוח ←" })).toHaveClass("btn-primary");
    expect(screen.queryByRole("contentinfo", { name: "הצעד הבא" })).toBeNull();
  });

  it("an empty text is refused; a pasted text joins the one list", async () => {
    const { user, calls } = await openCase();
    const list = await toMaterials(user);
    // The demo case has no earlier reports yet: that cell is open, and offers to paste one.
    await user.click(screen.getByRole("button", { name: "הוספה לאבחונים קודמים" }));
    await user.click(await screen.findByRole("menuitem", { name: "הדבקת טקסט" }));
    const dialog = await screen.findByRole("dialog", { name: "הוספת טקסט" });
    const text = within(dialog).getByLabelText("הטקסט");
    expect(text).toHaveFocus();
    expect(within(dialog).getByLabelText("סוג החומר")).toHaveValue("prior_report");
    await user.click(within(dialog).getByRole("button", { name: "שמירה בתיק" }));
    expect(within(dialog).getByRole("alert")).toHaveTextContent("אין טקסט לשמור.");
    expect(calls.some((c) => c.cmd === "add_input")).toBe(false);

    await user.type(within(dialog).getByLabelText("כותרת"), "דוח קלינאית בדוי");
    await user.type(text, "נועם הגיע לטיפול בשפה בגיל ארבע.");
    await user.click(within(dialog).getByRole("button", { name: "שמירה בתיק" }));
    await waitFor(() => expect(screen.queryByRole("dialog")).toBeNull());
    expect(calls.find((c) => c.cmd === "add_input")?.args).toMatchObject({ kind: "prior_report", title: "דוח קלינאית בדוי", content: "נועם הגיע לטיפול בשפה בגיל ארבע." });
    expect(await screen.findByRole("status")).toHaveTextContent("נשמר בתיק.");
    // Newest first, with its kind as a tag; the cell is marked as there.
    const row = await within(list).findByRole("button", { name: /דוח קלינאית בדוי/ });
    expect(within(list).getAllByRole("button")[0]).toBe(row);
    expect(row).toHaveTextContent("דוח קודם");
    const needs = screen.getByRole("list", { name: "מה עוד חסר" });
    expect(within(needs).getByText("אבחונים קודמים").closest("li")).toHaveClass("have");
  });

  it("every material is in one list, whatever its kind", async () => {
    const { user } = await openCase(async (core) => {
      const cases = (await core.handle("list_cases", {})) as { id: string; meta: { code: string } }[];
      for (const c of cases) {
        for (let i = 0; i < 12; i++) {
          await core.handle("add_input", { caseId: c.id, kind: "prior_report", title: `דוח בדוי ${i}`, content: "טקסט בדוי." });
        }
      }
    });
    const list = await toMaterials(user);
    expect(within(list).getAllByRole("button", { name: /^דוח קודם\s*דוח בדוי/ })).toHaveLength(12);
    expect(within(list).getByRole("button", { name: /מפגש שני/ })).toBeInTheDocument();
  });

  it("a deleted material can be brought back from the notice", async () => {
    const { user, calls } = await openCase();
    const list = await toMaterials(user);
    const title = "מפגש שני";
    await user.click(within(list).getByRole("button", { name: new RegExp(title) }));
    const dialog = await screen.findByRole("dialog", { name: title });
    await user.click(within(dialog).getByRole("button", { name: "מחיקה" }));
    await waitFor(() => expect(within(list).queryByRole("button", { name: new RegExp(title) })).toBeNull());
    const notice = await screen.findByRole("status");
    expect(notice).toHaveTextContent("החומר נמחק מהתיק.");
    await user.click(within(notice).getByRole("button", { name: "ביטול" }));
    expect(await within(list).findByRole("button", { name: new RegExp(title) })).toBeInTheDocument();
    expect(calls.map((c) => c.cmd)).toEqual(expect.arrayContaining(["delete_input", "restore_input"]));
  });
});

describe("keyboard on the report page", () => {
  it("Ctrl+Enter on a proposed paragraph approves it", async () => {
    const { user, calls, page } = await openCase();
    const write = within(page).getAllByRole("button", { name: "✦ לכתוב עם Claude" })[0];
    if (!write) throw new Error("no empty section to write");
    await user.click(write);
    const review = await screen.findByRole("dialog", { name: "לפני שליחה ל-Claude" });
    await user.click(within(review).getByRole("button", { name: /שליחה/ }));
    await screen.findByRole("button", { name: "✓ לאשר" }, { timeout: 6000 });
    const target = page.querySelector('[aria-label^="טיוטה שממתינה לאישור"]') as HTMLElement;
    expect(target).not.toBeNull();
    target.focus();
    fireEvent.keyDown(target, { key: "Enter", code: "Enter", ctrlKey: true });
    await waitFor(() => expect(calls.map((c) => c.cmd)).toContain("approve_paragraph"));
  });
});
