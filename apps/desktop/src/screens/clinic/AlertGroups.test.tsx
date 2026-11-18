// "מה דורש אותי" by topic (review item 4 of 18.11.2026, D-204): one row per kind that opens and
// folds, the first one open, the cap of four lines, the step "התקיימה" for a visit that was
// not marked, and where the focus goes when the line she dealt with goes (WCAG 2.4.3). Every
// name and sentence is invented.
import { act, configure, render, screen, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import type { Alert } from "../../ipc/generated/Alert";
import { a11yViolations } from "../../test/a11y";
import { appApi, Host } from "../../test/host";
import { ALERT_ORDER, AlertGroups, ClinicNavContext } from "./parts";

configure({ asyncUtilTimeout: 5000 });

function alert(kind: Alert["kind"], n: number, over: Partial<Alert> = {}): Alert {
  return { kind, patient_id: `p-${kind}-${n}`, patient_name: `ילד ${n} לדוגמה`, visit_id: null, task_id: null, text: `משפט בדוי ${kind} ${n}`, since: `2026-10-${String(n + 10)}`, ...over };
}

const UNMARKED_1 = alert("unmarked", 1, { visit_id: "v-1" });
const UNMARKED_2 = alert("unmarked", 2, { visit_id: "v-2" });
const NO_NOTE = alert("no_note", 3, { visit_id: "v-3" });
const DEBTS = [1, 2, 3, 4, 5].map((n) => alert("debt", n));
const SOON = alert("feedback_soon", 6, { visit_id: "v-6" });
const WAITING = alert("waiting", 7);
// Given in the clinic's own order, not by kind.
const ALL = [WAITING, ...DEBTS, SOON, NO_NOTE, UNMARKED_1, UNMARKED_2];

function show(alerts: Alert[], holdOf?: (visitId: string) => (() => void) | null) {
  const app = appApi();
  const onVisit = vi.fn();
  const user = userEvent.setup();
  const nav = { openPatient: vi.fn(), openTab: vi.fn() };
  const tree = (list: Alert[]) => (
    <Host app={app}>
      <ClinicNavContext.Provider value={nav}>
        <section aria-labelledby="needs-title">
          <h2 id="needs-title">מה דורש אותי</h2>
          <AlertGroups alerts={list} onVisit={onVisit} {...(holdOf ? { holdOf } : {})} empty="אין כרגע שום דבר שמחכה." />
        </section>
        <button type="button">מחוץ לרשימה</button>
      </ClinicNavContext.Provider>
    </Host>
  );
  const view = render(tree(alerts));
  return { user, view, onVisit, rerender: (list: Alert[]) => view.rerender(tree(list)) };
}

const toggles = () => screen.getAllByRole("heading", { level: 3 }).map((h) => within(h).getByRole("button"));
const toggle = (name: string) => screen.getByRole("button", { name });
const body = (name: string) => document.getElementById(toggle(name).getAttribute("aria-controls") ?? "") as HTMLElement;

describe("“מה דורש אותי” by topic", () => {
  it("shows the kinds in the order she deals with them, each toggle named by its label and count", () => {
    show(ALL);
    expect(toggles()).toEqual(["פגישה שלא סומנה: 2", "פגישה בלי סיכום: 1", "שיחת משוב קרובה: 1", "חוב פתוח: 5", "המתנה ארוכה: 1"].map(toggle));
    // That is ALERT_ORDER, whatever order the clinic gave them in.
    const order = ["unmarked", "no_note", "feedback_soon", "debt", "waiting"];
    expect(order).toEqual(ALERT_ORDER.filter((k) => order.includes(k)));
    // The name is the words, not an aria-label over them.
    for (const t of toggles()) expect(t).not.toHaveAttribute("aria-label");
    // The count is for the eye only; a screen reader hears it in the name.
    for (const t of toggles()) expect(t.querySelector(".clinic-need-count")).toHaveAttribute("aria-hidden", "true");
    // A heads-up is teal, something late is orange.
    expect(toggle("שיחת משוב קרובה: 1").querySelector(".clinic-need-count")).not.toHaveClass("chip-warn");
    expect(toggle("חוב פתוח: 5").querySelector(".clinic-need-count")).toHaveClass("chip-warn");
  });

  it("the first toggle is open and the others are folded", () => {
    show(ALL);
    expect(toggles().map((t) => t.getAttribute("aria-expanded"))).toEqual(["true", "false", "false", "false", "false"]);
    expect(within(body("פגישה שלא סומנה: 2")).getAllByRole("listitem")).toHaveLength(2);
    expect(body("חוב פתוח: 5")).not.toBeVisible();
    expect(body("חוב פתוח: 5")).toBeEmptyDOMElement();
  });

  it("a click opens a group, and another folds it again", async () => {
    const { user } = show(ALL);
    await user.click(toggle("חוב פתוח: 5"));
    expect(toggle("חוב פתוח: 5")).toHaveAttribute("aria-expanded", "true");
    expect(within(body("חוב פתוח: 5")).getAllByRole("listitem")).toHaveLength(4);
    // The first one stays open.
    expect(toggle("פגישה שלא סומנה: 2")).toHaveAttribute("aria-expanded", "true");
    await user.click(toggle("פגישה שלא סומנה: 2"));
    expect(toggle("פגישה שלא סומנה: 2")).toHaveAttribute("aria-expanded", "false");
    expect(body("פגישה שלא סומנה: 2")).not.toBeVisible();
  });

  it("over four lines, “להציג את כל ה-5” shows the rest", async () => {
    const { user } = show(ALL);
    await user.click(toggle("חוב פתוח: 5"));
    const debts = body("חוב פתוח: 5");
    const more = within(debts).getByRole("button", { name: "להציג את כל ה-5" });
    expect(more).toHaveAttribute("aria-expanded", "false");
    await user.click(more);
    expect(within(debts).getAllByRole("listitem")).toHaveLength(5);
    expect(more).toHaveTextContent("להציג פחות");
    expect(more).toHaveAttribute("aria-expanded", "true");
    // A group of four or fewer has no such button.
    expect(within(body("פגישה שלא סומנה: 2")).queryByRole("button", { name: /^להציג/ })).toBeNull();
  });

  it("a grouped line has no kind chip and no icon of its own, and its sentence is quiet", async () => {
    const { user } = show(ALL);
    await user.click(toggle("חוב פתוח: 5"));
    for (const name of ["פגישה שלא סומנה: 2", "חוב פתוח: 5"]) {
      const lines = within(body(name)).getAllByRole("listitem");
      for (const line of lines) {
        expect(line.querySelector(".chip")).toBeNull();
        expect(line.querySelector(".clinic-alert-icon")).toBeNull();
      }
    }
    expect(within(body("חוב פתוח: 5")).getByText("משפט בדוי debt 1")).toHaveClass("muted");
    expect(within(body("חוב פתוח: 5")).queryByText("חוב פתוח")).toBeNull();
  });

  it("holdOf turns the step of a visit that was not marked into “התקיימה”", async () => {
    const hold = vi.fn();
    const { user, onVisit } = show(ALL, (visitId) => (visitId === "v-1" ? hold : null));
    const [first, second] = within(body("פגישה שלא סומנה: 2")).getAllByRole("listitem") as [HTMLElement, HTMLElement];
    await user.click(within(first).getByRole("button", { name: "התקיימה" }));
    expect(hold).toHaveBeenCalledTimes(1);
    // Where it has none, the line still opens the visit.
    expect(within(second).queryByRole("button", { name: "התקיימה" })).toBeNull();
    await user.click(within(second).getByRole("button", { name: "לפתוח את הפגישה" }));
    expect(onVisit).toHaveBeenCalledWith(UNMARKED_2, "v-2");
    // A visit without a note keeps its own step.
    await user.click(toggle("פגישה בלי סיכום: 1"));
    expect(within(body("פגישה בלי סיכום: 1")).getByRole("button", { name: "לכתוב סיכום" })).toBeInTheDocument();
  });

  it("when the line she was on goes, the focus goes to its group, then to the group in its place, then to the empty line", async () => {
    const { user, rerender } = show(ALL, () => vi.fn());
    // She marks the first visit: its line goes, its kind stays.
    await user.click(within(body("פגישה שלא סומנה: 2")).getAllByRole("button", { name: "התקיימה" })[0] as HTMLElement);
    rerender(ALL.filter((a) => a !== UNMARKED_1));
    expect(toggle("פגישה שלא סומנה: 1")).toHaveFocus();
    // She marks the second: the whole kind goes, and the focus goes to the toggle now first.
    await user.click(within(body("פגישה שלא סומנה: 1")).getByRole("button", { name: "התקיימה" }));
    const rest = ALL.filter((a) => a.kind !== "unmarked");
    rerender(rest);
    expect(toggle("פגישה בלי סיכום: 1")).toHaveFocus();
    // The new first group opens by itself.
    expect(toggle("פגישה בלי סיכום: 1")).toHaveAttribute("aria-expanded", "true");
    // Everything goes: the focus goes to the empty line.
    await user.click(within(body("פגישה בלי סיכום: 1")).getByRole("button", { name: "לכתוב סיכום" }));
    rerender([]);
    expect(screen.getByText("אין כרגע שום דבר שמחכה.")).toHaveFocus();
  });

  it("a read that comes again never takes the focus from where she put it", async () => {
    const { user, rerender } = show(ALL, () => vi.fn());
    await user.click(within(body("פגישה שלא סומנה: 2")).getAllByRole("button", { name: "התקיימה" })[0] as HTMLElement);
    // She went on to something else; then the line goes.
    const outside = screen.getByRole("button", { name: "מחוץ לרשימה" });
    await user.click(outside);
    rerender(ALL.filter((a) => a !== UNMARKED_1));
    expect(outside).toHaveFocus();
    // She clicked on nothing (the focus is on the page); the minute's read gives the same lines.
    await user.click(within(body("פגישה שלא סומנה: 1")).getByRole("button", { name: "התקיימה" }));
    act(() => (document.activeElement as HTMLElement).blur());
    rerender([...ALL.filter((a) => a !== UNMARKED_1)]);
    expect(document.body).toHaveFocus();
  });

  it("passes the accessibility check, open and folded", async () => {
    const { user, view } = show(ALL, (visitId) => (visitId === "v-1" ? vi.fn() : null));
    expect(await a11yViolations(view.container)).toEqual([]);
    await user.click(toggle("חוב פתוח: 5"));
    await user.click(within(body("חוב פתוח: 5")).getByRole("button", { name: "להציג את כל ה-5" }));
    expect(await a11yViolations(view.container)).toEqual([]);
    for (const t of toggles()) if (t.getAttribute("aria-expanded") === "true") await user.click(t);
    expect(toggles().every((t) => t.getAttribute("aria-expanded") === "false")).toBe(true);
    expect(await a11yViolations(view.container)).toEqual([]);
  });
});
