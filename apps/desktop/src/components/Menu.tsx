// Menus for folders and cases (D-023): the same actions from a "⋯" button (keyboard) and from a
// right click. Built on Radix for focus handling, arrow keys and RTL.
//
// `modal={false}` everywhere: the modal mode locks scrolling by injecting a <style> element,
// which the app's Content-Security-Policy (no inline styles) refuses.
import { useCallback, useRef, useState, type MouseEvent, type PointerEvent, type ReactElement, type ReactNode, type RefObject } from "react";
import * as ContextMenu from "@radix-ui/react-context-menu";
import * as DropdownMenu from "@radix-ui/react-dropdown-menu";
import { MoreIcon } from "./ui";
import "./Menu.css";

export interface MenuItem {
  label: string;
  run: () => void;
  danger?: boolean;
  disabled?: boolean;
}

/**
 * A chosen item: the focus goes back to where the menu was opened from first (its button, or the
 * row of a right-click menu), and only then does the item run. A dialog the item opens takes the
 * focused element as its opener and gives the focus back to it when it closes; the item itself is
 * gone by then, and the focus would fall to the page (WCAG 2.4.3). The focus leaving the menu
 * closes it, before the item runs. Nowhere to go back to: the item just runs.
 */
function backTo(back: RefObject<HTMLElement | null>, run: () => void): () => void {
  return () => {
    back.current?.focus();
    run();
  };
}

/** What can take the focus: a control, or an element with a tabindex of its own. */
const CONTROL = 'button:not([disabled]), a[href], input:not([disabled]):not([type="hidden"]), select:not([disabled]), textarea:not([disabled]), [tabindex]';

/**
 * Where the focus goes back to from a right-click menu opened on `row`: the focused element, when
 * it is in that row (the menu key or Shift+F10 on it); after a click with the focus elsewhere,
 * the row itself when it takes the focus, or else its first Tab stop (a case's name); else nowhere.
 */
function backOf(row: Element): HTMLElement | null {
  const at = document.activeElement;
  if (at instanceof HTMLElement && at !== document.body && row.contains(at)) return at;
  if (row instanceof HTMLElement && row.matches(CONTROL)) return row;
  return [...row.querySelectorAll<HTMLElement>(CONTROL)].find((el) => el.tabIndex >= 0) ?? null;
}

/** The row of a list that `target` is in: the list's own child that holds it (the list itself
 *  when it is the target). */
function rowIn(list: Element, target: Element): Element {
  let row = target;
  while (row !== list && row.parentElement !== null && row.parentElement !== list) row = row.parentElement;
  return row.parentElement === list ? row : list;
}

/** A "⋯" button that opens the actions of one item. It is a plain button until first used: a
 *  list of 50 cases would otherwise build 50 closed menus on every visit (plan 2.4).
 *  `buttonRef`: the "⋯" button, for a screen that puts the focus on it. */
export function ActionsMenu({ items, label, buttonRef }: { items: MenuItem[]; label: string; buttonRef?: RefObject<HTMLButtonElement | null> }) {
  const [armed, setArmed] = useState(false);
  const own = useRef<HTMLButtonElement | null>(null);
  // The same ref follows the plain button and the menu's own, which takes its place once armed.
  const ref = useCallback((el: HTMLButtonElement | null) => {
    own.current = el;
    if (buttonRef) buttonRef.current = el;
  }, [buttonRef]);
  const button = (arm: boolean) => (
    <button ref={ref} type="button" className="btn btn-icon menu-trigger" aria-label={label} aria-haspopup="menu"
      onClick={(e) => { e.stopPropagation(); if (arm) setArmed(true); }}
      onKeyDown={arm ? (e) => { if (e.key === "ArrowDown") { e.preventDefault(); setArmed(true); } } : undefined}>
      <MoreIcon />
    </button>
  );
  if (!armed) return button(true);
  return (
    <DropdownMenu.Root dir="rtl" modal={false} defaultOpen>
      <DropdownMenu.Trigger asChild>{button(false)}</DropdownMenu.Trigger>
      <DropdownMenu.Portal>
        <DropdownMenu.Content className="menu" align="end" sideOffset={4} onClick={(e) => e.stopPropagation()}>
          <Items items={items} Item={DropdownMenu.Item} back={own} />
        </DropdownMenu.Content>
      </DropdownMenu.Portal>
    </DropdownMenu.Root>
  );
}

/** The items of a menu. A chosen item gives the focus back to `back` (the menu's button, or the
 *  element of the row a right-click menu was opened on) before it runs (`backTo`). */
function Items({ items, Item, back }: {
  items: MenuItem[];
  Item: typeof DropdownMenu.Item | typeof ContextMenu.Item;
  back: RefObject<HTMLElement | null>;
}) {
  return (
    <>
      {items.map((it) => (
        <Item key={it.label} className={it.danger ? "menu-item danger" : "menu-item"} disabled={it.disabled ?? false}
          onSelect={backTo(back, it.run)}>
          {it.label}
        </Item>
      ))}
    </>
  );
}

/** A small "+" (or any trigger) that opens a list of ways to add something. */
export function AddMenu({ items, label, children, className }: { items: MenuItem[]; label: string; children: ReactNode; className?: string }) {
  const own = useRef<HTMLButtonElement | null>(null);
  return (
    <DropdownMenu.Root dir="rtl" modal={false}>
      <DropdownMenu.Trigger asChild>
        <button ref={own} type="button" className={className} aria-label={label} title={label}>{children}</button>
      </DropdownMenu.Trigger>
      <DropdownMenu.Portal>
        <DropdownMenu.Content className="menu" align="end" sideOffset={4}>
          <Items items={items} Item={DropdownMenu.Item} back={own} />
        </DropdownMenu.Content>
      </DropdownMenu.Portal>
    </DropdownMenu.Root>
  );
}

/** The same actions on right click over `children` (one row: a folder in the tree). */
export function RightClick({ items, children }: { items: MenuItem[]; children: ReactNode }) {
  const back = useRef<HTMLElement | null>(null);
  // Runs before Radix opens the menu, while the focus is still where she was.
  const mark = (e: MouseEvent | PointerEvent) => {
    back.current = backOf(e.currentTarget);
  };
  return (
    <ContextMenu.Root dir="rtl" modal={false}>
      <ContextMenu.Trigger asChild onContextMenu={mark} onPointerDown={(e) => { if (e.pointerType !== "mouse") mark(e); }}>{children}</ContextMenu.Trigger>
      <ContextMenu.Portal>
        <ContextMenu.Content className="menu">
          <Items items={items} Item={ContextMenu.Item} back={back} />
        </ContextMenu.Content>
      </ContextMenu.Portal>
    </ContextMenu.Root>
  );
}

/** One right-click menu for a whole list: `itemsAt` names the actions of the row under the
 *  pointer, or none (then nothing opens). One menu instead of one per row (plan 2.4). */
export function RightClickArea({ itemsAt, children }: { itemsAt: (target: Element) => MenuItem[] | null; children: ReactElement }) {
  const [items, setItems] = useState<MenuItem[]>([]);
  const back = useRef<HTMLElement | null>(null);
  // Runs before Radix's own handlers (right click, and a long press on touch), which skip
  // opening when the default is prevented.
  const pick = (e: MouseEvent | PointerEvent) => {
    const target = e.target instanceof Element ? e.target : null;
    const found = target ? itemsAt(target) : null;
    if (!target || !found?.length) {
      e.preventDefault();
      return;
    }
    setItems(found);
    back.current = backOf(rowIn(e.currentTarget, target));
  };
  return (
    <ContextMenu.Root dir="rtl" modal={false}>
      <ContextMenu.Trigger asChild onContextMenu={pick} onPointerDown={(e) => { if (e.pointerType !== "mouse") pick(e); }}>{children}</ContextMenu.Trigger>
      <ContextMenu.Portal>
        <ContextMenu.Content className="menu">
          <Items items={items} Item={ContextMenu.Item} back={back} />
        </ContextMenu.Content>
      </ContextMenu.Portal>
    </ContextMenu.Root>
  );
}
