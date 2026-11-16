// The calendar's arithmetic: hours, places on the grid, overlaps, weeks of a month, titles,
// and where a visit lands when it is moved. Every name is invented.
import type { Visit } from "../../ipc/generated/Visit";
import type { VisitView } from "../../ipc/generated/VisitView";
import {
  applyMove, byDate, gridHeight, gridHours, hourMarks, HOUR_PX, MIN_VISIT_PX, minutesToPx, monthGridRange, monthWeeks, movedTo, nowOffset, placeDay,
  rangeTitle, shiftAnchor, slotAt, suggestStart, viewRange, weekDays,
} from "./calendarLayout";

const view = (id: string, date: string, start: number, over: Partial<Visit> = {}): VisitView => ({
  visit: {
    id, patient_id: "patient-1", kind: "therapy", title: "", date, start, minutes: 50, place: "", status: "planned", status_note: "", fee: 0,
    charge_missed: false, series_id: null, note: "", ...over,
  },
  patient_name: "מאיה",
  has_note: false,
  overlaps: false,
});

describe("the hours of the grid", () => {
  it("are her working hours when every visit is inside them", () => {
    expect(gridHours(480, 1200, [{ start: 540, minutes: 90 }])).toEqual({ start: 480, end: 1200 });
    expect(gridHours(480, 1200, [])).toEqual({ start: 480, end: 1200 });
  });

  it("widen to whole hours for a visit outside them", () => {
    expect(gridHours(480, 1200, [{ start: 450, minutes: 30 }, { start: 1190, minutes: 50 }])).toEqual({ start: 420, end: 1260 });
    // Hours that do not start on the hour are widened too, and never pass midnight.
    expect(gridHours(510, 1410, [{ start: 1400, minutes: 40 }])).toEqual({ start: 480, end: 1440 });
  });

  it("always show at least one hour", () => {
    expect(gridHours(600, 600, [])).toEqual({ start: 600, end: 660 });
    expect(gridHours(1440, 1440, [])).toEqual({ start: 1440, end: 1440 });
  });

  it("mark the start of every hour, and measure in pixels", () => {
    const hours = { start: 480, end: 720 };
    expect(hourMarks(hours)).toEqual([480, 540, 600, 660]);
    expect(gridHeight(hours)).toBe(4 * HOUR_PX);
    expect(minutesToPx(30)).toBe(HOUR_PX / 2);
    expect(minutesToPx(15, 80)).toBe(20);
  });
});

describe("placing the visits of a day", () => {
  const hours = { start: 480, end: 1200 };
  const byId = (placed: ReturnType<typeof placeDay>) => Object.fromEntries(placed.map((p) => [p.id, p]));

  it("puts a visit by its start and its length", () => {
    const [p] = placeDay([{ id: "a", start: 540, minutes: 90 }], hours);
    expect(p).toEqual({ id: "a", top: HOUR_PX, height: 1.5 * HOUR_PX, column: 0, columns: 1 });
  });

  it("gives visits that do not meet the whole width, also when one ends as the next begins", () => {
    const placed = byId(placeDay([{ id: "a", start: 540, minutes: 60 }, { id: "b", start: 600, minutes: 60 }, { id: "c", start: 900, minutes: 50 }], hours));
    expect([placed.a?.columns, placed.b?.columns, placed.c?.columns]).toEqual([1, 1, 1]);
    expect([placed.a?.column, placed.b?.column, placed.c?.column]).toEqual([0, 0, 0]);
  });

  it("puts two overlapping visits side by side", () => {
    const placed = byId(placeDay([{ id: "late", start: 570, minutes: 50 }, { id: "early", start: 540, minutes: 90 }], hours));
    expect(placed.early).toMatchObject({ column: 0, columns: 2 });
    expect(placed.late).toMatchObject({ column: 1, columns: 2 });
  });

  it("divides a chain of overlaps into the same number of columns, and reuses a column that is free again", () => {
    // a 9:00–10:30, b 9:30–10:20, c 10:20–11:00 (meets a, after b), d 12:00 on its own.
    const placed = byId(placeDay([
      { id: "a", start: 540, minutes: 90 }, { id: "b", start: 570, minutes: 50 }, { id: "c", start: 620, minutes: 40 }, { id: "d", start: 720, minutes: 30 },
    ], hours));
    expect(placed.a).toMatchObject({ column: 0, columns: 2 });
    expect(placed.b).toMatchObject({ column: 1, columns: 2 });
    expect(placed.c).toMatchObject({ column: 1, columns: 2 });
    expect(placed.d).toMatchObject({ column: 0, columns: 1 });
  });

  it("three at once take three columns", () => {
    const placed = placeDay([{ id: "a", start: 540, minutes: 60 }, { id: "b", start: 540, minutes: 60 }, { id: "c", start: 550, minutes: 30 }], hours);
    expect(placed.map((p) => p.columns)).toEqual([3, 3, 3]);
    expect(placed.map((p) => p.column).sort()).toEqual([0, 1, 2]);
  });

  it("never draws a visit lower than can be pressed, and counts that height as taken", () => {
    const placed = byId(placeDay([{ id: "short", start: 540, minutes: 5 }, { id: "next", start: 550, minutes: 60 }], hours));
    expect(placed.short?.height).toBeCloseTo(MIN_VISIT_PX, 6);
    // Drawn, the short one reaches past 9:10: the next one goes beside it.
    expect(placed.short?.columns).toBe(2);
    expect(placed.next?.column).toBe(1);
  });

  it("keeps a visit at the very end of the grid inside it", () => {
    const [p] = placeDay([{ id: "a", start: 1195, minutes: 5 }], hours);
    expect(p?.height).toBeCloseTo(MIN_VISIT_PX, 6);
    expect((p?.top ?? 0) + (p?.height ?? 0)).toBeCloseTo(gridHeight(hours), 6);
  });

  it("is in the order of the day", () => {
    expect(placeDay([{ id: "c", start: 900, minutes: 30 }, { id: "a", start: 500, minutes: 30 }, { id: "b", start: 700, minutes: 30 }], hours).map((p) => p.id)).toEqual(["a", "b", "c"]);
  });
});

describe("a press on the grid", () => {
  const hours = { start: 480, end: 1200 };

  it("falls in the half hour under it", () => {
    expect(slotAt(0, hours)).toBe(480);
    expect(slotAt(HOUR_PX / 2 - 1, hours)).toBe(480);
    expect(slotAt(HOUR_PX / 2, hours)).toBe(510);
    expect(slotAt(2.6 * HOUR_PX, hours)).toBe(630);
  });

  it("stays on the grid", () => {
    expect(slotAt(-20, hours)).toBe(480);
    expect(slotAt(10_000, hours)).toBe(1170);
  });
});

describe("the line of now", () => {
  it("is drawn only inside the hours shown", () => {
    const hours = { start: 480, end: 1200 };
    expect(nowOffset(540, hours)).toBe(HOUR_PX);
    expect(nowOffset(479, hours)).toBeNull();
    expect(nowOffset(1201, hours)).toBeNull();
  });
});

describe("the hour offered for a new visit", () => {
  it("is the next half hour today, and the start of her day otherwise", () => {
    const now = { date: "2026-11-15", minute: 9 * 60 + 10 };
    expect(suggestStart("2026-11-15", now, 480, 1200)).toBe(570);
    expect(suggestStart("2026-11-16", now, 480, 1200)).toBe(480);
    // Before her day, or after it: the start of her day.
    expect(suggestStart("2026-11-15", { date: "2026-11-15", minute: 300 }, 480, 1200)).toBe(480);
    expect(suggestStart("2026-11-15", { date: "2026-11-15", minute: 1190 }, 480, 1200)).toBe(480);
  });
});

describe("the days of a week", () => {
  it("are her working days", () => {
    expect(weekDays("2026-11-18", [0, 1, 2, 3, 4], [])).toEqual(["2026-11-15", "2026-11-16", "2026-11-17", "2026-11-18", "2026-11-19"]);
  });

  it("with any other day that has a visit, in its place", () => {
    expect(weekDays("2026-11-15", [0, 2, 4], ["2026-11-20", "2026-11-16", "2026-12-01"])).toEqual(["2026-11-15", "2026-11-16", "2026-11-17", "2026-11-19", "2026-11-20"]);
  });

  it("are the whole week when no working day is known", () => {
    expect(weekDays("2026-11-15", [], [])).toHaveLength(7);
  });
});

describe("the page of a month", () => {
  it("is whole weeks from Sunday, with the days of the months beside it", () => {
    // November 2026 starts on a Sunday and ends on a Monday.
    const weeks = monthWeeks("2026-11-15");
    expect(weeks).toHaveLength(5);
    expect(weeks.every((w) => w.length === 7)).toBe(true);
    expect(weeks[0]?.[0]).toBe("2026-11-01");
    expect(weeks[4]).toEqual(["2026-11-29", "2026-11-30", "2026-12-01", "2026-12-02", "2026-12-03", "2026-12-04", "2026-12-05"]);
    expect(monthGridRange("2026-11")).toEqual({ from: "2026-11-01", to: "2026-12-05" });
  });

  it("has four, five or six weeks", () => {
    // February 2026: Sunday the 1st to Saturday the 28th.
    expect(monthWeeks("2026-02-10")).toHaveLength(4);
    // August 2026 starts on a Saturday: six weeks.
    const august = monthWeeks("2026-08-01");
    expect(august).toHaveLength(6);
    expect(august[0]?.[0]).toBe("2026-07-26");
    expect(august[5]?.[6]).toBe("2026-09-05");
  });

  it("is never more than the calendar answers in one request", () => {
    for (let m = 1; m <= 12; m++) {
      const days = monthWeeks(`2027-${String(m).padStart(2, "0")}-01`).flat();
      expect(days.length).toBeLessThanOrEqual(42);
    }
  });
});

describe("what a view asks for and how it steps", () => {
  it("the day and the week ask for the week; the month for its page", () => {
    expect(viewRange("day", "2026-11-18")).toEqual({ from: "2026-11-15", to: "2026-11-21" });
    expect(viewRange("week", "2026-11-18")).toEqual({ from: "2026-11-15", to: "2026-11-21" });
    expect(viewRange("month", "2026-11-18")).toEqual({ from: "2026-11-01", to: "2026-12-05" });
  });

  it("steps by a day, a week or a month", () => {
    expect(shiftAnchor("day", "2026-11-30", 1)).toBe("2026-12-01");
    expect(shiftAnchor("day", "2026-01-01", -1)).toBe("2025-12-31");
    expect(shiftAnchor("week", "2026-11-18", 1)).toBe("2026-11-25");
    expect(shiftAnchor("week", "2026-11-18", -1)).toBe("2026-11-11");
    // From any day of a month to the first of the month beside it (the 31st has no twin in February).
    expect(shiftAnchor("month", "2026-01-31", 1)).toBe("2026-02-01");
    expect(shiftAnchor("month", "2026-03-31", -1)).toBe("2026-02-01");
    expect(shiftAnchor("month", "2026-12-15", 1)).toBe("2027-01-01");
    expect(shiftAnchor("month", "2026-01-15", -1)).toBe("2025-12-01");
  });
});

describe("the title of what is shown", () => {
  it("names the day, the week or the month in Hebrew", () => {
    expect(rangeTitle("day", "2026-11-15")).toBe("יום ראשון, 15.11.2026");
    expect(rangeTitle("month", "2026-11-15")).toBe("נובמבר 2026");
    expect(rangeTitle("week", "2026-11-18")).toBe("15 עד 21 בנובמבר 2026");
  });

  it("names both months, and both years, when a week crosses them", () => {
    expect(rangeTitle("week", "2026-12-01")).toBe("29 בנובמבר עד 5 בדצמבר 2026");
    expect(rangeTitle("week", "2026-12-30")).toBe("27 בדצמבר 2026 עד 2 בינואר 2027");
  });
});

describe("the visits of each date", () => {
  it("keeps the order of the answer", () => {
    const days = byDate([view("a", "2026-11-15", 540), view("b", "2026-11-15", 600), view("c", "2026-11-16", 480)]);
    expect(days.get("2026-11-15")?.map((v) => v.visit.id)).toEqual(["a", "b"]);
    expect(days.get("2026-11-16")?.map((v) => v.visit.id)).toEqual(["c"]);
    expect(days.get("2026-11-17")).toBeUndefined();
  });
});

describe("moving a visit", () => {
  it("lands a step earlier or later, or a day away", () => {
    const v = { date: "2026-11-15", start: 540, minutes: 90 };
    expect(movedTo(v, { minutes: -15 })).toEqual({ date: "2026-11-15", start: 525 });
    expect(movedTo(v, { minutes: 15 })).toEqual({ date: "2026-11-15", start: 555 });
    expect(movedTo(v, { days: 1 })).toEqual({ date: "2026-11-16", start: 540 });
    expect(movedTo({ ...v, date: "2026-11-01" }, { days: -1 })).toEqual({ date: "2026-10-31", start: 540 });
  });

  it("never leaves the day", () => {
    expect(movedTo({ date: "2026-11-15", start: 10, minutes: 30 }, { minutes: -15 })).toBeNull();
    expect(movedTo({ date: "2026-11-15", start: 1380, minutes: 50 }, { minutes: 15 })).toBeNull();
    expect(movedTo({ date: "2026-11-15", start: 1380, minutes: 60 }, { minutes: 15 })).toBeNull();
    expect(movedTo({ date: "not a date", start: 600, minutes: 60 }, { days: 1 })).toBeNull();
  });

  it("shows the visit at its new hour, in the order of the day, with overlaps worked out anew", () => {
    const before = [view("a", "2026-11-15", 540, { minutes: 60 }), view("b", "2026-11-15", 600, { minutes: 60 }), view("c", "2026-11-16", 540)];
    const after = applyMove(before, "b", { date: "2026-11-15", start: 525 }, "one");
    expect(after.map((v) => v.visit.id)).toEqual(["b", "a", "c"]);
    expect(after.map((v) => v.overlaps)).toEqual([true, true, false]);
    // What was given is not changed.
    expect(before.map((v) => v.visit.start)).toEqual([540, 600, 540]);
    expect(before.every((v) => !v.overlaps)).toBe(true);
    // And moving it away again clears the mark on both.
    expect(applyMove(after, "b", { date: "2026-11-16", start: 720 }, "one").map((v) => v.overlaps)).toEqual([false, false, false]);
  });

  it("a cancelled visit overlaps nothing", () => {
    const after = applyMove([view("a", "2026-11-15", 540, { status: "cancelled" }), view("b", "2026-11-15", 600)], "b", { date: "2026-11-15", start: 540 }, "one");
    expect(after.map((v) => v.overlaps)).toEqual([false, false]);
  });

  it("with “this and the following”, the later planned visits of the series move with it", () => {
    const series = { series_id: "series-1" };
    const before = [
      view("w1", "2026-11-08", 960, { ...series, status: "held" }),
      view("w2", "2026-11-15", 960, series),
      view("w3", "2026-11-22", 960, series),
      view("w4", "2026-11-29", 960, { ...series, status: "cancelled" }),
      view("other", "2026-11-22", 960, { series_id: "series-2" }),
      view("single", "2026-11-23", 600),
    ];
    const after = applyMove(before, "w2", { date: "2026-11-16", start: 990 }, "following");
    const at = (id: string) => { const v = after.find((x) => x.visit.id === id)?.visit; return `${v?.date} ${v?.start}`; };
    expect(at("w2")).toBe("2026-11-16 990");
    expect(at("w3")).toBe("2026-11-23 990");
    // The one before it, the one that is no longer planned and the other series stay.
    expect(at("w1")).toBe("2026-11-08 960");
    expect(at("w4")).toBe("2026-11-29 960");
    expect(at("other")).toBe("2026-11-22 960");
    expect(at("single")).toBe("2026-11-23 600");
    // "Only this one" leaves the rest of the series where it was.
    const one = applyMove(before, "w2", { date: "2026-11-16", start: 990 }, "one");
    expect(one.find((x) => x.visit.id === "w3")?.visit).toMatchObject({ date: "2026-11-22", start: 960 });
  });

  it("a later visit of the series that has a session note stays where it is, as the vault leaves it (D-176)", () => {
    const series = { series_id: "series-1" };
    const noted = { ...view("w3", "2026-11-22", 960, series), has_note: true };
    const after = applyMove([view("w2", "2026-11-15", 960, series), noted, view("w4", "2026-11-29", 960, series)], "w2", { date: "2026-11-16", start: 990 }, "following");
    expect(after.map((v) => `${v.visit.id} ${v.visit.date} ${v.visit.start}`)).toEqual(["w2 2026-11-16 990", "w3 2026-11-22 960", "w4 2026-11-30 990"]);
  });

  it("an answer that does not hold the visit is left as it is", () => {
    const views = [view("a", "2026-11-15", 540)];
    expect(applyMove(views, "missing", { date: "2026-11-16", start: 540 }, "one")).toBe(views);
  });
});
