// Which help belongs to what is on the screen now (D-206). Small and loaded with the app; the
// help itself (its words and pictures) loads only when she opens it.
import type { Route } from "../App";
import type { HelpId } from "./types";
import { useEffect } from "react";

/** Help asked for by a part of the page that is not a route (the scores view, a settings pane),
 *  in the order they appeared: the last one is what she is looking at. */
const shown: { id: HelpId }[] = [];

/** While the calling component is on the page, its help is what "?" opens (unless a window with
 *  help of its own is open over it). */
export function useHelpTopic(id: HelpId | undefined): void {
  useEffect(() => {
    if (!id) return;
    const entry = { id };
    shown.push(entry);
    return () => {
      const i = shown.lastIndexOf(entry);
      if (i >= 0) shown.splice(i, 1);
    };
  }, [id]);
}

/** The page's own help, by its route. */
export function helpOfRoute(route: Route | undefined): HelpId {
  if (!route) return "cases";
  switch (route.name) {
    case "cases":
      return "cases";
    case "case":
      return route.view === "details" ? "case-details"
        : route.view === "materials" ? "case-materials"
        : route.view === "finish" ? "case-finish"
        : "case-report";
    case "consult":
      return "consult";
    case "style":
      return "style";
    case "clinic":
      return route.patientId !== undefined ? "clinic-patient" : `clinic-${route.tab ?? "today"}`;
    case "settings":
      return `settings-${route.pane ?? "general"}`;
  }
}

/** The help of the window on top, if it has one (`data-help` on its frame). */
function helpOfTopWindow(): HelpId | undefined {
  const frames = Array.from(document.querySelectorAll<HTMLElement>('[role="dialog"][aria-modal="true"]'))
    .filter((f) => !f.hasAttribute("data-help-panel"));
  // The innermost and last one is on top.
  for (let i = frames.length - 1; i >= 0; i--) {
    const f = frames[i];
    if (!f || frames.some((o) => o !== f && f.contains(o))) continue;
    const id = f.dataset.help as HelpId | undefined;
    if (id) return id;
    // A window without help of its own (a small question): the page's help, below it.
    return undefined;
  }
  return undefined;
}

/** What "?" or F1 opens now: the window on top, else the part of the page, else the page. */
export function currentHelp(route: Route | undefined): HelpId {
  return helpOfTopWindow() ?? shown[shown.length - 1]?.id ?? helpOfRoute(route);
}

/** Opens the help from anywhere (a window's own "?"); set by the app while the vault is open. */
let opener: ((id?: HelpId) => void) | null = null;
export function setHelpOpener(fn: ((id?: HelpId) => void) | null): void {
  opener = fn;
}
export function openHelp(id?: HelpId): void {
  opener?.(id);
}
export function canOpenHelp(): boolean {
  return opener !== null;
}
