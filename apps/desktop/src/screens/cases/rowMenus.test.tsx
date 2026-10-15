// The actions of a case or a folder in the list: from the "⋯" button (mouse or keyboard) and
// from a right click, through one menu for the whole table (plan 2.4). Invented cases only.
import { configure, fireEvent, render, screen, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { mockIPC } from "@tauri-apps/api/mocks";
import { App } from "../../App";
import { FakeCore } from "../../web/fakeCore";

configure({ asyncUtilTimeout: 5000 });
vi.setConfig({ testTimeout: 30_000 });

async function openList() {
  const core = new FakeCore();
  await core.handle("unlock", { password: "x" });
  await core.handle("create_folder", { parentId: null, name: "גן הדקל" });
  mockIPC((cmd, args) => core.handle(cmd, args));
  const user = userEvent.setup();
  render(<App />);
  await screen.findByRole("button", { name: /בתיק של נועם$/ });
  return user;
}

function rowOf(el: HTMLElement): HTMLElement {
  const row = el.closest("tr");
  if (!row) throw new Error("not in a row");
  return row;
}

const labels = async () => (await screen.findAllByRole("menuitem")).map((m) => m.textContent);

describe("the actions of a row", () => {
  it("open from the ⋯ button, and a choice does what it says", async () => {
    const user = await openList();
    await user.click(screen.getByRole("button", { name: "פעולות על התיק של נועם" }));
    expect(await labels()).toContain("העברה לסל המחזור");
    await user.click(screen.getByRole("menuitem", { name: "העברה לסל המחזור" }));
    expect(await screen.findByRole("dialog")).toBeInTheDocument();
  });

  it("open from the keyboard, and Esc gives the focus back to the button", async () => {
    const user = await openList();
    const button = screen.getByRole("button", { name: "פעולות על התיק של נועם" });
    button.focus();
    await user.keyboard("{ArrowDown}");
    expect(await labels()).toContain("אבחון מעקב לילד/ה הזה/ו");
    await user.keyboard("{Escape}");
    expect(screen.queryByRole("menuitem")).not.toBeInTheDocument();
    expect(screen.getByRole("button", { name: "פעולות על התיק של נועם" })).toHaveFocus();
  });

  it("a right click shows the actions of the row under the pointer", async () => {
    await openList();
    const table = screen.getByRole("table");
    const caseRow = rowOf(within(table).getByRole("button", { name: /בתיק של נועם$/ }));
    fireEvent.contextMenu(within(caseRow).getByText("נועם", { exact: false, selector: ".case-name" }));
    expect(await labels()).toEqual(["פתיחה", "העברה לתיקייה…", "אבחון מעקב לילד/ה הזה/ו", "העברה לסל המחזור"]);
    fireEvent.keyDown(document.activeElement ?? document.body, { key: "Escape" });
    await vi.waitFor(() => expect(screen.queryByRole("menuitem")).not.toBeInTheDocument());

    const folderRow = rowOf(within(table).getByText("גן הדקל"));
    fireEvent.contextMenu(folderRow.querySelector("svg") ?? folderRow);
    expect(await labels()).toContain("מחיקת התיקייה");
    expect(await labels()).not.toContain("העברה לסל המחזור");
  });
});
