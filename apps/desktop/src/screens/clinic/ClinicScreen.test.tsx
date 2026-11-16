// The clinic's page over the demo core: the tabs (click, keyboard, deep links), the front
// page with the demo clinic's visits, the quick actions, the three dialogs, discreet mode and
// the accessibility check. Every name is invented; the clock is held at a Sunday morning so
// "today" is the same day on every run.
import { configure, render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { useState } from "react";
import type { Route } from "../../App";
import type { ClinicRequest } from "../../ipc/generated/ClinicRequest";
import type { PatientCard } from "../../ipc/generated/PatientCard";
import type { PatientRow } from "../../ipc/generated/PatientRow";
import type { Visit } from "../../ipc/generated/Visit";
import { a11yViolations } from "../../test/a11y";
import { appApi, Host, serve, type Override } from "../../test/host";
import { ClinicScreen, forgetDiscreet } from "./ClinicScreen";
import { NoteDialog } from "./NoteDialog";

configure({ asyncUtilTimeout: 5000 });
vi.setConfig({ testTimeout: 30_000 });

beforeEach(() => {
  // Only the clock is held; timers run, so a debounce and a toast behave as they do for her.
  vi.useFakeTimers({ toFake: ["Date"] });
  vi.setSystemTime(new Date(2026, 10, 15, 8, 0));
});

afterEach(() => {
  vi.useRealTimers();
  forgetDiscreet();
  window.history.replaceState({}, "", "/");
});

/** The clinic as the app shows it: the route is the app's, and `go` changes it. */
function Clinic({ first, onGo, notify }: { first: Route; onGo: (r: Route) => void; notify: (text: string) => void }) {
  const [route, setRoute] = useState(first);
  const app = appApi({ route, notify, go: (r) => { onGo(r); setRoute(r); } });
  return (
    <Host app={app}>
      {route.name === "clinic" ? <ClinicScreen tab={route.tab} patientId={route.patientId} /> : <p>מסך אחר: {route.name}</p>}
    </Host>
  );
}

// The protected clipboard is the app's own; here it only says how long the copy stays.
const withClipboard: Override = (cmd) => (cmd === "copy_secret" ? 60 : undefined);

/** Opens the clinic; unless `ready` is off, waits until the front page has its data. */
async function openClinic(first: Route = { name: "clinic" }, over: Override = withClipboard, ready = true) {
  const served = await serve(over);
  const user = userEvent.setup();
  const went: Route[] = [];
  const notes: string[] = [];
  const view = render(<Clinic first={first} onGo={(r) => went.push(r)} notify={(text) => notes.push(text)} />);
  const requests = () => served.calls.filter((c) => c.cmd === "clinic").map((c) => c.args.request as ClinicRequest);
  if (ready && first.name === "clinic" && (first.tab ?? "today") === "today" && first.patientId === undefined) {
    await screen.findByRole("region", { name: "הפגישות של היום" });
  }
  return { ...served, user, view, went, notes, requests };
}

const tab = (name: string) => screen.getByRole("tab", { name });
const todays = () => screen.getByRole("region", { name: "הפגישות של היום" });
/** The row of a patient's visit on today's list. */
const rowOf = (name: string) => within(todays()).getByRole("button", { name }).closest("li") as HTMLElement;

describe("the clinic's page", () => {
  it("says it is a beta, shows the seven tabs, and passes the accessibility check", async () => {
    const { view } = await openClinic();
    expect(screen.getByRole("note")).toHaveTextContent("ניהול הקליניקה בגרסת בטא. הכל נשמר מוצפן בכספת שבמחשב, ושום דבר מכאן לא נשלח לשום מקום.");
    expect(screen.getAllByRole("tab").map((t) => t.textContent)).toEqual(["היום", "יומן", "מטופלים", "המתנה", "מעקבים", "כספים", "הגדרות"]);
    expect(tab("היום")).toHaveAttribute("aria-selected", "true");
    expect(screen.getByRole("tabpanel")).toHaveAccessibleName("היום");
    // The bar marks the clinic as the page she is on, with its beta badge.
    expect(screen.getByRole("button", { name: /קליניקה/ })).toHaveAttribute("aria-current", "page");
    expect(screen.getByRole("button", { name: /קליניקה/ })).toHaveTextContent("בטא");
    await within(todays()).findByRole("button", { name: "תמר" });
    expect(await a11yViolations(view.container)).toEqual([]);
  });

  it("switches tabs by click, and the address follows", async () => {
    const { user, went } = await openClinic();
    for (const [label, id] of [
      ["יומן", "calendar"], ["מטופלים", "patients"], ["המתנה", "waiting"],
      ["מעקבים", "tasks"], ["כספים", "money"], ["הגדרות", "settings"],
    ] as const) {
      await user.click(tab(label));
      expect(tab(label)).toHaveAttribute("aria-selected", "true");
      expect(screen.getByRole("tabpanel")).toHaveAccessibleName(label);
      expect(went[went.length - 1]).toEqual({ name: "clinic", tab: id });
    }
    await user.click(tab("היום"));
    expect(await within(todays()).findByRole("button", { name: "תמר" })).toBeInTheDocument();
  });

  it("walks the tabs with the arrow keys, right to left, and Home and End", async () => {
    const { user } = await openClinic();
    tab("היום").focus();
    // Only the tab she is on is a Tab stop; the arrows move between them.
    expect(screen.getAllByRole("tab").filter((t) => t.tabIndex === 0)).toEqual([tab("היום")]);
    await user.keyboard("{ArrowLeft}");
    expect(tab("יומן")).toHaveAttribute("aria-selected", "true");
    expect(tab("יומן")).toHaveFocus();
    await user.keyboard("{ArrowLeft}{ArrowLeft}");
    expect(tab("המתנה")).toHaveFocus();
    await user.keyboard("{ArrowRight}");
    expect(tab("מטופלים")).toHaveAttribute("aria-selected", "true");
    await user.keyboard("{End}");
    expect(tab("הגדרות")).toHaveFocus();
    // Past the last comes the first.
    await user.keyboard("{ArrowLeft}");
    expect(tab("היום")).toHaveAttribute("aria-selected", "true");
    await user.keyboard("{ArrowRight}");
    expect(tab("הגדרות")).toHaveAttribute("aria-selected", "true");
    await user.keyboard("{Home}");
    expect(tab("היום")).toHaveFocus();
  });

  it("opens on the tab and the patient the route names", async () => {
    const { user, went } = await openClinic({ name: "clinic", tab: "money" });
    expect(tab("כספים")).toHaveAttribute("aria-selected", "true");
    expect(await screen.findByText(/אינה רשומה ברשות המסים/)).toBeInTheDocument();
    // A patient's card is a page of the patients tab.
    await user.click(tab("היום"));
    await user.click(await within(todays()).findByRole("button", { name: "תמר" }));
    expect(went[went.length - 1]).toMatchObject({ name: "clinic", tab: "patients" });
    expect((went[went.length - 1] as { patientId?: string }).patientId).toMatch(/^patient-/);
    expect(tab("מטופלים")).toHaveAttribute("aria-selected", "true");
    await user.click(await screen.findByRole("button", { name: "חזרה לרשימת המטופלים" }));
    expect(went[went.length - 1]).toEqual({ name: "clinic", tab: "patients" });
  });
});

describe("the front page", () => {
  it("greets her with the date and shows today's visits by time", async () => {
    await openClinic();
    expect(await screen.findByRole("heading", { name: "בוקר טוב" })).toBeInTheDocument();
    expect(screen.getByText("יום ראשון, 15.11.2026")).toBeInTheDocument();
    const rows = within(todays()).getAllByRole("listitem");
    expect(rows.map((r) => within(r).getAllByRole("button")[0]?.textContent)).toEqual(["09:00–10:30", "11:30–12:30", "13:00–15:00", "16:00–16:50"]);
    expect(within(rowOf("תמר")).getByText("מפגש אבחון")).toBeInTheDocument();
    expect(within(rowOf("איתי")).getByText("פגישת היכרות")).toBeInTheDocument();
    expect(within(rowOf("מאיה")).getByText("פגישת טיפול")).toBeInTheDocument();
    // Her own time shows her title, and has nothing to mark.
    const own = rows[2] as HTMLElement;
    expect(own).toHaveTextContent("כתיבת דוחות");
    expect(within(own).queryByRole("button", { name: "התקיימה" })).toBeNull();
    // While today still has visits, the next day is not shown.
    expect(screen.queryByRole("heading", { name: /הפגישות הבאות/ })).toBeNull();
  });

  it("shows what needs her, the open tasks and the four figures", async () => {
    const { user, went } = await openClinic();
    const needs = await screen.findByRole("region", { name: "מה דורש אותי" });
    for (const label of ["פגישה בלי סיכום", "חוב פתוח", "המתנה ארוכה", "שיחת משוב קרובה", "משימה שמועדה עבר"]) {
      expect(within(needs).getAllByText(label).length, label).toBeGreaterThan(0);
    }
    const tasks = screen.getByRole("region", { name: "משימות פתוחות" });
    expect(within(tasks).getByLabelText("לחזור לרופאת הילדים")).not.toBeChecked();
    const figures = within(screen.getByRole("list", { name: "הקליניקה במספרים" }));
    expect(figures.getByRole("button", { name: /פגישות השבוע/ })).toHaveTextContent("5");
    expect(figures.getByRole("button", { name: /ברשימת ההמתנה/ })).toHaveTextContent("2");
    expect(figures.getByRole("button", { name: /יתרות פתוחות/ })).toHaveTextContent("₪");
    await user.click(figures.getByRole("button", { name: /ברשימת ההמתנה/ }));
    expect(went[went.length - 1]).toEqual({ name: "clinic", tab: "waiting" });
  });

  it("when today is over, the next day with visits follows", async () => {
    vi.setSystemTime(new Date(2026, 10, 15, 19, 0));
    await openClinic();
    const next = await screen.findByRole("region", { name: "הפגישות הבאות: יום שני, 16.11.2026" });
    expect(within(next).getByRole("button", { name: "תמר" })).toBeInTheDocument();
    expect(within(next).getByText("תצפית")).toBeInTheDocument();
    expect(screen.getByRole("heading", { name: "ערב טוב" })).toBeInTheDocument();
    // A visit still ahead has not happened: there is nothing to mark yet, as on the card.
    const ahead = within(next).getByRole("button", { name: "תמר" }).closest("li") as HTMLElement;
    expect(within(ahead).queryByRole("button", { name: "התקיימה" })).toBeNull();
    expect(within(ahead).getByRole("button", { name: "תזכורת" })).toBeInTheDocument();
  });

  it("marking a visit “התקיימה” updates its row at once, and the vault has it", async () => {
    const { user, requests } = await openClinic();
    await within(todays()).findByRole("button", { name: "תמר" });
    expect(within(rowOf("תמר")).getByText("מתוכננת")).toBeInTheDocument();
    await user.click(within(rowOf("תמר")).getByRole("button", { name: "התקיימה" }));
    expect(within(rowOf("תמר")).getByText("התקיימה")).toBeInTheDocument();
    expect(within(rowOf("תמר")).queryByText("מתוכננת")).toBeNull();
    await waitFor(() => expect(requests().some((r) => r.op === "save_visit" && r.visit.status === "held" && r.scope === "one")).toBe(true));
    // After the vault answered, the row still says so, and the quick action is gone.
    await waitFor(() => expect(within(rowOf("תמר")).queryByRole("button", { name: "התקיימה" })).toBeNull());
    expect(within(rowOf("תמר")).getByText("התקיימה")).toBeInTheDocument();
    // The other visits are as they were.
    expect(within(rowOf("איתי")).getByText("מתוכננת")).toBeInTheDocument();
  });

  it("a mark the vault refuses rolls back, and says why", async () => {
    const refuse: Override = (cmd, args) => {
      if (cmd === "copy_secret") return 60;
      if (cmd === "clinic" && (args.request as ClinicRequest).op === "save_visit") throw { code: "refused", message: "הפגישה לא נשמרה.", details: [], action: { kind: "dismiss" } };
      return undefined;
    };
    const { user, notes } = await openClinic({ name: "clinic" }, refuse);
    await within(todays()).findByRole("button", { name: "תמר" });
    await user.click(within(rowOf("תמר")).getByRole("button", { name: "התקיימה" }));
    await waitFor(() => expect(notes).toContain("הפגישה לא נשמרה."));
    await waitFor(() => expect(within(rowOf("תמר")).getByText("מתוכננת")).toBeInTheDocument());
    expect(within(rowOf("תמר")).getByRole("button", { name: "התקיימה" })).toBeInTheDocument();
  });

  it("ticking a task shows it done at once and saves it as done today", async () => {
    const { user, requests } = await openClinic();
    const tasks = await screen.findByRole("region", { name: "משימות פתוחות" });
    await user.click(within(tasks).getByLabelText("לחזור לרופאת הילדים"));
    await waitFor(() => expect(requests().some((r) => r.op === "save_task" && r.task.title === "לחזור לרופאת הילדים" && r.task.done_on === "2026-11-15")).toBe(true));
    // The next read takes it off the list of what is open.
    await waitFor(() => expect(within(tasks).queryByLabelText("לחזור לרופאת הילדים")).toBeNull());
    expect(within(tasks).getByLabelText("לשלוח שאלון לגננת")).not.toBeChecked();
  });

  it("“תזכורת” fills the message and copies it through the protected clipboard only", async () => {
    const { user, calls, notes } = await openClinic();
    await within(todays()).findByRole("button", { name: "תמר" });
    await user.click(within(rowOf("תמר")).getByRole("button", { name: "תזכורת" }));
    await waitFor(() => expect(notes).toHaveLength(1));
    expect(notes[0]).toBe("התזכורת הועתקה ללוח המוגן, ואפשר להדביק אותה בהודעה. יימחק מהלוח בעוד דקה.");
    const copied = calls.filter((c) => c.cmd === "copy_secret");
    expect(copied).toHaveLength(1);
    expect(copied[0]?.args).toEqual({ text: "שלום יעל, תזכורת לפגישה ביום ראשון, 15.11.2026, בשעה 09:00. להתראות, ד\"ר רותם בדויה" });
    // The default text leaves the child's name out; the toast never carries the message.
    expect(String(copied[0]?.args.text)).not.toContain("תמר");
  });

  it("a copy that fails is said, and nothing is claimed", async () => {
    const broken: Override = (cmd) => {
      if (cmd === "copy_secret") throw { code: "internal", message: "ההעתקה לא הצליחה.", details: [], action: { kind: "dismiss" } };
      return undefined;
    };
    const { user, notes } = await openClinic({ name: "clinic" }, broken);
    await within(todays()).findByRole("button", { name: "תמר" });
    await user.click(within(rowOf("תמר")).getByRole("button", { name: "תזכורת" }));
    await waitFor(() => expect(notes).toEqual(["ההעתקה לא הצליחה."]));
  });

  it("a new clinic gets a friendly start with two first steps", async () => {
    window.history.replaceState({}, "", "/?clinic=empty");
    const { user, view } = await openClinic({ name: "clinic" }, withClipboard, false);
    expect(await screen.findByRole("heading", { name: "הקליניקה מוכנה, ועוד ריקה" })).toBeInTheDocument();
    expect(screen.queryByRole("region", { name: "הפגישות של היום" })).toBeNull();
    expect(screen.getByRole("button", { name: "2. לקבוע פגישה" })).toBeInTheDocument();
    expect(await a11yViolations(view.container)).toEqual([]);
    await user.click(screen.getByRole("button", { name: "1. להוסיף מטופל" }));
    expect(await screen.findByRole("dialog", { name: "מטופל חדש" })).toBeInTheDocument();
  });

  it("shows the shape of the page while it loads", async () => {
    let release: () => void = () => undefined;
    const wait = new Promise<void>((r) => (release = r));
    const slow: Override = async (cmd, args, core) => {
      if (cmd !== "clinic" || (args.request as ClinicRequest).op !== "overview") return undefined;
      await wait;
      return core.handle(cmd, args);
    };
    await openClinic({ name: "clinic" }, slow, false);
    expect(screen.getByRole("status", { name: "טוען את הפגישות" })).toBeInTheDocument();
    release();
    expect(await within(await screen.findByRole("region", { name: "הפגישות של היום" })).findByRole("button", { name: "תמר" })).toBeInTheDocument();
    expect(screen.queryByRole("status", { name: "טוען את הפגישות" })).toBeNull();
  });
});

describe("discreet mode", () => {
  it("shows initials in place of every name, and is remembered while the program is open", async () => {
    const { user, view } = await openClinic();
    await within(todays()).findByRole("button", { name: "תמר" });
    const toggle = screen.getByRole("switch", { name: "מצב דיסקרטי" });
    expect(toggle).toHaveAttribute("aria-checked", "false");
    const stored = () => JSON.stringify([{ ...window.localStorage }, { ...window.sessionStorage }]);
    const before = stored();
    await user.click(toggle);
    expect(toggle).toHaveAttribute("aria-checked", "true");
    expect(within(todays()).getByRole("button", { name: "ת." })).toBeInTheDocument();
    const panel = screen.getByRole("tabpanel");
    for (const name of ["תמר", "איתי", "מאיה", "נועם", "יובל", "עומר"]) expect(within(panel).queryByText(name), name).toBeNull();
    // In a dialog too: the note of a visit is headed by initials.
    await user.click(within(todays()).getAllByRole("button", { name: "סיכום" })[0] as HTMLElement);
    const dialog = await screen.findByRole("dialog", { name: "סיכום מפגש אבחון" });
    await within(dialog).findByLabelText("רשומה");
    expect(dialog).toHaveTextContent("ת. · 15.11.2026, 09:00");
    expect(dialog).not.toHaveTextContent("תמר");
    await user.click(within(dialog).getByRole("button", { name: "סגירה" }));

    // Leaving the clinic and coming back keeps it; nothing was written to storage.
    view.unmount();
    expect(stored()).toBe(before);
    await openClinic();
    expect(screen.getByRole("switch", { name: "מצב דיסקרטי" })).toHaveAttribute("aria-checked", "true");
    await user.click(screen.getByRole("switch", { name: "מצב דיסקרטי" }));
    expect(await within(todays()).findByRole("button", { name: "תמר" })).toBeInTheDocument();
  });
});

describe("discreet mode in the dialogs", () => {
  it("the personal note waits for “להציג”, and the history shows only what was done and when", async () => {
    const { user } = await openClinic();
    await within(todays()).findByRole("button", { name: "תמר" });
    await user.click(within(rowOf("תמר")).getByRole("button", { name: "סיכום" }));
    const first = await screen.findByRole("dialog", { name: "סיכום מפגש אבחון" });
    await user.type(await within(first).findByLabelText("רשומה"), "שיתפה פעולה לאורך המפגש.");
    await user.type(within(first).getByLabelText("תרשומת אישית"), "לבדוק שוב קשב בפעם הבאה.");
    await user.click(within(first).getByRole("button", { name: "שמירה" }));
    await waitFor(() => expect(screen.queryByRole("dialog")).toBeNull());
    await within(rowOf("תמר")).findByText("יש סיכום");

    await user.click(screen.getByRole("switch", { name: "מצב דיסקרטי" }));
    await user.click(within(rowOf("ת.")).getByRole("button", { name: "סיכום" }));
    const dialog = await screen.findByRole("dialog", { name: "סיכום מפגש אבחון" });
    await within(dialog).findByLabelText("רשומה");
    expect(within(dialog).queryByLabelText("תרשומת אישית")).toBeNull();
    expect(dialog).toHaveTextContent("התרשומת האישית מוסתרת במצב דיסקרטי.");
    expect(within(dialog).queryByDisplayValue("לבדוק שוב קשב בפעם הבאה.")).toBeNull();
    expect(dialog).not.toHaveTextContent("לבדוק שוב קשב");

    // The history of the note: what was done and when, and none of its lines.
    await user.click(within(dialog).getByRole("button", { name: "היסטוריית שינויים" }));
    expect(await within(dialog).findByText("נוצר")).toBeInTheDocument();
    expect(dialog).toHaveTextContent("פרטי השינויים מוסתרים במצב דיסקרטי.");
    const entries = within(dialog).getAllByRole("listitem");
    expect(entries.every((e) => /^נוצר\d\d\.\d\d\.\d{4}, \d\d:\d\d$/.test(e.textContent ?? ""))).toBe(true);
    expect(within(dialog).queryByDisplayValue("לבדוק שוב קשב בפעם הבאה.")).toBeNull();
    expect(dialog).not.toHaveTextContent("לבדוק שוב קשב");
    expect(dialog).not.toHaveTextContent("תמר");

    // She asks for the personal note: there it is, to read and to edit.
    await user.click(within(dialog).getByRole("button", { name: "להציג" }));
    expect(within(dialog).getByLabelText("תרשומת אישית")).toHaveValue("לבדוק שוב קשב בפעם הבאה.");
  });

  it("the history of a visit shows no lines while discreet mode is on", async () => {
    const { user } = await openClinic();
    await within(todays()).findByRole("button", { name: "מאיה" });
    await user.click(screen.getByRole("switch", { name: "מצב דיסקרטי" }));
    await user.click(within(rowOf("מ.")).getByRole("button", { name: /פתיחת הפגישה/ }));
    const dialog = await screen.findByRole("dialog", { name: "פרטי הפגישה" });
    await user.click(await within(dialog).findByRole("button", { name: "היסטוריית שינויים" }));
    expect(await within(dialog).findByText("נוצר")).toBeInTheDocument();
    expect(dialog).toHaveTextContent("פרטי השינויים מוסתרים במצב דיסקרטי.");
    expect(dialog).not.toHaveTextContent("סוג: פגישת טיפול");
  });
});

describe("a visit of a patient stays on the patient's card", () => {
  it("offers no other patient and no “זמן שמור” for it; a new visit offers both", async () => {
    const { user } = await openClinic();
    await within(todays()).findByRole("button", { name: "איתי" });
    await user.click(within(rowOf("איתי")).getByRole("button", { name: /פתיחת הפגישה/ }));
    const dialog = await screen.findByRole("dialog", { name: "פרטי הפגישה" });
    const kinds = await within(dialog).findByLabelText("סוג הפגישה");
    expect(within(kinds).queryByRole("option", { name: "זמן שמור" })).toBeNull();
    expect(within(kinds).getByRole("option", { name: "פגישת היכרות" })).toBeInTheDocument();
    expect(within(dialog).getByRole("group", { name: "מטופל" })).toHaveTextContent("איתי");
    expect(within(dialog).queryByRole("button", { name: "החלפה" })).toBeNull();
    await user.click(within(dialog).getByRole("button", { name: "ביטול" }));
    await waitFor(() => expect(screen.queryByRole("dialog")).toBeNull());

    await user.click(screen.getByRole("button", { name: "פגישה חדשה" }));
    const fresh = await screen.findByRole("dialog", { name: "פגישה חדשה" });
    expect(within(await within(fresh).findByLabelText("סוג הפגישה")).getByRole("option", { name: "זמן שמור" })).toBeInTheDocument();
  });
});

describe("adding a patient", () => {
  it("the dialog saves the card, and the new family can be scheduled at once", async () => {
    const { user, requests, notes } = await openClinic();
    await within(todays()).findByRole("button", { name: "תמר" });
    await user.click(screen.getByRole("button", { name: "מטופל חדש" }));
    const dialog = await screen.findByRole("dialog", { name: "מטופל חדש" });
    expect(await a11yViolations(dialog)).toEqual([]);

    // Nothing typed: refused in her words, and nothing is sent.
    await user.click(within(dialog).getByRole("button", { name: "הוספה" }));
    expect(within(dialog).getByRole("alert")).toHaveTextContent("צריך לכתוב את שם הילד או הילדה.");
    expect(requests().some((r) => r.op === "save_patient")).toBe(false);

    await user.type(within(dialog).getByLabelText("שם הילד או הילדה"), "אלה לדוגמה");
    await user.selectOptions(within(dialog).getByLabelText("מין"), "בת");
    await user.type(within(dialog).getByLabelText("תאריך לידה"), "15.5.2021");
    // The age beside the birth date, on today.
    expect(within(dialog).getByText("היום: בת 5 שנים ו-6 חודשים")).toBeInTheDocument();
    const mother = within(dialog).getByRole("group", { name: "איש קשר 1" });
    await user.type(within(mother).getByLabelText("שם"), "דנה");
    await user.click(within(dialog).getByRole("button", { name: "איש קשר" }));
    const second = within(dialog).getByRole("group", { name: "איש קשר 2" });
    expect(within(second).getByLabelText("קשר")).toHaveValue("father");
    await user.click(within(dialog).getByRole("button", { name: "הסרת איש קשר 2" }));
    expect(within(dialog).queryByRole("group", { name: "איש קשר 2" })).toBeNull();
    await user.type(within(dialog).getByLabelText("מחיר קבוע לפגישה (לא חובה)"), "420");
    await user.click(within(dialog).getByRole("button", { name: "הוספה" }));

    await waitFor(() => expect(screen.queryByRole("dialog")).toBeNull());
    expect(notes).toContain("המטופל נוסף");
    const sent = requests().find((r) => r.op === "save_patient");
    expect(sent).toMatchObject({ patient: {
      id: "", name: "אלה לדוגמה", gender: "female", birth_date: "2021-05-15", status: "active", waiting_since: null, session_fee: 42_000,
      contacts: [{ role: "mother", name: "דנה", phone: "", email: "" }], created_on: "2026-11-15", case_id: null,
    } });

    // She is there to choose for a visit, with her own fee.
    await user.click(screen.getByRole("button", { name: "פגישה חדשה" }));
    const visit = await screen.findByRole("dialog", { name: "פגישה חדשה" });
    await user.type(await within(visit).findByLabelText("מטופל"), "אלה");
    await user.click(await within(visit).findByRole("button", { name: "אלה לדוגמה" }));
    expect(within(visit).getByLabelText("מחיר")).toHaveValue("420");
    expect(within(visit).getByLabelText("משך (דקות)")).toHaveValue("90");
  });

  it("typed details are not lost by an accidental close", async () => {
    const { user } = await openClinic();
    await within(todays()).findByRole("button", { name: "תמר" });
    await user.click(screen.getByRole("button", { name: "מטופל חדש" }));
    const dialog = await screen.findByRole("dialog", { name: "מטופל חדש" });
    await user.type(within(dialog).getByLabelText("שם הילד או הילדה"), "אלה");
    await user.keyboard("{Escape}");
    expect(within(dialog).getByRole("alertdialog", { name: "לסגור בלי לשמור" })).toBeInTheDocument();
    await user.click(within(dialog).getByRole("button", { name: "להמשיך לערוך" }));
    expect(within(dialog).getByLabelText("שם הילד או הילדה")).toHaveValue("אלה");
    // "ביטול" at the bottom asks too, like Escape and ×.
    await user.click(within(dialog).getByRole("button", { name: "ביטול" }));
    expect(within(dialog).getByRole("alertdialog", { name: "לסגור בלי לשמור" })).toBeInTheDocument();
    expect(within(dialog).getByLabelText("שם הילד או הילדה")).toHaveValue("אלה");
    await user.click(within(dialog).getByRole("button", { name: "לסגור בלי לשמור" }));
    await waitFor(() => expect(screen.queryByRole("dialog")).toBeNull());
  });

  it("“ביטול” closes at once when nothing was typed, and asks first in a note and a visit with typed text", async () => {
    const { user } = await openClinic();
    await within(todays()).findByRole("button", { name: "תמר" });
    await user.click(screen.getByRole("button", { name: "מטופל חדש" }));
    await user.click(within(await screen.findByRole("dialog", { name: "מטופל חדש" })).getByRole("button", { name: "ביטול" }));
    await waitFor(() => expect(screen.queryByRole("dialog")).toBeNull());

    await user.click(within(rowOf("תמר")).getByRole("button", { name: "סיכום" }));
    const note = await screen.findByRole("dialog", { name: "סיכום מפגש אבחון" });
    await user.type(await within(note).findByLabelText("רשומה"), "טיוטה");
    await user.click(within(note).getByRole("button", { name: "ביטול" }));
    expect(within(note).getByRole("alertdialog", { name: "לסגור בלי לשמור" })).toBeInTheDocument();
    await user.click(within(note).getByRole("button", { name: "להמשיך לערוך" }));
    expect(within(note).getByLabelText("רשומה")).toHaveValue("טיוטה");
    await user.click(within(note).getByRole("button", { name: "ביטול" }));
    await user.click(within(note).getByRole("button", { name: "לסגור בלי לשמור" }));
    await waitFor(() => expect(screen.queryByRole("dialog")).toBeNull());

    await user.click(within(rowOf("איתי")).getByRole("button", { name: /פתיחת הפגישה/ }));
    const visit = await screen.findByRole("dialog", { name: "פרטי הפגישה" });
    await user.type(await within(visit).findByLabelText("מקום"), " ב");
    await user.click(within(visit).getByRole("button", { name: "ביטול" }));
    expect(within(visit).getByRole("alertdialog", { name: "לסגור בלי לשמור" })).toBeInTheDocument();
  });
});

describe("the session note", () => {
  it("keeps the record and the personal note apart, and says what each is", async () => {
    const { user, requests, view } = await openClinic();
    await within(todays()).findByRole("button", { name: "תמר" });
    await user.click(within(rowOf("תמר")).getByRole("button", { name: "סיכום" }));
    const dialog = await screen.findByRole("dialog", { name: "סיכום מפגש אבחון" });
    const record = await within(dialog).findByLabelText("רשומה");
    const personal = within(dialog).getByLabelText("תרשומת אישית");
    expect(record).not.toBe(personal);
    expect(record).toHaveAccessibleDescription("זו הרשומה הרשמית של הפגישה. המשפחה רשאית לבקש לעיין בה.");
    expect(personal).toHaveAccessibleDescription("שלך בלבד: מחשבות והשערות לעצמך. היא לא מודפסת, לא מיוצאת ולא עוברת לתיק האבחון.");
    expect(await a11yViolations(view.baseElement)).toEqual([]);
    // Nothing to save yet.
    expect(within(dialog).getByRole("button", { name: "שמירה" })).toBeDisabled();

    await user.type(record, "שיתפה פעולה לאורך המפגש.");
    await user.type(personal, "לבדוק שוב קשב בפעם הבאה.");
    await user.click(within(dialog).getByRole("button", { name: "שמירה" }));
    await waitFor(() => expect(screen.queryByRole("dialog")).toBeNull());
    const sent = requests().find((r) => r.op === "save_note");
    expect(sent).toMatchObject({ note: { id: "", record: "שיתפה פעולה לאורך המפגש.", personal: "לבדוק שוב קשב בפעם הבאה.", date: "2026-11-15", case_input_id: null } });
    expect(sent?.op === "save_note" && sent.note.visit_id).toMatch(/^visit-/);

    // The row says there is a note; opening it again shows each text in its own box.
    expect(await within(rowOf("תמר")).findByText("יש סיכום")).toBeInTheDocument();
    await user.click(within(rowOf("תמר")).getByRole("button", { name: "סיכום" }));
    const again = await screen.findByRole("dialog", { name: "סיכום מפגש אבחון" });
    expect(await within(again).findByLabelText("רשומה")).toHaveValue("שיתפה פעולה לאורך המפגש.");
    expect(within(again).getByLabelText("תרשומת אישית")).toHaveValue("לבדוק שוב קשב בפעם הבאה.");
    // תמר has no diagnostic case yet: there is nowhere to send the record.
    expect(within(again).queryByRole("button", { name: /להעביר/ })).toBeNull();
    expect(again).toHaveTextContent("למטופל עוד אין תיק אבחון.");
  });

  it("a template fills an empty record only, and unsaved text asks before it is dropped", async () => {
    const { user } = await openClinic();
    await within(todays()).findByRole("button", { name: "תמר" });
    await user.click(within(rowOf("תמר")).getByRole("button", { name: "סיכום" }));
    const dialog = await screen.findByRole("dialog", { name: "סיכום מפגש אבחון" });
    const record = await within(dialog).findByLabelText("רשומה");
    const insert = within(dialog).getByRole("button", { name: "הוספת התבנית" });
    expect(insert).toBeDisabled();
    await user.selectOptions(within(dialog).getByLabelText("תבנית"), "מפגש אבחון");
    await user.click(insert);
    expect((record as HTMLTextAreaElement).value).toMatch(/^כלים שהועברו:\n\nהתנהגות במפגש:/);
    // The personal note is not touched by a template, and a record with text takes none.
    expect(within(dialog).getByLabelText("תרשומת אישית")).toHaveValue("");
    expect(insert).toBeDisabled();

    await user.click(within(dialog).getByRole("button", { name: "סגירה" }));
    expect(within(dialog).getByRole("alertdialog", { name: "לסגור בלי לשמור" })).toBeInTheDocument();
    await user.click(within(dialog).getByRole("button", { name: "להמשיך לערוך" }));
    expect((record as HTMLTextAreaElement).value).toContain("כלים שהועברו:");
  });

  it("“להעביר לחומרי התיק” copies only the record, and shows that it was sent", async () => {
    // נועם is linked to the demo case, and his last testing session has no note yet. The
    // session is not on today's page, so the dialog is opened as his card will open it.
    const served = await serve(withClipboard);
    const user = userEvent.setup();
    const now = { date: "2026-11-15", minute: 480 };
    const ask = async (request: ClinicRequest) => ((await served.core.handle("clinic", { request })) as { data: unknown }).data;
    const rows = (await ask({ op: "patients", today: now })) as PatientRow[];
    const noam = rows.find((r) => r.patient.name === "נועם")?.patient.id ?? "";
    const card = (await ask({ op: "patient_card", id: noam, today: now })) as PatientCard;
    const visit = card.visits.find((v) => v.visit.kind === "assessment" && !v.has_note)?.visit as Visit;
    const notes: string[] = [];
    render(<Host app={appApi({ notify: (text) => notes.push(text) })}><NoteDialog patientId={noam} visit={visit} onClose={() => undefined} /></Host>);

    const dialog = await screen.findByRole("dialog", { name: "סיכום מפגש אבחון" });
    await user.type(await within(dialog).findByLabelText("רשומה"), "רשומה לתיק.");
    await user.type(within(dialog).getByLabelText("תרשומת אישית"), "מחשבה שנשארת אצלי.");
    expect(dialog).toHaveTextContent("רק הרשומה מועתקת. התרשומת האישית נשארת כאן.");
    await user.click(within(dialog).getByRole("button", { name: "להעביר לחומרי התיק" }));
    expect(await within(dialog).findByText(/^הועברה לתיק /)).toBeInTheDocument();
    expect(within(dialog).getByRole("button", { name: "להעביר שוב לחומרי התיק" })).toBeInTheDocument();
    expect(notes).toContain("הרשומה הועתקה לחומרי התיק. התרשומת האישית נשארה כאן בלבד.");
    // What she typed is still in its two boxes, saved.
    expect(within(dialog).getByLabelText("תרשומת אישית")).toHaveValue("מחשבה שנשארת אצלי.");
    expect(within(dialog).getByRole("button", { name: "שמירה" })).toBeDisabled();
    // What reached the case is the record alone.
    const detail = (await served.core.handle("case_detail", { caseId: served.caseId })) as { inputs: { title: string; content: string }[] };
    expect(detail.inputs.find((i) => i.title.startsWith("סיכום מפגש אבחון"))?.content).toBe("רשומה לתיק.");
    expect(JSON.stringify(detail)).not.toContain("מחשבה שנשארת אצלי.");
  });
});

describe("the visit dialog", () => {
  it("a new visit takes its length and fee from the kind, and an overlap is said after it is saved", async () => {
    const { user, requests, notes } = await openClinic();
    await within(todays()).findByRole("button", { name: "תמר" });
    await user.click(screen.getByRole("button", { name: "פגישה חדשה" }));
    const dialog = await screen.findByRole("dialog", { name: "פגישה חדשה" });
    expect(await a11yViolations(dialog)).toEqual([]);
    await user.selectOptions(await within(dialog).findByLabelText("סוג הפגישה"), "הדרכת הורים");
    expect(within(dialog).getByLabelText("משך (דקות)")).toHaveValue("50");
    expect(within(dialog).getByLabelText("מחיר")).toHaveValue("380");
    // Without a patient it is refused here, in her words.
    await user.click(within(dialog).getByRole("button", { name: "קביעת הפגישה" }));
    expect(within(dialog).getByRole("alert")).toHaveTextContent("צריך לבחור מטופל לפגישה.");
    await user.type(within(dialog).getByLabelText("מטופל"), "אית");
    await user.click(await within(dialog).findByRole("button", { name: "איתי" }));
    await user.clear(within(dialog).getByLabelText("שעת התחלה"));
    await user.type(within(dialog).getByLabelText("שעת התחלה"), "9:30");
    await user.click(within(dialog).getByRole("button", { name: "קביעת הפגישה" }));

    // 9:30 is inside תמר's session: saved anyway, and said.
    const saved = await screen.findByRole("dialog", { name: "הפגישה נשמרה" });
    expect(within(saved).getByRole("alert")).toHaveTextContent("הפגישה חופפת לפגישה אחרת באותו זמן");
    expect(notes).toContain("הפגישה נשמרה");
    expect(requests().find((r) => r.op === "save_visit")).toMatchObject({
      repeat: null, scope: "one", visit: { id: "", kind: "guidance", date: "2026-11-15", start: 570, minutes: 50, fee: 38_000, status: "planned", place: "הקליניקה" },
    });
    await user.click(within(saved).getByRole("button", { name: "הבנתי" }));
    await waitFor(() => expect(screen.queryByRole("dialog")).toBeNull());
    expect(await within(todays()).findAllByText("חופפת לפגישה אחרת")).toHaveLength(2);
  });

  it("time she keeps for herself has no patient and no fee, and can repeat", async () => {
    const { user, requests, notes } = await openClinic();
    await within(todays()).findByRole("button", { name: "תמר" });
    await user.click(screen.getByRole("button", { name: "פגישה חדשה" }));
    const dialog = await screen.findByRole("dialog", { name: "פגישה חדשה" });
    await user.selectOptions(await within(dialog).findByLabelText("סוג הפגישה"), "זמן שמור");
    expect(within(dialog).queryByLabelText("מטופל")).toBeNull();
    expect(within(dialog).queryByLabelText("מחיר")).toBeNull();
    await user.type(within(dialog).getByLabelText("מה בזמן הזה"), "הדרכה");
    await user.clear(within(dialog).getByLabelText("שעת התחלה"));
    await user.type(within(dialog).getByLabelText("שעת התחלה"), "18:00");
    await user.click(within(dialog).getByLabelText("פגישה חוזרת"));
    await user.selectOptions(within(dialog).getByLabelText("כל כמה זמן"), "כל שבועיים");
    await user.clear(within(dialog).getByLabelText("כמה פגישות בסך הכל"));
    await user.type(within(dialog).getByLabelText("כמה פגישות בסך הכל"), "3");
    expect(within(dialog).getByText("האחרונה: 13.12.2026")).toBeInTheDocument();
    await user.click(within(dialog).getByRole("button", { name: "קביעת הפגישה" }));
    await waitFor(() => expect(screen.queryByRole("dialog")).toBeNull());
    expect(notes).toContain("נשמרו 3 פגישות");
    expect(requests().find((r) => r.op === "save_visit")).toMatchObject({
      repeat: { every_weeks: 2, count: 3 }, visit: { kind: "block", patient_id: null, fee: 0, title: "הדרכה", start: 1080 },
    });
  });

  it("a visit of a series asks what the change applies to, marks what happened, and shows its history", async () => {
    const { user, requests } = await openClinic();
    await within(todays()).findByRole("button", { name: "מאיה" });
    await user.click(within(rowOf("מאיה")).getByRole("button", { name: /פתיחת הפגישה/ }));
    const dialog = await screen.findByRole("dialog", { name: "פרטי הפגישה" });
    const scope = await within(dialog).findByRole("radiogroup", { name: "על מה השינוי חל" });
    expect(within(scope).getByRole("radio", { name: "רק הפגישה הזו" })).toHaveAttribute("aria-checked", "true");
    expect(await a11yViolations(dialog)).toEqual([]);
    await user.click(within(scope).getByRole("radio", { name: "זו וכל הבאות" }));
    expect(dialog).toHaveTextContent("יעברו גם לפגישות הבאות שעוד מתוכננות");

    // What happened: cancelled, with a reason, charged anyway.
    const what = within(dialog).getByRole("radiogroup", { name: "מה קרה בפגישה" });
    expect(within(what).getAllByRole("radio").map((r) => r.textContent)).toEqual(["מתוכננת", "התקיימה", "בוטלה", "לא הגיעו"]);
    expect(within(dialog).queryByLabelText("לחייב בכל זאת")).toBeNull();
    await user.click(within(what).getByRole("radio", { name: "בוטלה" }));
    await user.type(within(dialog).getByLabelText("למה בוטלה (לא חובה)"), "מחלה");
    await user.click(within(dialog).getByLabelText("לחייב בכל זאת"));

    // Every saved state, on request.
    await user.click(within(dialog).getByRole("button", { name: "היסטוריית שינויים" }));
    expect(await within(dialog).findByText("נוצר")).toBeInTheDocument();
    expect(dialog).toHaveTextContent("סוג: פגישת טיפול");

    await user.click(within(dialog).getByRole("button", { name: "שמירה" }));
    await waitFor(() => expect(screen.queryByRole("dialog")).toBeNull());
    expect(requests().find((r) => r.op === "save_visit")).toMatchObject({
      scope: "following", repeat: null, visit: { status: "cancelled", status_note: "מחלה", charge_missed: true, kind: "therapy", fee: 35_000 },
    });
    expect(await within(rowOf("מאיה")).findByText("בוטלה")).toBeInTheDocument();
  });

  it("only a planned visit can be deleted, after she confirms", async () => {
    const { user, requests, notes } = await openClinic();
    await within(todays()).findByRole("button", { name: "איתי" });
    await user.click(within(rowOf("איתי")).getByRole("button", { name: /פתיחת הפגישה/ }));
    const dialog = await screen.findByRole("dialog", { name: "פרטי הפגישה" });
    await user.click(await within(dialog).findByRole("button", { name: "מחיקת הפגישה" }));
    const confirm = await screen.findByRole("dialog", { name: "מחיקת הפגישה" });
    expect(confirm).toHaveTextContent("הפגישה תימחק מהיומן.");
    expect(requests().some((r) => r.op === "delete_visit")).toBe(false);
    await user.click(within(confirm).getByRole("button", { name: "מחיקה" }));
    await waitFor(() => expect(screen.queryByRole("dialog")).toBeNull());
    expect(requests().find((r) => r.op === "delete_visit")).toMatchObject({ scope: "one" });
    expect(notes).toContain("הפגישה נמחקה");
    await waitFor(() => expect(within(todays()).queryByRole("button", { name: "איתי" })).toBeNull());
  });
});
