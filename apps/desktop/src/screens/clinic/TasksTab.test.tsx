// The follow-ups tab over the demo core: what needs her by kind, her own follow-ups by when
// they are due, adding in one line (with the suggestions), ticking with a rollback, editing,
// deleting, the folded list of what was done, the empty clinic, discreet mode and
// accessibility. Every name is invented; the clock is held at a Sunday morning.
import { configure, screen, waitFor, within } from "@testing-library/react";
import type { Alert } from "../../ipc/generated/Alert";
import { a11yViolations } from "../../test/a11y";
import { forgetDiscreet } from "./ClinicScreen";
import { ask, holding, idOf, NOW, openOn, refusing } from "./officeTestKit";
import { addMonths, alertGroups, groupTasks, SUGGESTIONS, taskGroupOf } from "./TasksTab";

configure({ asyncUtilTimeout: 5000 });
vi.setConfig({ testTimeout: 30_000 });

beforeEach(() => {
  vi.useFakeTimers({ toFake: ["Date"] });
  vi.setSystemTime(new Date(2026, 10, 15, 8, 0));
});

afterEach(() => {
  vi.useRealTimers();
  forgetDiscreet();
  window.history.replaceState({}, "", "/");
});

const TASKS = { name: "clinic", tab: "tasks" } as const;
const mine = () => screen.getByRole("region", { name: "המעקבים שלי" });
const needs = () => screen.getByRole("region", { name: "מה דורש אותי" });
/** A group of open follow-ups, by its name ("היום: 1"). */
const group = (label: string) => within(mine()).getByRole("region", { name: new RegExp(`^${label}: \\d+$`) });
const ready = () => within(mine()).findByLabelText("לחזור לרופאת הילדים");
const lineOf = (title: string) => within(mine()).getByLabelText(title).closest("li") as HTMLElement;

describe("the pure pieces", () => {
  it("a follow-up belongs to late, today, this week (Sunday to Saturday), later or undated", () => {
    expect(taskGroupOf(null, "2026-11-15")).toBe("undated");
    expect(taskGroupOf("2026-11-14", "2026-11-15")).toBe("late");
    expect(taskGroupOf("2026-11-15", "2026-11-15")).toBe("today");
    expect(taskGroupOf("2026-11-16", "2026-11-15")).toBe("week");
    expect(taskGroupOf("2026-11-21", "2026-11-15")).toBe("week");
    expect(taskGroupOf("2026-11-22", "2026-11-15")).toBe("later");
    // On a Saturday the week has nothing left: tomorrow is next week.
    expect(taskGroupOf("2026-11-22", "2026-11-21")).toBe("later");
  });

  it("groups keep the clinic's order, and done follow-ups stand apart", () => {
    const view = (id: string, due: string | null, doneOn: string | null = null) => ({
      task: { id, patient_id: null, kind: "other" as const, title: id, due, done_on: doneOn, note: "" }, patient_name: null, overdue: false,
    });
    const out = groupTasks([view("a", "2026-11-01"), view("b", "2026-11-15"), view("c", "2026-12-01"), view("d", "2026-12-05"), view("e", null), view("f", "2026-11-01", "2026-11-10")], "2026-11-15");
    expect(out.open.map((g) => [g.id, g.tasks.map((t) => t.task.id)])).toEqual([["late", ["a"]], ["today", ["b"]], ["later", ["c", "d"]], ["undated", ["e"]]]);
    expect(out.done.map((t) => t.task.id)).toEqual(["f"]);
    expect(out.openCount).toBe(5);
  });

  it("half a year later is the same day, or the month's last", () => {
    expect(addMonths("2026-11-15", 6)).toBe("2027-05-15");
    expect(addMonths("2026-08-31", 6)).toBe("2027-02-28");
    expect(addMonths("2027-08-31", 6)).toBe("2028-02-29");
    expect(addMonths("2026-01-10", 1)).toBe("2026-02-10");
  });

  it("alerts are grouped by kind, each group oldest first as given", () => {
    const alert = (kind: Alert["kind"], since: string): Alert => ({ kind, patient_id: null, patient_name: null, visit_id: null, task_id: null, text: "", since });
    const groups = alertGroups([alert("debt", "2026-10-01"), alert("no_note", "2026-10-05"), alert("debt", "2026-10-09"), alert("unmarked", "2026-11-01")]);
    expect(groups.map((g) => [g.kind, g.alerts.map((a) => a.since)])).toEqual([["unmarked", ["2026-11-01"]], ["no_note", ["2026-10-05"]], ["debt", ["2026-10-01", "2026-10-09"]]]);
  });

  it("offers the six follow-ups of an assessment clinic", () => {
    expect(SUGGESTIONS.map((s) => s.title)).toEqual(["לשלוח שאלון להורים", "לקבל שאלון מהגן", "לתאם תצפית בגן", "לכתוב את הדוח", "לתאם שיחת משוב", "מעקב אחרי חצי שנה"]);
  });
});

describe("the follow-ups tab", () => {
  it("shows what needs her in the whole clinic, by kind with a count", async () => {
    const served = await openOn(TASKS);
    await ready();
    const alerts = (await ask(served, { op: "overview", today: NOW }, "overview")).alerts;
    expect(alerts.length).toBeGreaterThan(3);
    // The number beside the heading is the number of alerts; each kind says how many it has.
    expect(within(needs()).getAllByText(String(alerts.length)).length).toBeGreaterThan(0);
    for (const g of alertGroups(alerts)) {
      const label = { unmarked: "פגישה שלא סומנה", no_note: "פגישה בלי סיכום", debt: "חוב פתוח", waiting: "המתנה ארוכה", report_due: "דוח מתעכב", no_next_visit: "אין פגישה הבאה", task_overdue: "משימה שמועדה עבר", feedback_soon: "שיחת משוב קרובה", materials_missing: "חסרים שאלונים או מסמכים", unreadable: "רישום שלא נקרא" }[g.kind];
      const box = within(needs()).getByRole("region", { name: `${label}: ${g.alerts.length}` });
      expect(within(box).getByRole("heading", { name: label })).toBeInTheDocument();
      expect(within(box).getAllByRole("listitem").length).toBe(Math.min(4, g.alerts.length));
    }
    // The long wait of עומר, with the way to his card.
    const waiting = within(needs()).getByRole("region", { name: "המתנה ארוכה: 1" });
    expect(within(waiting).getByRole("button", { name: "עומר" })).toBeInTheDocument();
    expect(waiting).toHaveTextContent("ברשימת ההמתנה כבר 8 שבועות");
    expect(await a11yViolations(served.view.container)).toEqual([]);
  });

  it("an alert's name opens the card", async () => {
    const { user, went } = await openOn(TASKS);
    await ready();
    await user.click(within(within(needs()).getByRole("region", { name: "המתנה ארוכה: 1" })).getByRole("button", { name: "עומר" }));
    expect(went[went.length - 1]).toMatchObject({ name: "clinic", tab: "patients" });
  });

  it("shows her follow-ups by when they are due, the late ones first", async () => {
    await openOn(TASKS);
    await ready();
    expect(within(mine()).getAllByRole("heading", { level: 3 }).map((h) => h.textContent)).toEqual(["באיחור1", "היום1", "השבוע1", "בלי תאריך1"]);
    expect(mine()).toHaveTextContent("4 מעקבים פתוחים");
    const late = group("באיחור");
    expect(within(late).getByLabelText("להזכיר להורים את יתרת התשלום")).not.toBeChecked();
    expect(late).toHaveTextContent("עד 12.11.2026, עבר");
    expect(within(late).getByRole("button", { name: "יובל" })).toBeInTheDocument();
    expect(within(late).getByText("תשלום")).toBeInTheDocument();
    expect(within(group("היום")).getByLabelText("לחזור לרופאת הילדים")).toBeInTheDocument();
    expect(within(group("השבוע")).getByLabelText("לשלוח שאלון לגננת")).toBeInTheDocument();
    expect(within(group("בלי תאריך")).getByLabelText("להזמין טפסי שאלונים")).toBeInTheDocument();
    // What was done is folded away until she asks.
    const toggle = within(mine()).getByRole("button", { name: "הושלמו לאחרונה (1)" });
    expect(toggle).toHaveAttribute("aria-expanded", "false");
    expect(within(mine()).queryByLabelText("לבקש מההורים את סיכום קלינאית התקשורת")).toBeNull();
  });

  it("a suggestion fills the line in one click, and the follow-up is saved for the child she chose", async () => {
    const { user, requests, notes, view } = await openOn(TASKS);
    await ready();
    const form = within(mine()).getByRole("form", { name: "מעקב חדש" });
    await user.click(within(form).getByRole("button", { name: "לקבל שאלון מהגן" }));
    expect(within(form).getByLabelText("מעקב חדש")).toHaveValue("לקבל שאלון מהגן");
    expect(within(form).getByLabelText("מעקב חדש")).toHaveFocus();
    expect(within(form).getByLabelText("סוג")).toHaveValue("questionnaire");
    expect(within(form).getByLabelText("עד מתי (לא חובה)")).toHaveValue("");

    await user.click(within(form).getByRole("button", { name: "לשייך לילד או לילדה (לא חובה)" }));
    await user.type(within(form).getByLabelText("מטופל"), "תמ");
    await user.click(await within(form).findByRole("button", { name: "תמר" }));
    expect(await a11yViolations(view.container)).toEqual([]);
    await user.click(within(form).getByRole("button", { name: "הוספה" }));

    await waitFor(() => expect(notes).toContain("המעקב נוסף"));
    const sent = requests().find((r) => r.op === "save_task");
    expect(sent).toMatchObject({ task: { id: "", title: "לקבל שאלון מהגן", kind: "questionnaire", due: null, done_on: null, note: "" } });
    expect(sent?.op === "save_task" && sent.task.patient_id).toMatch(/^patient-/);
    // It is on the list, with her name, and the line is ready for the next one.
    const added = await within(mine()).findByLabelText("לקבל שאלון מהגן");
    expect(within(added.closest("li") as HTMLElement).getByRole("button", { name: "תמר" })).toBeInTheDocument();
    expect(within(form).getByLabelText("מעקב חדש")).toHaveValue("");
    expect(within(form).queryByLabelText("מטופל")).toBeNull();
  });

  it("“מעקב אחרי חצי שנה” comes with its date, and Enter adds it", async () => {
    const { user, requests } = await openOn(TASKS);
    await ready();
    const form = within(mine()).getByRole("form", { name: "מעקב חדש" });
    await user.click(within(form).getByRole("button", { name: "מעקב אחרי חצי שנה" }));
    expect(within(form).getByLabelText("עד מתי (לא חובה)")).toHaveValue("15.05.2027");
    await user.keyboard("{Enter}");
    await waitFor(() => expect(requests().find((r) => r.op === "save_task")).toMatchObject({ task: { title: "מעקב אחרי חצי שנה", kind: "follow_up", due: "2027-05-15", patient_id: null } }));
    expect(await within(mine()).findByRole("region", { name: "בהמשך: 1" })).toHaveTextContent("עד 15.05.2027");
  });

  it("a follow-up typed by hand takes a date, and an empty one is refused in her words", async () => {
    const { user, requests } = await openOn(TASKS);
    await ready();
    const form = within(mine()).getByRole("form", { name: "מעקב חדש" });
    await user.click(within(form).getByRole("button", { name: "הוספה" }));
    expect(within(form).getByRole("alert")).toHaveTextContent("צריך לכתוב מה המעקב.");
    expect(requests().some((r) => r.op === "save_task")).toBe(false);

    await user.type(within(form).getByLabelText("מעקב חדש"), "לדבר עם היועצת");
    await user.selectOptions(within(form).getByLabelText("סוג"), "שיחת טלפון");
    await user.type(within(form).getByLabelText("עד מתי (לא חובה)"), "18.11.2026");
    await user.click(within(form).getByRole("button", { name: "הוספה" }));
    await waitFor(() => expect(requests().find((r) => r.op === "save_task")).toMatchObject({ task: { title: "לדבר עם היועצת", kind: "call", due: "2026-11-18" } }));
    expect(await within(group("השבוע")).findByLabelText("לדבר עם היועצת")).toBeInTheDocument();
    expect(within(form).queryByRole("alert")).toBeNull();
  });

  it("the vault's refusal of a new follow-up is shown on the line, and what she typed stays", async () => {
    const { user } = await openOn(TASKS, refusing("save_task", "המעקב לא נשמר, כי הכספת עסוקה."));
    await ready();
    const form = within(mine()).getByRole("form", { name: "מעקב חדש" });
    await user.type(within(form).getByLabelText("מעקב חדש"), "לדבר עם היועצת");
    await user.click(within(form).getByRole("button", { name: "הוספה" }));
    expect(await within(form).findByRole("alert")).toHaveTextContent("המעקב לא נשמר, כי הכספת עסוקה.");
    expect(within(form).getByLabelText("מעקב חדש")).toHaveValue("לדבר עם היועצת");
  });

  it("ticking a follow-up moves it to what was done at once; unticking there opens it again", async () => {
    const { user, requests } = await openOn(TASKS);
    await user.click(await ready());
    // At once: off the open list, and counted with what was done.
    expect(within(mine()).queryByLabelText("לחזור לרופאת הילדים")).toBeNull();
    expect(within(mine()).getByRole("button", { name: "הושלמו לאחרונה (2)" })).toBeInTheDocument();
    await waitFor(() => expect(requests().some((r) => r.op === "save_task" && r.task.title === "לחזור לרופאת הילדים" && r.task.done_on === "2026-11-15")).toBe(true));
    await waitFor(() => expect(mine()).toHaveTextContent("3 מעקבים פתוחים"));

    await user.click(within(mine()).getByRole("button", { name: "הושלמו לאחרונה (2)" }));
    const done = within(mine()).getByRole("list", { name: "מעקבים שהושלמו" });
    const box = within(done).getByLabelText("לחזור לרופאת הילדים");
    expect(box).toBeChecked();
    expect(box.closest("li")).toHaveTextContent("בוצע ביום 15.11.2026");
    await user.click(box);
    await waitFor(() => expect(requests().some((r) => r.op === "save_task" && r.task.title === "לחזור לרופאת הילדים" && r.task.done_on === null)).toBe(true));
    expect(await within(group("היום")).findByLabelText("לחזור לרופאת הילדים")).not.toBeChecked();
  });

  it("a tick the vault refuses rolls back, and says why", async () => {
    const { user, notes } = await openOn(TASKS, refusing("save_task", "המשימה לא נשמרה."));
    await user.click(await ready());
    await waitFor(() => expect(notes).toContain("המשימה לא נשמרה."));
    expect(await within(group("היום")).findByLabelText("לחזור לרופאת הילדים")).not.toBeChecked();
    expect(within(mine()).getByRole("button", { name: "הושלמו לאחרונה (1)" })).toBeInTheDocument();
  });

  it("“בוצע” on the alert of an overdue follow-up closes it", async () => {
    const { user, requests } = await openOn(TASKS);
    await ready();
    const overdue = within(needs()).getByRole("region", { name: "משימה שמועדה עבר: 1" });
    await user.click(within(overdue).getByRole("button", { name: "בוצע" }));
    await waitFor(() => expect(requests().some((r) => r.op === "save_task" && r.task.title === "להזכיר להורים את יתרת התשלום" && r.task.done_on === "2026-11-15")).toBe(true));
    await waitFor(() => expect(within(needs()).queryByRole("region", { name: /^משימה שמועדה עבר/ })).toBeNull());
    expect(within(mine()).queryByRole("region", { name: /^באיחור/ })).toBeNull();
  });

  it("a follow-up is edited in a dialog, which keeps unsaved text from an accidental close", async () => {
    const { user, requests, notes } = await openOn(TASKS);
    await ready();
    await user.click(within(lineOf("להזמין טפסי שאלונים")).getByRole("button", { name: "עריכת המעקב: להזמין טפסי שאלונים" }));
    const dialog = await screen.findByRole("dialog", { name: "עריכת מעקב" });
    expect(await a11yViolations(dialog)).toEqual([]);
    expect(within(dialog).getByRole("button", { name: "שמירה" })).toBeDisabled();
    const title = within(dialog).getByLabelText("מה המעקב");
    await user.clear(title);
    await user.type(title, "להזמין טפסים");
    await user.keyboard("{Escape}");
    expect(within(dialog).getByRole("alertdialog", { name: "לסגור בלי לשמור" })).toBeInTheDocument();
    await user.click(within(dialog).getByRole("button", { name: "להמשיך לערוך" }));
    await user.type(within(dialog).getByLabelText("עד מתי (לא חובה)"), "25.11.2026");
    await user.type(within(dialog).getByLabelText("הערה (לא חובה)"), "אצל הספק הקבוע");
    await user.click(within(dialog).getByRole("button", { name: "שמירה" }));
    await waitFor(() => expect(screen.queryByRole("dialog")).toBeNull());
    expect(notes).toContain("המעקב נשמר");
    const sent = requests().find((r) => r.op === "save_task");
    expect(sent).toMatchObject({ task: { title: "להזמין טפסים", due: "2026-11-25", note: "אצל הספק הקבוע", kind: "other", done_on: null } });
    expect(sent?.op === "save_task" && sent.task.id).toMatch(/^task-/);
    const later = await within(mine()).findByRole("region", { name: "בהמשך: 1" });
    expect(later).toHaveTextContent("אצל הספק הקבוע");
  });

  it("a follow-up is deleted only after she confirms, and a refusal stays in the dialog", async () => {
    const { user, requests, notes } = await openOn(TASKS);
    await ready();
    await user.click(within(lineOf("להזמין טפסי שאלונים")).getByRole("button", { name: "מחיקת המעקב: להזמין טפסי שאלונים" }));
    const confirm = await screen.findByRole("dialog", { name: "מחיקת המעקב" });
    expect(confirm).toHaveTextContent("המעקב \"להזמין טפסי שאלונים\" יימחק מהרשימה.");
    expect(requests().some((r) => r.op === "delete_task")).toBe(false);
    await user.click(within(confirm).getByRole("button", { name: "מחיקה" }));
    await waitFor(() => expect(screen.queryByRole("dialog")).toBeNull());
    expect(notes).toContain("המעקב נמחק");
    await waitFor(() => expect(within(mine()).queryByLabelText("להזמין טפסי שאלונים")).toBeNull());
  });

  it("a delete the vault refuses says so in the dialog, and the follow-up stays", async () => {
    const { user } = await openOn(TASKS, refusing("delete_task", "המשימה לא נמחקה."));
    await ready();
    await user.click(within(lineOf("להזמין טפסי שאלונים")).getByRole("button", { name: /^מחיקת המעקב/ }));
    const confirm = await screen.findByRole("dialog", { name: "מחיקת המעקב" });
    await user.click(within(confirm).getByRole("button", { name: "מחיקה" }));
    expect(await within(confirm).findByRole("alert")).toHaveTextContent("המשימה לא נמחקה.");
    await user.click(within(confirm).getByRole("button", { name: "ביטול" }));
    expect(within(mine()).getByLabelText("להזמין טפסי שאלונים")).toBeInTheDocument();
  });

  it("a long group comes in pages", async () => {
    const served = await openOn(TASKS, undefined, async (s) => {
      for (let i = 1; i <= 23; i++) {
        await ask(s, { op: "save_task", task: { id: "", patient_id: null, kind: "other", title: `מעקב מספר ${String(i).padStart(2, "0")}`, due: null, done_on: null, note: "" } }, "task");
      }
    });
    await ready();
    const undated = group("בלי תאריך");
    expect(undated).toHaveAccessibleName("בלי תאריך: 24");
    expect(within(undated).getAllByRole("checkbox")).toHaveLength(20);
    const pager = within(undated).getByRole("navigation", { name: "עמודים של בלי תאריך" });
    expect(pager).toHaveTextContent("עמוד 1 מתוך 2");
    await served.user.click(within(pager).getByRole("button", { name: "הבא" }));
    expect(within(undated).getAllByRole("checkbox")).toHaveLength(4);
  });

  it("a clinic with nothing yet says what the list is for, and nothing needs her", async () => {
    window.history.replaceState({}, "", "/?clinic=empty");
    const { view } = await openOn(TASKS);
    expect(await screen.findByRole("heading", { name: "עוד אין מעקבים" })).toBeInTheDocument();
    expect(await within(needs()).findByText("אין כרגע שום דבר שמחכה לטיפול בקליניקה.")).toBeInTheDocument();
    expect(within(mine()).getByRole("form", { name: "מעקב חדש" })).toBeInTheDocument();
    expect(await a11yViolations(view.container)).toEqual([]);
  });

  it("shows the shape of the lists while they load", async () => {
    const { over, release } = holding("tasks");
    await openOn(TASKS, over);
    expect(await screen.findByRole("status", { name: "טוען את המעקבים" })).toBeInTheDocument();
    release();
    await ready();
    expect(screen.queryByRole("status", { name: "טוען את המעקבים" })).toBeNull();
  });

  it("a list that could not be read says so", async () => {
    await openOn(TASKS, refusing("tasks", "המעקבים לא נקראו מהכספת."));
    expect(await within(mine()).findByRole("alert")).toHaveTextContent("המעקבים לא נקראו מהכספת.");
  });

  it("in discreet mode every name is initials, in the lists and in the alerts", async () => {
    const served = await openOn(TASKS);
    await ready();
    const yuval = await idOf(served, "יובל");
    expect(yuval).toMatch(/^patient-/);
    await served.user.click(screen.getByRole("switch", { name: "מצב דיסקרטי" }));
    const panel = screen.getByRole("tabpanel");
    for (const name of ["תמר", "איתי", "מאיה", "נועם", "יובל", "עומר", "שירה"]) expect(within(panel).queryByText(name), name).toBeNull();
    expect(within(group("באיחור")).getByRole("button", { name: "י." })).toBeInTheDocument();
    expect(within(needs()).getAllByRole("button", { name: "ע." }).length).toBeGreaterThan(0);
  });
});
