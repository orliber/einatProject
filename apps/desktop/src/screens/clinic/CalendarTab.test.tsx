// The calendar over the demo core: the three views and what each asks for, the visits on the
// grid, opening and scheduling, moving a visit (at once, with rollback), the day's quick
// actions, discreet mode and the accessibility check. Every name is invented; the clock is
// held at Sunday 15.11.2026, 08:00.
import { act, configure, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { useState } from "react";
import type { Route } from "../../App";
import type { ClinicRequest } from "../../ipc/generated/ClinicRequest";
import type { PatientRow } from "../../ipc/generated/PatientRow";
import type { Visit } from "../../ipc/generated/Visit";
import type { VisitView } from "../../ipc/generated/VisitView";
import { a11yViolations } from "../../test/a11y";
import { appApi, Host, serve, type Override, type Served } from "../../test/host";
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
    expect(asked()).toEqual(["2026-11-15 2026-11-21"]);
    expect(await a11yViolations(view.container)).toEqual([]);
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
        await ask(served, { op: "save_visit", repeat: null, scope: "one", visit: { ...base, id: "", patient_id: itay, kind: "guidance", start: 570, minutes: 50, series_id: null, note: "" } });
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
    await user.click(await within(dialog).findByRole("button", { name: "איתי" }));
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
    expect(screen.getByText("אין פגישות בשבוע הזה. כדי לקבוע פגישה לוחצים על שעה ביומן.")).toBeInTheDocument();
    expect(screen.getAllByRole("region")).toHaveLength(5);
    expect(await a11yViolations(view.container)).toEqual([]);
    await user.click(viewRadio("יום"));
    expect(screen.getByRole("heading", { name: "אין פגישות ביום הזה" })).toBeInTheDocument();
    await user.click(viewRadio("חודש"));
    expect(await screen.findByText("אין פגישות בחודש הזה. לחיצה על יום פותחת אותו, ושם אפשר לקבוע פגישה.")).toBeInTheDocument();
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
    expect(screen.getByText("כדי להזיז פגישה בלי לפתוח אותה, מצביעים עליה ולוחצים על החצים שבפינה שלה.")).toBeInTheDocument();
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
    await user.click(step("סיום"));
    expect(screen.queryByRole("group", { name: "הזזת הפגישה שנבחרה" })).toBeNull();
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
    await user.click(screen.getByRole("button", { name: "יום שני 16.11.2026, לתצוגת היום" }));
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
    await user.click(within(row("תמר")).getByRole("button", { name: "התקיימה" }));
    expect(within(row("תמר")).getByText("התקיימה")).toBeInTheDocument();
    expect(await screen.findByRole("button", { name: "09:00–10:30 תמר, מפגש אבחון, התקיימה" })).toBeInTheDocument();
    await waitFor(() => expect(saves()[0]).toMatchObject({ scope: "one", visit: { status: "held" } }));
    await waitFor(() => expect(within(row("תמר")).queryByRole("button", { name: "התקיימה" })).toBeNull());
    // Her own time has nothing to mark.
    const own = within(screen.getByRole("region", { name: "סדר היום" })).getAllByRole("listitem")[2] as HTMLElement;
    expect(own).toHaveTextContent("כתיבת דוחות");
    expect(within(own).queryByRole("button", { name: "התקיימה" })).toBeNull();
  });

  it("a visit of a day still ahead has nothing to mark yet", async () => {
    const { user, row } = await openDay();
    await user.click(screen.getByRole("button", { name: "היום הבא" }));
    await waitFor(() => expect(within(screen.getByRole("region", { name: "סדר היום" })).getByRole("button", { name: "תמר" })).toBeInTheDocument());
    expect(within(row("תמר")).queryByRole("button", { name: "התקיימה" })).toBeNull();
    expect(within(row("תמר")).getByRole("button", { name: "תזכורת" })).toBeInTheDocument();
  });

  it("a mark the vault refuses rolls back, and says why", async () => {
    const refuse: Override = (cmd, args) => {
      if (isOp(cmd, args, "save_visit")) throw refused("הפגישה לא נשמרה.");
      return undefined;
    };
    const { user, row, notes } = await openDay(refuse);
    await user.click(within(row("איתי")).getByRole("button", { name: "התקיימה" }));
    await waitFor(() => expect(notes).toContain("הפגישה לא נשמרה."));
    await waitFor(() => expect(within(row("איתי")).getByText("מתוכננת")).toBeInTheDocument());
    expect(within(row("איתי")).getByRole("button", { name: "התקיימה" })).toBeInTheDocument();
  });

  it("“סיכום” opens the visit's note, and “תזכורת” copies through the protected clipboard only", async () => {
    const { user, row, calls, notes } = await openDay();
    await user.click(within(row("תמר")).getByRole("button", { name: "תזכורת" }));
    await user.click(await screen.findByRole("menuitem", { name: "רק להעתיק" }));
    await waitFor(() => expect(notes).toEqual(["התזכורת הועתקה ללוח המוגן, ואפשר להדביק אותה בהודעה. יימחק מהלוח בעוד דקה."]));
    const copied = calls.filter((c) => c.cmd === "copy_secret");
    expect(copied).toHaveLength(1);
    expect(String(copied[0]?.args.text)).toContain("15.11.2026, בשעה 09:00");
    await user.click(within(row("תמר")).getByRole("button", { name: "סיכום" }));
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
    await user.click(screen.getByRole("button", { name: "הזזת הפגישה של ת., ראשון 09:00" }));
    expect(within(bar()).getByRole("status")).toHaveTextContent("ת.יום ראשון, 15.11.2026");
    hidden();
    await user.click(viewRadio("יום"));
    expect(within(screen.getByRole("region", { name: "סדר היום" })).getByRole("button", { name: "ת." })).toBeInTheDocument();
    hidden();
    await user.click(viewRadio("חודש"));
    const table = await screen.findByRole("table", { name: "נובמבר 2026" });
    expect(within(within(table).getByRole("button", { name: `${SUNDAY}, 4 פגישות` }).closest("td") as HTMLElement).getAllByRole("listitem")[0]).toHaveTextContent("09:00ת.");
    hidden();
  });
});
