// The calendar's arithmetic, apart from its screen: which days and hours a view shows, where a
// visit sits on the time grid (and beside which others, when they overlap), where the grid opens,
// the kinds its legend explains, the weeks of a month, and where a visit lands when she moves it.
// Pure functions over ISO dates and minutes from midnight; nothing here reads the clock or the page.
import type { Scope } from "../../ipc/generated/Scope";
import type { VisitKind } from "../../ipc/generated/VisitKind";
import type { VisitView } from "../../ipc/generated/VisitView";
import { addDays, dayNumber, formatDateLong, formatMonth, monthOf, monthRange, VISIT_KINDS, weekday, weekRange, type DateRange } from "./format";

export type CalendarView = "day" | "week" | "month";

/** The height of one hour on the time grid, in CSS pixels. */
export const HOUR_PX = 56;
/** A visit is never drawn lower than this, so it can always be read and pressed. */
export const MIN_VISIT_PX = 24;
/** One step of "earlier" or "later". */
export const STEP_MINUTES = 15;
/** How many visits a day of the month view lists before "ועוד". */
export const MONTH_ROWS = 3;

/** The hours a time grid shows: whole hours, minutes from midnight. */
export interface Hours {
  start: number;
  end: number;
}

interface Timed {
  start: number;
  minutes: number;
}

/** Her working hours, widened (to whole hours) so a visit outside them is still on the grid. */
export function gridHours(dayStart: number, dayEnd: number, visits: Timed[]): Hours {
  let from = dayStart;
  let to = dayEnd;
  for (const v of visits) {
    from = Math.min(from, v.start);
    to = Math.max(to, v.start + v.minutes);
  }
  const start = Math.max(0, Math.floor(from / 60) * 60);
  const end = Math.min(1440, Math.ceil(to / 60) * 60);
  return { start, end: Math.max(end, Math.min(1440, start + 60)) };
}

/** The start of every hour of the grid. */
export function hourMarks(hours: Hours): number[] {
  const marks: number[] = [];
  for (let m = hours.start; m < hours.end; m += 60) marks.push(m);
  return marks;
}

export function minutesToPx(minutes: number, hourPx = HOUR_PX): number {
  return (minutes * hourPx) / 60;
}

/** The height of the whole grid. */
export function gridHeight(hours: Hours, hourPx = HOUR_PX): number {
  return minutesToPx(hours.end - hours.start, hourPx);
}

/** Where one visit is drawn in its day: pixels from the top of the grid, and which of how many
 *  side-by-side columns it takes (1 of 1 when nothing is beside it). */
export interface Placed {
  id: string;
  top: number;
  height: number;
  column: number;
  columns: number;
}

/**
 * Places the visits of one day. Visits whose times meet (as drawn: a very short one still has
 * the least height) share the width, each in the first free column; a group of visits that
 * touch one another, directly or through others, is divided into the same number of columns.
 */
export function placeDay(visits: (Timed & { id: string })[], hours: Hours, hourPx = HOUR_PX): Placed[] {
  const least = (MIN_VISIT_PX * 60) / hourPx;
  const items = visits
    .map((v) => {
      let start = Math.max(hours.start, Math.min(v.start, hours.end));
      let end = Math.max(v.start + v.minutes, start + least);
      if (end > hours.end) {
        // At the very end of the grid the least height is kept by drawing it a little earlier.
        end = hours.end;
        start = Math.max(hours.start, Math.min(start, end - least));
      }
      return { id: v.id, start, end };
    })
    .sort((a, b) => a.start - b.start || b.end - a.end || (a.id < b.id ? -1 : 1));

  const placed: Placed[] = [];
  let group: Placed[] = [];
  let ends: number[] = [];
  let groupEnd = -1;
  const close = () => {
    for (const p of group) p.columns = ends.length;
    group = [];
    ends = [];
  };
  for (const it of items) {
    if (group.length > 0 && it.start >= groupEnd) close();
    let column = ends.findIndex((end) => end <= it.start);
    if (column < 0) column = ends.length;
    ends[column] = it.end;
    groupEnd = Math.max(groupEnd, it.end);
    const p: Placed = {
      id: it.id,
      top: minutesToPx(it.start - hours.start, hourPx),
      height: minutesToPx(it.end - it.start, hourPx),
      column,
      columns: 1,
    };
    group.push(p);
    placed.push(p);
  }
  close();
  return placed;
}

/** The half hour that a press at `offsetPx` from the top of the grid falls in. */
export function slotAt(offsetPx: number, hours: Hours, hourPx = HOUR_PX): number {
  const minute = hours.start + (Math.max(0, offsetPx) * 60) / hourPx;
  const slot = Math.floor(minute / 30) * 30;
  return Math.max(hours.start, Math.min(slot, hours.end - 30));
}

/** Where the line of "now" is drawn; `null` when the hour is not on the grid. */
export function nowOffset(minute: number, hours: Hours, hourPx = HOUR_PX): number | null {
  return minute < hours.start || minute > hours.end ? null : minutesToPx(minute - hours.start, hourPx);
}

/**
 * Where a time grid is scrolled to when a range is first shown (D-204): a week opens at the top
 * of its grid (the start of her day, or the hour of an earlier visit); a day that is today opens
 * an hour before now, so now is in sight.
 */
export function openingScroll(opensAt: "start" | "now", nowAt: number | null, hourPx = HOUR_PX): number {
  return opensAt === "now" && nowAt !== null ? Math.max(0, nowAt - hourPx) : 0;
}

/** The first half hour still ahead when the day is today (inside her hours), else the start of her day. */
export function suggestStart(date: string, today: { date: string; minute: number }, dayStart: number, dayEnd: number): number {
  if (date !== today.date) return dayStart;
  const next = Math.ceil(today.minute / 30) * 30;
  return next >= dayStart && next + 30 <= dayEnd ? next : dayStart;
}

/** The seven dates of a range that starts on a Sunday. */
function datesOf(range: DateRange): string[] {
  const days = (dayNumber(range.to) ?? 0) - (dayNumber(range.from) ?? 0);
  return Array.from({ length: Math.max(0, days) + 1 }, (_, i) => addDays(range.from, i));
}

/** The columns of a week: her working days, and any other day that has a visit on it. */
export function weekDays(anchor: string, workDays: number[], visitDates: Iterable<string>): string[] {
  const busy = new Set(visitDates);
  const all = datesOf(weekRange(anchor));
  const shown = all.filter((d) => busy.has(d) || workDays.includes(weekday(d) ?? -1));
  return shown.length > 0 ? shown : all;
}

/** The weeks a month's page shows, Sunday first: whole weeks, so the first and the last may
 *  hold days of the months beside it. */
export function monthWeeks(isoOrMonth: string): string[][] {
  const range = monthGridRange(isoOrMonth);
  const days = datesOf(range);
  const weeks: string[][] = [];
  for (let i = 0; i < days.length; i += 7) weeks.push(days.slice(i, i + 7));
  return weeks;
}

/** From the Sunday of the month's first week to the Saturday of its last: at most 42 days. */
export function monthGridRange(isoOrMonth: string): DateRange {
  const month = monthRange(isoOrMonth);
  return { from: weekRange(month.from).from, to: weekRange(month.to).to };
}

/** What one request brings for a view: the day and the week share the week (so stepping
 *  through its days asks nothing more), the month its whole page. */
export function viewRange(view: CalendarView, anchor: string): DateRange {
  return view === "month" ? monthGridRange(anchor) : weekRange(anchor);
}

/** The day to show after "previous" (-1) or "next" (+1): a day, a week or a month away. */
export function shiftAnchor(view: CalendarView, anchor: string, step: 1 | -1): string {
  if (view === "day") return addDays(anchor, step);
  if (view === "week") return addDays(anchor, 7 * step);
  const edge = monthRange(anchor);
  // The first day of the month beside this one.
  return step === 1 ? addDays(edge.to, 1) : monthRange(addDays(edge.from, -1)).from;
}

const monthName = (iso: string) => formatMonth(iso).split(" ")[0] ?? "";
const dayOfMonth = (iso: string) => String(Number(iso.slice(8, 10)));

/** "יום ראשון, 15.11.2026" · "15 עד 21 בנובמבר 2026" · "נובמבר 2026". */
export function rangeTitle(view: CalendarView, anchor: string): string {
  if (view === "day") return formatDateLong(anchor);
  if (view === "month") return formatMonth(anchor);
  const { from, to } = weekRange(anchor);
  const year = (iso: string) => iso.slice(0, 4);
  if (monthOf(from) === monthOf(to)) return `${dayOfMonth(from)} עד ${dayOfMonth(to)} ב${monthName(to)} ${year(to)}`;
  if (year(from) === year(to)) return `${dayOfMonth(from)} ב${monthName(from)} עד ${dayOfMonth(to)} ב${monthName(to)} ${year(to)}`;
  return `${dayOfMonth(from)} ב${monthName(from)} ${year(from)} עד ${dayOfMonth(to)} ב${monthName(to)} ${year(to)}`;
}

/** The visits of each date, in the order given (the calendar answers by date and time). */
export function byDate(views: VisitView[]): Map<string, VisitView[]> {
  const days = new Map<string, VisitView[]>();
  for (const v of views) {
    const list = days.get(v.visit.date);
    if (list) list.push(v);
    else days.set(v.visit.date, [v]);
  }
  return days;
}

/** The order of the legend: her kinds as she chooses them, then one a later version wrote, and
 *  her own time last. */
const LEGEND_ORDER: VisitKind[] = [...VISIT_KINDS.filter((k) => k !== "block"), "unknown", "block"];

/** The kinds the legend explains: those of the visits shown on these dates (`null`: every date
 *  of the answer), a cancelled visit aside, in the legend's fixed order. */
export function kindsShown(views: VisitView[], dates: string[] | null): VisitKind[] {
  const on = dates === null ? null : new Set(dates);
  const seen = new Set<VisitKind>();
  for (const v of views) {
    if (v.visit.status === "cancelled" || (on !== null && !on.has(v.visit.date))) continue;
    seen.add(v.visit.kind);
  }
  return LEGEND_ORDER.filter((k) => seen.has(k));
}

/** A step she can take with a visit: minutes earlier (-) or later (+), days back (-) or ahead (+). */
export interface MoveBy {
  minutes?: number;
  days?: number;
}

/** Where a visit lands after a step; `null` when it would leave the day (before midnight or past it). */
export function movedTo(visit: Timed & { date: string }, by: MoveBy): { date: string; start: number } | null {
  const start = visit.start + (by.minutes ?? 0);
  if (start < 0 || start + visit.minutes > 1440) return null;
  if (dayNumber(visit.date) === null) return null;
  return { date: addDays(visit.date, by.days ?? 0), start };
}

const sameSlot = (a: VisitView, b: VisitView) =>
  a.visit.id !== b.visit.id && a.visit.status !== "cancelled" && b.visit.status !== "cancelled" && a.visit.date === b.visit.date
  && a.visit.start < b.visit.start + b.visit.minutes && b.visit.start < a.visit.start + a.visit.minutes;

/**
 * The calendar's answer as it will be once a move is saved, for showing at once: the visit at
 * its new date and hour; with "this and the following", every later planned visit of its series
 * that has no session note, at the new hour and moved by the same number of days (the core's
 * rule, D-176: a visit that has a note or was marked stays as it is); the list by date and
 * time again, and "overlaps" worked out anew for the days the answer holds.
 */
export function applyMove(views: VisitView[], id: string, to: { date: string; start: number }, scope: Scope): VisitView[] {
  const target = views.find((v) => v.visit.id === id)?.visit;
  if (!target) return views;
  const days = (dayNumber(to.date) ?? 0) - (dayNumber(target.date) ?? 0);
  const follows = (v: VisitView) =>
    scope === "following" && target.series_id !== null && v.visit.series_id === target.series_id && v.visit.status === "planned" && !v.has_note
    && (v.visit.date > target.date || (v.visit.date === target.date && v.visit.start > target.start));
  const next = views.map((v) => {
    if (v.visit.id === id) return { ...v, visit: { ...v.visit, date: to.date, start: to.start } };
    if (follows(v)) return { ...v, visit: { ...v.visit, date: addDays(v.visit.date, days), start: to.start } };
    return v;
  });
  next.sort((a, b) => (a.visit.date < b.visit.date ? -1 : a.visit.date > b.visit.date ? 1 : a.visit.start - b.visit.start));
  return next.map((v) => {
    const overlaps = next.some((o) => sameSlot(v, o));
    return overlaps === v.overlaps ? v : { ...v, overlaps };
  });
}
