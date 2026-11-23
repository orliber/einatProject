// The calendar over the demo core: the three views and what each asks for, the visits on the
// grid with the colour of their kind and the legend under it, where the grid opens, the "?" of
// the bar, opening and scheduling, moving a visit (at once, with rollback), the day list's one
// step and "⋯", discreet mode and the accessibility check. Every name is invented; the clock is
// held at Sunday 15.11.2026, 08:00 unless a test says otherwise.
import { act, configure, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { useState } from "react";
import type { Route } from "../../App";
import type { ClinicRequest } from "../../ipc/generated/ClinicRequest";
import type { PatientRow } from "../../ipc/generated/PatientRow";
import type { Visit } from "../../ipc/generated/Visit";
import type { VisitView } from "../../ipc/generated/VisitView";
import tokens from "../../styles/tokens.css?raw";
import { a11yViolations } from "../../test/a11y";
import { appApi, Host, serve, type Override, type Served } from "../../test/host";
import { applyTextSize } from "../../textSize";
import calendarCss from "./CalendarTab.css?raw";
import clinicCss from "./ClinicScreen.css?raw";
import { ClinicScreen, forgetDiscreet } from "./ClinicScreen";

configure({ asyncUtilTimeout: 5000 });
vi.setConfig({ testTimeout: 30_000 });

beforeEach(() => {
  // Only the clock is held; timers run, so a toast and a refetch behave as they do for her.
  vi.useFakeTimers({ toFake: ["Date"] });
  vi.setSystemTime(new Date(2026, 10, 15, 8, 0));
});

afterEach(() => {
  vi.useRealTimers();
  forgetDiscreet();
  window.history.replaceState({}, "", "/");
});

function Clinic({ notify }: { notify: (text: string) => void }) {
  const [route, setRoute] = useState<Route>({ name: "clinic", tab: "calendar" });
  const app = appApi({ route, notify, go: setRoute });
  return <Host app={app}>{route.name === "clinic" ? <ClinicScreen tab={route.tab} patientId={route.patientId} /> : <p>מסך אחר</p>}</Host>;
}

const NOW = { date: "2026-11-15", minute: 480 };
const withClipboard: Override = (cmd) => (cmd === "copy_secret" ? 60 : undefined);
const isOp = (cmd: string, args: Record<string, unknown>, op: ClinicRequest["op"]) => cmd === "clinic" && (args.request as ClinicRequest).op === op;
const refused = (message: string) => ({ code: "refused", message, details: [], action: { kind: "dismiss" } });

/** Asks the demo clinic directly, as the window would. */
async function ask<T>(served: Served, request: ClinicRequest): Promise<T> {
  return ((await served.core.handle("clinic", { request })) as { data: T }).data;
}

/** Opens the clinic on the calendar. `prepare` changes the demo clinic first; unless `ready`
 *  is off, waits until the week is drawn. */
async function openCalendar(options: { over?: Override; prepare?: (served: Served) => Promise<void>; ready?: boolean } = {}) {
  const served = await serve(options.over ?? withClipboard);
  await options.prepare?.(served);
  served.calls.length = 0;
  const user = userEvent.setup();
  const notes: string[] = [];
  const view = render(<Clinic notify={(text) => notes.push(text)} />);
  const requests = () => served.calls.filter((c) => c.cmd === "clinic").map((c) => c.args.request as ClinicRequest);
  const asked = () => requests().filter((r) => r.op === "calendar").map((r) => (r.op === "calendar" ? `${r.from} ${r.to}` : ""));
  const saves = () => requests().flatMap((r) => (r.op === "save_visit" ? [r] : []));
  if (options.ready !== false) await screen.findByRole("group", { name: "היומן לפי שעות" });
  return { ...served, user, view, notes, requests, asked, saves };
}

const title = () => screen.getByRole("heading", { level: 2, name: /\d{4}$/ }).textContent;
const viewRadio = (name: string) => within(screen.getByRole("radiogroup", { name: "תצוגת היומן" })).getByRole("radio", { name });
const day = (name: string) => screen.getByRole("region", { name });
const SUNDAY = "יום ראשון, 15.11.2026";
const MONDAY = "יום שני, 16.11.2026";
/** The visits of a day's column, as the screen reader names them, in the order of the keyboard. */
const visitsOf = (name: string) => Array.from(day(name).querySelectorAll<HTMLElement>("[data-visit]")).map((b) => (b.textContent ?? "").replace(/\s+/g, " ").trim());
const visit = (name: RegExp) => screen.getByRole("button", { name });
const bar = () => screen.getByRole("group", { name: "הזזת הפגישה שנבחרה" });
const step = (name: string) => within(bar()).getByRole("button", { name });
const LEGEND = "מקרא: צבעי סוגי הפגישות";
const legend = () => screen.queryByRole("list", { name: LEGEND });
const MOVE_HELP = "כדי להזיז פגישה בלי לפתוח אותה, מצביעים עליה ולוחצים על החצים שבפינה שלה.";

/** The "⋯" of a visit's row in the day list. */
const moreOf = (row: HTMLElement) => within(row).getByRole("button", { name: /^עוד פעולות לפגישה של / });

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

/** The visit dialog, once its form is drawn (it shows the shape of the form while it reads). */
async function visitDialog(name: "פגישה חדשה" | "פרטי הפגישה"): Promise<HTMLElement> {
  await screen.findByLabelText("שעת התחלה");
  return screen.getByRole("dialog", { name });
}
/** The keyboard's focus on an element, as Tab would put it. */
const focus = (el: HTMLElement) => act(() => el.focus());

/** The demo's visits of the week, to change one before the calendar opens. */
async function weekVisits(served: Served): Promise<VisitView[]> {
  return ask<VisitView[]>(served, { op: "calendar", from: "2026-11-15", to: "2026-11-21" });
}
const visitOfPatient = (views: VisitView[], name: string) => views.find((v) => v.patient_name === name)?.visit as Visit;

describe("the week", () => {
  it("is the first view: her working days, the visits by day and hour, one request, and it passes the accessibility check", async () => {
    const { view, asked } = await openCalendar();
    expect(viewRadio("שבוע")).toHaveAttribute("aria-checked", "true");
    expect(title()).toBe("15 עד 21 בנובמבר 2026");
    // Sunday to Thursday, and Friday too: נועם's feedback meeting is on it.
    expect(screen.getAllByRole("region").map((r) => r.getAttribute("aria-label"))).toEqual([
      SUNDAY, MONDAY, "יום שלישי, 17.11.2026", "יום רביעי, 18.11.2026", "יום חמישי, 19.11.2026", "יום שישי, 20.11.2026",
    ]);
    expect(visitsOf(SUNDAY)).toEqual([
      "09:00–10:30 תמר, מפגש אבחון, מתוכננת", "11:30–12:30 איתי, פגישת היכרות, מתוכננת", "13:00–15:00 כתיבת דוחות, זמן שמור",
      "16:00–16:50 מאיה, פגישת טיפול, מתוכננת",
    ]);
    expect(visitsOf(MONDAY)).toEqual(["10:00–11:00 תמר, תצפית, מתוכננת"]);
    expect(visitsOf("יום שלישי, 17.11.2026")).toEqual(["08:00–11:00 השתלמות, זמן שמור"]);
    expect(visitsOf("יום רביעי, 18.11.2026")).toEqual([]);
    expect(visitsOf("יום שישי, 20.11.2026")).toEqual(["17:00–18:00 נועם, שיחת משוב, מתוכננת"]);
    // Today is marked in words too, and every day has its own "add".
    expect(within(day(SUNDAY)).getByText("היום")).toBeInTheDocument();
    expect(within(day(MONDAY)).queryByText("היום")).toBeNull();
    expect(screen.getAllByRole("button", { name: /^פגישה חדשה ביום / })).toHaveLength(6);
    // The side of a visit is the colour of its kind, and the legend under the grid says what
    // each colour means: the kinds of the week, in a fixed order, her own time last.
    expect(within(legend() as HTMLElement).getAllByRole("listitem").map((li) => li.textContent)).toEqual([
      "פגישת היכרות", "מפגש אבחון", "תצפית", "שיחת משוב", "פגישת טיפול", "זמן שמור",
    ]);
    expect(visit(/ תמר, מפגש אבחון/).closest("li")).toHaveClass("cal-kind-assessment");
    expect(visit(/^13:00–15:00 כתיבת דוחות/).closest("li")).toHaveClass("cal-kind-block");
    // The line of now is drawn only in today's column.
    expect(day(SUNDAY).querySelector(".cal-now")).not.toBeNull();
    expect(day(MONDAY).querySelector(".cal-now")).toBeNull();
    expect(asked()).toEqual(["2026-11-15 2026-11-21"]);
    expect(await a11yViolations(view.container)).toEqual([]);
  });

  it("opens the week at the start of her working day, even late in the day", async () => {
    vi.setSystemTime(new Date(2026, 10, 15, 15, 12));
    const { user } = await openCalendar();
    const grid = () => screen.getByRole("group", { name: "היומן לפי שעות" });
    // 08:00 is at the top, though now (15:12) is on this week's Sunday.
    expect(grid().scrollTop).toBe(0);
    expect(day(SUNDAY).querySelector(".cal-now")).not.toBeNull();
    // Today's day opens an hour before now: 15:12 is 403.2 pixels down a grid from 08:00.
    await user.click(viewRadio("יום"));
    expect(title()).toBe(SUNDAY);
    expect(grid().scrollTop).toBeCloseTo(347.2);
    // Another day, and another week, open at the top.
    await user.click(screen.getByRole("button", { name: "היום הבא" }));
    expect(title()).toBe(MONDAY);
    expect(grid().scrollTop).toBe(0);
    await user.click(viewRadio("שבוע"));
    await user.click(screen.getByRole("button", { name: "השבוע הבא" }));
    expect(title()).toBe("22 עד 28 בנובמבר 2026");
    await screen.findByRole("button", { name: /^16:00–16:50 מאיה, פגישת טיפול/ });
    expect(grid().scrollTop).toBe(0);
  });

  it("places a visit by its start and its length", async () => {
    await openCalendar();
    // The grid starts at 08:00, an hour is 56 pixels: 09:00 for an hour and a half.
    const tamar = visit(/ תמר, מפגש אבחון/).closest("li") as HTMLElement;
    expect(tamar.style.top).toBe("56px");
    expect(tamar.style.height).toBe("84px");
    expect(tamar.style.width).toBe("100%");
    // Time she kept for herself is its own, quieter kind.
    expect(visit(/^13:00–15:00 כתיבת דוחות/).closest("li")).toHaveClass("cal-visit-block");
    expect(screen.queryByRole("button", { name: "הזזת הפגישה של כתיבת דוחות, ראשון 13:00" })).toBeInTheDocument();
  });

  it("draws overlapping visits side by side and marks them; what happened shows in words and form", async () => {
    await openCalendar({
      prepare: async (served) => {
        const views = await weekVisits(served);
        const rows = await ask<PatientRow[]>(served, { op: "patients", today: NOW });
        const itay = rows.find((r) => r.patient.name === "איתי")?.patient.id ?? "";
        const base = visitOfPatient(views, "תמר");
        // Another visit inside תמר's; איתי's own is cancelled; מאיה's was held.
        await ask(served, { op: "save_visit", repeat: null, scope: "one", visit: { ...base, id: "", patient_id: itay, kind: "guidance", start: 570, minutes: 50, series_id: null, note: "", remember: [] } });
        await ask(served, { op: "save_visit", repeat: null, scope: "one", visit: { ...visitOfPatient(views, "איתי"), status: "cancelled" } });
        await ask(served, { op: "save_visit", repeat: null, scope: "one", visit: { ...visitOfPatient(views, "מאיה"), status: "held" } });
        await ask(served, { op: "save_visit", repeat: null, scope: "one", visit: { ...visitOfPatient(views, "נועם"), status: "no_show" } });
      },
    });
    expect(visitsOf(SUNDAY).slice(0, 2)).toEqual([
      "09:00–10:30 תמר, מפגש אבחון, מתוכננת, חופפת לפגישה אחרת", "09:30–10:20 איתי, הדרכת הורים, מתוכננת, חופפת לפגישה אחרת",
    ]);
    const first = visit(/ תמר, מפגש אבחון/).closest("li") as HTMLElement;
    const second = visit(/ איתי, הדרכת הורים/).closest("li") as HTMLElement;
    expect([first.style.width, first.style.insetInlineStart]).toEqual(["50%", "0%"]);
    expect([second.style.width, second.style.insetInlineStart]).toEqual(["50%", "50%"]);
    expect(first).toHaveClass("cal-visit-overlap");
    expect(within(first).getByText("חופפת")).toBeInTheDocument();

    const cancelled = visit(/^11:30–12:30 איתי, פגישת היכרות, בוטלה$/).closest("li") as HTMLElement;
    expect(cancelled).toHaveClass("cal-visit-cancelled");
    expect(within(cancelled).getByText("בוטלה")).toBeInTheDocument();
    const held = visit(/^16:00–16:50 מאיה, פגישת טיפול, התקיימה$/).closest("li") as HTMLElement;
    expect(held).toHaveClass("cal-visit-held");
    expect(within(held).getByText("התקיימה")).toBeInTheDocument();
    expect(visit(/^17:00–18:00 נועם, שיחת משוב, לא הגיעו$/).closest("li")).toHaveClass("cal-visit-no_show");

    // A visit that was already marked is not moved from here: the bar sends her to its details.
    expect(within(held).queryByRole("button", { name: /^הזזת הפגישה/ })).toBeNull();
    focus(visit(/ מאיה, פגישת טיפול/));
    expect(bar()).toHaveTextContent("פגישה שכבר סומנה לא מזיזים מכאן. אפשר לשנות אותה בפרטי הפגישה.");
    expect(within(bar()).queryByRole("button", { name: "לדחות ברבע שעה" })).toBeNull();
    expect(within(bar()).getByRole("button", { name: "פרטי הפגישה" })).toBeInTheDocument();
  });

  it("goes back and forth, each range asked once, and “היום” comes home", async () => {
    const { user, asked } = await openCalendar();
    await user.click(screen.getByRole("button", { name: "השבוע הבא" }));
    expect(title()).toBe("22 עד 28 בנובמבר 2026");
    expect(await screen.findByRole("button", { name: /^16:00–16:50 מאיה, פגישת טיפול/ })).toBeInTheDocument();
    expect(visitsOf("יום ראשון, 22.11.2026")).toHaveLength(1);
    await user.click(screen.getByRole("button", { name: "השבוע הקודם" }));
    await user.click(screen.getByRole("button", { name: "השבוע הקודם" }));
    expect(title()).toBe("8 עד 14 בנובמבר 2026");
    await screen.findByRole("group", { name: "היומן לפי שעות" });
    await user.click(screen.getByRole("button", { name: "היום" }));
    expect(title()).toBe("15 עד 21 בנובמבר 2026");
    expect(await screen.findByRole("button", { name: / תמר, מפגש אבחון/ })).toBeInTheDocument();
    // What was already read is not asked for again on the way back.
    expect(asked().filter((a) => a === "2026-11-22 2026-11-28")).toHaveLength(1);
    expect(asked().filter((a) => a === "2026-11-08 2026-11-14")).toHaveLength(1);
  });

  it("a press on an empty hour schedules a visit there, on the half hour", async () => {
    const { user } = await openCalendar();
    const body = day(MONDAY).querySelector(".cal-day-body") as HTMLElement;
    // 140 pixels down a grid that starts at 08:00: 10:30.
    fireEvent.click(body, { clientY: 140 });
    const dialog = await visitDialog("פגישה חדשה");
    expect(await within(dialog).findByLabelText("שעת התחלה")).toHaveValue("10:30");
    expect(within(dialog).getByLabelText("תאריך")).toHaveValue("16.11.2026");
    await user.click(within(dialog).getByRole("button", { name: "ביטול" }));
    // A press on a visit is not a press on the hour under it.
    fireEvent.click(visit(/ תמר, תצפית/), { clientY: 140 });
    expect(await visitDialog("פרטי הפגישה")).toBeInTheDocument();
  });

  it("each day's “add” schedules on that day, and the new visit is on the grid", async () => {
    const { user, saves } = await openCalendar();
    await user.click(screen.getByRole("button", { name: "פגישה חדשה ביום רביעי, 18.11.2026" }));
    const dialog = await visitDialog("פגישה חדשה");
    expect(await within(dialog).findByLabelText("תאריך")).toHaveValue("18.11.2026");
    expect(within(dialog).getByLabelText("שעת התחלה")).toHaveValue("08:00");
    await user.type(within(dialog).getByLabelText("מטופל"), "אית");
    await user.click(await within(dialog).findByRole("option", { name: "איתי" }));
    await user.click(within(dialog).getByRole("button", { name: "קביעת הפגישה" }));
    await waitFor(() => expect(screen.queryByRole("dialog")).toBeNull());
    expect(saves()[0]).toMatchObject({ visit: { id: "", date: "2026-11-18", start: 480 } });
    await waitFor(() => expect(visitsOf("יום רביעי, 18.11.2026")).toEqual(["08:00–09:30 איתי, מפגש אבחון, מתוכננת"]));
  });

  it("a press on a visit opens it", async () => {
    const { user } = await openCalendar();
    await user.click(visit(/ איתי, פגישת היכרות/));
    const dialog = await visitDialog("פרטי הפגישה");
    expect(await within(dialog).findByLabelText("שעת התחלה")).toHaveValue("11:30");
  });

  it("shows the shape of the page while it loads, and a failure offers to try again", async () => {
    let release: () => void = () => undefined;
    const wait = new Promise<void>((r) => (release = r));
    let fail = true;
    const slow: Override = async (cmd, args, core) => {
      if (!isOp(cmd, args, "calendar")) return undefined;
      await wait;
      if (fail) throw { ...refused("היומן לא נקרא."), action: { kind: "retry", after_seconds: 0 } };
      return core.handle(cmd, args);
    };
    const { user } = await openCalendar({ over: slow, ready: false });
    expect(screen.getByRole("status", { name: "טוען את היומן" })).toBeInTheDocument();
    // The bar is there from the first moment.
    expect(title()).toBe("15 עד 21 בנובמבר 2026");
    release();
    expect(await screen.findByText("היומן לא נקרא.")).toBeInTheDocument();
    fail = false;
    await user.click(screen.getByRole("button", { name: "לנסות שוב" }));
    expect(await screen.findByRole("button", { name: / תמר, מפגש אבחון/ })).toBeInTheDocument();
  });

  it("an empty week says so and how to start, and still shows her working days", async () => {
    window.history.replaceState({}, "", "/?clinic=empty");
    const { view, user } = await openCalendar();
    // Under the grid, in the place of the legend, which has nothing to explain.
    expect(screen.getByText("אין פגישות בשבוע הזה. כדי לקבוע פגישה לוחצים על שעה ביומן.").closest(".cal-foot")).not.toBeNull();
    expect(legend()).toBeNull();
    expect(screen.getAllByRole("region")).toHaveLength(5);
    expect(await a11yViolations(view.container)).toEqual([]);
    await user.click(viewRadio("יום"));
    expect(screen.getByRole("heading", { name: "אין פגישות ביום הזה" })).toBeInTheDocument();
    expect(screen.getByText("אין פגישות ביום הזה. כדי לקבוע פגישה לוחצים על שעה ביומן.").closest(".cal-foot")).not.toBeNull();
    expect(legend()).toBeNull();
    await user.click(viewRadio("חודש"));
    expect((await screen.findByText("אין פגישות בחודש הזה. לחיצה על יום פותחת אותו, ושם אפשר לקבוע פגישה.")).closest(".cal-foot")).not.toBeNull();
    expect(legend()).toBeNull();
    // How to open a day is behind the month's "?".
    const help = screen.getByRole("button", { name: "איך פותחים יום" });
    expect(screen.queryByRole("button", { name: "איך מזיזים פגישה" })).toBeNull();
    await user.click(help);
    expect(help).toHaveAttribute("aria-expanded", "true");
    expect(screen.getByText("לחיצה על יום פותחת אותו, ושם אפשר לקבוע פגישה.")).toBeInTheDocument();
  });
});

describe("moving a visit", () => {
  it("the arrows at its corner choose it; a step shows at once, before the vault answered", async () => {
    let release: () => void = () => undefined;
    const wait = new Promise<void>((r) => (release = r));
    const slow: Override = async (cmd, args, core) => {
      if (cmd === "copy_secret") return 60;
      if (!isOp(cmd, args, "save_visit")) return undefined;
      await wait;
      return core.handle(cmd, args);
    };
    const { user, saves, view } = await openCalendar({ over: slow });
    // How to move a visit is behind the bar's "?", closed until she asks; no bar until a visit is chosen.
    const help = screen.getByRole("button", { name: "איך מזיזים פגישה" });
    expect(help).toHaveAttribute("aria-expanded", "false");
    expect(screen.queryByText(MOVE_HELP)).toBeNull();
    expect(screen.queryByRole("group", { name: "הזזת הפגישה שנבחרה" })).toBeNull();
    await user.click(help);
    expect(help).toHaveAttribute("aria-expanded", "true");
    expect(screen.getByText(MOVE_HELP)).toBeInTheDocument();
    expect(screen.getByText("מהמקלדת: עומדים על הפגישה ולוחצים Alt עם החצים.")).toBeInTheDocument();
    expect(screen.getByText("כדי לקבוע פגישה לוחצים על שעה ביומן.")).toBeInTheDocument();
    expect(await a11yViolations(view.container)).toEqual([]);
    await user.keyboard("{Escape}");
    expect(help).toHaveAttribute("aria-expanded", "false");
    expect(help).toHaveFocus();
    expect(screen.queryByText(MOVE_HELP)).toBeNull();
    expect(screen.queryByRole("group", { name: "הזזת הפגישה שנבחרה" })).toBeNull();

    await user.click(screen.getByRole("button", { name: "הזזת הפגישה של תמר, ראשון 09:00" }));
    expect(screen.getByRole("button", { name: "הזזת הפגישה של תמר, ראשון 09:00" })).toHaveAttribute("aria-pressed", "true");
    expect(within(bar()).getByRole("status")).toHaveTextContent("תמריום ראשון, 15.11.202609:00–10:30");
    expect(await a11yViolations(view.container)).toEqual([]);

    await user.click(step("לדחות ברבע שעה"));
    expect(await screen.findByRole("button", { name: "09:15–10:45 תמר, מפגש אבחון, מתוכננת" })).toBeInTheDocument();
    expect(within(bar()).getByRole("status")).toHaveTextContent("09:15–10:45");
    expect(saves()).toHaveLength(1);
    release();
    await waitFor(() => expect(saves()[0]).toMatchObject({ scope: "one", repeat: null, visit: { date: "2026-11-15", start: 555, minutes: 90, status: "planned" } }));

    // To the next day: it is drawn in Monday's column, and the vault has it.
    await user.click(step("ליום הבא"));
    await waitFor(() => expect(visitsOf(MONDAY)).toEqual(["09:15–10:45 תמר, מפגש אבחון, מתוכננת, חופפת לפגישה אחרת", "10:00–11:00 תמר, תצפית, מתוכננת, חופפת לפגישה אחרת"]));
    await waitFor(() => expect(saves()[1]).toMatchObject({ visit: { date: "2026-11-16", start: 555 } }));
    expect(within(bar()).getByText("חופפת לפגישה אחרת")).toBeInTheDocument();
    // And back, earlier: still the same visit under her hand.
    await user.click(step("ליום הקודם"));
    await user.click(step("להקדים ברבע שעה"));
    await waitFor(() => expect(visitsOf(SUNDAY)[0]).toBe("09:00–10:30 תמר, מפגש אבחון, מתוכננת"));
    await waitFor(() => expect(saves()).toHaveLength(4));
    // The bar sat under the grid, where the legend comes back once she is done.
    expect(bar().closest(".cal-foot")).not.toBeNull();
    expect(legend()).toBeNull();
    await user.click(step("סיום"));
    expect(screen.queryByRole("group", { name: "הזזת הפגישה שנבחרה" })).toBeNull();
    expect(legend()?.closest(".cal-foot")).not.toBeNull();
  });

  it("a move the vault refuses goes back, and says why", async () => {
    const refuse: Override = (cmd, args) => {
      if (isOp(cmd, args, "save_visit")) throw refused("הפגישה לא נשמרה.");
      return undefined;
    };
    const { user, notes } = await openCalendar({ over: refuse });
    await user.click(screen.getByRole("button", { name: "הזזת הפגישה של איתי, ראשון 11:30" }));
    await user.click(step("ליום הבא"));
    await waitFor(() => expect(notes).toContain("הפגישה לא נשמרה."));
    await waitFor(() => expect(visitsOf(SUNDAY)[1]).toBe("11:30–12:30 איתי, פגישת היכרות, מתוכננת"));
    expect(visitsOf(MONDAY)).toEqual(["10:00–11:00 תמר, תצפית, מתוכננת"]);
    // She can try again: the visit is still the chosen one, where it was.
    expect(within(bar()).getByRole("status")).toHaveTextContent("יום ראשון, 15.11.202611:30–12:30");
  });

  it("a visit of a series asks once what the move applies to", async () => {
    const { user, saves } = await openCalendar();
    await user.click(screen.getByRole("button", { name: "הזזת הפגישה של מאיה, ראשון 16:00" }));
    await user.click(step("ליום הבא"));
    const dialog = await screen.findByRole("dialog", { name: "פגישה מסדרה חוזרת" });
    expect(dialog).toHaveTextContent("הפגישה הזו חלק מסדרה חוזרת. מה להזיז?");
    expect(within(dialog).getByRole("button", { name: "רק הפגישה הזו" })).toHaveFocus();
    expect(await a11yViolations(dialog)).toEqual([]);
    // Nothing moved until she answered.
    expect(saves()).toHaveLength(0);
    await user.click(within(dialog).getByRole("button", { name: "זו וכל הבאות" }));
    await waitFor(() => expect(saves()[0]).toMatchObject({ scope: "following", visit: { date: "2026-11-16", start: 960 } }));
    expect(bar()).toHaveTextContent("ההזזה חלה על: זו וכל הבאות");

    // The next step does not ask again.
    await user.click(step("לדחות ברבע שעה"));
    await waitFor(() => expect(saves()[1]).toMatchObject({ scope: "following", visit: { date: "2026-11-16", start: 975 } }));
    expect(screen.queryByRole("dialog")).toBeNull();
    // She can change her mind for the steps that follow.
    await user.click(within(bar()).getByRole("button", { name: "שינוי" }));
    await user.click(within(await screen.findByRole("dialog", { name: "פגישה מסדרה חוזרת" })).getByRole("button", { name: "רק הפגישה הזו" }));
    expect(bar()).toHaveTextContent("ההזזה חלה על: רק הפגישה הזו");
    expect(saves()).toHaveLength(2);

    // The following week's visit moved with it.
    await user.click(screen.getByRole("button", { name: "השבוע הבא" }));
    await waitFor(() => expect(visitsOf("יום שני, 23.11.2026")).toEqual(["16:15–17:05 מאיה, פגישת טיפול, מתוכננת"]));
  });

  it("cancelling the question moves nothing", async () => {
    const { user, saves } = await openCalendar();
    await user.click(screen.getByRole("button", { name: "הזזת הפגישה של מאיה, ראשון 16:00" }));
    await user.click(step("להקדים ברבע שעה"));
    await user.click(within(await screen.findByRole("dialog", { name: "פגישה מסדרה חוזרת" })).getByRole("button", { name: "ביטול" }));
    expect(saves()).toHaveLength(0);
    expect(visitsOf(SUNDAY)[3]).toBe("16:00–16:50 מאיה, פגישת טיפול, מתוכננת");
  });

  it("from the keyboard: the visit under the focus is the chosen one, Alt and the arrows move it, and the focus stays on it", async () => {
    const { user, saves } = await openCalendar();
    const tamar = () => visit(/ תמר, מפגש אבחון/);
    focus(tamar());
    expect(within(bar()).getByRole("status")).toHaveTextContent("תמר");
    await user.keyboard("{Alt>}{ArrowDown}{/Alt}");
    await waitFor(() => expect(tamar()).toHaveAccessibleName("09:15–10:45 תמר, מפגש אבחון, מתוכננת"));
    expect(tamar()).toHaveFocus();
    // Left, in a right-to-left week, is the next day: another column, the same focus.
    await user.keyboard("{Alt>}{ArrowLeft}{/Alt}");
    await waitFor(() => expect(visitsOf(MONDAY)[0]).toBe("09:15–10:45 תמר, מפגש אבחון, מתוכננת, חופפת לפגישה אחרת"));
    await waitFor(() => expect(tamar()).toHaveFocus());
    await user.keyboard("{Alt>}{ArrowRight}{ArrowUp}{/Alt}");
    await waitFor(() => expect(visitsOf(SUNDAY)[0]).toBe("09:00–10:30 תמר, מפגש אבחון, מתוכננת"));
    await waitFor(() => expect(tamar()).toHaveFocus());
    await waitFor(() => expect(saves().map((r) => `${r.visit.date} ${r.visit.start}`)).toEqual(["2026-11-15 555", "2026-11-16 555", "2026-11-15 555", "2026-11-15 540"]));
    // An arrow without Alt moves nothing; Escape lets the visit go.
    await user.keyboard("{ArrowDown}");
    expect(saves()).toHaveLength(4);
    await user.keyboard("{Escape}");
    expect(screen.queryByRole("group", { name: "הזזת הפגישה שנבחרה" })).toBeNull();
  });

  it("a visit never leaves its day by a step in time", async () => {
    const { user } = await openCalendar({
      prepare: async (served) => {
        const base = visitOfPatient(await weekVisits(served), "איתי");
        await ask(served, { op: "save_visit", repeat: null, scope: "one", visit: { ...base, start: 0, minutes: 30 } });
      },
    });
    // A visit before her hours is still on the grid, which starts earlier for it.
    await user.click(screen.getByRole("button", { name: "הזזת הפגישה של איתי, ראשון 00:00" }));
    expect(visitsOf(SUNDAY)[0]).toBe("00:00–00:30 איתי, פגישת היכרות, מתוכננת");
    expect(step("להקדים ברבע שעה")).toBeDisabled();
    expect(step("לדחות ברבע שעה")).toBeEnabled();
  });
});

describe("the day", () => {
  it("shows one day's grid beside its list, without asking again, and the next day is one press away", async () => {
    const { user, asked, view } = await openCalendar();
    // (jsdom applies no stylesheet: the one letter of a narrow column is in the name here.)
    await user.click(screen.getByRole("button", { name: /^יום שני (ב׳ )?16\.11\.2026, לתצוגת היום$/ }));
    expect(viewRadio("יום")).toHaveAttribute("aria-checked", "true");
    expect(title()).toBe(MONDAY);
    expect(screen.getAllByRole("region").map((r) => r.getAttribute("aria-label"))).toEqual([MONDAY, null]);
    const agenda = screen.getByRole("region", { name: "סדר היום" });
    expect(within(agenda).getByRole("button", { name: "תמר" })).toBeInTheDocument();
    expect(within(agenda).getByText("תצפית")).toBeInTheDocument();
    expect(await a11yViolations(view.container)).toEqual([]);
    await user.click(screen.getByRole("button", { name: "היום הקודם" }));
    expect(title()).toBe(SUNDAY);
    expect(within(screen.getByRole("region", { name: "סדר היום" })).getAllByRole("listitem")).toHaveLength(4);
    expect(visitsOf(SUNDAY)).toHaveLength(4);
    // The week's answer serves each of its days.
    expect(asked()).toEqual(["2026-11-15 2026-11-21"]);
    await user.click(screen.getByRole("button", { name: "היום הקודם" }));
    expect(title()).toBe("שבת, 14.11.2026");
    await waitFor(() => expect(asked()).toEqual(["2026-11-15 2026-11-21", "2026-11-08 2026-11-14"]));
  });

  const openDay = async (over?: Override) => {
    const opened = await openCalendar(over ? { over } : {});
    await opened.user.click(viewRadio("יום"));
    const agenda = () => screen.getByRole("region", { name: "סדר היום" });
    const row = (name: string) => within(agenda()).getByRole("button", { name }).closest("li") as HTMLElement;
    return { ...opened, agenda, row };
  };

  it("“התקיימה” marks the visit at once, on the list and on the grid", async () => {
    const { user, row, saves } = await openDay();
    expect(within(row("תמר")).getByText("מתוכננת")).toBeInTheDocument();
    // At 08:00 her visit has not begun: its one step is "תזכורת", and "התקיימה" is under "⋯".
    expect(within(row("תמר")).queryByRole("button", { name: "התקיימה" })).toBeNull();
    expect(within(row("תמר")).getByRole("button", { name: "תזכורת" })).toBeInTheDocument();
    expect(await menuOf(user, row("תמר"))).toEqual(["התקיימה", "לכתוב סיכום", "לפתוח את הפגישה"]);
    await fromMenu(user, row("תמר"), "התקיימה");
    expect(within(row("תמר")).getByText("התקיימה")).toBeInTheDocument();
    expect(await screen.findByRole("button", { name: "09:00–10:30 תמר, מפגש אבחון, התקיימה" })).toBeInTheDocument();
    await waitFor(() => expect(saves()[0]).toMatchObject({ scope: "one", visit: { status: "held" } }));
    // Held without a note: its one step is "לכתוב סיכום", and nothing is left to mark.
    expect(within(row("תמר")).getByRole("button", { name: "לכתוב סיכום" })).toBeInTheDocument();
    expect(await menuOf(user, row("תמר"))).toEqual(["לרשום תקבול", "לפתוח את הפגישה"]);
    // Her own time has nothing to mark, and no "⋯".
    const own = within(screen.getByRole("region", { name: "סדר היום" })).getAllByRole("listitem")[2] as HTMLElement;
    expect(own).toHaveTextContent("כתיבת דוחות");
    expect(within(own).queryByRole("button", { name: "התקיימה" })).toBeNull();
    expect(within(own).queryByRole("button", { name: /^עוד פעולות/ })).toBeNull();
  });

  it("a visit that has begun offers “התקיימה” as its one step, and the focus stays on it as it becomes “לכתוב סיכום”", async () => {
    vi.setSystemTime(new Date(2026, 10, 15, 10, 45));
    const { user, row, saves } = await openDay();
    // תמר's visit (09:00–10:30) is over: "התקיימה" is the main button, and not in "⋯" again.
    const held = within(row("תמר")).getByRole("button", { name: "התקיימה" });
    expect(held).toHaveClass("btn-primary");
    expect(within(row("תמר")).queryByRole("button", { name: "תזכורת" })).toBeNull();
    // A reminder no longer makes sense once it has begun: none in "⋯" either (D-204 §4).
    expect(await menuOf(user, row("תמר"))).toEqual(["לכתוב סיכום", "להקליט לעצמך", "לפתוח את הפגישה"]);
    await user.click(held);
    await waitFor(() => expect(saves()[0]).toMatchObject({ visit: { status: "held" } }));
    await waitFor(() => expect(within(row("תמר")).getByRole("button", { name: "לכתוב סיכום" })).toHaveFocus());
    await user.keyboard("{Enter}");
    expect(await screen.findByRole("dialog", { name: "סיכום מפגש אבחון" })).toBeInTheDocument();
  });

  it("a visit of a day still ahead has nothing to mark yet", async () => {
    const { user, row } = await openDay();
    await user.click(screen.getByRole("button", { name: "היום הבא" }));
    await waitFor(() => expect(within(screen.getByRole("region", { name: "סדר היום" })).getByRole("button", { name: "תמר" })).toBeInTheDocument());
    expect(within(row("תמר")).queryByRole("button", { name: "התקיימה" })).toBeNull();
    expect(within(row("תמר")).getByRole("button", { name: "תזכורת" })).toBeInTheDocument();
    expect(await menuOf(user, row("תמר"))).toEqual(["לכתוב סיכום", "לפתוח את הפגישה"]);
  });

  it("a mark the vault refuses rolls back, and says why", async () => {
    const refuse: Override = (cmd, args) => {
      if (isOp(cmd, args, "save_visit")) throw refused("הפגישה לא נשמרה.");
      return undefined;
    };
    const { user, row, notes } = await openDay(refuse);
    await fromMenu(user, row("איתי"), "התקיימה");
    await waitFor(() => expect(notes).toContain("הפגישה לא נשמרה."));
    await waitFor(() => expect(within(row("איתי")).getByText("מתוכננת")).toBeInTheDocument());
    expect(await menuOf(user, row("איתי"))).toEqual(["התקיימה", "לכתוב סיכום", "לפתוח את הפגישה"]);
  });

  it("“לכתוב סיכום” opens the visit's note, and “תזכורת” copies through the protected clipboard only", async () => {
    const { user, row, calls, notes } = await openDay();
    await user.click(within(row("תמר")).getByRole("button", { name: "תזכורת" }));
    await user.click(await screen.findByRole("menuitem", { name: "רק להעתיק" }));
    await waitFor(() => expect(notes).toEqual(["התזכורת הועתקה ללוח המוגן, ואפשר להדביק אותה בהודעה. יימחק מהלוח בעוד דקה."]));
    const copied = calls.filter((c) => c.cmd === "copy_secret");
    expect(copied).toHaveLength(1);
    expect(String(copied[0]?.args.text)).toContain("15.11.2026, בשעה 09:00");
    await fromMenu(user, row("תמר"), "לכתוב סיכום");
    expect(await screen.findByRole("dialog", { name: "סיכום מפגש אבחון" })).toBeInTheDocument();
  });

  it("the list opens a visit, and “פגישה חדשה” is for the day shown", async () => {
    const { user, row } = await openDay();
    await user.click(within(row("איתי")).getByRole("button", { name: /^11:30–12:30, פתיחת הפגישה של איתי/ }));
    const details = await visitDialog("פרטי הפגישה");
    await user.click(within(details).getByRole("button", { name: "ביטול" }));
    await user.click(screen.getByRole("button", { name: "היום הבא" }));
    await user.click(screen.getByRole("button", { name: "פגישה חדשה" }));
    const dialog = await visitDialog("פגישה חדשה");
    expect(await within(dialog).findByLabelText("תאריך")).toHaveValue("16.11.2026");
  });

  it("moving a visit to the next day takes the page along with it", async () => {
    const { user } = await openDay();
    await user.click(screen.getByRole("button", { name: "הזזת הפגישה של איתי, ראשון 11:30" }));
    await user.click(step("ליום הבא"));
    expect(title()).toBe(MONDAY);
    await waitFor(() => expect(visitsOf(MONDAY)).toEqual(["10:00–11:00 תמר, תצפית, מתוכננת", "11:30–12:30 איתי, פגישת היכרות, מתוכננת"]));
    expect(within(bar()).getByRole("status")).toHaveTextContent("יום שני, 16.11.202611:30–12:30");
  });
});

describe("the month", () => {
  it("is a table of weeks from Sunday, three visits a day and the rest counted, in one request", async () => {
    const { user, asked, view } = await openCalendar();
    await user.click(viewRadio("חודש"));
    expect(title()).toBe("נובמבר 2026");
    const table = await screen.findByRole("table", { name: "נובמבר 2026" });
    expect(within(table).getAllByRole("columnheader").map((h) => h.textContent)).toEqual(["ראשון", "שני", "שלישי", "רביעי", "חמישי", "שישי", "שבת"]);
    // Five weeks: 1.11 is a Sunday, and the last week reaches into December.
    expect(within(table).getAllByRole("row")).toHaveLength(6);
    expect(within(table).getAllByRole("cell")).toHaveLength(35);
    expect(asked()).toEqual(["2026-11-15 2026-11-21", "2026-11-01 2026-12-05"]);

    const today = within(table).getByRole("button", { name: `${SUNDAY}, 4 פגישות` }).closest("td") as HTMLElement;
    expect(within(today).getAllByRole("listitem").map((li) => li.textContent)).toEqual(["09:00תמר", "11:30איתי", "13:00כתיבת דוחות"]);
    expect(today).toHaveTextContent("+1 פגישות נוספות");
    expect(within(today).getByText("היום")).toBeInTheDocument();
    expect(within(table).getByRole("button", { name: "יום רביעי, 18.11.2026, אין פגישות" })).toBeInTheDocument();
    expect(within(table).getByRole("button", { name: "יום שני, 16.11.2026, פגישה אחת" })).toBeInTheDocument();
    // A visit that was held says so to a screen reader too.
    const held = within(table).getByRole("button", { name: "יום ראשון, 8.11.2026, פגישה אחת" }).closest("td") as HTMLElement;
    expect(within(held).getByRole("listitem")).toHaveTextContent("16:00מאיה, התקיימה");
    expect(await a11yViolations(view.container)).toEqual([]);

    await user.click(screen.getByRole("button", { name: "החודש הבא" }));
    expect(title()).toBe("דצמבר 2026");
    await screen.findByRole("table", { name: "דצמבר 2026" });
    expect(asked()[2]).toBe("2026-11-29 2027-01-02");
    await user.click(screen.getByRole("button", { name: "החודש הקודם" }));
    expect(title()).toBe("נובמבר 2026");
  });

  it("a press on a day, on its date or anywhere in it, opens that day", async () => {
    const { user } = await openCalendar();
    await user.click(viewRadio("חודש"));
    const table = await screen.findByRole("table", { name: "נובמבר 2026" });
    await user.click(within(table).getByRole("button", { name: "יום שישי, 20.11.2026, פגישה אחת" }));
    expect(viewRadio("יום")).toHaveAttribute("aria-checked", "true");
    expect(title()).toBe("יום שישי, 20.11.2026");
    expect(await screen.findByRole("button", { name: "17:00–18:00 נועם, שיחת משוב, מתוכננת" })).toBeInTheDocument();

    await user.click(viewRadio("חודש"));
    const again = await screen.findByRole("table", { name: "נובמבר 2026" });
    await user.click(within(again).getByRole("button", { name: /^יום שלישי, 17\.11\.2026/ }).closest("td") as HTMLElement);
    expect(title()).toBe("יום שלישי, 17.11.2026");
  });
});

describe("discreet mode on the calendar", () => {
  it("shows initials in place of every name: on the grid, in the bar, in the list and in the month", async () => {
    const { user } = await openCalendar();
    await user.click(screen.getByRole("switch", { name: "מצב דיסקרטי" }));
    const panel = screen.getByRole("tabpanel");
    const hidden = () => {
      for (const name of ["תמר", "איתי", "מאיה", "נועם"]) {
        expect(within(panel).queryByText(name), name).toBeNull();
        expect(panel.querySelector(`[aria-label*="${name}"]`), name).toBeNull();
        expect(panel.textContent, name).not.toContain(name);
      }
    };
    expect(visitsOf(SUNDAY)[0]).toBe("09:00–10:30 ת., מפגש אבחון, מתוכננת");
    // Her own titles are not names, and stay.
    expect(visitsOf(SUNDAY)[2]).toBe("13:00–15:00 כתיבת דוחות, זמן שמור");
    hidden();
    // The "?" holds fixed words only.
    await user.click(screen.getByRole("button", { name: "איך מזיזים פגישה" }));
    expect(screen.getByText(MOVE_HELP)).toBeInTheDocument();
    hidden();
    await user.click(screen.getByRole("button", { name: "הזזת הפגישה של ת., ראשון 09:00" }));
    expect(within(bar()).getByRole("status")).toHaveTextContent("ת.יום ראשון, 15.11.2026");
    hidden();
    await user.click(viewRadio("יום"));
    expect(within(screen.getByRole("region", { name: "סדר היום" })).getByRole("button", { name: "ת." })).toBeInTheDocument();
    expect(within(screen.getByRole("region", { name: "סדר היום" })).getByRole("button", { name: "עוד פעולות לפגישה של ת., 09:00" })).toBeInTheDocument();
    hidden();
    await user.click(viewRadio("חודש"));
    const table = await screen.findByRole("table", { name: "נובמבר 2026" });
    expect(within(within(table).getByRole("button", { name: `${SUNDAY}, 4 פגישות` }).closest("td") as HTMLElement).getAllByRole("listitem")[0]).toHaveTextContent("09:00ת.");
    hidden();
  });
});

describe("the bar and the height of the calendar", () => {
  /** The top-level rules of CalendarTab.css for a selector, each from its selector to its closing brace. */
  const rule = (selector: string) => {
    const found = calendarCss.split(`\n${selector} {`).slice(1).map((after) => after.slice(0, after.indexOf("}")));
    expect(found.length, selector).toBeGreaterThan(0);
    return found.join("\n");
  };

  it("keeps the “?” beside “פגישה חדשה” in every view: one group at the end of the bar, so they fold together", async () => {
    const { user } = await openCalendar();
    for (const [name, help] of [["שבוע", "איך מזיזים פגישה"], ["יום", "איך מזיזים פגישה"], ["חודש", "איך פותחים יום"]] as const) {
      await user.click(viewRadio(name));
      const add = screen.getByRole("button", { name: "פגישה חדשה" });
      const bar = add.closest("header") as HTMLElement;
      const group = add.parentElement as HTMLElement;
      // Not loose in the bar, where a narrow window would send one of them to the next line alone.
      expect(group, name).not.toBe(bar);
      expect(within(group).getAllByRole("button"), name).toEqual([screen.getByRole("button", { name: help }), add]);
      expect(bar.lastElementChild, name).toBe(group);
    }
  });

  it("a day and a week fill the page under the tabs; the month takes the height it needs", async () => {
    const { user } = await openCalendar();
    const cal = () => screen.getByRole("tabpanel").firstElementChild as HTMLElement;
    expect(cal()).toHaveClass("cal", "cal-fill");
    await user.click(viewRadio("יום"));
    expect(cal()).toHaveClass("cal", "cal-fill");
    await user.click(viewRadio("חודש"));
    await screen.findByRole("table", { name: "נובמבר 2026" });
    expect(cal()).toHaveClass("cal");
    expect(cal()).not.toHaveClass("cal-fill");
    // By flex: the grid takes what the bar and the legend under it leave. Its floor, for a short
    // window, starts it at what is left under the tabs, between 300px and 420px (404px on a
    // laptop at 125%, as before the page was filled); its hours never push the page further.
    expect(calendarCss).toContain(".clinic-panel:has(> .cal-fill),\n.cal-fill,\n.cal-fill > .cal-stage,\n.cal-fill > .cal-day-view {\n  flex-grow: 1;");
    expect(rule(".cal-grid")).toContain("flex: 1 0 clamp(300px, calc(100vh / var(--zoom, 1) - 210px), 420px);");
    expect(rule(".cal-grid")).toContain("max-height: max-content;");
    expect(rule(".cal-grid")).not.toContain("min-height");
    // The window's height only after the text size, as the dialogs take it: at 125% a bare
    // 100vh is 25% taller than the window.
    const code = calendarCss.replace(/\/\*[\s\S]*?\*\//g, "");
    expect(code).toContain("100vh / var(--zoom, 1)");
    expect(code.match(/\d+(vh|dvh|svh|lvh)\b(?! \/ var\(--zoom, 1\))/g)).toBeNull();
  });

  it("in a week the chosen visit's bar lies over the line under the grid and goes down from it, never over the grid", async () => {
    const { user } = await openCalendar();
    const foot = () => (legend() as HTMLElement).closest(".cal-foot") as HTMLElement;
    // No band kept empty for a bar that is not there (it was 54px to 173px, under a 20px legend).
    expect(rule(".cal-foot")).not.toContain("min-height");
    expect(calendarCss).not.toContain("@container cal-stage");
    expect(foot().firstElementChild).toBe(legend());
    // Chosen: the bar is out of the flow, from the top of the line down, and the legend stays
    // under it, unseen and unread, to keep the line's height.
    await user.click(screen.getByRole("button", { name: "הזזת הפגישה של תמר, ראשון 09:00" }));
    const line = bar().closest(".cal-foot") as HTMLElement;
    const under = line.firstElementChild as HTMLElement;
    expect(under).toHaveClass("cal-foot-under");
    expect(under).toHaveAttribute("aria-hidden", "true");
    expect(within(under).getAllByRole("listitem", { hidden: true }).map((li) => li.textContent)).toEqual([
      "פגישת היכרות", "מפגש אבחון", "תצפית", "שיחת משוב", "פגישת טיפול", "זמן שמור",
    ]);
    // The bar is alone in its rail, a plain box (no role), after the grid as the keyboard goes.
    const rail = under.nextElementSibling as HTMLElement;
    expect(rail).toHaveClass("cal-move-rail");
    expect(rail).not.toHaveAttribute("role");
    expect(Array.from(rail.children)).toEqual([bar()]);
    expect(screen.getByRole("group", { name: "היומן לפי שעות" }).compareDocumentPosition(bar()) & Node.DOCUMENT_POSITION_FOLLOWING).toBeTruthy();
    expect(legend()).toBeNull();
    expect(rule(".cal-foot")).toContain("position: relative;");
    expect(rule(".cal-foot-under")).toContain("visibility: hidden;");
    // The rail starts at the line's top and goes down from it: never over the grid, so the bar
    // never comes up under the pointer that chose the visit, nor over the visit being moved (a
    // bar that rose over the grid caught a second click and moved the visit). A folded bar
    // reaches past the page's margin, and the page can be scrolled to it.
    expect(rule(".cal-move-rail")).toMatch(/^\n\s*position: absolute;\n\s*z-index: 6;\n\s*inset-inline: 0;\n\s*inset-block-start: 0;\n$/);
    expect(calendarCss).not.toMatch(/\.cal-move-rail[^{]*\{[^}]*(sticky|-200px|inset-block-end)/);
    expect(calendarCss).not.toContain("--cal-foot-room");
    // What the bar is about is first for a screen reader, but after the buttons on the screen:
    // it changes as she moves the visit ("חופפת לפגישה אחרת" comes and goes, the day's name
    // changes), and a button that moved with it caught a second click on "לדחות ברבע שעה" as
    // "להקדים ברבע שעה". Nothing before the buttons changes size, so none moves under the pointer.
    const what = bar().firstElementChild as HTMLElement;
    expect(what).toHaveClass("cal-move-what");
    expect(what).toHaveAttribute("role", "status");
    expect(rule(".cal-move-what")).toContain("order: 1;");
    // Kept at its tallest while a visit is chosen, its lines stay at its top.
    expect(rule(".cal-move")).toContain("align-content: flex-start;");
    expect(rule(".cal-move-what")).toContain("margin-inline-start: auto;");
    expect(rule(".cal-move-what")).not.toContain("margin-inline-end");
    expect(calendarCss).not.toMatch(/\.cal-move[^{]*\{[^}]*\border: -/);
    await user.click(step("סיום"));
    expect(foot().firstElementChild).toBe(legend());
    expect(document.querySelector(".cal-foot-under")).toBeNull();
    expect(document.querySelector(".cal-move-rail")).toBeNull();
  });

  it("in a day the chosen visit's bar is on top of the day's list and pushes it down, and the grid keeps its height", async () => {
    // The window's ResizeObserver (jsdom has none), and a bar 98px high, as it folds beside the grid.
    const watchers: (() => void)[] = [];
    const watching: { by: object; el: Element }[] = [];
    vi.stubGlobal("ResizeObserver", class {
      constructor(told: () => void) {
        watchers.push(told);
      }
      observe(el: Element) {
        watching.push({ by: this, el });
      }
      unobserve() {}
      disconnect() {
        for (let i = watching.length - 1; i >= 0; i--) if (watching[i]?.by === this) watching.splice(i, 1);
      }
    });
    const watched = () => watching.map((w) => w.el);
    // The bar's height as it folds; like a browser, never under the height it is kept at.
    let folded = 98;
    const height = vi.spyOn(HTMLElement.prototype, "offsetHeight", "get").mockImplementation(function (this: HTMLElement) {
      if (this.classList.contains("cal-day-side")) return 400;
      return this.classList.contains("cal-move") ? Math.max(folded, parseFloat(this.style.minBlockSize) || 0) : 0;
    });
    try {
      const { user, view } = await openCalendar();
      await user.click(viewRadio("יום"));
      const list = () => screen.getByRole("region", { name: "סדר היום" });
      const side = () => list().parentElement as HTMLElement;
      expect(side()).toHaveClass("cal-day-side");
      expect(side().style.getPropertyValue("--cal-move-h")).toBe("");
      await user.click(screen.getByRole("button", { name: "הזזת הפגישה של תמר, ראשון 09:00" }));
      // On top of the list, which it pushes down; under the grid the legend is seen as before.
      const rail = side().firstElementChild as HTMLElement;
      expect(rail).toHaveClass("cal-day-rail");
      expect(Array.from(rail.children)).toEqual([bar()]);
      expect(Array.from(side().children)).toEqual([rail, list()]);
      expect(legend()?.closest(".cal-foot")).not.toBeNull();
      expect(document.querySelector(".cal-foot-under, .cal-move-rail")).toBeNull();
      // As the keyboard goes: the grid's visits, the bar, the list. A plain box, no region.
      expect(screen.getByRole("group", { name: "היומן לפי שעות" }).compareDocumentPosition(bar()) & Node.DOCUMENT_POSITION_FOLLOWING).toBeTruthy();
      expect(bar().compareDocumentPosition(list()) & Node.DOCUMENT_POSITION_FOLLOWING).toBeTruthy();
      expect(screen.getAllByRole("region")).toEqual([day(SUNDAY), list()]);
      expect(await a11yViolations(view.container)).toEqual([]);
      // Its height is measured as it folds, for the room the column gives back at its end.
      expect(side().style.getPropertyValue("--cal-move-h")).toBe("98px");
      expect(new Set(watched())).toEqual(new Set([bar(), side()]));
      folded = 142;
      act(() => watchers.forEach((w) => w()));
      expect(side().style.getPropertyValue("--cal-move-h")).toBe("142px");
      // A step that takes a line out of the bar ("חופפת לפגישה אחרת" going) does not make it,
      // or the list's room, shorter while the visit is chosen: on a page scrolled to its end the
      // browser would pull the page down, and the button under the pointer away from it.
      expect(bar().style.minBlockSize).toBe("142px");
      folded = 60;
      act(() => watchers.forEach((w) => w()));
      expect(bar().style.minBlockSize).toBe("142px");
      expect(side().style.getPropertyValue("--cal-move-h")).toBe("142px");
      // A resized window lets go, and the bar is measured again once the page has settled (two
      // frames, after the bar's own height was taken anew): a box measured with the old height
      // in between is not kept.
      const frame = () => new Promise<void>((done) => requestAnimationFrame(() => done()));
      const settled = async () => {
        await frame();
        await frame();
      };
      act(() => {
        window.dispatchEvent(new Event("resize"));
      });
      expect(bar().style.minBlockSize).toBe("");
      expect(side().style.minBlockSize).toBe("");
      act(() => watchers.forEach((w) => w()));
      expect(bar().style.minBlockSize).toBe("");
      await act(frame);
      expect(bar().style.minBlockSize).toBe("");
      await act(frame);
      expect(bar().style.minBlockSize).toBe("60px");
      // So does a change of the text size (the window is not resized then), and only a change: a
      // key at the end of the list keeps the height.
      folded = 120;
      act(() => watchers.forEach((w) => w()));
      expect(bar().style.minBlockSize).toBe("120px");
      folded = 75;
      act(() => applyTextSize(125));
      expect(bar().style.minBlockSize).toBe("");
      await act(settled);
      expect(bar().style.minBlockSize).toBe("75px");
      folded = 70;
      act(() => applyTextSize(125));
      await act(settled);
      expect(bar().style.minBlockSize).toBe("75px");
      act(() => applyTextSize(100));
      await act(settled);
      expect(bar().style.minBlockSize).toBe("70px");
      // Kept to the fraction, in the bar's own pixels: its height on the screen over its zoom (a
      // hair more, as the browser rounds it down to 1/64 px), never offsetHeight's whole pixels.
      const zoomed = vi.spyOn(HTMLElement.prototype, "getBoundingClientRect").mockImplementation(function (this: HTMLElement) {
        return new DOMRect(0, 0, 300, this.classList.contains("cal-move") ? 122.5 : 0);
      });
      Object.defineProperty(HTMLElement.prototype, "currentCSSZoom", { configurable: true, get: () => 1.25 });
      try {
        act(() => watchers.forEach((w) => w()));
        expect(bar().style.minBlockSize).toBe("98.0001px");
        expect(side().style.getPropertyValue("--cal-move-h")).toBe("98.0001px");
      } finally {
        zoomed.mockRestore();
        Reflect.deleteProperty(HTMLElement.prototype, "currentCSSZoom");
      }
      // "סיום" while a resize is settling: nothing is measured or kept after the bar went.
      act(() => {
        window.dispatchEvent(new Event("resize"));
        fireEvent.click(step("סיום"));
      });
      await act(settled);
      expect(side().style.minBlockSize).toBe("");
      expect(screen.queryByRole("group", { name: "הזזת הפגישה שנבחרה" })).toBeNull();
      expect(side().style.getPropertyValue("--cal-move-h")).toBe("");
      expect(side().style.minBlockSize).toBe("");
      expect(watched()).toEqual([]);
    } finally {
      height.mockRestore();
      vi.unstubAllGlobals();
    }
    // Beside the grid the bar is out of the flow, at the column's top: the column makes room for
    // it and gives that room back at its end, so the list alone counts for the day's height (the
    // grid follows a longer list), and the grid never grows, not even before the bar is measured.
    expect(rule(".cal-day-side")).toContain("--cal-move-room: calc(var(--cal-move-h) + var(--space-3));");
    expect(rule(".cal-day-side")).toContain("padding-block-start: var(--cal-move-room, 0px);\n  padding-block-end: var(--cal-move-end, 0px);\n  margin-block-end: calc(-1 * (var(--cal-move-room, 0px) + var(--cal-move-end, 0px)));");
    // The list pushed past the page's end keeps a margin under it there, only with a bar.
    expect(rule(".cal-day-side")).toContain("--cal-move-end: calc(var(--cal-move-h) * 0 + var(--space-5));");
    // Its rail covers the column, lets the pointer through to the list, and the bar keeps 6px
    // under the window's top while the page is scrolled down: over the list, never the grid.
    expect(rule(".cal-day-rail")).toMatch(/position: absolute;\n\s*z-index: 6;\n\s*inset: 0;\n\s*pointer-events: none;/);
    expect(rule(".cal-day-rail > .cal-move")).toMatch(/position: sticky;\n\s*inset-block-start: 6px;\n\s*pointer-events: auto;/);
    // What takes the keyboard in the list is scrolled to under the bar, never behind it
    // (WCAG 2.4.11), and under the grid, where the bar lies over nothing, it is scrolled as usual.
    const under = ".cal-day-side:has(> .cal-day-rail) > :not(.cal-day-rail) *";
    expect(rule(under)).toContain("scroll-margin-block-start: calc(var(--cal-move-h, 0px) + 6px + var(--space-3));");
    expect(calendarCss).toMatch(/@container page \(max-width: 720px\) \{[^@]*\.cal-day-side:has\(> \.cal-day-rail\) > :not\(\.cal-day-rail\) \* \{\n {4}scroll-margin-block-start: 0;/);
    // Under the grid (a narrow page) it is in the flow under the legend, nothing is given back,
    // and nothing lies over the grid.
    expect(calendarCss).toMatch(/@container page \(max-width: 720px\) \{[^@]*\.cal-day-side \{\n {4}padding-block: 0;\n {4}margin-block-end: 0;\n {2}\}\n {2}\.cal-day-rail \{\n {4}position: static;\n {4}pointer-events: auto;\n {2}\}\n {2}\.cal-day-rail > \.cal-move \{\n {4}position: static;/);
    expect(calendarCss).not.toContain("-200px");
  });

  it("keeps room for the page's scrollbar on every page of the clinic, so its tabs and views have one width", async () => {
    // Before, only a day or a week kept it: on Windows, the month beside the week, or a tab that
    // does not scroll beside the calendar, moved the page 15px sideways. The reports keep theirs.
    expect(clinicCss).toMatch(/\nhtml:has\(\.clinic-panel\) \{\n {2}scrollbar-gutter: stable;\n\}/);
    expect(calendarCss).not.toContain("scrollbar-gutter");
    const { user } = await openCalendar();
    for (const name of ["שבוע", "חודש", "יום"]) {
      await user.click(viewRadio(name));
      expect(screen.getByRole("tabpanel")).toHaveClass("clinic-panel");
    }
  });

  it("a narrow column never cuts the date: “היום” steps back first, then the weekday is one letter", async () => {
    const { user } = await openCalendar();
    // Each head has its weekday, its weekday in one letter (seen only in a narrow column) and
    // its date; the screen reader hears the same name as before.
    const heads = () => screen.getAllByRole("region").map((r) => r.querySelector(".cal-day-name") as HTMLElement);
    const parts = (name: HTMLElement) => [".cal-day-weekday", ".cal-day-weekday-short", ".num"].map((s) => name.querySelector(s)?.textContent);
    expect(heads().map(parts)).toEqual([
      ["ראשון", "א׳", "15.11"], ["שני", "ב׳", "16.11"], ["שלישי", "ג׳", "17.11"], ["רביעי", "ד׳", "18.11"], ["חמישי", "ה׳", "19.11"], ["שישי", "ו׳", "20.11"],
    ]);
    // Wide, the one letter is not shown (display: none, below) and the name is as before; narrow,
    // the letter she sees is in the name too, so saying what she sees finds it (WCAG 2.5.3).
    const short = heads().map((name) => name.querySelector<HTMLElement>(".cal-day-weekday-short"));
    for (const x of short) expect(x).not.toHaveAttribute("aria-hidden");
    for (const x of short) if (x) x.style.display = "none";
    expect(screen.getByRole("button", { name: "יום שני 16.11.2026, לתצוגת היום" })).toBe(heads()[1]);
    for (const x of short) if (x) x.style.display = "";
    expect(screen.getByRole("button", { name: "יום שני ב׳ 16.11.2026, לתצוגת היום" })).toBe(heads()[1]);
    await user.click(viewRadio("יום"));
    expect(parts(heads()[0] as HTMLElement)).toEqual(["ראשון", "א׳", "15.11"]);
    // The head is measured by itself: under 166px "היום" is said only to a screen reader, under
    // 124px the weekday is one letter. The weekday may take an ellipsis, never the date.
    expect(rule(".cal-day-head")).toContain("container: cal-day-head / inline-size;");
    expect(rule(".cal-day-weekday")).toContain("text-overflow: ellipsis;");
    expect(rule(".cal-day-weekday-short")).toContain("display: none;");
    expect(calendarCss).toMatch(/@container cal-day-head \(max-width: 166px\) \{\n {2}\.cal-day-head \.chip \{\n {4}position: absolute;[^}]*clip-path: inset\(50%\);/);
    expect(calendarCss).toMatch(/@container cal-day-head \(max-width: 124px\) \{[^@]*\.cal-day-weekday \{\n {4}position: absolute;[^}]*clip-path: inset\(50%\);\n {2}\}\n {2}\.cal-day-weekday-short \{\n {4}display: inline;/);
    expect(calendarCss).not.toMatch(/\.num[^{]*\{[^}]*(overflow|ellipsis)/);
    // A holiday's name beside it gives way first: the name takes its own width (not a share of
    // what is left, which would cut the weekday by a fraction of a pixel), and the holiday only
    // what is left over, down to nothing.
    expect(rule(".cal-day-head h3")).toMatch(/flex: 0 1 auto;[^}]*margin-inline-end: auto;/);
    expect(rule(".cal-day-head .cal-holiday")).toMatch(/flex: 1 1 0px;\n\s*min-width: 0;/);
  });

  it("a holiday's name too narrow to read is hidden from the eye only, never cut to a letter that reads like a weekday", async () => {
    const { user, view } = await openCalendar();
    for (let i = 0; i < 3; i++) await user.click(screen.getByRole("button", { name: "השבוע הבא" }));
    expect(title()).toBe("6 עד 12 בדצמבר 2026");
    // Hanukkah in every day's head: the names are a box of their own inside the holiday's,
    // which keeps the whole name in its title; nothing in it is hidden from a screen reader.
    const holiday = await within(day("יום ראשון, 06.12.2026")).findByTitle("חנוכה (נר 3)");
    expect(holiday).toHaveClass("cal-holiday");
    expect(Array.from(holiday.children).map((c) => c.className)).toEqual(["cal-holiday-names"]);
    expect(holiday.firstElementChild).toHaveTextContent("חנוכה (נר 3)");
    expect(holiday.querySelector("[aria-hidden]")).toBeNull();
    expect(await a11yViolations(view.container)).toEqual([]);
    // Its width is what the head leaves over (basis 0), so it is measured by itself: under 3em
    // the names are clipped away, as for a screen reader only (at 640 to 950px it had 4 to 31px,
    // and showed "ח" or "ו"). The month's names are not measured, and stay as they were.
    expect(rule(".cal-day-head .cal-holiday")).toMatch(/flex: 1 1 0px;[^}]*container: cal-holiday \/ inline-size;/);
    expect(calendarCss).toMatch(/@container cal-holiday \(max-width: 3em\) \{\n {2}\.cal-holiday-names \{\n {4}position: absolute;\n {4}width: 1px;\n {4}height: 1px;\n {4}overflow: hidden;\n {4}clip-path: inset\(50%\);/);
    expect(calendarCss).not.toMatch(/\.cal-holiday-names[^{]*\{[^}]*(display: none|visibility: hidden)/);
    expect(calendarCss.match(/container: cal-holiday/g)).toHaveLength(1);
    await user.click(viewRadio("חודש"));
    const month = await screen.findByRole("table", { name: "דצמבר 2026" });
    expect(within(month).getAllByTitle("חנוכה (נר 3)")[0]).toHaveClass("cal-holiday", "cal-month-holiday");
  });

  it("“היום” stays beside “+” on a holiday too: the holiday's name takes the room between the date and it", async () => {
    vi.setSystemTime(new Date(2026, 11, 7, 10, 0));
    const { user } = await openCalendar();
    const parts = (head: Element) => Array.from(head.children).map((c) =>
      c.tagName === "H3" ? "h3" : c.classList.contains("cal-holiday") ? "חג" : c.classList.contains("chip") ? "היום" : c.classList.contains("cal-day-add") ? "+" : c.className);
    for (const name of ["שבוע", "יום"]) {
      await user.click(viewRadio(name));
      const head = day("יום שני, 07.12.2026").querySelector(".cal-day-head") as HTMLElement;
      await within(head).findByTitle("חנוכה (נר 4)");
      // Before, "היום" came right after the date and the growing name pushed it away from "+".
      expect(parts(head), name).toEqual(["h3", "חג", "היום", "+"]);
    }
    // Another day has neither the word nor a place kept for it.
    await user.click(viewRadio("שבוע"));
    expect(parts(day("יום שלישי, 08.12.2026").querySelector(".cal-day-head") as HTMLElement)).toEqual(["h3", "חג", "+"]);
  });

  it("every day of a week has the same width, the first beside the hours too, so a narrow week folds its names together", () => {
    // The first day had no border: 1px wider than the others, it kept "ראשון 15.11" beside
    // "ב׳ 16.11" for a few pixels of window. Its border is there now, unseen, under the hours' line.
    expect(rule(".cal-day")).toContain("border-inline-start: 1px solid var(--line);");
    expect(rule(".cal-gutter + .cal-day")).toContain("border-inline-start-color: transparent;");
    expect(rule(".cal-gutter + .cal-day")).not.toMatch(/border-inline-start(-width)?: 0/);
    expect(rule(".cal-gutter")).toContain("border-inline-end: 1px solid var(--line);");
    expect(rule(".cal-gutter")).toContain("margin-inline-end: -1px;");
  });

  it("today's column is tinted in the dark theme too, with its hour lines and its head seen on the tint, and text on it at 4.5:1", () => {
    // The dark colours as Settings choose them (the same as the computer's: theme.test.ts), and the light ones.
    const block = (selector: string) => {
      const at = tokens.indexOf(`${selector} {`);
      expect(at, selector).toBeGreaterThan(-1);
      return tokens.slice(at, tokens.indexOf("}", at));
    };
    const dark = block(':root[data-theme="dark"]');
    const light = block(":root,\n.a4,\n.mini-page");
    const color = (theme: string, name: string) => {
      const m = new RegExp(`--${name}: (#[0-9a-f]{6});`).exec(theme);
      expect(m, name).not.toBeNull();
      return (m as RegExpExecArray)[1] as string;
    };
    const luminance = (hex: string) => {
      const [r, g, b] = [1, 3, 5].map((i) => {
        const c = parseInt(hex.slice(i, i + 2), 16) / 255;
        return c <= 0.03928 ? c / 12.92 : ((c + 0.055) / 1.055) ** 2.4;
      }) as [number, number, number];
      return 0.2126 * r + 0.7152 * g + 0.0722 * b;
    };
    const ratio = (a: string, b: string) => {
      const [hi, lo] = [luminance(a), luminance(b)].sort((x, y) => y - x) as [number, number];
      return (hi + 0.05) / (lo + 0.05);
    };
    const wash = color(dark, "primary-wash");
    expect(rule(".cal-day-today .cal-day-body")).toContain("background-color: var(--primary-wash);");
    // 1.09:1 (#1b2a28) could hardly be told from the other days.
    expect(ratio(wash, color(dark, "surface"))).toBeGreaterThanOrEqual(1.15);
    expect(ratio(color(dark, "ink"), wash)).toBeGreaterThanOrEqual(4.5);
    expect(ratio(color(dark, "ink-muted"), wash)).toBeGreaterThanOrEqual(4.5);
    // Its hour lines are seen on the tint as on the other days (--line was 1.11:1 on it, 1.30:1
    // on the surface), and its head a step above it, as the tint is above the surface (--primary-soft
    // was 1.08:1). The light theme keeps --line and --primary-soft, already apart from its tint.
    expect(rule(".cal-day-today .cal-day-body")).toContain("linear-gradient(to bottom, var(--primary-wash-line, var(--line)) 1px, transparent 1px)");
    expect(rule(".cal-day-today .cal-day-head")).toContain("background: var(--primary-wash-head, var(--primary-soft));");
    expect(ratio(color(dark, "primary-wash-line"), wash)).toBeGreaterThanOrEqual(ratio(color(dark, "line"), color(dark, "surface")));
    const head = color(dark, "primary-wash-head");
    expect(ratio(head, wash)).toBeGreaterThanOrEqual(1.15);
    for (const ink of ["ink", "ink-muted", "primary", "warn-ink"]) expect(ratio(color(dark, ink), head), ink).toBeGreaterThanOrEqual(4.5);
    expect(light).not.toContain("--primary-wash-line");
    expect(light).not.toContain("--primary-wash-head");
    expect(ratio(color(light, "line"), color(light, "primary-wash"))).toBeGreaterThanOrEqual(1.15);
    expect(ratio(color(light, "primary-soft"), color(light, "primary-wash"))).toBeGreaterThanOrEqual(1.1);
  });
});
