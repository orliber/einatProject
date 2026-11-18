// The case explorer moves, never copies: a case lives in one folder, so a drag to a folder takes
// it out of where it was, and the recycle bin takes exactly the one case. Invented cases only.
import { configure, fireEvent, render, screen, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { mockIPC } from "@tauri-apps/api/mocks";
import { App } from "../../App";
import { FakeCore } from "../../web/fakeCore";
import { status } from "../../test/fixtures";
import { casesShown, folderStats, goesIn, inWorkLine, readDrag } from "../CasesScreen";
import type { CaseSummary, Folder } from "../../ipc/client";

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

/** The list over the demo core, with the demo case "נועם" changed as the core would list it. */
async function openWith(change: (c: CaseSummary) => CaseSummary) {
  const core = new FakeCore();
  await core.handle("unlock", { password: "x" });
  mockIPC(async (cmd, args) => {
    const out = await core.handle(cmd, args);
    return cmd === "list_cases" ? (out as CaseSummary[]).map((c) => (c.child_name === "נועם" ? change(c) : c)) : out;
  });
  render(<App />);
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
    // D-203 §2: no prominent button in the table. Beside "+ תיק חדש", the one prominent button
    // is on the card above it, on the case she is continuing.
    expect(table().querySelectorAll(".btn-primary")).toHaveLength(0);
    const card = screen.getByRole("region", { name: "להמשיך איפה שהפסקת" });
    const prominent = Array.from(screen.getByRole("main").querySelectorAll(".btn-primary")).filter((b) => b.textContent !== "+ תיק חדש");
    expect(prominent).toEqual([within(card).getByRole("button", { name: "להמשיך לכתוב ←" })]);
    // The folder's name is its stop on the keyboard: Tab to it, Enter opens it.
    within(row).getByRole("button", { name: "פתיחת התיקייה אבחונים פרטיים" }).focus();
    await user.keyboard("{Enter}");
    expect(await screen.findByRole("button", { name: /בתיק של מאיה$/ })).toBeInTheDocument();
    expect(within(screen.getByRole("navigation", { name: "מיקום" })).getByRole("button", { name: "אבחונים פרטיים" })).toHaveAttribute("aria-current", "page");
    // Inside a folder too, no row has a prominent button.
    expect(table().querySelectorAll(".btn-primary")).toHaveLength(0);
    await user.click(screen.getByRole("button", { name: "לתיקייה שמעל" }));
    await vi.waitFor(() => expect(screen.queryByRole("navigation", { name: "מיקום" })).toBeNull());
  });

  it("the sentence about dragging is gone, and the case's name is the button that opens it", async () => {
    const { user } = await openList();
    expect(screen.queryByText(/גוררים תיק לתיקייה/)).toBeNull();
    // The foot still counts the cases.
    expect(document.querySelector(".explorer-foot")).toHaveTextContent("תיק אחד");
    const row = caseRow("נועם");
    const name = within(row).getByRole("button", { name: "להמשיך בתיק של נועם" });
    // Its visible words are the child's name; "⋯" is the only other control in the row.
    expect(name).toHaveTextContent("נועם");
    expect(within(row).getAllByRole("button")).toEqual([name, within(row).getByRole("button", { name: "פעולות על התיק של נועם" })]);
    await user.click(name);
    expect(await screen.findByRole("article", { name: "הדוח, כמו בקובץ" })).toBeInTheDocument();
  });

  it("a case ready to go out is one green chip, and its name says “להוציא”", async () => {
    await openWith((c) => {
      const ready = { ...c, approved_sections: Array.from({ length: c.total_sections }, (_, i) => `section-${i}`) };
      delete ready.next_section;
      return ready;
    });
    const name = await screen.findByRole("button", { name: "להוציא בתיק של נועם" });
    const row = name.closest("tr") as HTMLElement;
    expect(within(row).getByText("כל הסעיפים אושרו · מוכן להוצאה")).toHaveClass("chip", "chip-ok");
    // No bar beside the chip: it says all there is.
    expect(row.querySelector("progress")).toBeNull();
  });

  it("a case with no name keeps “ללא שם” inside its button's name", async () => {
    await openWith((c) => ({ ...c, child_name: null }));
    const name = await screen.findByRole("button", { name: "להמשיך בתיק של תיק-1024 (ללא שם)" });
    expect(name).toHaveTextContent(/^ללא שם$/);
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

  it("a case dragged by its name moves like one dragged by its row (D-203)", async () => {
    await openList();
    const name = within(caseRow("נועם")).getByRole("button", { name: "להמשיך בתיק של נועם" });
    expect(name).toHaveAttribute("draggable", "true");
    expect(drag(name, folderRow("הפניות מהמכון"))).toBe(true);
    expect(await screen.findByText(/התיק של נועם הועבר ל"הפניות מהמכון"/)).toBeInTheDocument();
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

  it("counts and dates every folder in one pass, with the cases of its subfolders", () => {
    const folder = (id: string, parent: string | null) => ({ id, parent_id: parent, name: id }) as unknown as Folder;
    const at = (id: string, f: string | null, updated: number) => ({ ...summary(id, f), updated_at: updated });
    const stats = folderStats([folder("a", null), folder("b", "a"), folder("c", null)], [at("1", "b", 10), at("2", "a", 30), at("3", null, 99), at("4", "gone", 5)]);
    expect(stats.get("a")).toEqual({ count: 2, last: 30 });
    expect(stats.get("b")).toEqual({ count: 1, last: 10 });
    expect(stats.get("c")).toBeUndefined();
  });

  it("says one, none and many in Hebrew", () => {
    expect(inWorkLine(0)).toBe("אין תיקים בעבודה.");
    expect(inWorkLine(1)).toBe("תיק אחד בעבודה.");
    expect(inWorkLine(4)).toBe("4 תיקים בעבודה.");
    expect(goesIn(0)).toBe("יימחק בקרוב");
    expect(goesIn(1)).toBe("יימחק מחר");
    expect(goesIn(2)).toBe("יימחק בעוד יומיים");
    expect(goesIn(12)).toBe("יימחק בעוד 12 ימים");
  });

  it("a refusal does not stay on the screen once something else worked", async () => {
    const { user, core } = await openList();
    let refuse = true;
    mockIPC((cmd, args) => {
      if (cmd === "create_follow_up" && refuse) throw { code: "refused", message: "אי אפשר לפתוח מעקב עכשיו." };
      return core.handle(cmd, args);
    });
    const followUp = async () => {
      await user.click(screen.getByRole("button", { name: "פעולות על התיק של נועם" }));
      await user.click(await screen.findByRole("menuitem", { name: /אבחון מעקב/ }));
    };
    await followUp();
    expect(await screen.findByText("אי אפשר לפתוח מעקב עכשיו.")).toBeInTheDocument();
    // Something else that works: the old refusal does not stay beside it.
    await user.click(screen.getByRole("button", { name: "פעולות על התיק של נועם" }));
    await user.click(await screen.findByRole("menuitem", { name: "העברה לסל המחזור" }));
    await user.click(within(await screen.findByRole("dialog")).getByRole("button", { name: "העברה לסל המחזור" }));
    await vi.waitFor(() => expect(screen.queryByText("אי אפשר לפתוח מעקב עכשיו.")).toBeNull());
    refuse = false;
  });
});

describe("the list with no case in it", () => {
  const binned = { ...summary("נועם", null), child_name: "נועם", deleted_at: Math.floor(Date.now() / 1000) - 86_400, updated_at: 1, meta: { code: "ת-1" } } as unknown as CaseSummary;
  function open(reply: (cmd: string) => unknown) {
    mockIPC((cmd) => {
      if (cmd === "app_status") return status({ vault_exists: true, unlocked: true });
      const r = reply(cmd);
      return r === undefined ? null : r;
    });
    render(<App />);
    return userEvent.setup();
  }

  it("keeps the recycle bin in reach when the only case was deleted", async () => {
    const user = open((cmd) => (cmd === "list_cases" || cmd === "folders" ? [] : cmd === "list_trash" ? [binned] : undefined));
    await user.click(await screen.findByRole("button", { name: /סל המחזור/ }));
    expect(await screen.findByRole("button", { name: "שחזור" })).toBeInTheDocument();
    expect(screen.queryByRole("heading", { name: "התיק הראשון" })).toBeNull();
  });

  it("says so when the sample case could not be made", async () => {
    const user = open((cmd) => {
      if (cmd === "create_sample_case") throw { code: "internal", message: "התיק לדוגמה לא נוצר." };
      return cmd === "list_cases" || cmd === "folders" || cmd === "list_trash" ? [] : undefined;
    });
    await user.click(await screen.findByRole("button", { name: "לנסות קודם על תיק לדוגמה" }));
    expect(await screen.findByText("התיק לדוגמה לא נוצר.")).toBeInTheDocument();
  });
});
