// Opening a case: the child's name is required (it is what gets hidden everywhere), names and
// nicknames reach the core trimmed and split, consent is recorded with its date, and names
// already in other cases are pointed out as text. All names are invented.
import { render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { mockIPC } from "@tauri-apps/api/mocks";
import { AppContext, type AppApi } from "../App";
import { status } from "../test/fixtures";
import { a11yViolations } from "../test/a11y";
import { NewCaseDialog, toIdentityInputs } from "./NewCaseDialog";
import { todayIso } from "./DateField";

function api(): AppApi {
  return { status: status(), go: vi.fn(), refresh: vi.fn(async () => undefined), notify: vi.fn(), fail: (e) => e.message, lockNow: vi.fn(async () => undefined) };
}

type Call = { cmd: string; args: Record<string, unknown> };

function open(handler: (cmd: string, args: Record<string, unknown>) => unknown = () => null, folderId: string | null = null) {
  const calls: Call[] = [];
  mockIPC((cmd, args) => {
    calls.push({ cmd, args: args as Record<string, unknown> });
    if (cmd === "find_name_matches") return handler(cmd, args as Record<string, unknown>) ?? [];
    return handler(cmd, args as Record<string, unknown>);
  });
  const onCreated = vi.fn();
  const onClose = vi.fn();
  const user = userEvent.setup();
  const view = render(
    <AppContext.Provider value={api()}>
      <NewCaseDialog onClose={onClose} onCreated={onCreated} folderId={folderId} />
    </AppContext.Provider>,
  );
  const dialog = screen.getByRole("dialog", { name: "תיק חדש" });
  return { calls, onCreated, onClose, user, view, dialog };
}

describe("toIdentityInputs", () => {
  it("drops empty rows, trims names and splits nicknames on commas", () => {
    expect(toIdentityInputs([
      { id: null, role: "child", value: "  נועה ", aliases: "נועי, , נועהל׳ה  " },
      { id: "p2", role: "mother", value: "   ", aliases: "x" },
      { id: null, role: "teacher", value: "רינה", aliases: "" },
    ])).toEqual([
      { id: null, role: "child", value: "נועה", aliases: ["נועי", "נועהל׳ה"] },
      { id: null, role: "teacher", value: "רינה", aliases: [] },
    ]);
  });
});

describe("NewCaseDialog", () => {
  it("passes the accessibility check", async () => {
    const { view } = open();
    expect(await a11yViolations(view.container)).toEqual([]);
  });

  it("without the child's name nothing is created, and it says why", async () => {
    const { calls, user, dialog, onCreated } = open();
    await user.click(within(dialog).getByRole("button", { name: "פתיחת התיק" }));
    expect(within(dialog).getByRole("alert")).toHaveTextContent("צריך את שם הילד/ה");
    expect(calls.some((c) => c.cmd === "create_case")).toBe(false);
    expect(onCreated).not.toHaveBeenCalled();
  });

  it("creates the case with what she typed: gender, age, consent, people", async () => {
    const { calls, user, dialog, onCreated } = open((cmd) => (cmd === "create_case" ? "case-7" : null));
    await user.clear(within(dialog).getByLabelText("קוד התיק"));
    await user.type(within(dialog).getByLabelText("קוד התיק"), "  תיק-בדיקה ");
    await user.click(within(dialog).getByRole("radio", { name: "בת" }));
    await user.type(within(dialog).getByLabelText("שנים"), "5");
    await user.clear(within(dialog).getByLabelText("חודשים"));
    await user.type(within(dialog).getByLabelText("חודשים"), "4");
    await user.type(dialog.querySelector("#name-0") as HTMLElement, "נועה");
    await user.type(dialog.querySelector("#alias-0") as HTMLElement, "נועי, נועל׳ה");
    await user.type(dialog.querySelector("#name-1") as HTMLElement, "דפנה");
    // No father in this case: the row is removed.
    await user.click(within(dialog).getByRole("button", { name: "הסרת אב" }));
    await user.click(within(dialog).getByRole("button", { name: "+ הוספת אדם" }));
    await user.selectOptions(dialog.querySelector("#role-2") as HTMLElement, "teacher");
    await user.type(dialog.querySelector("#name-2") as HTMLElement, "רינה");
    await user.click(within(dialog).getByRole("checkbox", { name: /ההורים חתמו על הסכמה/ }));
    expect(within(dialog).getByLabelText("תאריך")).toHaveValue(todayIso().split("-").reverse().join("."));
    await user.clear(within(dialog).getByLabelText("מי חתם (תפקיד, לא שם)"));
    await user.type(within(dialog).getByLabelText("מי חתם (תפקיד, לא שם)"), "האם");
    await user.click(within(dialog).getByRole("button", { name: "פתיחת התיק" }));

    await waitFor(() => expect(onCreated).toHaveBeenCalledWith("case-7"));
    const create = calls.find((c) => c.cmd === "create_case");
    expect(create?.args).toEqual({
      meta: {
        code: "תיק-בדיקה",
        age: { years: 5, months: 4 },
        child_gender: "female",
        current_section: null,
        retention_until: null,
        consent: { given_on: todayIso(), form_version: "v1", given_by: "האם" },
      },
      identities: [
        { id: null, role: "child", value: "נועה", aliases: ["נועי", "נועל׳ה"] },
        { id: null, role: "mother", value: "דפנה", aliases: [] },
        { id: null, role: "teacher", value: "רינה", aliases: [] },
      ],
    });
    // Not in a folder: nothing is moved.
    expect(calls.some((c) => c.cmd === "move_case")).toBe(false);
  });

  it("without consent the case opens with no consent recorded", async () => {
    const { calls, user, dialog, onCreated } = open((cmd) => (cmd === "create_case" ? "case-8" : null));
    await user.type(dialog.querySelector("#name-0") as HTMLElement, "איתי");
    await user.click(within(dialog).getByRole("button", { name: "פתיחת התיק" }));
    await waitFor(() => expect(onCreated).toHaveBeenCalledWith("case-8"));
    const meta = calls.find((c) => c.cmd === "create_case")?.args.meta as { consent: unknown; age: unknown };
    expect(meta.consent).toBeNull();
    expect(meta.age).toBeNull();
  });

  it("consent with no valid date is refused before anything is created", async () => {
    const { calls, user, dialog } = open();
    await user.type(dialog.querySelector("#name-0") as HTMLElement, "איתי");
    await user.click(within(dialog).getByRole("checkbox", { name: /ההורים חתמו על הסכמה/ }));
    const date = within(dialog).getByLabelText("תאריך");
    await user.clear(date);
    await user.type(date, "31.02.2026");
    await user.tab();
    expect(within(dialog).getByText("תאריך לא תקין. כותבים יום.חודש.שנה")).toBeInTheDocument();
    expect(date).toHaveAttribute("aria-invalid", "true");
    await user.click(within(dialog).getByRole("button", { name: "פתיחת התיק" }));
    expect(within(dialog).getByRole("alert")).toHaveTextContent("צריך את תאריך החתימה");
    expect(calls.some((c) => c.cmd === "create_case")).toBe(false);
  });

  it("a case opened inside a folder is moved there", async () => {
    const { calls, user, dialog, onCreated } = open((cmd) => (cmd === "create_case" ? "case-9" : null), "folder-2");
    await user.type(dialog.querySelector("#name-0") as HTMLElement, "איתי");
    await user.click(within(dialog).getByRole("button", { name: "פתיחת התיק" }));
    await waitFor(() => expect(onCreated).toHaveBeenCalledWith("case-9"));
    expect(calls.find((c) => c.cmd === "move_case")?.args).toEqual({ caseId: "case-9", folderId: "folder-2" });
  });

  it("a core failure is shown and the dialog stays open for another try", async () => {
    const { user, dialog, onCreated } = open((cmd) => {
      if (cmd === "create_case") throw { code: "locked", message: "הכספת ננעלה. פותחים אותה שוב.", details: [] };
      return null;
    });
    await user.type(dialog.querySelector("#name-0") as HTMLElement, "איתי");
    await user.click(within(dialog).getByRole("button", { name: "פתיחת התיק" }));
    expect(await within(dialog).findByRole("alert")).toHaveTextContent("הכספת ננעלה. פותחים אותה שוב.");
    expect(onCreated).not.toHaveBeenCalled();
    expect(within(dialog).getByRole("button", { name: "פתיחת התיק" })).toBeEnabled();
  });

  it("names already in other cases are pointed out, as text", async () => {
    const { calls, user, dialog } = open((cmd) =>
      cmd === "find_name_matches"
        ? [{ case_id: "c3", case_code: "תיק-12", child_name: "<img src=x onerror=alert(1)>", typed: "דפנה", value: "דפנה", role: "mother", trashed: true }]
        : null,
    );
    await user.type(dialog.querySelector("#name-1") as HTMLElement, "דפנה");
    const found = await within(dialog).findByRole("status", {}, { timeout: 3000 });
    expect(found).toHaveTextContent('"דפנה": אם בתיק תיק-12 (<img src=x onerror=alert(1)>), בסל המחזור');
    expect(dialog.querySelector("img")).toBeNull();
    // One-letter fragments are not looked up; the typed names are.
    const asked = calls.filter((c) => c.cmd === "find_name_matches").at(-1)?.args;
    expect(asked).toEqual({ caseId: null, names: ["דפנה"] });
  });

  it("Escape with a typed name asks first; a clean dialog closes at once", async () => {
    const first = open();
    await first.user.keyboard("{Escape}");
    expect(first.onClose).toHaveBeenCalledTimes(1);
    first.view.unmount();

    const second = open();
    await second.user.type(second.dialog.querySelector("#name-0") as HTMLElement, "איתי");
    await second.user.keyboard("{Escape}");
    expect(second.onClose).not.toHaveBeenCalled();
    expect(within(second.dialog).getByRole("alertdialog", { name: "לסגור בלי לשמור" })).toBeInTheDocument();
  });
});
