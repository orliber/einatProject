// Two aids for the keyboard and for a screen reader, on every page of the open program
// (WCAG 2.4.1 and 2.4.2): a link that skips the bars and lands on the page's content, and a
// window title that says which page this is. The title never holds a name from the vault:
// a window's title shows in the taskbar and in the list of open windows.
import { useEffect } from "react";
import type { Route } from "../App";
import { CLINIC_TABS } from "../screens/clinic/tabs";
import { he } from "../i18n/he";
import "./PageAids.css";

const CASE_VIEWS: Record<string, string> = {
  materials: "חומרים",
  details: "פרטי התיק",
  report: "כתיבת הדוח",
  finish: "סיום ואישור",
};

/** The page's name for the window title: the page, never whose it is. */
export function pageName(route: Route): string {
  switch (route.name) {
    case "cases": return "תיקים";
    case "case": return `תיק: ${CASE_VIEWS[route.view] ?? "עבודה על הדוח"}`;
    case "consult": return "התייעצות";
    case "style": return "סגנון הכתיבה";
    case "settings": return "הגדרות";
    case "clinic": {
      if (route.patientId !== undefined) return "ניהול הקליניקה: כרטיס מטופל";
      const tab = CLINIC_TABS.find((t) => t.id === (route.tab ?? "today"));
      return `ניהול הקליניקה: ${tab?.label ?? "היום"}`;
    }
  }
}

/** The window's title follows the page, and returns to the program's name when it closes. */
export function usePageTitle(route: Route): void {
  const name = pageName(route);
  useEffect(() => {
    document.title = `${name} · ${he.appName}`;
    return () => { document.title = he.appName; };
  }, [name]);
}

/** The page's content: its `main`, or the part that stands for it (the work area of a case,
 *  the conversation of the consultation, which is a named region of its own). */
function content(): HTMLElement | null {
  return document.querySelector<HTMLElement>("[data-page-content]") ?? document.querySelector<HTMLElement>("main, [role=main]");
}

/** The first stop of the keyboard: invisible until it has the focus, then a button at the top. */
export function SkipLink() {
  const skip = () => {
    const to = content();
    if (!to) return;
    // A place that is not a stop of its own takes the focus once, for this jump only.
    if (!to.hasAttribute("tabindex")) to.setAttribute("tabindex", "-1");
    to.focus();
    to.scrollIntoView?.({ block: "start" });
  };
  return <button type="button" className="skip-link" onClick={skip}>דילוג לתוכן העמוד</button>;
}
