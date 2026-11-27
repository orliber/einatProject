// "סגירת היום" on the clinic's front page: shown only once the day's visits are over, says what
// is still open, and says so plainly when nothing is. Over the demo core; names are invented.
import { configure, render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import type { Overview } from "../../ipc/generated/Overview";
import type { Visit } from "../../ipc/generated/Visit";
import type { VisitView } from "../../ipc/generated/VisitView";
import { a11yViolations } from "../../test/a11y";
import { appApi, Host, serve } from "../../test/host";
import { ClinicScreen, forgetDiscreet } from "./ClinicScreen";
import { dayClose } from "./DayClose";

configure({ asyncUtilTimeout: 5000 });
vi.setConfig({ testTimeout: 30_000 });

afterEach(() => {
  vi.useRealTimers();
  forgetDiscreet();
});

const view = (over: Partial<Visit>, hasNote = false): VisitView => ({
  visit: {
    id: `v-${over.start ?? 0}`, patient_id: "p", kind: "assessment", title: "", date: "2026-11-15", start: 540, minutes: 60, place: "", status: "planned",
    status_note: "", fee: 0, charge_missed: false, series_id: null, note: "", remember: [], ...over,
  },
  patient_name: "ילד בדוי", has_note: hasNote, overlaps: false,
});
const overview = (visits: VisitView[], next: VisitView[] = []): Overview => ({
  date: "2026-11-15", visits, next_day: next.length > 0 ? "2026-11-16" : null, next_visits: next, tasks: [], alerts: [],
  week_visits: 0, month_received: 0, total_due: 0, waiting: 0, active: 1,
});
const at = (minute: number) => ({ date: "2026-11-15", minute });

describe("when the day counts as over", () => {
  it("not while a visit with a family is still ahead or under way", () => {
    const o = overview([view({ start: 540 }), view({ start: 900 })]);
    expect(dayClose(o, at(600))).toBeNull();
    expect(dayClose(o, at(930))).toBeNull();
    expect(dayClose(o, at(960))).not.toBeNull();
  });

  it("her own time and a cancelled visit do not hold the day open, and a day with no family has no closing", () => {
    const o = overview([view({ start: 540, status: "held" }, true), view({ start: 1000, kind: "block", patient_id: null }), view({ start: 1100, status: "cancelled" })]);
    expect(dayClose(o, at(700))).toMatchObject({ sessions: 1, unmarked: 0, noNote: 0 });
    expect(dayClose(overview([view({ start: 540, kind: "supervision", patient_id: null })]), at(900))).toBeNull();
  });

  it("counts what is still open, and names the next day's first hour", () => {
    const o = overview(
      [view({ start: 540 }), view({ start: 660, status: "held" }), view({ start: 780, status: "held" }, true), view({ start: 840, status: "no_show" })],
      [view({ start: 720, date: "2026-11-16" }), view({ start: 600, date: "2026-11-16" }), view({ start: 500, date: "2026-11-16", status: "cancelled" })],
    );
    expect(dayClose(o, at(1000))).toEqual({ sessions: 4, unmarked: 1, noNote: 1, next: { date: "2026-11-16", count: 2, start: 600 } });
  });
});

describe("on the front page", () => {
  async function open(hour: number) {
    vi.useFakeTimers({ toFake: ["Date"] });
    vi.setSystemTime(new Date(2026, 10, 15, hour, 0));
    const served = await serve((cmd) => (cmd === "copy_secret" ? 60 : undefined));
    const user = userEvent.setup();
    const went: unknown[] = [];
    render(<Host app={appApi({ route: { name: "clinic" }, go: (r) => void went.push(r) })}><ClinicScreen /></Host>);
    await screen.findByRole("region", { name: "הפגישות של היום" });
    return { ...served, user, went };
  }

  it("is not shown in the morning", async () => {
    await open(8);
    expect(screen.queryByRole("region", { name: "סגירת היום" })).toBeNull();
  });

  it("in the evening says what is left; marking the visits and nothing else leaves the notes open", async () => {
    const { user } = await open(20);
    const box = await screen.findByRole("region", { name: "סגירת היום" });
    expect(box).toHaveTextContent("נשאר לסגור");
    expect(box).toHaveTextContent(/עוד לא סומנ/);
    expect(await a11yViolations(box)).toEqual([]);
    const today = screen.getByRole("region", { name: "הפגישות של היום" });
    for (;;) {
      const hold = within(today).queryAllByRole("button", { name: "התקיימה" })[0];
      if (!hold) break;
      await user.click(hold);
      await waitFor(() => expect(hold).not.toHaveTextContent("התקיימה"));
    }
    await waitFor(() => expect(box).toHaveTextContent(/סומנ(ה|ו)$|סומנ(ה|ו)[^.]*בלי סיכום/));
    expect(box).toHaveTextContent(/שהתקיימ(ה|ו) היום בלי סיכום/);
    expect(box).toHaveTextContent("נשאר לסגור");
    expect(box).not.toHaveTextContent("אפשר לנעול");
  });

  it("“כולן התקיימו” lists the visits first, and marks them all only after she confirms", async () => {
    const { user, calls } = await open(20);
    const box = await screen.findByRole("region", { name: "סגירת היום" });
    const today = screen.getByRole("region", { name: "הפגישות של היום" });
    const waiting = within(today).getAllByRole("button", { name: "התקיימה" }).length;
    expect(waiting).toBeGreaterThanOrEqual(2);
    await user.click(within(box).getByRole("button", { name: "כולן התקיימו" }));
    const ask = await screen.findByRole("dialog", { name: "לסמן את הפגישות של היום" });
    expect(within(within(ask).getByRole("list", { name: "הפגישות שיסומנו" })).getAllByRole("listitem")).toHaveLength(waiting);
    expect(ask).toHaveTextContent("סימון שנעשה בטעות אפשר לתקן");
    expect(await a11yViolations(ask)).toEqual([]);
    const saves = () => calls.filter((c) => c.cmd === "clinic" && (c.args as { request: { op: string } }).request.op === "save_visit").length;
    // Nothing is written until she says so.
    expect(saves()).toBe(0);
    await user.click(within(ask).getByRole("button", { name: `לסמן ${waiting} פגישות כ"התקיימה"` }));
    await waitFor(() => expect(screen.queryByRole("dialog")).toBeNull());
    expect(saves()).toBe(waiting);
    await waitFor(() => expect(within(today).queryByRole("button", { name: "התקיימה" })).toBeNull());
    await waitFor(() => expect(box).not.toHaveTextContent(/עוד לא סומנ/));
    expect(within(box).queryByRole("button", { name: "כולן התקיימו" })).toBeNull();
    // The visits charged and nothing came in today: the ₪ line says so, not a quiet "nothing".
    expect(await within(box).findByText(/^\d+ פגישות חויבו היום, ולא נרשם תקבול$/)).toBeInTheDocument();
    expect(box).not.toHaveTextContent("הכול רשום");
  });

  it("the confirmation tells where a visit nobody came to is marked, and where a cancelled one is", async () => {
    const { user } = await open(20);
    const box = await screen.findByRole("region", { name: "סגירת היום" });
    await user.click(within(box).getByRole("button", { name: "כולן התקיימו" }));
    const ask = await screen.findByRole("dialog", { name: "לסמן את הפגישות של היום" });
    expect(ask).toHaveTextContent("מסמנים אותה קודם ב\"לא הגיעו\", בתפריט \"⋯\" שבשורה שלה. פגישה שבוטלה: פותחים אותה ומסמנים בה \"בוטלה\".");
  });

  it("closing the window marks nothing", async () => {
    const { user } = await open(20);
    const box = await screen.findByRole("region", { name: "סגירת היום" });
    await user.click(within(box).getByRole("button", { name: "כולן התקיימו" }));
    const ask = await screen.findByRole("dialog", { name: "לסמן את הפגישות של היום" });
    await user.click(within(ask).getByRole("button", { name: "ביטול" }));
    expect(box).toHaveTextContent(/עוד לא סומנ/);
  });
});
