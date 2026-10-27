// The case explorer moves, never copies: a case lives in one folder, so a drag to a folder takes
// it out of where it was, and the recycle bin takes exactly the one case. Invented cases only.
import { configure, fireEvent, render, screen, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { mockIPC } from "@tauri-apps/api/mocks";
import { App } from "../../App";
import { FakeCore } from "../../web/fakeCore";
import { casesShown, readDrag } from "../CasesScreen";
import type { CaseSummary } from "../../ipc/client";

configure({ asyncUtilTimeout: 5000 });
vi.setConfig({ testTimeout: 30_000 });

/** A drag's data, as a browser carries it between dragstart and drop. */
function transfer() {
  const data = new Map<string, string>();
  return {
    data,
    types: [] as string[],
    effectAllowed: "all",
    dropEffect: "none",
    setData(type: string, value: string) {
      data.set(type, value);
      if (!this.types.includes(type)) this.types.push(type);
    },
    getData: (type: string) => data.get(type) ?? "",
  };
}

/** Drags `from` onto `to`; returns whether the target took the drag (dragover was accepted). */
function drag(from: Element, to: Element): boolean {
  const dataTransfer = transfer();
  fireEvent.dragStart(from, { dataTransfer });
  const notTaken = fireEvent.dragOver(to, { dataTransfer });
  if (!notTaken) fireEvent.drop(to, { dataTransfer });
  fireEvent.dragEnd(from, { dataTransfer });
  return !notTaken;
}

async function openList() {
  const core = new FakeCore();
  await core.handle("unlock", { password: "x" });
  const calls: string[] = [];
  mockIPC((cmd, args) => {
    calls.push(cmd);
    return core.handle(cmd, args);
  });
  const user = userEvent.setup();
  render(<App />);
  await screen.findByRole("button", { name: /בתיק של נועם$/ });
  return { user, core, calls };
}

const table = () => screen.getByRole("table");
const caseRow = (name: string) => {
  const row = within(table()).getByText(name, { exact: false, selector: ".case-name" }).closest("tr");
  if (!row) throw new Error("no row");
  return row;
};
const folderRow = (name: string) => {
  const row = within(table()).getByText(name, { selector: ".case-name" }).closest("tr");
  if (!row) throw new Error("no row");
  return row;
};
const shownNames = () => Array.from(table().querySelectorAll("tr[data-case] .case-name")).map((n) => n.firstChild?.textContent);
const treeRow = (name: RegExp) => within(screen.getByRole("tree")).getByRole("treeitem", { name });
const summary = (id: string, folder: string | null): CaseSummary =>
  ({ id, folder_id: folder, child_name: id, meta: { code: id } }) as unknown as CaseSummary;

describe("the case explorer", () => {
  it("a folder lists only the cases directly in it; a search looks everywhere", () => {
    const all = [summary("top", null), summary("in-a", "a"), summary("in-a1", "a1")];
    expect(casesShown(all, null, null).map((c) => c.id)).toEqual(["top"]);
    expect(casesShown(all, "a", null).map((c) => c.id)).toEqual(["in-a"]);
    expect(casesShown(all, "a1", null).map((c) => c.id)).toEqual(["in-a1"]);
    expect(casesShown(all, "a", (c) => c.id.startsWith("in")).map((c) => c.id)).toEqual(["in-a", "in-a1"]);
  });

  it("reads only its own drags", () => {
    expect(readDrag(JSON.stringify({ case: "c1" }))).toEqual({ case: "c1" });
    expect(readDrag(JSON.stringify({ folder: "f1" }))).toEqual({ folder: "f1" });
    for (const raw of ["", "not json", "null", "[]", JSON.stringify({ case: 3 }), JSON.stringify({ other: "x" })]) {
      expect(readDrag(raw)).toBeNull();
    }
  });

  it("the top level shows a case that sits in a folder only inside that folder", async () => {
    const { user } = await openList();
    expect(shownNames()).toEqual(["נועם"]);
    await user.click(screen.getByRole("button", { name: "פתיחת התיקייה אבחונים פרטיים" }));
    expect(await screen.findByRole("button", { name: /בתיק של מאיה$/ })).toBeInTheDocument();
    expect(shownNames()).toEqual(["מאיה"]);
  });

  it("is quiet: no path at the top, a folder opens from its name, one prominent button (D-113)", async () => {
    const { user } = await openList();
    // At the top the tab and the tree already say "תיקים": no path and no "up" beside them.
    expect(screen.queryByRole("navigation", { name: "מיקום" })).toBeNull();
    expect(screen.queryByRole("button", { name: "לתיקייה שמעל" })).toBeNull();
    // A folder row has no "פתיחה" button of its own.
    const row = folderRow("אבחונים פרטיים");
    expect(within(row).queryByRole("button", { name: /^פתיחה/ })).toBeNull();
    // "עודכן": when a case inside last changed; nothing for an empty folder.
    expect(row.querySelector("td.col-when")?.textContent).not.toBe("");
    expect(folderRow("הפניות מהמכון").querySelector("td.col-when")?.textContent).toBe("");
    // One prominent button in the list, on the case she is continuing.
    const primaries = table().querySelectorAll(".btn-primary");
    expect(primaries).toHaveLength(1);
    expect(caseRow("נועם").contains(primaries[0] ?? null)).toBe(true);
    // The folder's name is its stop on the keyboard: Tab to it, Enter opens it.
    within(row).getByRole("button", { name: "פתיחת התיקייה אבחונים פרטיים" }).focus();
    await user.keyboard("{Enter}");
    expect(await screen.findByRole("button", { name: /בתיק של מאיה$/ })).toBeInTheDocument();
    expect(within(screen.getByRole("navigation", { name: "מיקום" })).getByRole("button", { name: "אבחונים פרטיים" })).toHaveAttribute("aria-current", "page");
    // Inside a folder no case is the one being continued: no prominent button there.
    expect(table().querySelectorAll(".btn-primary")).toHaveLength(0);
    await user.click(screen.getByRole("button", { name: "לתיקייה שמעל" }));
    await vi.waitFor(() => expect(screen.queryByRole("navigation", { name: "מיקום" })).toBeNull());
  });

  it("clicking anywhere on a folder row opens it", async () => {
    const { user } = await openList();
    await user.click(within(folderRow("אבחונים פרטיים")).getByText("תיק אחד"));
    expect(await screen.findByRole("button", { name: /בתיק של מאיה$/ })).toBeInTheDocument();
  });

  it("a case dragged to a folder moves there (it is no longer at the top), and ביטול brings it back", async () => {
    const { user, core } = await openList();
    expect(drag(caseRow("נועם"), folderRow("הפניות מהמכון"))).toBe(true);
    expect(await screen.findByText(/התיק של נועם הועבר ל"הפניות מהמכון"/)).toBeInTheDocument();
    await vi.waitFor(() => expect(within(table()).queryByText("נועם", { exact: false, selector: ".case-name" })).toBeNull());
    const cases = (await core.handle("list_cases", {})) as CaseSummary[];
    expect(cases.filter((c) => c.child_name === "נועם")).toHaveLength(1);

    await user.click(screen.getByRole("button", { name: "ביטול" }));
    expect(await screen.findByRole("button", { name: /בתיק של נועם$/ })).toBeInTheDocument();
    expect(shownNames()).toEqual(["נועם"]);
  });

  it("a moved case thrown in the recycle bin takes nothing else with it", async () => {
    const { user, core } = await openList();
    drag(caseRow("נועם"), folderRow("אבחונים פרטיים"));
    await user.click(screen.getByRole("button", { name: "פתיחת התיקייה אבחונים פרטיים" }));
    await screen.findByRole("button", { name: /בתיק של נועם$/ });
    expect(shownNames().sort()).toEqual(["מאיה", "נועם"]);

    await user.click(screen.getByRole("button", { name: "פעולות על התיק של נועם" }));
    await user.click(await screen.findByRole("menuitem", { name: "העברה לסל המחזור" }));
    await user.click(within(await screen.findByRole("dialog")).getByRole("button", { name: "העברה לסל המחזור" }));
    await vi.waitFor(() => expect(shownNames()).toEqual(["מאיה"]));
    expect(((await core.handle("list_trash", {})) as CaseSummary[]).map((c) => c.child_name)).toEqual(["נועם"]);
    expect(((await core.handle("list_cases", {})) as CaseSummary[]).map((c) => c.child_name)).toEqual(["מאיה"]);

    // ביטול on the notice restores it to the folder it was in.
    await user.click(screen.getByRole("button", { name: "ביטול" }));
    await vi.waitFor(() => expect(shownNames().sort()).toEqual(["מאיה", "נועם"]));
  });

  it("a drop where nothing would change is refused, and nothing is sent", async () => {
    const { calls } = await openList();
    // The case is already at the top; a folder into itself or into its own subfolder.
    expect(drag(caseRow("נועם"), treeRow(/^תיקים/))).toBe(false);
    expect(drag(folderRow("אבחונים פרטיים"), folderRow("אבחונים פרטיים"))).toBe(false);
    expect(drag(folderRow("אבחונים פרטיים"), treeRow(/^תיקים/))).toBe(false);
    expect(calls).not.toContain("move_case");
    expect(calls).not.toContain("move_folder");
  });

  it("a folder dragged onto another moves with what is inside it", async () => {
    const { user } = await openList();
    expect(drag(folderRow("הפניות מהמכון"), folderRow("אבחונים פרטיים"))).toBe(true);
    expect(await screen.findByText(/התיקייה "הפניות מהמכון" הועברה ל"אבחונים פרטיים"/)).toBeInTheDocument();
    await vi.waitFor(() => expect(within(table()).queryByText("הפניות מהמכון")).toBeNull());
    await user.click(screen.getByRole("button", { name: "פתיחת התיקייה אבחונים פרטיים" }));
    expect(await within(table()).findByText("הפניות מהמכון")).toBeInTheDocument();
  });

  it("a folder dragged by its name moves like one dragged by its row (D-113)", async () => {
    await openList();
    const name = within(folderRow("הפניות מהמכון")).getByRole("button", { name: "פתיחת התיקייה הפניות מהמכון" });
    expect(name).toHaveAttribute("draggable", "true");
    expect(drag(name, folderRow("אבחונים פרטיים"))).toBe(true);
    expect(await screen.findByText(/התיקייה "הפניות מהמכון" הועברה ל"אבחונים פרטיים"/)).toBeInTheDocument();
  });

  it("something dropped from outside the app is ignored", async () => {
    const { calls } = await openList();
    const dataTransfer = transfer();
    dataTransfer.setData("text/plain", "hello");
    expect(fireEvent.dragOver(folderRow("אבחונים פרטיים"), { dataTransfer })).toBe(true);
    // Even a forged drag of our type carrying junk does nothing.
    const forged = transfer();
    forged.setData("application/x-dv-item", "{not json");
    fireEvent.drop(folderRow("אבחונים פרטיים"), { dataTransfer: forged });
    expect(calls).not.toContain("move_case");
    expect(screen.queryByRole("alert")).toBeNull();
  });

  it("a case dropped on the recycle bin asks first", async () => {
    await openList();
    const bin = within(screen.getByRole("navigation", { name: "תיקיות" })).getByRole("button", { name: /סל המחזור/ });
    expect(drag(caseRow("נועם"), bin)).toBe(true);
    expect(within(await screen.findByRole("dialog")).getByText(/העברת התיק של נועם לסל המחזור/)).toBeInTheDocument();
  });

  it("clicking a folder in the search results opens it and ends the search", async () => {
    const { user } = await openList();
    await user.type(screen.getByRole("searchbox", { name: "חיפוש תיקים" }), "אבחונים");
    await user.click(await screen.findByRole("button", { name: "פתיחת התיקייה אבחונים פרטיים" }));
    expect(screen.getByRole("searchbox", { name: "חיפוש תיקים" })).toHaveValue("");
    expect(await screen.findByRole("button", { name: /בתיק של מאיה$/ })).toBeInTheDocument();
    expect(within(table()).getByText("2026")).toBeInTheDocument();
  });

  it("deleting the folder she is in leaves her where its contents went", async () => {
    const { user } = await openList();
    await user.click(screen.getByRole("button", { name: "פתיחת התיקייה אבחונים פרטיים" }));
    await user.click(await screen.findByRole("button", { name: "פתיחת התיקייה 2026" }));
    expect(await screen.findByText(/התיקייה ריקה/)).toBeInTheDocument();
    fireEvent.contextMenu(treeRow(/^2026/));
    await user.click(await screen.findByRole("menuitem", { name: "מחיקת התיקייה" }));
    await user.click(within(await screen.findByRole("dialog")).getByRole("button", { name: "מחיקת התיקייה" }));
    // Back in "אבחונים פרטיים", not thrown to the top.
    expect(await screen.findByRole("button", { name: /בתיק של מאיה$/ })).toBeInTheDocument();
    expect(within(screen.getByRole("navigation", { name: "מיקום" })).getByRole("button", { name: "אבחונים פרטיים" })).toHaveAttribute("aria-current", "page");
  });

  it("two folders side by side cannot share a name", async () => {
    const { user } = await openList();
    await user.click(screen.getByRole("button", { name: /תיקייה כאן/ }));
    await user.type(screen.getByLabelText("שם התיקייה"), " אבחונים פרטיים ");
    await user.click(screen.getByRole("button", { name: "שמירה" }));
    expect(await within(screen.getByRole("dialog")).findByText(/כבר יש כאן תיקייה בשם/)).toBeInTheDocument();
  });
});
