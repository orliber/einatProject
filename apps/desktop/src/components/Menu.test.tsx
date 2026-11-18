// The menus of a button ("⋯" and "+"): a chosen item gives the focus back to the button before
// it runs, so a dialog it opens returns the focus there when it closes, and not to the page
// (WCAG 2.4.3; the item itself is gone by then). Escape still closes the menu onto its button.
// The right-click menus do the same with the row they were opened on. All names are invented.
import { act, configure, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { useState } from "react";
import { ActionsMenu, AddMenu, RightClick, RightClickArea, type MenuItem } from "./Menu";
import { Dialog } from "./ui";

configure({ asyncUtilTimeout: 5000 });

/** A screen with a menu whose first item opens a dialog, and whose second does nothing visible. */
function Screen({ kind }: { kind: "actions" | "add" }) {
  const [open, setOpen] = useState(false);
  const [ran, setRan] = useState(0);
  const items: MenuItem[] = [
    { label: "פתיחת חלון", run: () => setOpen(true) },
    { label: "פעולה שקטה", run: () => setRan((n) => n + 1) },
  ];
  return (
    <div>
      <button type="button">לפני התפריט</button>
      {kind === "actions" ? <ActionsMenu items={items} label="עוד פעולות" /> : <AddMenu items={items} label="עוד פעולות">+</AddMenu>}
      <span>{`רץ ${ran}`}</span>
      {open && (
        <Dialog title="חלון בדוי" onClose={() => setOpen(false)}>
          <p>תוכן בדוי</p>
        </Dialog>
      )}
    </div>
  );
}

const trigger = () => screen.getByRole("button", { name: "עוד פעולות" });

describe.each(["actions", "add"] as const)("a menu with a button (%s)", (kind) => {
  it("a dialog opened from an item gives the focus back to the menu's button when it closes", async () => {
    const user = userEvent.setup();
    render(<Screen kind={kind} />);
    await user.click(trigger());
    await user.click(await screen.findByRole("menuitem", { name: "פתיחת חלון" }));
    const dialog = await screen.findByRole("dialog", { name: "חלון בדוי" });
    await waitFor(() => expect(screen.queryByRole("menu")).toBeNull());
    // The dialog has the focus (the menu's closing did not take it back to the button).
    await waitFor(() => expect(dialog).toHaveFocus());
    await new Promise((r) => setTimeout(r, 20));
    expect(dialog).toHaveFocus();
    await user.click(screen.getByRole("button", { name: "סגירה" }));
    await waitFor(() => expect(screen.queryByRole("dialog")).toBeNull());
    expect(document.activeElement).toBe(trigger());
  });

  it("the same from the keyboard: Enter on the item, Escape in the dialog", async () => {
    const user = userEvent.setup();
    render(<Screen kind={kind} />);
    trigger().focus();
    await user.keyboard("{Enter}");
    const item = await screen.findByRole("menuitem", { name: "פתיחת חלון" });
    act(() => item.focus());
    await user.keyboard("{Enter}");
    const dialog = await screen.findByRole("dialog", { name: "חלון בדוי" });
    await waitFor(() => expect(dialog).toHaveFocus());
    await user.keyboard("{Escape}");
    await waitFor(() => expect(screen.queryByRole("dialog")).toBeNull());
    expect(document.activeElement).toBe(trigger());
  });

  it("an item that opens nothing leaves the focus on the button, and Escape still closes the menu onto it", async () => {
    const user = userEvent.setup();
    render(<Screen kind={kind} />);
    await user.click(trigger());
    await user.click(await screen.findByRole("menuitem", { name: "פעולה שקטה" }));
    expect(screen.getByText("רץ 1")).toBeInTheDocument();
    await waitFor(() => expect(screen.queryByRole("menu")).toBeNull());
    await new Promise((r) => setTimeout(r, 20));
    expect(trigger()).toHaveFocus();
    // Opened again and closed with Escape: back on the button, as before.
    await user.keyboard("{Enter}");
    await screen.findByRole("menu");
    await user.keyboard("{Escape}");
    await waitFor(() => expect(screen.queryByRole("menu")).toBeNull());
    await waitFor(() => expect(trigger()).toHaveFocus());
  });
});

/** A list with one right-click menu for all its rows (as the case list has, `area`), or a row
 *  with a menu of its own (as a folder in the tree has, `row`). The first item opens a dialog. */
function List({ kind }: { kind: "area" | "row" }) {
  const [open, setOpen] = useState<string | null>(null);
  const [ran, setRan] = useState(0);
  const itemsOf = (name: string): MenuItem[] => [
    { label: "העברה לתיקייה…", run: () => setOpen(name) },
    { label: "פעולה שקטה", run: () => setRan((n) => n + 1) },
  ];
  const names = ["תיק בדוי א", "תיק בדוי ב"];
  return (
    <div>
      <button type="button">לפני הרשימה</button>
      {kind === "area" ? (
        <RightClickArea itemsAt={(t) => {
          const name = t.closest<HTMLElement>("li[data-name]")?.dataset.name;
          return name ? itemsOf(name) : null;
        }}>
          <ul aria-label="רשימה בדויה">
            {names.map((name) => (
              <li key={name} data-name={name}>
                <button type="button">{name}</button>
                <span>{`פרטים של ${name}`}</span>
                <button type="button">{`עוד על ${name}`}</button>
              </li>
            ))}
            {/* A row with nothing that takes the focus. */}
            <li data-name="שורה שקטה"><span>שורה בלי כפתור</span></li>
          </ul>
        </RightClickArea>
      ) : (
        <div role="tree" aria-label="עץ בדוי">
          {names.map((name) => (
            <RightClick key={name} items={itemsOf(name)}>
              <div role="treeitem" aria-selected={false} tabIndex={-1}>{name}</div>
            </RightClick>
          ))}
        </div>
      )}
      <span>{`רץ ${ran}`}</span>
      {open !== null && (
        <Dialog title={`העברה של ${open}`} onClose={() => setOpen(null)}>
          <p>תוכן בדוי</p>
        </Dialog>
      )}
    </div>
  );
}

/** What she works on in row ב: its name (a button in the list, the row itself in the tree). */
const rowB = (kind: "area" | "row") => (kind === "area" ? screen.getByRole("button", { name: "תיק בדוי ב" }) : screen.getByRole("treeitem", { name: "תיק בדוי ב" }));

describe.each(["area", "row"] as const)("a right-click menu (%s)", (kind) => {
  it("opened from the keyboard: a dialog of an item gives the focus back to where she was, not to the page", async () => {
    const user = userEvent.setup();
    render(<List kind={kind} />);
    const name = rowB(kind);
    act(() => name.focus());
    // Shift+F10, or the menu key: the browser fires "contextmenu" at the focused element.
    fireEvent.contextMenu(name);
    const item = await screen.findByRole("menuitem", { name: "העברה לתיקייה…" });
    act(() => item.focus());
    await user.keyboard("{Enter}");
    const dialog = await screen.findByRole("dialog", { name: "העברה של תיק בדוי ב" });
    await waitFor(() => expect(screen.queryByRole("menu")).toBeNull());
    await waitFor(() => expect(dialog).toHaveFocus());
    // The menu's closing did not take the focus out of the dialog.
    await new Promise((r) => setTimeout(r, 20));
    expect(dialog).toHaveFocus();
    await user.keyboard("{Escape}");
    await waitFor(() => expect(screen.queryByRole("dialog")).toBeNull());
    expect(document.activeElement).toBe(name);
  });

  it("an item that opens nothing leaves the focus where she was, and Escape still closes the menu onto it", async () => {
    const user = userEvent.setup();
    render(<List kind={kind} />);
    const name = rowB(kind);
    act(() => name.focus());
    fireEvent.contextMenu(name);
    act(() => screen.getByRole("menuitem", { name: "פעולה שקטה" }).focus());
    await user.keyboard("{Enter}");
    expect(screen.getByText("רץ 1")).toBeInTheDocument();
    await waitFor(() => expect(screen.queryByRole("menu")).toBeNull());
    await new Promise((r) => setTimeout(r, 20));
    expect(name).toHaveFocus();
    fireEvent.contextMenu(name);
    await screen.findByRole("menu");
    await user.keyboard("{Escape}");
    await waitFor(() => expect(screen.queryByRole("menu")).toBeNull());
    await waitFor(() => expect(name).toHaveFocus());
  });

  it("a right click with the focus elsewhere: the dialog gives the focus back to the row's own name", async () => {
    const user = userEvent.setup();
    render(<List kind={kind} />);
    act(() => screen.getByRole("button", { name: "לפני הרשימה" }).focus());
    // The click is on the row's text, not on its name.
    fireEvent.contextMenu(kind === "area" ? screen.getByText("פרטים של תיק בדוי ב") : within(rowB(kind)).getByText("תיק בדוי ב"));
    await user.click(await screen.findByRole("menuitem", { name: "העברה לתיקייה…" }));
    const dialog = await screen.findByRole("dialog", { name: "העברה של תיק בדוי ב" });
    await waitFor(() => expect(dialog).toHaveFocus());
    await user.click(within(dialog).getByRole("button", { name: "סגירה" }));
    await waitFor(() => expect(screen.queryByRole("dialog")).toBeNull());
    expect(document.activeElement).toBe(rowB(kind));
  });
});

it("a right click on a row with nothing that takes the focus: the item still runs, and the focus is left alone", async () => {
  const user = userEvent.setup();
  render(<List kind="area" />);
  fireEvent.contextMenu(screen.getByText("שורה בלי כפתור"));
  await user.click(await screen.findByRole("menuitem", { name: "העברה לתיקייה…" }));
  const dialog = await screen.findByRole("dialog", { name: "העברה של שורה שקטה" });
  await waitFor(() => expect(dialog).toHaveFocus());
  await user.click(within(dialog).getByRole("button", { name: "סגירה" }));
  await waitFor(() => expect(screen.queryByRole("dialog")).toBeNull());
  // Not on another row's button.
  expect(screen.getAllByRole("button").filter((b) => b === document.activeElement)).toEqual([]);
});
