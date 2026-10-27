// The retention reminder on the cases screen: nothing when no case is due, a line when one is,
// and for each case "open", "keep another year" or "to the recycle bin", never an erase on its
// own. Every case here is invented.
import { configure, render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import type { RetentionItem } from "../ipc/generated/RetentionItem";
import { a11yViolations } from "../test/a11y";
import { appApi, Host, serve, type Override } from "../test/host";
import { RetentionNotice } from "./RetentionNotice";

configure({ asyncUtilTimeout: 5000 });

const item = (over: Partial<RetentionItem> = {}): RetentionItem => ({
  case_id: "case-1", label: "נועם · תיק-1024", until: "2026-09-01", by_default: false, ...over,
});

/** The demo core with `due` past their retention date; each "keep" or "delete" takes the case off. */
async function showNotice(due: RetentionItem[], over: Override = () => undefined) {
  let left = due;
  const served = await serve((cmd, args, core) => {
    const out = over(cmd, args, core);
    if (out !== undefined) return out;
    if (cmd === "retention_due") return left;
    if (cmd === "keep_case_longer" || cmd === "delete_case") {
      left = left.filter((i) => i.case_id !== args.caseId);
      return null;
    }
    return undefined;
  });
  const app = appApi();
  const user = userEvent.setup();
  const view = render(<Host app={app}><RetentionNotice /></Host>);
  return { ...served, app, user, view };
}

describe("the retention reminder", () => {
  it("shows nothing while no case is past its retention date", async () => {
    const { calls, view } = await showNotice([]);
    await waitFor(() => expect(calls.some((c) => c.cmd === "retention_due")).toBe(true));
    expect(view.container).toBeEmptyDOMElement();
  });

  it("one case: one line, in the singular, and the list passes the accessibility check", async () => {
    const { user, view } = await showNotice([item({ by_default: true })]);
    const line = await screen.findByRole("status");
    expect(line).toHaveTextContent("תיק אחד עבר את תקופת השמירה. כדאי להחליט מה לעשות.");
    await user.click(within(line).getByRole("button", { name: "לעבור עליהם" }));
    const dialog = await screen.findByRole("dialog", { name: "תיקים שעברו את תקופת השמירה" });
    expect(within(dialog).getByText("התוכנה לא מוחקת לבד. לכל תיק: להשאיר, או לסל המחזור (30 יום).")).toBeInTheDocument();
    expect(within(dialog).getByText("נועם · תיק-1024")).toBeInTheDocument();
    expect(within(dialog).getByText("תקופת השמירה הסתיימה ב-01.09.2026 (לפי ברירת המחדל המוצעת)")).toBeInTheDocument();
    expect(await a11yViolations(view.container)).toEqual([]);
  });

  it("“keep another year” keeps the case for one more year, and the reminder goes", async () => {
    const { user, calls } = await showNotice([item()]);
    await user.click(await screen.findByRole("button", { name: "לעבור עליהם" }));
    const dialog = await screen.findByRole("dialog");
    await user.click(within(dialog).getByRole("button", { name: "להשאיר עוד שנה" }));
    await waitFor(() => expect(screen.queryByRole("status")).toBeNull());
    expect(calls.find((c) => c.cmd === "keep_case_longer")?.args).toEqual({ caseId: "case-1", years: 1 });
    expect(calls.some((c) => c.cmd === "delete_case")).toBe(false);
  });

  it("several cases: the count is said, and one moved to the recycle bin leaves the others", async () => {
    const { user, calls } = await showNotice([item(), item({ case_id: "case-12", label: "מאיה · תיק-1025", until: "2026-08-15" })]);
    expect(await screen.findByRole("status")).toHaveTextContent("2 תיקים עברו את תקופת השמירה.");
    await user.click(screen.getByRole("button", { name: "לעבור עליהם" }));
    const dialog = await screen.findByRole("dialog");
    const maya = within(dialog).getByText("מאיה · תיק-1025").closest("li") as HTMLElement;
    await user.click(within(maya).getByRole("button", { name: "לסל המחזור" }));
    await waitFor(() => expect(screen.getByRole("status")).toHaveTextContent("תיק אחד עבר"));
    expect(calls.find((c) => c.cmd === "delete_case")?.args).toEqual({ caseId: "case-12" });
  });

  it("“פתיחה” goes to the case's details", async () => {
    const { user, app } = await showNotice([item()]);
    await user.click(await screen.findByRole("button", { name: "לעבור עליהם" }));
    await user.click(within(await screen.findByRole("dialog")).getByRole("button", { name: "פתיחה" }));
    expect(app.go).toHaveBeenCalledWith({ name: "case", id: "case-1", view: "details" });
  });

  it("a failed action says why in Hebrew, and the case stays in the list", async () => {
    const { user } = await showNotice([item()], (cmd) => {
      if (cmd === "keep_case_longer") throw { code: "internal", message: "השמירה של התיק לא עודכנה.", details: [] };
      return undefined;
    });
    await user.click(await screen.findByRole("button", { name: "לעבור עליהם" }));
    const dialog = await screen.findByRole("dialog");
    await user.click(within(dialog).getByRole("button", { name: "להשאיר עוד שנה" }));
    expect(await within(dialog).findByRole("alert")).toHaveTextContent("השמירה של התיק לא עודכנה.");
    expect(within(dialog).getByText("נועם · תיק-1024")).toBeInTheDocument();
  });

  it("works from the keyboard: Enter opens the list, Escape closes it", async () => {
    const { user } = await showNotice([item()]);
    const open = await screen.findByRole("button", { name: "לעבור עליהם" });
    open.focus();
    await user.keyboard("{Enter}");
    const dialog = await screen.findByRole("dialog");
    // The frame takes the focus, so the next Tab is inside the list.
    expect(dialog).toHaveFocus();
    await user.tab();
    expect(dialog).toContainElement(document.activeElement as HTMLElement);
    await user.keyboard("{Escape}");
    expect(screen.queryByRole("dialog")).toBeNull();
  });
});
