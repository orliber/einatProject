// The two parts of the program (D-191): writing reports and running the clinic. Each has its
// own row of pages under the top bar; the settings belong to the whole program and sit in
// neither. The part she worked in last is where the program opens next time (one word in
// the window's storage, never anything about a case or a patient).
import type { Route } from "./App";

export type Area = "reports" | "clinic";

export const AREA_LABEL: Record<Area, string> = {
  reports: "כתיבת הדוחות",
  clinic: "ניהול הקליניקה",
};

/** The pages of "כתיבת הדוחות", in the order of the row. */
export const REPORT_PAGES: { name: "cases" | "consult" | "style"; label: string }[] = [
  { name: "cases", label: "תיקים" },
  { name: "consult", label: "התייעצות" },
  { name: "style", label: "הסגנון שלי" },
];

/** Which part a page belongs to; the settings belong to neither. */
export function areaOf(name: Route["name"]): Area | null {
  switch (name) {
    case "cases":
    case "case":
    case "consult":
    case "style":
      return "reports";
    case "clinic":
      return "clinic";
    case "settings":
      return null;
  }
}

/** The first page of a part. */
export function homeOf(area: Area): Route {
  return area === "clinic" ? { name: "clinic" } : { name: "cases" };
}

const KEY = "dv.area";

/** The part she worked in last; "כתיבת הדוחות" the first time. */
export function lastArea(): Area {
  try {
    return localStorage.getItem(KEY) === "clinic" ? "clinic" : "reports";
  } catch {
    return "reports";
  }
}

export function rememberArea(area: Area): void {
  try {
    if (area === "reports") localStorage.removeItem(KEY);
    else localStorage.setItem(KEY, area);
  } catch {
    // Storage refused: the program opens on the reports, as the first time.
  }
}
