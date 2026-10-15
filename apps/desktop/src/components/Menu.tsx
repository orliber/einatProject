// Menus for folders and cases (D-023): the same actions from a "⋯" button (keyboard) and from a
// right click. Built on Radix for focus handling, arrow keys and RTL.
//
// `modal={false}` everywhere: the modal mode locks scrolling by injecting a <style> element,
// which the app's Content-Security-Policy (no inline styles) refuses.
import { useState, type MouseEvent, type PointerEvent, type ReactElement, type ReactNode } from "react";
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

/** A "⋯" button that opens the actions of one item. It is a plain button until first used: a
 *  list of 50 cases would otherwise build 50 closed menus on every visit (plan 2.4). */
export function ActionsMenu({ items, label }: { items: MenuItem[]; label: string }) {
  const [armed, setArmed] = useState(false);
  const button = (arm: boolean) => (
    <button type="button" className="btn btn-icon menu-trigger" aria-label={label} aria-haspopup="menu"
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
          <Items items={items} Item={DropdownMenu.Item} />
        </DropdownMenu.Content>
      </DropdownMenu.Portal>
    </DropdownMenu.Root>
  );
}

function Items({ items, Item }: { items: MenuItem[]; Item: typeof DropdownMenu.Item | typeof ContextMenu.Item }) {
  return (
    <>
      {items.map((it) => (
        <Item key={it.label} className={it.danger ? "menu-item danger" : "menu-item"} disabled={it.disabled ?? false} onSelect={it.run}>
          {it.label}
        </Item>
      ))}
    </>
  );
}

/** A small "+" (or any trigger) that opens a list of ways to add something. */
export function AddMenu({ items, label, children, className }: { items: MenuItem[]; label: string; children: ReactNode; className?: string }) {
  return (
    <DropdownMenu.Root dir="rtl" modal={false}>
      <DropdownMenu.Trigger asChild>
        <button type="button" className={className} aria-label={label} title={label}>{children}</button>
      </DropdownMenu.Trigger>
      <DropdownMenu.Portal>
        <DropdownMenu.Content className="menu" align="end" sideOffset={4}>
          {items.map((it) => (
            <DropdownMenu.Item key={it.label} className="menu-item" disabled={it.disabled ?? false} onSelect={it.run}>
              {it.label}
            </DropdownMenu.Item>
          ))}
        </DropdownMenu.Content>
      </DropdownMenu.Portal>
    </DropdownMenu.Root>
  );
}

/** The same actions on right click over `children`. */
export function RightClick({ items, children }: { items: MenuItem[]; children: ReactNode }) {
  return (
    <ContextMenu.Root dir="rtl" modal={false}>
      <ContextMenu.Trigger asChild>{children}</ContextMenu.Trigger>
      <ContextMenu.Portal>
        <ContextMenu.Content className="menu">
          <Items items={items} Item={ContextMenu.Item} />
        </ContextMenu.Content>
      </ContextMenu.Portal>
    </ContextMenu.Root>
  );
}

/** One right-click menu for a whole list: `itemsAt` names the actions of the row under the
 *  pointer, or none (then nothing opens). One menu instead of one per row (plan 2.4). */
export function RightClickArea({ itemsAt, children }: { itemsAt: (target: Element) => MenuItem[] | null; children: ReactElement }) {
  const [items, setItems] = useState<MenuItem[]>([]);
  // Runs before Radix's own handlers (right click, and a long press on touch), which skip
  // opening when the default is prevented.
  const pick = (e: MouseEvent | PointerEvent) => {
    const found = e.target instanceof Element ? itemsAt(e.target) : null;
    if (!found?.length) e.preventDefault();
    else setItems(found);
  };
  return (
    <ContextMenu.Root dir="rtl" modal={false}>
      <ContextMenu.Trigger asChild onContextMenu={pick} onPointerDown={(e) => { if (e.pointerType !== "mouse") pick(e); }}>{children}</ContextMenu.Trigger>
      <ContextMenu.Portal>
        <ContextMenu.Content className="menu">
          <Items items={items} Item={ContextMenu.Item} />
        </ContextMenu.Content>
      </ContextMenu.Portal>
    </ContextMenu.Root>
  );
}
