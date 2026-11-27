// The clinic's page over the demo core: the tabs (click, keyboard, deep links), the front
// page with the demo clinic's visits, the quick actions, the three dialogs, discreet mode and
// the accessibility check. Every name is invented; the clock is held at a Sunday morning so
// "today" is the same day on every run (and at 15:12 for the afternoon of the board, D-204).
import { configure, render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { useState } from "react";
import type { Route } from "../../App";
import type { ClinicRequest } from "../../ipc/generated/ClinicRequest";
import type { PatientCard } from "../../ipc/generated/PatientCard";
import type { PatientRow } from "../../ipc/generated/PatientRow";
import type { Visit } from "../../ipc/generated/Visit";
import { holdNow } from "../../components/Unsaved";
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
/** The "⋯" of a visit on a list. */
const moreOf = (row: HTMLElement) => within(row).getByRole("button", { name: /^עוד פעולות לפגישה/ });

/** Picks a step from the "⋯" of a visit's row. */
async function fromMenu(user: ReturnType<typeof userEvent.setup>, row: HTMLElement, item: string) {
  await user.click(moreOf(row));
  await user.click(await screen.findByRole("menuitem", { name: item }));
}

/** The words of a visit's "⋯", in order; the menu is closed again after. */
async function menuOf(user: ReturnType<typeof userEvent.setup>, row: HTMLElement): Promise<string[]> {
  await user.click(moreOf(row));
  const menu = await screen.findByRole("menu");
  const words = within(menu).getAllByRole("menuitem").map((i) => i.textContent ?? "");
  await user.keyboard("{Escape}");
  await waitFor(() => expect(screen.queryByRole("menu")).toBeNull());
  return words;
}

/** The afternoon of the board (ClinicToday.dc.html): תמר's and איתי's visits are over and not
 *  marked, her own time ended at 15:00, and מאיה's is at 16:00. */
const afternoon = () => vi.setSystemTime(new Date(2026, 10, 15, 15, 12));
const NAMES = ["תמר", "איתי", "מאיה", "נועם", "יובל", "עומר", "יעל"];

describe("the clinic's page", () => {
  it("marks the clinic as beta only on the bar's switch, shows the seven tabs, and passes the accessibility check", async () => {
    const { view } = await openClinic();
    // The page has no line of its own about the beta (D-204): the switch says it, below.
    expect(screen.queryByText(/בגרסת בטא/)).toBeNull();
    expect(screen.queryByRole("note")).toBeNull();
    expect(screen.getAllByRole("tab").map((t) => t.textContent)).toEqual(["היום", "יומן", "מטופלים", "המתנה", "מעקבים", "כספים", "תמונת מצב"]);
    // The clinic's settings are a part of the program's settings (D-205), not a tab here.
    expect(tab("היום")).toHaveAttribute("aria-selected", "true");
    expect(screen.getByRole("tabpanel")).toHaveAccessibleName("היום");
    // The bar marks the clinic as the part she is in (D-191), with its beta badge; the tabs
    // sit in the row under it.
    const parts = screen.getByRole("navigation", { name: "ניווט ראשי" });
    expect(within(parts).getByRole("button", { name: /ניהול הקליניקה/ })).toHaveAttribute("aria-current", "true");
    expect(within(parts).getByRole("button", { name: /ניהול הקליניקה/ })).toHaveTextContent("בטא");
    expect(within(parts).getByRole("button", { name: "כתיבת הדוחות" })).not.toHaveAttribute("aria-current");
    await within(todays()).findByRole("button", { name: "תמר" });
    expect(await a11yViolations(view.container)).toEqual([]);
  });

  it("switches tabs by click, and the address follows", async () => {
    const { user, went } = await openClinic();
    for (const [label, id] of [
      ["יומן", "calendar"], ["מטופלים", "patients"], ["המתנה", "waiting"],
      ["מעקבים", "tasks"], ["כספים", "money"], ["תמונת מצב", "insights"],
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
    expect(tab("תמונת מצב")).toHaveFocus();
    // Past the last comes the first.
    await user.keyboard("{ArrowLeft}");
    expect(tab("היום")).toHaveAttribute("aria-selected", "true");
    await user.keyboard("{ArrowRight}");
    expect(tab("תמונת מצב")).toHaveAttribute("aria-selected", "true");
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
    for (const label of ["פגישה בלי סיכום", "חוב פתוח", "המתנה ארוכה", "שיחת משוב קרובה", "מעקב שמועדו עבר"]) {
      expect(within(needs).getAllByText(label).length, label).toBeGreaterThan(0);
    }
    const tasks = screen.getByRole("region", { name: "מעקבים פתוחים" });
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
    const { user } = await openClinic();
    const next = await screen.findByRole("region", { name: "הפגישות הבאות: יום שני, 16.11.2026" });
    expect(within(next).getByRole("button", { name: "תמר" })).toBeInTheDocument();
    expect(within(next).getByText("תצפית")).toBeInTheDocument();
    expect(screen.getByRole("heading", { name: "ערב טוב" })).toBeInTheDocument();
    // A visit still ahead has not happened: there is nothing to mark yet, as on the card.
    const ahead = within(next).getByRole("button", { name: "תמר" }).closest("li") as HTMLElement;
    expect(within(ahead).queryByRole("button", { name: "התקיימה" })).toBeNull();
    expect(within(ahead).getByRole("button", { name: "תזכורת" })).toBeInTheDocument();
    // Not in its "⋯" either: the note and the visit itself.
    expect(moreOf(ahead)).toHaveAccessibleName("עוד פעולות לפגישה של תמר, 10:00");
    expect(await menuOf(user, ahead)).toEqual(["לכתוב סיכום", "לפתוח את הפגישה"]);
  });

  it("marking a visit “התקיימה” updates its row at once, and the vault has it", async () => {
    // תמר's visit (09:00–10:30) is over: "התקיימה" is its one step.
    vi.setSystemTime(new Date(2026, 10, 15, 10, 45));
    const { user, requests } = await openClinic();
    await within(todays()).findByRole("button", { name: "תמר" });
    expect(within(rowOf("תמר")).getByText("מתוכננת")).toBeInTheDocument();
    await user.click(within(rowOf("תמר")).getByRole("button", { name: "התקיימה" }));
    expect(within(rowOf("תמר")).getByText("התקיימה")).toBeInTheDocument();
    expect(within(rowOf("תמר")).queryByText("מתוכננת")).toBeNull();
    await waitFor(() => expect(requests().some((r) => r.op === "save_visit" && r.visit.status === "held" && r.scope === "one")).toBe(true));
    // After the vault answered, the row still says so, and the same button asks for the note.
    await waitFor(() => expect(within(rowOf("תמר")).queryByRole("button", { name: "התקיימה" })).toBeNull());
    expect(within(rowOf("תמר")).getByText("התקיימה")).toBeInTheDocument();
    expect(within(rowOf("תמר")).getByRole("button", { name: "לכתוב סיכום" })).toBeInTheDocument();
    // The other visits are as they were.
    expect(within(rowOf("איתי")).getByText("מתוכננת")).toBeInTheDocument();
  });

  it("a mark the vault refuses rolls back, and says why", async () => {
    const refuse: Override = (cmd, args) => {
      if (cmd === "copy_secret") return 60;
      if (cmd === "clinic" && (args.request as ClinicRequest).op === "save_visit") throw { code: "refused", message: "הפגישה לא נשמרה.", details: [], action: { kind: "dismiss" } };
      return undefined;
    };
    vi.setSystemTime(new Date(2026, 10, 15, 10, 45));
    const { user, notes } = await openClinic({ name: "clinic" }, refuse);
    await within(todays()).findByRole("button", { name: "תמר" });
    await user.click(within(rowOf("תמר")).getByRole("button", { name: "התקיימה" }));
    await waitFor(() => expect(notes).toContain("הפגישה לא נשמרה."));
    await waitFor(() => expect(within(rowOf("תמר")).getByText("מתוכננת")).toBeInTheDocument());
    expect(within(rowOf("תמר")).getByRole("button", { name: "התקיימה" })).toBeInTheDocument();
  });

  it("ticking a task shows it done at once and saves it as done today", async () => {
    const { user, requests } = await openClinic();
    const tasks = await screen.findByRole("region", { name: "מעקבים פתוחים" });
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
    await user.click(await screen.findByRole("menuitem", { name: "רק להעתיק" }));
    await waitFor(() => expect(notes).toHaveLength(1));
    expect(notes[0]).toBe("התזכורת הועתקה ללוח המוגן, ואפשר להדביק אותה בהודעה. הלוח יתנקה בעוד דקה.");
    const copied = calls.filter((c) => c.cmd === "copy_secret");
    expect(copied).toHaveLength(1);
    expect(copied[0]?.args).toEqual({ text: "שלום יעל, תזכורת לפגישה ביום ראשון, 15.11.2026, בשעה 09:00. להתראות, ד\"ר רותם בדויה" });
    // The default text leaves the child's name out; the toast never carries the message.
    expect(String(copied[0]?.args.text)).not.toContain("תמר");
  });

  it("“תזכורת” can open WhatsApp at the parent's chat; the text goes to the protected clipboard, never in the address", async () => {
    const { user, calls, notes } = await openClinic();
    await within(todays()).findByRole("button", { name: "תמר" });
    await user.click(within(rowOf("תמר")).getByRole("button", { name: "תזכורת" }));
    await user.click(await screen.findByRole("menuitem", { name: "לשלוח בוואטסאפ" }));
    await waitFor(() => expect(notes).toHaveLength(1));
    expect(notes[0]).toBe("וואטסאפ נפתח מול יעל. התזכורת בלוח המוגן: מדביקים (Ctrl+V) ושולחים. הלוח יתנקה בעוד דקה.");
    const sent = calls.filter((c) => c.cmd === "send_message");
    expect(sent).toHaveLength(1);
    expect(sent[0]?.args).toMatchObject({ kind: "reminder", channel: "whatsapp", contact: null });
    // The window never copies it itself, and nothing else carries the text.
    expect(calls.some((c) => c.cmd === "copy_secret")).toBe(false);
  });

  it("a copy that fails is said, and nothing is claimed", async () => {
    const broken: Override = (cmd) => {
      if (cmd === "copy_secret") throw { code: "internal", message: "ההעתקה לא הצליחה.", details: [], action: { kind: "dismiss" } };
      return undefined;
    };
    const { user, notes } = await openClinic({ name: "clinic" }, broken);
    await within(todays()).findByRole("button", { name: "תמר" });
    await user.click(within(rowOf("תמר")).getByRole("button", { name: "תזכורת" }));
    await user.click(await screen.findByRole("menuitem", { name: "רק להעתיק" }));
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

describe("the front page by the time (D-204)", () => {
  it("one action by the time: “התקיימה” on visits that are over, “תזכורת” on the one ahead, the rest under “⋯”", async () => {
    afternoon();
    const { user } = await openClinic();
    await within(todays()).findByRole("button", { name: "תמר" });
    // Over and not marked: "התקיימה" is the step, and the page's main one for that visit.
    for (const name of ["תמר", "איתי"]) {
      const row = rowOf(name);
      expect(within(row).getByRole("button", { name: "התקיימה" }), name).toHaveClass("btn", "btn-small", "btn-primary");
      expect(within(row).queryByRole("button", { name: "תזכורת" }), name).toBeNull();
      expect(within(row).queryByRole("button", { name: /סיכום/ }), name).toBeNull();
    }
    // Still ahead: "תזכורת", quiet, and nothing to mark yet.
    expect(within(rowOf("מאיה")).getByRole("button", { name: "תזכורת" })).toHaveClass("btn", "btn-small");
    expect(within(rowOf("מאיה")).getByRole("button", { name: "תזכורת" })).not.toHaveClass("btn-primary");
    expect(within(rowOf("מאיה")).queryByRole("button", { name: "התקיימה" })).toBeNull();
    // Each visit's row has its time, its name, one step and "⋯": no more.
    expect(within(rowOf("איתי")).getAllByRole("button").map((b) => b.getAttribute("aria-label") ?? b.textContent))
      .toEqual(["11:30–12:30, פתיחת הפגישה של איתי", "איתי", "התקיימה", "עוד פעולות לפגישה של איתי, 11:30"]);
    // Time she kept for herself: no step, no "⋯", and no empty box for them.
    const own = within(todays()).getByText("כתיבת דוחות").closest("li") as HTMLElement;
    expect(within(own).getAllByRole("button").map((b) => b.getAttribute("aria-label"))).toEqual(["13:00–15:00, פתיחת הפגישה של כתיבת דוחות"]);
    expect(own.querySelector(".clinic-visit-actions")).toBeNull();
    expect(own).toHaveTextContent("זמן שמור");

    // What is not the step waits in "⋯", always ending with the visit itself; a visit that has
    // begun offers no reminder there either.
    // "לא הגיעו" is there too, as the day's closing tells her to mark it in the row.
    expect(await menuOf(user, rowOf("תמר"))).toEqual(["לא הגיעו", "לכתוב סיכום", "להקליט לעצמך", "לפתוח את הפגישה"]);
    expect(await menuOf(user, rowOf("מאיה"))).toEqual(["התקיימה", "לא הגיעו", "לכתוב סיכום", "לפתוח את הפגישה"]);
    await fromMenu(user, rowOf("מאיה"), "לפתוח את הפגישה");
    const dialog = await screen.findByRole("dialog", { name: "פרטי הפגישה" });
    expect(within(dialog).getByRole("group", { name: "מטופל" })).toHaveTextContent("מאיה");
  });

  it("“התקיימה” and “לא הגיעו” from a row say so in a toast whose “ביטול” puts the visit back", async () => {
    afternoon();
    const served = await serve(withClipboard);
    const user = userEvent.setup();
    const toasts: { text: string; action?: { label: string; run: () => void } | undefined }[] = [];
    render(<Host app={appApi({ route: { name: "clinic" }, notify: (text, action) => void toasts.push({ text, action }) })}><ClinicScreen /></Host>);
    await within(await screen.findByRole("region", { name: "הפגישות של היום" })).findByRole("button", { name: "תמר" });
    const saves = () => served.calls.filter((c) => c.cmd === "clinic" && (c.args.request as ClinicRequest).op === "save_visit")
      .map((c) => (c.args.request as { visit: Visit }).visit);
    await user.click(within(rowOf("תמר")).getByRole("button", { name: "התקיימה" }));
    await waitFor(() => expect(toasts.at(-1)?.text).toBe("הפגישה סומנה \"התקיימה\""));
    expect(toasts.at(-1)?.action?.label).toBe("ביטול");
    toasts.at(-1)?.action?.run();
    await waitFor(() => expect(toasts.at(-1)?.text).toBe("הסימון בוטל"));
    expect(saves().map((v) => v.status)).toEqual(["held", "planned"]);
    expect(await within(rowOf("תמר")).findByRole("button", { name: "התקיימה" })).toBeInTheDocument();

    await fromMenu(user, rowOf("איתי"), "לא הגיעו");
    await waitFor(() => expect(toasts.at(-1)?.text).toBe("הפגישה סומנה \"לא הגיעו\""));
    expect(saves().at(-1)).toMatchObject({ status: "no_show", charge_missed: false });
  });

  it("draws “עכשיו 15:12” between the visits, counts “4 בלוח · אחת אחרי עכשיו”, and says “בעוד 48 דקות” on the next one", async () => {
    afternoon();
    const { view } = await openClinic();
    await within(todays()).findByRole("button", { name: "תמר" });
    const items = within(todays()).getAllByRole("listitem");
    expect(items.map((li) => within(li).queryAllByRole("button")[0]?.textContent ?? li.textContent))
      .toEqual(["09:00–10:30", "11:30–12:30", "13:00–15:00", "עכשיו 15:12", "16:00–16:50"]);
    // The line is text in its place in the list: no name of its own, nothing that speaks up.
    const line = items[3] as HTMLElement;
    expect(line).not.toHaveAttribute("aria-label");
    expect(within(line).queryByRole("status")).toBeNull();
    expect(line.querySelector(".clinic-now-rule")).toHaveAttribute("aria-hidden", "true");
    // How many the day holds, how many are still ahead, and how soon the next one is.
    expect(within(todays()).getByText("4 בלוח · אחת אחרי עכשיו")).toBeInTheDocument();
    expect(rowOf("מאיה")).toHaveTextContent("בעוד 48 דקות");
    expect(items.filter((li) => li.textContent?.includes("בעוד"))).toEqual([rowOf("מאיה")]);
    // "מתוכננת" is said to a screen reader only; the visits that are over keep it too.
    expect(within(rowOf("מאיה")).getByText("מתוכננת")).toHaveClass("visually-hidden");
    expect(within(rowOf("תמר")).getByText("מתוכננת")).toHaveClass("visually-hidden");
    expect(await a11yViolations(view.container)).toEqual([]);

    // Before the first visit there is no line (it only stands between two), and the first
    // visit says how soon it is.
    view.unmount();
    vi.setSystemTime(new Date(2026, 10, 15, 8, 0));
    const morning = await openClinic();
    await within(todays()).findByRole("button", { name: "תמר" });
    expect(within(todays()).queryByText(/^עכשיו/)).toBeNull();
    expect(within(todays()).getAllByRole("listitem")).toHaveLength(4);
    expect(within(todays()).getByText("4 בלוח")).toBeInTheDocument();
    expect(rowOf("תמר")).toHaveTextContent("בעוד שעה");

    // Just before her own time: that is not "the next one", מאיה's visit is.
    morning.view.unmount();
    vi.setSystemTime(new Date(2026, 10, 15, 12, 45));
    await openClinic();
    await within(todays()).findByRole("button", { name: "תמר" });
    expect(within(todays()).getAllByRole("listitem").map((li) => within(li).queryAllByRole("button")[0]?.textContent ?? li.textContent))
      .toEqual(["09:00–10:30", "11:30–12:30", "עכשיו 12:45", "13:00–15:00", "16:00–16:50"]);
    expect(within(todays()).getByText("4 בלוח · 2 אחרי עכשיו")).toBeInTheDocument();
    expect(rowOf("מאיה")).toHaveTextContent("בעוד 3 שעות ו-15 דקות");
    expect(within(todays()).getAllByRole("listitem").filter((li) => li.textContent?.includes("בעוד"))).toEqual([rowOf("מאיה")]);
  });

  it("after “התקיימה” the focus stays on the same button, which now reads “לכתוב סיכום”", async () => {
    afternoon();
    const { user, requests } = await openClinic();
    await within(todays()).findByRole("button", { name: "תמר" });
    const needs = screen.getByRole("region", { name: "מה דורש אותי" });
    expect(within(needs).getByRole("button", { name: "פגישה בלי סיכום: 2" })).toBeInTheDocument();
    const step = within(rowOf("תמר")).getByRole("button", { name: "התקיימה" });
    await user.click(step);
    expect(step).toHaveFocus();
    expect(step).toHaveTextContent("לכתוב סיכום");
    await waitFor(() => expect(requests().some((r) => r.op === "save_visit" && r.visit.status === "held")).toBe(true));
    // The page is read again: the visit now waits for its note, and the button is still hers.
    await within(needs).findByRole("button", { name: "פגישה בלי סיכום: 3" });
    expect(step.isConnected).toBe(true);
    expect(step).toHaveFocus();
    expect(step).toHaveAccessibleName("לכתוב סיכום");
    expect(step).not.toHaveClass("btn-primary");
    await user.keyboard("{Enter}");
    const dialog = await screen.findByRole("dialog", { name: "סיכום מפגש אבחון" });
    expect(dialog).toHaveTextContent("תמר · 15.11.2026, 09:00");
    await user.click(within(dialog).getByRole("button", { name: "סגירה" }));
    await waitFor(() => expect(screen.queryByRole("dialog")).toBeNull());

    // The same step on its line in "מה דורש אותי", for a visit of today, opens its note too.
    await user.click(within(needs).getByRole("button", { name: "פגישה בלי סיכום: 3" }));
    const tamar = within(needs).getByRole("button", { name: "תמר" }).closest("li") as HTMLElement;
    await user.click(within(tamar).getByRole("button", { name: "לכתוב סיכום" }));
    expect(await screen.findByRole("dialog", { name: "סיכום מפגש אבחון" })).toHaveTextContent("תמר · 15.11.2026, 09:00");
  });

  it("a note saved from “לכתוב סיכום” takes that step off the row, and the focus goes to the row's “⋯”, not to the page", async () => {
    afternoon();
    const { user } = await openClinic();
    await within(todays()).findByRole("button", { name: "תמר" });
    // Marked held, the same button asks for the note; Enter opens it.
    const step = within(rowOf("תמר")).getByRole("button", { name: "התקיימה" });
    await user.click(step);
    expect(step).toHaveAccessibleName("לכתוב סיכום");
    await user.keyboard("{Enter}");
    const dialog = await screen.findByRole("dialog", { name: "סיכום מפגש אבחון" });
    await user.type(await within(dialog).findByLabelText("רשומה"), "שיתפה פעולה לאורך המפגש.");
    await user.click(within(dialog).getByRole("button", { name: "שמירה" }));
    await waitFor(() => expect(screen.queryByRole("dialog")).toBeNull());
    await within(rowOf("תמר")).findByText("יש סיכום");
    expect(within(rowOf("תמר")).queryByRole("button", { name: "לכתוב סיכום" })).toBeNull();
    await waitFor(() => expect(document.activeElement).toBe(moreOf(rowOf("תמר"))));
  });

  it("groups “מה דורש אותי” by topic, the first open, with “N דברים” beside the title; passes the accessibility check open and closed", async () => {
    afternoon();
    const { user, view, went } = await openClinic();
    const needs = screen.getByRole("region", { name: "מה דורש אותי" });
    const toggles = () => within(needs).getAllByRole("heading", { level: 3 }).map((h) => within(h).getByRole("button"));
    const bodyOf = (toggle: HTMLElement) => document.getElementById(toggle.getAttribute("aria-controls") ?? "") as HTMLElement;
    expect(within(needs).getByText("8 דברים")).toBeInTheDocument();
    // In the order she would deal with them, with the clinic's words and a count; the first open.
    const topics = ["פגישה שלא סומנה: 2", "פגישה בלי סיכום: 2", "מעקב שמועדו עבר: 1", "שיחת משוב קרובה: 1", "חוב פתוח: 1", "המתנה ארוכה: 1"];
    expect(toggles()).toHaveLength(topics.length);
    toggles().forEach((toggle, i) => {
      expect(toggle).toHaveAccessibleName(topics[i]);
      expect(toggle).toHaveAttribute("aria-expanded", i === 0 ? "true" : "false");
    });
    // A line under its topic has no topic of its own.
    const [unmarked, noNote] = toggles() as [HTMLElement, HTMLElement];
    const open = bodyOf(unmarked);
    expect(within(open).getAllByRole("listitem")).toHaveLength(2);
    expect(open.querySelector(".chip")).toBeNull();
    expect(within(open).queryByText("פגישה שלא סומנה")).toBeNull();
    expect(within(open).getAllByRole("button", { name: "התקיימה" })).toHaveLength(2);
    expect(bodyOf(noNote)).not.toBeVisible();
    expect(await a11yViolations(view.container)).toEqual([]);

    // Folded and opened by her; one she opened stays open.
    await user.click(unmarked);
    expect(unmarked).toHaveAttribute("aria-expanded", "false");
    expect(await a11yViolations(view.container)).toEqual([]);
    await user.click(noNote);
    expect(noNote).toHaveAttribute("aria-expanded", "true");
    const lines = within(bodyOf(noNote)).getAllByRole("listitem");
    expect(lines.map((li) => li.textContent)).toEqual([
      "נועםעדיין אין סיכום למפגש אבחון מיום 01.11.2026.לכתוב סיכום",
      "מאיהעדיין אין סיכום לפגישת טיפול מיום 08.11.2026.לכתוב סיכום",
    ]);
    expect(await a11yViolations(view.container)).toEqual([]);
    // A visit of an earlier day is not on this page: its note is written from the card.
    await user.click(within(lines[0] as HTMLElement).getByRole("button", { name: "לכתוב סיכום" }));
    expect(went[went.length - 1]).toMatchObject({ name: "clinic", tab: "patients" });
    expect((went[went.length - 1] as { patientId?: string }).patientId).toMatch(/^patient-/);
  });

  it("“התקיימה” on an unmarked line saves at once, the line leaves, the count drops and the focus goes to its topic; a refusal brings it back", async () => {
    afternoon();
    let release: () => void = () => undefined;
    let refuse = false;
    const slowSave: Override = async (cmd, args, core) => {
      if (cmd === "copy_secret") return 60;
      if (cmd !== "clinic" || (args.request as ClinicRequest).op !== "save_visit") return undefined;
      await new Promise<void>((r) => (release = r));
      if (refuse) throw { code: "refused", message: "הפגישה לא נשמרה.", details: [], action: { kind: "dismiss" } };
      return core.handle(cmd, args);
    };
    const { user, requests, notes } = await openClinic({ name: "clinic" }, slowSave);
    const needs = screen.getByRole("region", { name: "מה דורש אותי" });
    const lineOf = (name: string) => within(needs).queryByRole("button", { name })?.closest("li") ?? null;
    const saves = () => requests().filter((r) => r.op === "save_visit");
    expect(within(needs).getByText("8 דברים")).toBeInTheDocument();

    await user.click(within(lineOf("תמר") as HTMLElement).getByRole("button", { name: "התקיימה" }));
    // Before the vault answers: the line is gone, the topic and the title count one less, and
    // the keyboard is on the topic's row.
    expect(lineOf("תמר")).toBeNull();
    const topic = within(needs).getByRole("button", { name: "פגישה שלא סומנה: 1" });
    expect(topic).toHaveFocus();
    expect(within(needs).getByText("7 דברים")).toBeInTheDocument();
    expect(within(rowOf("תמר")).getByText("התקיימה")).toBeInTheDocument();
    await waitFor(() => expect(saves()).toHaveLength(1));
    expect(saves()[0]).toMatchObject({ scope: "one", visit: { status: "held", start: 540 } });
    release();
    // The page is read again: the visit waits for its note now, and the focus was not moved.
    await within(needs).findByRole("button", { name: "פגישה בלי סיכום: 3" });
    expect(topic).toHaveFocus();
    expect(lineOf("תמר")).toBeNull();

    // The vault refuses the next one: the line comes back, and the toast says why.
    refuse = true;
    await user.click(within(lineOf("איתי") as HTMLElement).getByRole("button", { name: "התקיימה" }));
    expect(lineOf("איתי")).toBeNull();
    // The whole topic went: the keyboard is on the row now in its place.
    expect(within(needs).queryByRole("button", { name: /^פגישה שלא סומנה/ })).toBeNull();
    expect(within(needs).getByRole("button", { name: "פגישה בלי סיכום: 3" })).toHaveFocus();
    await waitFor(() => expect(saves()).toHaveLength(2));
    release();
    await waitFor(() => expect(notes).toContain("הפגישה לא נשמרה."));
    expect(await within(needs).findByRole("button", { name: "פגישה שלא סומנה: 1" })).toHaveAttribute("aria-expanded", "true");
    expect(within(lineOf("איתי") as HTMLElement).getByRole("button", { name: "התקיימה" })).toBeInTheDocument();
    expect(within(rowOf("איתי")).getByText("מתוכננת")).toBeInTheDocument();
    expect(within(rowOf("איתי")).getByRole("button", { name: "התקיימה" })).toBeInTheDocument();
  });

  it("in discreet mode every “⋯” is named by initials, no aria-label or title carries a full name, and a WhatsApp reminder from “תזכורת” toasts initials", async () => {
    afternoon();
    const { user, notes, calls } = await openClinic();
    await within(todays()).findByRole("button", { name: "תמר" });
    await user.click(screen.getByRole("switch", { name: "מצב דיסקרטי" }));
    expect(within(todays()).getAllByRole("button", { name: /^עוד פעולות לפגישה/ }).map((b) => b.getAttribute("aria-label")))
      .toEqual(["עוד פעולות לפגישה של ת., 09:00", "עוד פעולות לפגישה של א., 11:30", "עוד פעולות לפגישה של מ., 16:00"]);
    // Every name the page gives a control, or a tip, is initials too.
    const named = [...document.querySelectorAll("[aria-label], [title]")]
      .flatMap((el) => [el.getAttribute("aria-label") ?? "", el.getAttribute("title") ?? ""]).join(" | ");
    for (const name of NAMES) expect(named, name).not.toContain(name);
    for (const name of NAMES) expect(screen.getByRole("tabpanel"), name).not.toHaveTextContent(name);

    // מאיה's visit is still ahead: "תזכורת" is its step.
    await user.click(within(rowOf("מ.")).getByRole("button", { name: "תזכורת" }));
    await user.click(await screen.findByRole("menuitem", { name: "לשלוח בוואטסאפ" }));
    await waitFor(() => expect(notes).toHaveLength(1));
    expect(notes[0]).toBe("וואטסאפ נפתח מול ה. התזכורת בלוח המוגן: מדביקים (Ctrl+V) ושולחים. הלוח יתנקה בעוד דקה.");
    expect(calls.filter((c) => c.cmd === "send_message").map((c) => c.args)).toEqual([expect.objectContaining({ kind: "reminder", channel: "whatsapp", contact: null })]);
    expect(calls.some((c) => c.cmd === "copy_secret")).toBe(false);
  });
});

describe("discreet mode", () => {
  it("shows initials in place of every name, and stays on until she turns it off (D-192)", async () => {
    const { user, view } = await openClinic();
    await within(todays()).findByRole("button", { name: "תמר" });
    const toggle = screen.getByRole("switch", { name: "מצב דיסקרטי" });
    expect(toggle).toHaveAttribute("aria-checked", "false");
    await user.click(toggle);
    expect(toggle).toHaveAttribute("aria-checked", "true");
    expect(within(todays()).getByRole("button", { name: "ת." })).toBeInTheDocument();
    const panel = screen.getByRole("tabpanel");
    for (const name of ["תמר", "איתי", "מאיה", "נועם", "יובל", "עומר"]) expect(within(panel).queryByText(name), name).toBeNull();
    // In a dialog too: the note of a visit is headed by initials.
    await user.click(within(todays()).getByRole("button", { name: "עוד פעולות לפגישה של ת., 09:00" }));
    await user.click(await screen.findByRole("menuitem", { name: "לכתוב סיכום" }));
    const dialog = await screen.findByRole("dialog", { name: "סיכום מפגש אבחון" });
    await within(dialog).findByLabelText("רשומה");
    expect(dialog).toHaveTextContent("ת. · 15.11.2026, 09:00");
    expect(dialog).not.toHaveTextContent("תמר");
    await user.click(within(dialog).getByRole("button", { name: "סגירה" }));

    // Leaving the clinic and coming back keeps it. What is kept is one word, never a name.
    view.unmount();
    expect(window.localStorage.getItem("dv.discreet")).toBe("on");
    const kept = JSON.stringify([{ ...window.localStorage }, { ...window.sessionStorage }]);
    for (const name of ["תמר", "איתי", "מאיה", "נועם", "יובל", "עומר"]) expect(kept).not.toContain(name);
    await openClinic();
    expect(screen.getByRole("switch", { name: "מצב דיסקרטי" })).toHaveAttribute("aria-checked", "true");
    await user.click(screen.getByRole("switch", { name: "מצב דיסקרטי" }));
    expect(await within(todays()).findByRole("button", { name: "תמר" })).toBeInTheDocument();
    expect(window.localStorage.getItem("dv.discreet")).toBeNull();
  });
});

describe("discreet mode in the dialogs", () => {
  it("the personal note waits for “להציג”, and the history shows only what was done and when", async () => {
    const { user } = await openClinic();
    await within(todays()).findByRole("button", { name: "תמר" });
    await fromMenu(user, rowOf("תמר"), "לכתוב סיכום");
    const first = await screen.findByRole("dialog", { name: "סיכום מפגש אבחון" });
    await user.type(await within(first).findByLabelText("רשומה"), "שיתפה פעולה לאורך המפגש.");
    await user.type(within(first).getByLabelText("תרשומת אישית"), "לבדוק שוב קשב בפעם הבאה.");
    await user.click(within(first).getByRole("button", { name: "שמירה" }));
    await waitFor(() => expect(screen.queryByRole("dialog")).toBeNull());
    await within(rowOf("תמר")).findByText("יש סיכום");

    await user.click(screen.getByRole("switch", { name: "מצב דיסקרטי" }));
    await fromMenu(user, rowOf("ת."), "לפתוח את הסיכום");
    const dialog = await screen.findByRole("dialog", { name: "סיכום מפגש אבחון" });
    await within(dialog).findByLabelText("רשומה");
    expect(within(dialog).queryByLabelText("תרשומת אישית")).toBeNull();
    expect(dialog).toHaveTextContent("התרשומת האישית וההקלטות מוסתרות במצב דיסקרטי.");
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

    // Nothing typed: refused in her words, by the name box (not at the foot of a long form,
    // out of sight), and the focus goes there. Nothing is sent.
    await user.click(within(dialog).getByRole("button", { name: "הוספה" }));
    const nameBox = within(dialog).getByLabelText("שם הילד או הילדה");
    expect(nameBox).toHaveAttribute("aria-invalid", "true");
    expect(nameBox).toHaveAccessibleDescription("צריך לכתוב את שם הילד או הילדה.");
    expect(nameBox).toHaveFocus();
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
    await user.click(await within(visit).findByRole("option", { name: "אלה לדוגמה" }));
    expect(within(visit).getByLabelText("מחיר")).toHaveValue("420");
    expect(within(visit).getByLabelText("משך (דקות)")).toHaveValue("90");
  });

  it("a birth date typed wrong is not saved as no date; a phone or an email that cannot be reached is pointed out", async () => {
    const { user, requests, notes } = await openClinic();
    await within(todays()).findByRole("button", { name: "תמר" });
    await user.click(screen.getByRole("button", { name: "מטופל חדש" }));
    const dialog = await screen.findByRole("dialog", { name: "מטופל חדש" });
    await user.type(within(dialog).getByLabelText("שם הילד או הילדה"), "אלה לדוגמה");
    const birth = within(dialog).getByLabelText("תאריך לידה");
    await user.type(birth, "31.02.2020");
    const mother = within(dialog).getByRole("group", { name: "איש קשר 1" });
    await user.type(within(mother).getByLabelText("טלפון"), "050-12");
    await user.type(within(mother).getByLabelText("מייל"), "dana@");
    await user.tab();
    expect(within(mother).getByLabelText("טלפון")).toHaveAccessibleDescription("המספר לא נראה תקין, ולא תהיה אפשרות לשלוח אליו הודעה.");
    expect(within(mother).getByLabelText("מייל")).toHaveAccessibleDescription("הכתובת לא נראית תקינה, ולא תהיה אפשרות לשלוח אליה מייל.");
    await user.click(within(dialog).getByRole("button", { name: "הוספה" }));
    expect(birth).toHaveFocus();
    expect(birth).toHaveAttribute("aria-invalid", "true");
    expect(requests().some((r) => r.op === "save_patient")).toBe(false);
    // Fixed: saved with the date. The phone and the email are warnings, not a refusal.
    await user.clear(birth);
    await user.type(birth, "28.02.2020");
    await user.click(within(dialog).getByRole("button", { name: "הוספה" }));
    await waitFor(() => expect(notes).toContain("המטופל נוסף"));
    expect(requests().find((r) => r.op === "save_patient")).toMatchObject({ patient: { birth_date: "2020-02-28" } });
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

    await fromMenu(user, rowOf("תמר"), "לכתוב סיכום");
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
    await fromMenu(user, rowOf("תמר"), "לכתוב סיכום");
    const dialog = await screen.findByRole("dialog", { name: "סיכום מפגש אבחון" });
    const record = await within(dialog).findByLabelText("רשומה");
    const personal = within(dialog).getByLabelText("תרשומת אישית");
    expect(record).not.toBe(personal);
    expect(record).toHaveAccessibleDescription("זו הרשומה הרשמית של הפגישה. המשפחה רשאית לבקש לעיין בה.");
    expect(personal).toHaveAccessibleDescription("שלך בלבד: מחשבות והשערות לעצמך, בכתב או בהקלטה. הן לא מודפסות, לא מיוצאות ולא עוברות לתיק האבחון.");
    expect(await a11yViolations(view.baseElement)).toEqual([]);
    // Nothing to save yet.
    expect(within(dialog).getByRole("button", { name: "שמירה" })).toBeDisabled();

    await user.type(record, "שיתפה פעולה לאורך המפגש.");
    await user.type(personal, "לבדוק שוב קשב בפעם הבאה.");
    await user.click(within(dialog).getByRole("button", { name: "שמירה" }));
    await waitFor(() => expect(screen.queryByRole("dialog")).toBeNull());
    const sent = requests().find((r) => r.op === "save_note");
    expect(sent).toMatchObject({ note: { id: "", record: "שיתפה פעולה לאורך המפגש.", personal: "לבדוק שוב קשב בפעם הבאה.", date: "2026-11-15", case_input_id: null, recordings: [] } });
    expect(sent?.op === "save_note" && sent.note.visit_id).toMatch(/^visit-/);

    // The row says there is a note; opening it again shows each text in its own box.
    expect(await within(rowOf("תמר")).findByText("יש סיכום")).toBeInTheDocument();
    await fromMenu(user, rowOf("תמר"), "לפתוח את הסיכום");
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
    await fromMenu(user, rowOf("תמר"), "לכתוב סיכום");
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
  it("a new visit today starts after now, not at an hour already gone; an hour gone is pointed out", async () => {
    afternoon();
    const { user } = await openClinic();
    await user.click(await screen.findByRole("button", { name: "פגישה חדשה" }));
    const dialog = await screen.findByRole("dialog", { name: "פגישה חדשה" });
    const start = await within(dialog).findByLabelText("שעת התחלה");
    // 15:12: the next half hour.
    expect(start).toHaveValue("15:30");
    expect(dialog).not.toHaveTextContent("השעה הזו כבר עברה");
    await user.clear(start);
    await user.type(start, "9:00");
    expect(within(dialog).getByText(/^השעה הזו כבר עברה\./)).toBeInTheDocument();
    // Her own time is never marked: nothing is said of an hour gone.
    await user.selectOptions(within(dialog).getByLabelText("סוג הפגישה"), "זמן שמור");
    expect(dialog).not.toHaveTextContent("השעה הזו כבר עברה");
  });

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
    await user.click(await within(dialog).findByRole("option", { name: "איתי" }));
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

  it("the patient's list opens only while she looks for one, and the keys walk it (Einat, 17.11.2026)", async () => {
    const { user } = await openClinic();
    await within(todays()).findByRole("button", { name: "תמר" });
    await user.click(screen.getByRole("button", { name: "פגישה חדשה" }));
    const dialog = await screen.findByRole("dialog", { name: "פגישה חדשה" });
    const field = within(dialog).getByRole("combobox", { name: "מטופל" });
    const list = () => within(dialog).queryByRole("listbox", { name: "מטופלים לבחירה" });
    // The names in the list (the window's own drop-downs have options too).
    const options = () => within(within(dialog).getByRole("listbox", { name: "מטופלים לבחירה" })).getAllByRole("option");
    const names = () => options().map((o) => o.textContent);
    const marked = () => options().filter((o) => o.getAttribute("aria-selected") === "true").map((o) => o.textContent);
    // The field has the focus, and the form under it stays in view.
    await waitFor(() => expect(field).toHaveFocus());
    expect(list()).toBeNull();
    expect(field).toHaveAttribute("aria-expanded", "false");
    // A click on the field shows the families in progress; Escape closes the list, not the window.
    await user.click(field);
    expect(await within(dialog).findByRole("option", { name: "איתי" })).toBeInTheDocument();
    expect(field).toHaveAttribute("aria-expanded", "true");
    expect(await a11yViolations(dialog)).toEqual([]);
    await user.keyboard("{Escape}");
    expect(list()).toBeNull();
    expect(screen.getByRole("dialog", { name: "פגישה חדשה" })).toBeInTheDocument();
    expect(field).toHaveFocus();
    // The down arrow opens it on the first name; the arrows mark a name and the focus stays in the field.
    await user.keyboard("{ArrowDown}");
    await waitFor(() => expect(list()).not.toBeNull());
    expect(list()).toHaveAttribute("tabindex", "-1"); // Tab goes on to the next field, also when the list scrolls
    const [first, second] = options();
    expect(field).toHaveAttribute("aria-activedescendant", first?.id);
    await user.keyboard("{ArrowDown}");
    expect(field).toHaveAttribute("aria-activedescendant", second?.id);
    expect(marked()).toEqual([second?.textContent]);
    expect(field).toHaveFocus();
    // Up from the first name is back to what she typed, with nothing marked.
    await user.keyboard("{ArrowUp}{ArrowUp}");
    expect(marked()).toEqual([]);
    expect(field).not.toHaveAttribute("aria-activedescendant");
    // Enter while the list is open never sends the form: with nothing marked it marks the first name.
    await user.keyboard("{Enter}");
    expect(marked()).toEqual([first?.textContent]);
    expect(within(dialog).queryByRole("alert")).toBeNull();
    // Every letter narrows it at once; Enter on the one name left chooses it, and the keyboard stays
    // on the chosen name (not on "החלפה", where the next Enter would undo it).
    await user.type(field, "אית");
    expect(names()).toEqual(["איתי"]);
    await user.keyboard("{Enter}");
    const picked = within(dialog).getByRole("group", { name: "מטופל" });
    expect(picked).toHaveTextContent("איתי");
    expect(picked).toHaveAccessibleDescription("איתי");
    expect(picked).toHaveFocus();
    expect(list()).toBeNull();
    expect(within(dialog).queryByRole("alert")).toBeNull();
    await user.keyboard("{Enter}");
    expect(within(dialog).getByRole("group", { name: "מטופל" })).toHaveTextContent("איתי");
    expect(screen.getByRole("dialog", { name: "פגישה חדשה" })).toBeInTheDocument();
    // "החלפה" starts the search again from an empty field, with the keyboard in it.
    await user.click(within(dialog).getByRole("button", { name: "החלפה" }));
    const again = within(dialog).getByRole("combobox", { name: "מטופל" });
    expect(again).toHaveValue("");
    expect(again).toHaveFocus();
    await user.type(again, "מא");
    expect(names()).toEqual(["מאיה"]);
    // Leaving the field closes the list too.
    await user.click(within(dialog).getByLabelText("כותרת (לא חובה)"));
    expect(list()).toBeNull();
    // Back in it, the arrow marks the name and Enter chooses it.
    await user.click(again);
    await user.keyboard("{ArrowDown}{Enter}");
    expect(within(dialog).getByRole("group", { name: "מטופל" })).toHaveTextContent("מאיה");
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

describe("supervision, what not to forget, and recordings (D-207, D-208)", () => {
  it("a supervision without a child is her own time: no fee, and its own words", async () => {
    const { user, requests } = await openClinic();
    await within(todays()).findByRole("button", { name: "תמר" });
    await user.click(screen.getByRole("button", { name: "פגישה חדשה" }));
    const dialog = await screen.findByRole("dialog", { name: "פגישה חדשה" });
    await user.selectOptions(await within(dialog).findByLabelText("סוג הפגישה"), "הדרכה");
    expect(within(dialog).getByLabelText("על איזה ילד (לא חובה)")).toBeInTheDocument();
    expect(within(dialog).queryByLabelText("מחיר")).toBeNull();
    expect(dialog).toHaveTextContent("הדרכה של צוות, גננת או מטפלת אחרת");
    await user.type(within(dialog).getByLabelText("עם מי או על מה (לא חובה)"), "הדרכה קבוצתית");
    await user.click(within(dialog).getByRole("button", { name: "קביעת הפגישה" }));
    await waitFor(() => expect(requests().find((r) => r.op === "save_visit")).toBeDefined());
    expect(requests().find((r) => r.op === "save_visit")).toMatchObject({ visit: { kind: "supervision", patient_id: null, fee: 0, title: "הדרכה קבוצתית" } });
  });

  it("a supervision about a child (the kindergarten teacher, the staff) goes on the child's card, with a fee", async () => {
    const { user, requests } = await openClinic();
    await within(todays()).findByRole("button", { name: "תמר" });
    await user.click(screen.getByRole("button", { name: "פגישה חדשה" }));
    const dialog = await screen.findByRole("dialog", { name: "פגישה חדשה" });
    await user.selectOptions(await within(dialog).findByLabelText("סוג הפגישה"), "הדרכה");
    await user.type(within(dialog).getByLabelText("על איזה ילד (לא חובה)"), "אית");
    await user.click(await within(dialog).findByRole("option", { name: "איתי" }));
    const fee = await within(dialog).findByLabelText("מחיר");
    await user.clear(fee);
    await user.type(fee, "300");
    // Without the child the fee is hidden; picking the child again brings back what she typed.
    await user.click(within(dialog).getByRole("button", { name: "החלפה" }));
    expect(within(dialog).queryByLabelText("מחיר")).toBeNull();
    await user.type(within(dialog).getByLabelText("על איזה ילד (לא חובה)"), "אית");
    await user.click(await within(dialog).findByRole("option", { name: "איתי" }));
    expect(await within(dialog).findByLabelText("מחיר")).toHaveValue("300");
    await user.type(within(dialog).getByLabelText("עם מי או על מה (לא חובה)"), "הדרכת הגננת");
    await user.click(within(dialog).getByRole("button", { name: "קביעת הפגישה" }));
    await waitFor(() => expect(requests().find((r) => r.op === "save_visit")).toBeDefined());
    const sent = requests().find((r) => r.op === "save_visit") as { visit: { patient_id: string | null } };
    expect(sent).toMatchObject({ visit: { kind: "supervision", fee: 30_000, title: "הדרכת הגננת" } });
    expect(sent.visit.patient_id).not.toBeNull();
  });

  it("what not to forget is written in the visit, shown first when it is opened, and under its row (only how many when discreet)", async () => {
    const { user, requests } = await openClinic();
    await within(todays()).findByRole("button", { name: "מאיה" });
    await user.click(within(rowOf("מאיה")).getByRole("button", { name: /פתיחת הפגישה של מאיה/ }));
    const dialog = await screen.findByRole("dialog", { name: "פרטי הפגישה" });
    const group = await within(dialog).findByRole("group", { name: "לא לשכוח לקראת הפגישה" });
    // It comes before the rest of the visit's details.
    const kind = within(dialog).getByLabelText("סוג הפגישה");
    expect(group.compareDocumentPosition(kind) & Node.DOCUMENT_POSITION_FOLLOWING).toBeTruthy();
    await user.type(within(group).getByLabelText("דבר חדש שלא לשכוח"), "לדבר עם הגננת{Enter}");
    await user.type(within(group).getByLabelText("דבר חדש שלא לשכוח"), "להביא שאלונים");
    await user.click(within(group).getByRole("button", { name: "הוספה" }));
    await user.click(within(group).getByLabelText("להביא שאלונים"));
    expect(within(group).getByLabelText("להביא שאלונים")).toBeChecked();
    await user.click(within(dialog).getByRole("button", { name: "שמירה" }));
    await waitFor(() => expect(screen.queryByRole("dialog")).toBeNull());
    expect(requests().find((r) => r.op === "save_visit")).toMatchObject({
      visit: { remember: [{ text: "לדבר עם הגננת", done: false }, { text: "להביא שאלונים", done: true }] },
    });
    // Only what is still not done, under the row.
    expect(await within(rowOf("מאיה")).findByText("לא לשכוח: לדבר עם הגננת")).toBeInTheDocument();
    await user.click(screen.getByRole("switch", { name: "מצב דיסקרטי" }));
    const row = within(todays()).getAllByRole("listitem").find((li) => li.textContent?.includes("דבר אחד שלא לשכוח"));
    expect(row).toBeDefined();
    expect(todays()).not.toHaveTextContent("לדבר עם הגננת");
  });

  it("a recording from a file is kept with the note, played from the vault, and removed", async () => {
    const urls: string[] = [];
    const created = vi.fn(() => {
      urls.push(`blob:invented-${urls.length}`);
      return urls[urls.length - 1] ?? "";
    });
    const revoked = vi.fn();
    Object.assign(URL, { createObjectURL: created, revokeObjectURL: revoked });
    const { user, requests, notes } = await openClinic();
    await within(todays()).findByRole("button", { name: "תמר" });
    await fromMenu(user, rowOf("תמר"), "לכתוב סיכום");
    const dialog = await screen.findByRole("dialog", { name: "סיכום מפגש אבחון" });
    await within(dialog).findByLabelText("רשומה");
    expect(dialog).toHaveTextContent("ההקלטה נשמרת מוצפנת בכספת, רק אצלך. היא לא נשלחת לשום מקום, לא ל-AI ולא לתיק האבחון.");
    const input = dialog.querySelector('input[type="file"]') as HTMLInputElement;
    const sound = new File([new Uint8Array([1, 2, 3, 4, 5, 6])], "אחרי הפגישה.m4a", { type: "audio/mp4" });
    await user.upload(input, sound);
    expect(await within(dialog).findByText("אחרי הפגישה.m4a", {}, { timeout: 8000 })).toBeInTheDocument();
    const sent = requests().find((r) => r.op === "save_recording");
    expect(sent).toMatchObject({ note_id: "", mime: "audio/mp4", name: "אחרי הפגישה.m4a", data: "AQIDBAUG" });
    expect(sent?.op === "save_recording" && sent.visit_id).toMatch(/^visit-/);
    expect(notes).toContain("ההקלטה נשמרה בכספת");
    // The first recording made the visit's note; the texts are still hers to write.
    expect(within(dialog).getByLabelText("רשומה")).toHaveValue("");

    await user.click(within(dialog).getByRole("button", { name: "להשמיע" }));
    const player = await within(dialog).findByLabelText("השמעת אחרי הפגישה.m4a");
    expect(player.tagName).toBe("AUDIO");
    const src = player.getAttribute("src") ?? "";
    expect(src).toMatch(/^blob:invented-/);
    expect(requests().some((r) => r.op === "recording")).toBe(true);

    await user.click(within(dialog).getByRole("button", { name: "מחיקת אחרי הפגישה.m4a" }));
    const confirm = await screen.findByRole("dialog", { name: "מחיקת ההקלטה" });
    await user.click(within(confirm).getByRole("button", { name: "מחיקה" }));
    await waitFor(() => expect(within(dialog).queryByText("אחרי הפגישה.m4a")).toBeNull());
    expect(requests().find((r) => r.op === "delete_recording")).toBeDefined();
    expect(revoked).toHaveBeenCalledWith(src);
  });

  it("writing the summary, what not to forget goes onto the patient's next planned visit", async () => {
    const { user, requests, notes } = await openClinic();
    await within(todays()).findByRole("button", { name: "תמר" });
    await fromMenu(user, rowOf("תמר"), "לכתוב סיכום");
    const dialog = await screen.findByRole("dialog", { name: "סיכום מפגש אבחון" });
    const group = await within(dialog).findByRole("group", { name: "לא לשכוח לפגישה הבאה" });
    // Tomorrow's observation is her next visit.
    expect(group).toHaveTextContent("ייכנס לרשימה של הפגישה ב");
    expect(group).toHaveTextContent("10:00");
    await user.type(within(group).getByLabelText("דבר שלא לשכוח בפגישה הבאה"), "לדבר עם הגננת{Enter}");
    await waitFor(() => expect(requests().find((r) => r.op === "save_visit")).toBeDefined());
    expect(requests().find((r) => r.op === "save_visit")).toMatchObject({
      scope: "one", visit: { kind: "observation", remember: [{ text: "לדבר עם הגננת", done: false }] },
    });
    expect(await within(group).findByText("לדבר עם הגננת")).toBeInTheDocument();
    expect(within(group).getByLabelText("דבר שלא לשכוח בפגישה הבאה")).toHaveValue("");
    expect(notes.some((n) => n.startsWith("השורה נוספה ל\"לא לשכוח\" של הפגישה ב-"))).toBe(true);
    // The note itself is untouched: nothing to save yet.
    expect(within(dialog).getByRole("button", { name: "שמירה" })).toBeDisabled();
  });

  it("a line for the next visit typed without “הוספה” is added on saving, and closing asks first", async () => {
    const { user, requests, notes } = await openClinic();
    await within(todays()).findByRole("button", { name: "תמר" });
    await fromMenu(user, rowOf("תמר"), "לכתוב סיכום");
    let dialog = await screen.findByRole("dialog", { name: "סיכום מפגש אבחון" });
    let group = await within(dialog).findByRole("group", { name: "לא לשכוח לפגישה הבאה" });
    // Only the line: Escape asks before it is dropped, and "שמירה" can save it.
    await user.type(within(group).getByLabelText("דבר שלא לשכוח בפגישה הבאה"), "לשאול על השינה");
    await user.keyboard("{Escape}");
    expect(within(dialog).getByRole("alertdialog", { name: "לסגור בלי לשמור" })).toBeInTheDocument();
    await user.click(within(dialog).getByRole("button", { name: "להמשיך לערוך" }));
    await user.type(within(dialog).getByLabelText("רשומה"), "רשומה בדויה.");
    await user.click(within(dialog).getByRole("button", { name: "שמירה" }));
    await waitFor(() => expect(screen.queryByRole("dialog")).toBeNull());
    expect(requests().find((r) => r.op === "save_note")).toMatchObject({ note: { record: "רשומה בדויה." } });
    expect(requests().find((r) => r.op === "save_visit")).toMatchObject({
      scope: "one", visit: { kind: "observation", remember: [{ text: "לשאול על השינה", done: false }] },
    });
    expect(notes.some((n) => /^הסיכום נשמר\. השורה נוספה ל"לא לשכוח" של הפגישה ב-/.test(n))).toBe(true);
    // Opened again, the next visit's list has it.
    await fromMenu(user, rowOf("תמר"), "לפתוח את הסיכום");
    dialog = await screen.findByRole("dialog", { name: "סיכום מפגש אבחון" });
    group = await within(dialog).findByRole("group", { name: "לא לשכוח לפגישה הבאה" });
    expect(await within(group).findByText("לשאול על השינה")).toBeInTheDocument();
  });

  it("a line under “לא לשכוח” typed without “הוספה” is saved with the visit, and Escape asks first", async () => {
    const { user, requests } = await openClinic();
    await within(todays()).findByRole("button", { name: "איתי" });
    await user.click(within(rowOf("איתי")).getByRole("button", { name: /פתיחת הפגישה של איתי/ }));
    const dialog = await screen.findByRole("dialog", { name: "פרטי הפגישה" });
    const group = await within(dialog).findByRole("group", { name: "לא לשכוח לקראת הפגישה" });
    await user.type(within(group).getByLabelText("דבר חדש שלא לשכוח"), "להביא את הקוביות");
    await user.keyboard("{Escape}");
    expect(within(dialog).getByRole("alertdialog", { name: "לסגור בלי לשמור" })).toBeInTheDocument();
    await user.click(within(dialog).getByRole("button", { name: "להמשיך לערוך" }));
    await user.click(within(dialog).getByRole("button", { name: "שמירה" }));
    await waitFor(() => expect(screen.queryByRole("dialog")).toBeNull());
    expect(requests().find((r) => r.op === "save_visit")).toMatchObject({ visit: { remember: [{ text: "להביא את הקוביות", done: false }] } });
  });

  it("a session note being typed is held for a lock, and let go once saved or closed on purpose", async () => {
    const { user, calls } = await openClinic();
    const held = () => calls.filter((c) => c.cmd === "hold_unsaved").at(-1)?.args.edits as { case_id: string; place: string; view: string; text: string }[] | undefined;
    await within(todays()).findByRole("button", { name: "תמר" });
    await fromMenu(user, rowOf("תמר"), "לכתוב סיכום");
    const dialog = await screen.findByRole("dialog", { name: "סיכום מפגש אבחון" });
    await user.type(await within(dialog).findByLabelText("רשומה"), "רשומה בדויה ארוכה.");
    await user.type(within(dialog).getByLabelText("תרשומת אישית"), "מחשבה בדויה.");
    await holdNow();
    expect(held()).toEqual([
      expect.objectContaining({ view: "clinic", text: "רשומה בדויה ארוכה.", place: expect.stringMatching(/^רשומה בסיכום \(פגישה ב-/) as unknown }),
      expect.objectContaining({ view: "clinic", text: "מחשבה בדויה.", place: expect.stringMatching(/^תרשומת אישית בסיכום/) as unknown }),
    ]);
    // The child's name is not in what is held: it is shown again after the next entry.
    expect((held() ?? []).some((e) => e.place.includes("תמר"))).toBe(false);
    await user.keyboard("{Escape}");
    await user.click(within(dialog).getByRole("button", { name: "לסגור בלי לשמור" }));
    await waitFor(() => expect(screen.queryByRole("dialog")).toBeNull());
    await holdNow();
    expect(held()).toEqual([]);
  });

  it("“להקליט לעצמך” in a visit's “⋯” opens its note and asks for the microphone at once", async () => {
    const getUserMedia = vi.fn(() => Promise.reject(Object.assign(new Error("denied"), { name: "NotAllowedError" })));
    Object.defineProperty(navigator, "mediaDevices", { value: { getUserMedia }, configurable: true });
    vi.stubGlobal("MediaRecorder", Object.assign(function MediaRecorder() {}, { isTypeSupported: () => true }));
    afternoon();
    try {
      const { user } = await openClinic();
      await within(todays()).findByRole("button", { name: "תמר" });
      await fromMenu(user, rowOf("תמר"), "להקליט לעצמך");
      const dialog = await screen.findByRole("dialog", { name: "סיכום מפגש אבחון" });
      expect(await within(dialog).findByRole("alert")).toHaveTextContent("אין לתוכנה רשות להשתמש במיקרופון.");
      expect(getUserMedia).toHaveBeenCalledTimes(1);
    } finally {
      vi.unstubAllGlobals();
      Reflect.deleteProperty(navigator, "mediaDevices");
    }
  });

  it("a microphone she did not allow is said in her words, and nothing is kept", async () => {
    const getUserMedia = vi.fn(() => Promise.reject(Object.assign(new Error("denied"), { name: "NotAllowedError" })));
    Object.defineProperty(navigator, "mediaDevices", { value: { getUserMedia }, configurable: true });
    vi.stubGlobal("MediaRecorder", Object.assign(function MediaRecorder() {}, { isTypeSupported: () => true }));
    try {
      const { user, requests } = await openClinic();
      await within(todays()).findByRole("button", { name: "תמר" });
      await fromMenu(user, rowOf("תמר"), "לכתוב סיכום");
      const dialog = await screen.findByRole("dialog", { name: "סיכום מפגש אבחון" });
      await user.click(await within(dialog).findByRole("button", { name: "להקליט" }));
      expect(await within(dialog).findByRole("alert")).toHaveTextContent("אין לתוכנה רשות להשתמש במיקרופון.");
      expect(getUserMedia).toHaveBeenCalledWith({ audio: true });
      expect(requests().some((r) => r.op === "save_recording")).toBe(false);
    } finally {
      vi.unstubAllGlobals();
      Reflect.deleteProperty(navigator, "mediaDevices");
    }
  });

  it("while the computer asks for the microphone nothing starts twice, and a microphone granted after the window closed is let go", async () => {
    let grant: (s: MediaStream) => void = () => undefined;
    const getUserMedia = vi.fn(() => new Promise<MediaStream>((ok) => { grant = ok; }));
    Object.defineProperty(navigator, "mediaDevices", { value: { getUserMedia }, configurable: true });
    const made = vi.fn();
    vi.stubGlobal("MediaRecorder", Object.assign(function MediaRecorder() { made(); }, { isTypeSupported: () => true }));
    try {
      const { user } = await openClinic();
      await within(todays()).findByRole("button", { name: "תמר" });
      await fromMenu(user, rowOf("תמר"), "לכתוב סיכום");
      const dialog = await screen.findByRole("dialog", { name: "סיכום מפגש אבחון" });
      await user.click(await within(dialog).findByRole("button", { name: "להקליט" }));
      const waiting = within(dialog).getByRole("button", { name: "מחכה למיקרופון…" });
      expect(waiting).toBeDisabled();
      expect(getUserMedia).toHaveBeenCalledTimes(1);
      // Closing now asks first; she leaves anyway.
      await user.keyboard("{Escape}");
      const ask = await within(dialog).findByRole("alertdialog");
      await user.click(within(ask).getAllByRole("button").find((b) => b.textContent !== "להמשיך לערוך") as HTMLElement);
      await waitFor(() => expect(screen.queryByRole("dialog", { name: "סיכום מפגש אבחון" })).toBeNull());
      const stop = vi.fn();
      grant({ getTracks: () => [{ stop }] } as unknown as MediaStream);
      await waitFor(() => expect(stop).toHaveBeenCalled());
      expect(made).not.toHaveBeenCalled();
    } finally {
      vi.unstubAllGlobals();
      Reflect.deleteProperty(navigator, "mediaDevices");
    }
  });
});
