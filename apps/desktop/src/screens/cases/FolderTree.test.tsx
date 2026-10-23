// The folder tree opens and closes branches, walks with the arrow keys and takes a drop (round 47).
// Every folder here is invented.
import { fireEvent, render, screen, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";
import type { Folder } from "../../ipc/client";
import { closeAllFolders, FolderTree, visibleRows } from "./FolderTree";
import { a11yViolations } from "../../test/a11y";

const folders: Folder[] = [
  { id: "a", parent_id: null, name: "גן חובה", created_at: 0 },
  { id: "a1", parent_id: "a", name: "תשפ״ו", created_at: 0 },
  { id: "a1x", parent_id: "a1", name: "מחצית א", created_at: 0 },
  { id: "b", parent_id: null, name: "בית ספר", created_at: 0 },
];

function tree(over: Partial<Parameters<typeof FolderTree>[0]> = {}) {
  const onPick = vi.fn();
  const dropped: (string | null)[] = [];
  const view = render(
    <FolderTree folders={folders} current={null} inTrash={false} trashCount={0} countIn={(id) => (id === "a" ? 3 : 0)} total={5}
      onPick={onPick} onPickTrash={vi.fn()} onNewFolder={vi.fn()} dropOn={null}
      dropProps={(target) => ({ onDrop: () => dropped.push(target) })}
      onDragStart={() => () => undefined} rightClick={(_f, row) => row} {...over} />,
  );
  return { onPick, dropped, view };
}

describe("FolderTree", () => {
  beforeEach(closeAllFolders);

  it("shows the top-level folders closed, with the count of the cases inside", () => {
    expect(visibleRows(folders, new Set()).map((r) => r.folder.id)).toEqual(["b", "a"]);
    expect(visibleRows(folders, new Set(["a"])).map((r) => r.folder.id)).toEqual(["b", "a", "a1"]);
    expect(visibleRows(folders, new Set(["a", "a1"])).map((r) => `${r.folder.id}:${r.depth}`)).toEqual(["b:1", "a:1", "a1:2", "a1x:3"]);
  });

  it("opens a branch with its arrow and closes it again", async () => {
    const user = userEvent.setup();
    const { view } = tree();
    const items = screen.getByRole("tree");
    expect(within(items).queryByText("תשפ״ו")).toBeNull();
    await user.click(screen.getByRole("button", { name: "לפתוח את גן חובה" }));
    expect(within(items).getByText("תשפ״ו")).toBeInTheDocument();
    expect(within(items).getByRole("treeitem", { name: /גן חובה/ })).toHaveAttribute("aria-expanded", "true");
    expect(await a11yViolations(view.container)).toEqual([]);
    await user.click(screen.getByRole("button", { name: "לסגור את גן חובה" }));
    expect(within(items).queryByText("תשפ״ו")).toBeNull();
  });

  it("keeps the ancestors of the current folder open", () => {
    tree({ current: "a1x" });
    const items = screen.getByRole("tree");
    expect(within(items).getByText("מחצית א")).toBeInTheDocument();
    expect(within(items).getByRole("treeitem", { name: /מחצית א/ })).toHaveAttribute("aria-selected", "true");
  });

  it("an ancestor of the current folder can still be closed, and the Tab stop is the selected row", async () => {
    const user = userEvent.setup();
    const { view } = tree({ current: "a1" });
    expect(screen.getByRole("treeitem", { name: /^תשפ״ו/ })).toHaveAttribute("tabindex", "0");
    expect(screen.getByRole("treeitem", { name: /^תיקים/ })).toHaveAttribute("tabindex", "-1");
    await user.click(screen.getByRole("button", { name: "לסגור את גן חובה" }));
    expect(screen.getByRole("treeitem", { name: /גן חובה/ })).toHaveAttribute("aria-expanded", "false");
    expect(screen.queryByText("תשפ״ו")).toBeNull();
    // Moving to another folder opens the branches above it again.
    view.rerender(
      <FolderTree folders={folders} current="a1x" inTrash={false} trashCount={0} countIn={() => 0} total={5}
        onPick={vi.fn()} onPickTrash={vi.fn()} onNewFolder={vi.fn()} dropOn={null} dropProps={() => ({})}
        onDragStart={() => () => undefined} rightClick={(_f, row) => row} />,
    );
    expect(screen.getByRole("treeitem", { name: /מחצית א/ })).toHaveAttribute("tabindex", "0");
  });

  it("walks with the arrow keys, and Enter picks", async () => {
    const user = userEvent.setup();
    const { onPick } = tree();
    const top = screen.getByRole("treeitem", { name: /^תיקים/ });
    top.focus();
    await user.keyboard("{ArrowDown}");
    expect(document.activeElement).toHaveTextContent("בית ספר");
    await user.keyboard("{ArrowDown}{ArrowLeft}");
    expect(document.activeElement).toHaveTextContent("גן חובה");
    expect(screen.getByRole("treeitem", { name: /גן חובה/ })).toHaveAttribute("aria-expanded", "true");
    await user.keyboard("{ArrowDown}{Enter}");
    expect(onPick).toHaveBeenLastCalledWith("a1");
    await user.keyboard("{ArrowRight}");
    expect(document.activeElement).toHaveTextContent("גן חובה");
  });

  it("takes a drop on a folder row and on the top level", () => {
    const { dropped } = tree();
    fireEvent.drop(screen.getByRole("treeitem", { name: /בית ספר/ }));
    fireEvent.drop(screen.getByRole("treeitem", { name: /^תיקים/ }));
    expect(dropped).toEqual(["b", null]);
  });
});
