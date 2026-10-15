// The folder tree of the case list, like the left pane of a file explorer (round 47, D-086).
// Folders nest to any depth; a folder with children opens and closes with its arrow, the
// arrow keys walk the visible rows, and a case or folder can be dropped on any row.
import { useRef, useState, type DragEvent, type KeyboardEvent, type ReactNode } from "react";
import type { Folder } from "../../ipc/client";
import { FolderIcon } from "../library/LibraryDialogs";

/** Which folders are open, kept while the vault is open (the page reloads on lock). */
const opened = new Set<string>();

/** Closes every branch (tests start from a closed tree). */
export function closeAllFolders() {
  opened.clear();
}

export type TreeRow = { folder: Folder; depth: number; children: boolean; open: boolean };

/** The rows the tree shows, top to bottom, with closed branches left out. */
export function visibleRows(folders: Folder[], open: Set<string>): TreeRow[] {
  const rows: TreeRow[] = [];
  const kids = (parent: string | null) =>
    folders.filter((f) => f.parent_id === parent).sort((a, b) => a.name.localeCompare(b.name, "he"));
  const walk = (parent: string | null, depth: number) => {
    for (const f of kids(parent)) {
      const children = folders.some((x) => x.parent_id === f.id);
      const isOpen = open.has(f.id);
      rows.push({ folder: f, depth, children, open: isOpen });
      if (children && isOpen) walk(f.id, depth + 1);
    }
  };
  walk(null, 1);
  return rows;
}

type Props = {
  folders: Folder[];
  /** The folder shown now; `null` = the top level. */
  current: string | null;
  inTrash: boolean;
  trashCount: number;
  /** Cases inside a folder, at any depth. */
  countIn: (id: string) => number;
  total: number;
  onPick: (id: string | null) => void;
  onPickTrash: () => void;
  onNewFolder: () => void;
  /** Drop handlers for a row (the top level is `null`). */
  dropProps: (target: string | null) => Record<string, unknown>;
  dropOn: string | null;
  onDragStart: (folderId: string) => (e: DragEvent) => void;
  /** Wraps a folder row with its right-click menu. */
  rightClick: (f: Folder, row: ReactNode) => ReactNode;
};

export function FolderTree(p: Props) {
  const [, bump] = useState(0);
  const listRef = useRef<HTMLDivElement>(null);
  // The current folder's ancestors are always open, so she sees where she is.
  for (let id = p.folders.find((x) => x.id === p.current)?.parent_id; id; ) {
    opened.add(id);
    id = p.folders.find((x) => x.id === id)?.parent_id ?? null;
  }
  const rows = visibleRows(p.folders, opened);
  const toggle = (id: string) => {
    if (opened.has(id)) opened.delete(id);
    else opened.add(id);
    bump((n) => n + 1);
  };

  // Arrow keys walk the rows; in a right-to-left tree the left arrow opens and the right closes.
  const onKey = (e: KeyboardEvent<HTMLDivElement>) => {
    const items = Array.from(listRef.current?.querySelectorAll<HTMLElement>("[data-tree-row]") ?? []);
    const i = items.indexOf(document.activeElement as HTMLElement);
    if (i < 0) return;
    const rowId = items[i]?.dataset.treeRow ?? "";
    const row = rows.find((r) => r.folder.id === rowId);
    const focus = (j: number) => items[Math.max(0, Math.min(items.length - 1, j))]?.focus();
    if (e.key === "ArrowDown") { e.preventDefault(); focus(i + 1); }
    else if (e.key === "ArrowUp") { e.preventDefault(); focus(i - 1); }
    else if (e.key === "Home") { e.preventDefault(); focus(0); }
    else if (e.key === "End") { e.preventDefault(); focus(items.length - 1); }
    else if (e.key === "ArrowLeft" && row?.children) { e.preventDefault(); if (!row.open) toggle(row.folder.id); else focus(i + 1); }
    else if (e.key === "ArrowRight" && row) {
      e.preventDefault();
      if (row.children && row.open) toggle(row.folder.id);
      else if (row.folder.parent_id) items.find((el) => el.dataset.treeRow === row.folder.parent_id)?.focus();
      else focus(0);
    }
  };

  const cls = (id: string | null, on: boolean) => ["tree-row", on ? "on" : "", p.dropOn === (id ?? "top") ? "drop" : ""].filter(Boolean).join(" ");
  const topOn = !p.inTrash && p.current === null;

  return (
    <nav className="tree" aria-label="תיקיות">
      <div ref={listRef} role="tree" aria-label="תיקיות" className="tree-list" onKeyDown={onKey}>
        <div role="treeitem" aria-selected={topOn} aria-level={1} tabIndex={0} data-tree-row="" className={cls(null, topOn)}
          onClick={() => p.onPick(null)} onKeyDown={(e) => { if (e.key === "Enter" || e.key === " ") { e.preventDefault(); p.onPick(null); } }}
          {...p.dropProps(null)}>
          <span className="tree-arrow" aria-hidden="true" />
          <span className="tree-icon"><FolderIcon size={18} /></span>
          <span className="tree-name">כל התיקים</span>
          <span className="tree-count">{p.total || ""}</span>
        </div>
        {rows.map((r) => {
          const on = !p.inTrash && p.current === r.folder.id;
          const n = p.countIn(r.folder.id);
          const row = (
            <div key={r.folder.id} role="treeitem" aria-selected={on} aria-level={r.depth + 1} aria-expanded={r.children ? r.open : undefined}
              tabIndex={-1} data-tree-row={r.folder.id} className={cls(r.folder.id, on)} style={{ paddingInlineStart: 6 + (r.depth - 1) * 18 }}
              draggable onDragStart={p.onDragStart(r.folder.id)}
              onClick={() => p.onPick(r.folder.id)}
              onKeyDown={(e) => { if (e.key === "Enter" || e.key === " ") { e.preventDefault(); p.onPick(r.folder.id); } }}
              {...p.dropProps(r.folder.id)}>
              {r.children ? (
                <button type="button" className="tree-arrow" tabIndex={-1} aria-label={r.open ? `לסגור את ${r.folder.name}` : `לפתוח את ${r.folder.name}`}
                  onClick={(e) => { e.stopPropagation(); toggle(r.folder.id); }}>
                  <svg width="12" height="12" viewBox="0 0 12 12" aria-hidden="true" style={{ transform: r.open ? "rotate(-90deg)" : "none" }}>
                    <path d="M8 2 4 6l4 4" fill="none" stroke="currentColor" strokeWidth="1.6" strokeLinecap="round" strokeLinejoin="round" />
                  </svg>
                </button>
              ) : <span className="tree-arrow" aria-hidden="true" />}
              <span className="tree-icon"><FolderIcon size={18} /></span>
              <span className="tree-name">{r.folder.name}</span>
              <span className="tree-count">{n || ""}</span>
            </div>
          );
          return <span key={r.folder.id} className="tree-wrap">{p.rightClick(r.folder, row)}</span>;
        })}
      </div>
      <button type="button" className="tree-row tree-new" onClick={p.onNewFolder}>
        <span className="tree-arrow" aria-hidden="true" />
        <span className="tree-icon" aria-hidden="true">+</span>
        <span className="tree-name">תיקייה חדשה</span>
      </button>
      <div className="tree-sep" role="separator" />
      <button type="button" className={p.inTrash ? "tree-row on" : "tree-row"} aria-current={p.inTrash ? "page" : undefined} onClick={p.onPickTrash}>
        <span className="tree-arrow" aria-hidden="true" />
        <span className="tree-icon" aria-hidden="true">
          <svg width="18" height="18" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.8" strokeLinecap="round" strokeLinejoin="round">
            <path d="M4 7h16M9 7V5h6v2M6 7l1 13h10l1-13" />
          </svg>
        </span>
        <span className="tree-name">סל המחזור</span>
        <span className="tree-count">{p.trashCount || ""}</span>
      </button>
    </nav>
  );
}
