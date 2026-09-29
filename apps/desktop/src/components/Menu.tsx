// Menus for folders and cases (D-023): the same actions from a "⋯" button (keyboard) and from a
// right click. Built on Radix for focus handling, arrow keys and RTL.
//
// `modal={false}` everywhere: the modal mode locks scrolling by injecting a <style> element,
// which the app's Content-Security-Policy (no inline styles) refuses.
import type { ReactNode } from "react";
import * as ContextMenu from "@radix-ui/react-context-menu";
import * as DropdownMenu from "@radix-ui/react-dropdown-menu";
import "./Menu.css";

export interface MenuItem {
  label: string;
  run: () => void;
  danger?: boolean;
  disabled?: boolean;
}

/** A "⋯" button that opens the actions of one item. */
export function ActionsMenu({ items, label }: { items: MenuItem[]; label: string }) {
  return (
    <DropdownMenu.Root dir="rtl" modal={false}>
      <DropdownMenu.Trigger asChild>
        <button type="button" className="btn btn-icon menu-trigger" aria-label={label} onClick={(e) => e.stopPropagation()}>
          ⋯
        </button>
      </DropdownMenu.Trigger>
      <DropdownMenu.Portal>
        <DropdownMenu.Content className="menu" align="end" sideOffset={4} onClick={(e) => e.stopPropagation()}>
          {items.map((it) => (
            <DropdownMenu.Item key={it.label} className={it.danger ? "menu-item danger" : "menu-item"} disabled={it.disabled ?? false} onSelect={it.run}>
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
          {items.map((it) => (
            <ContextMenu.Item key={it.label} className={it.danger ? "menu-item danger" : "menu-item"} disabled={it.disabled ?? false} onSelect={it.run}>
              {it.label}
            </ContextMenu.Item>
          ))}
        </ContextMenu.Content>
      </ContextMenu.Portal>
    </ContextMenu.Root>
  );
}
