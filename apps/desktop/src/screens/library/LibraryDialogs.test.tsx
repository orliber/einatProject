// The small dialogs of the case library: naming a folder, moving, confirming and erasing for
// good. What she sees when the core refuses, and the keyboard. Every name is invented.
import { render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import type { Folder } from "../../ipc/client";
import { a11yViolations } from "../../test/a11y";
import { ConfirmDialog, descendants, FolderNameDialog, MoveDialog, PurgeDialog } from "./LibraryDialogs";

const folder = (id: string, name: string, parent_id: string | null = null): Folder => ({ id, name, parent_id, created_at: 1_760_000_000 });
const FOLDERS = [folder("f1", "גני ילדים"), folder("f2", "גן הדקל", "f1"), folder("f3", "בתי ספר"), folder("f4", "כיתה א", "f3")];
const refused = (message: string) => ({ code: "refused", message, details: [], action: { kind: "dismiss" } });

describe("naming a folder", () => {
  it("starts in the name box, saves the trimmed name on Enter, and passes the accessibility check", async () => {
    const onSave = vi.fn(async () => undefined);
    const user = userEvent.setup();
    const view = render(<FolderNameDialog title="תיקייה חדשה" onClose={vi.fn()} onSave={onSave} />);
    const box = screen.getByLabelText("שם התיקייה");
    expect(box).toHaveFocus();
    expect(await a11yViolations(view.container)).toEqual([]);
    await user.type(box, "  גני ילדים  {Enter}");
    expect(onSave).toHaveBeenCalledWith("גני ילדים");
  });

  it("an empty name is refused before anything is saved", async () => {
    const onSave = vi.fn(async () => undefined);
    const user = userEvent.setup();
    render(<FolderNameDialog title="תיקייה חדשה" onClose={vi.fn()} onSave={onSave} />);
    await user.type(screen.getByLabelText("שם התיקייה"), "   ");
    await user.click(screen.getByRole("button", { name: "שמירה" }));
    expect(screen.getByRole("alert")).toHaveTextContent("צריך לתת שם לתיקייה.");
    expect(onSave).not.toHaveBeenCalled();
  });

  it("renaming starts from the current name; a refusal says why and she can try again", async () => {
    const onSave = vi.fn().mockRejectedValueOnce(refused('כבר יש כאן תיקייה בשם "בתי ספר".')).mockResolvedValueOnce(undefined);
    const user = userEvent.setup();
    render(<FolderNameDialog title="שינוי שם" initial="גני ילדים" onClose={vi.fn()} onSave={onSave} />);
    const box = screen.getByLabelText("שם התיקייה");
    expect(box).toHaveValue("גני ילדים");
    await user.clear(box);
    await user.type(box, "בתי ספר");
    await user.click(screen.getByRole("button", { name: "שמירה" }));
    expect(await screen.findByRole("alert")).toHaveTextContent('כבר יש כאן תיקייה בשם "בתי ספר".');
    expect(screen.getByRole("button", { name: "שמירה" })).toBeEnabled();
    await user.clear(box);
    await user.type(box, "גנים{Enter}");
    expect(onSave).toHaveBeenLastCalledWith("גנים");
  });

  it("Escape and “ביטול” close without saving", async () => {
    const onClose = vi.fn();
    const onSave = vi.fn(async () => undefined);
    const user = userEvent.setup();
    render(<FolderNameDialog title="תיקייה חדשה" onClose={onClose} onSave={onSave} />);
    await user.keyboard("{Escape}");
    await user.click(screen.getByRole("button", { name: "ביטול" }));
    expect(onClose).toHaveBeenCalledTimes(2);
    expect(onSave).not.toHaveBeenCalled();
  });
});

describe("moving", () => {
  it("lists the folders as a tree of choices, from where it is now; passes the accessibility check", async () => {
    const view = render(<MoveDialog what="התיק" folders={FOLDERS} current="f2" onClose={vi.fn()} onMove={vi.fn(async () => undefined)} />);
    const group = screen.getByRole("radiogroup", { name: "תיקיית היעד" });
    expect(within(group).getAllByRole("radio").map((r) => r.closest("label")?.textContent?.trim())).toEqual([
      "תיקים (הרמה העליונה)", "בתי ספר", "כיתה א", "גני ילדים", "גן הדקל",
    ]);
    expect(within(group).getByRole("radio", { name: "גן הדקל" })).toBeChecked();
    // Moving to where it already is does nothing.
    expect(screen.getByRole("button", { name: "העברה" })).toBeDisabled();
    expect(await a11yViolations(view.container)).toEqual([]);
  });

  it("a folder is never offered as a place inside itself", () => {
    const exclude = descendants(FOLDERS, "f1");
    expect([...exclude].sort()).toEqual(["f1", "f2"]);
    render(<MoveDialog what="התיקייה" folders={FOLDERS} current={null} exclude={exclude} onClose={vi.fn()} onMove={vi.fn(async () => undefined)} />);
    expect(screen.queryByRole("radio", { name: "גני ילדים" })).toBeNull();
    expect(screen.queryByRole("radio", { name: "גן הדקל" })).toBeNull();
    expect(screen.getByRole("radio", { name: "כיתה א" })).toBeInTheDocument();
  });

  it("the target is chosen from the keyboard and moved to; a refusal says why", async () => {
    const onMove = vi.fn().mockRejectedValueOnce(refused("התיקייה לא נמצאה")).mockResolvedValueOnce(undefined);
    const user = userEvent.setup();
    render(<MoveDialog what="התיק" folders={FOLDERS} current={null} onClose={vi.fn()} onMove={onMove} />);
    const top = screen.getByRole("radio", { name: /תיקים/ });
    top.focus();
    await user.keyboard("{ArrowDown}");
    await waitFor(() => expect(screen.getByRole("radio", { name: "בתי ספר" })).toBeChecked());
    await user.click(screen.getByRole("button", { name: "העברה" }));
    expect(await screen.findByRole("alert")).toHaveTextContent("התיקייה לא נמצאה");
    expect(onMove).toHaveBeenLastCalledWith("f3");
    await user.click(screen.getByRole("button", { name: "העברה" }));
    expect(onMove).toHaveBeenCalledTimes(2);
  });
});

describe("confirming", () => {
  it("says what will happen, and does it only on the action button", async () => {
    const onConfirm = vi.fn(async () => undefined);
    const onClose = vi.fn();
    const user = userEvent.setup();
    const view = render(<ConfirmDialog danger title="מחיקת תיקייה" text="התיקייה תימחק. התיקים שבה עוברים לרמה העליונה." action="מחיקה" onClose={onClose} onConfirm={onConfirm} />);
    expect(screen.getByRole("dialog", { name: "מחיקת תיקייה" })).toHaveTextContent("התיקייה תימחק. התיקים שבה עוברים לרמה העליונה.");
    expect(await a11yViolations(view.container)).toEqual([]);
    await user.click(screen.getByRole("button", { name: "ביטול" }));
    expect(onConfirm).not.toHaveBeenCalled();
    await user.click(screen.getByRole("button", { name: "מחיקה" }));
    expect(onConfirm).toHaveBeenCalledTimes(1);
    // While it works, a second click does nothing.
    expect(screen.getByRole("button", { name: "מחיקה" })).toBeDisabled();
  });

  it("a refusal says why, and the button is free again", async () => {
    const user = userEvent.setup();
    render(<ConfirmDialog title="מחיקת שיחה" text="השיחה תימחק." action="מחיקה" onClose={vi.fn()} onConfirm={() => Promise.reject(refused("השיחה לא נמצאה"))} />);
    await user.click(screen.getByRole("button", { name: "מחיקה" }));
    expect(await screen.findByRole("alert")).toHaveTextContent("השיחה לא נמצאה");
    expect(screen.getByRole("button", { name: "מחיקה" })).toBeEnabled();
  });
});

describe("erasing for good", () => {
  it("asks for the vault's password, says backups still hold the case, and passes the accessibility check", async () => {
    const view = render(<PurgeDialog name="נועם · תיק-1024" onClose={vi.fn()} onPurge={vi.fn(async () => undefined)} />);
    const dialog = screen.getByRole("dialog", { name: "מחיקה לצמיתות" });
    expect(dialog).toHaveTextContent("גיבויים קודמים עדיין מכילים אותו");
    const pw = within(dialog).getByLabelText("הסיסמה של הכספת, לאישור");
    expect(pw).toHaveAttribute("type", "password");
    expect(pw).toHaveFocus();
    expect(within(dialog).getByRole("button", { name: "מחיקה לצמיתות" })).toBeDisabled();
    expect(await a11yViolations(view.container)).toEqual([]);
  });

  it("a wrong password is said, the box is emptied, and Enter tries again", async () => {
    const onPurge = vi.fn().mockRejectedValueOnce({ code: "wrong_secret", message: "הסיסמה לא נכונה.", details: [] }).mockResolvedValueOnce(undefined);
    const user = userEvent.setup();
    render(<PurgeDialog name="נועם · תיק-1024" onClose={vi.fn()} onPurge={onPurge} />);
    const pw = screen.getByLabelText("הסיסמה של הכספת, לאישור");
    await user.type(pw, "לא-נכונה{Enter}");
    expect(await screen.findByRole("alert")).toHaveTextContent("הסיסמה לא נכונה.");
    expect(pw).toHaveValue("");
    // An empty box does not try.
    await user.type(pw, "{Enter}");
    expect(onPurge).toHaveBeenCalledTimes(1);
    await user.type(pw, "נכונה{Enter}");
    expect(onPurge).toHaveBeenLastCalledWith("נכונה");
  });
});
