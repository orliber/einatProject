// The calendar: day, week and month (docs/CLINIC.md, "The window").
// The day shown is the tab's own state; today comes from `useToday()`. One request per range
// shown: the day and the week share the week's answer, the month asks for its whole page.
// A visit is opened by pressing it; it is moved, without opening, by the buttons of the bar
// under the grid, or on top of the day's list in a day (or Alt and the arrow keys on the visit):
// at once on screen, and back where it was if the vault refuses. How to move one is behind the
// bar's "?" (D-204). The side of a visit is the colour of its kind, explained by the legend under
// the grid; this window only, never sent anywhere (D-202). The arithmetic is in calendarLayout.ts.
// Israel's holidays (D-196), when she has them on, are named in each day's head; a day the clinic
// is closed is shaded.
import { createContext, useContext, useEffect, useLayoutEffect, useMemo, useRef, useState, type KeyboardEvent, type MouseEvent, type ReactNode, type RefObject } from "react";
import { useApp } from "../../App";
import { HelpTip } from "../../components/HelpTip";
import { Segmented } from "../../components/Switch";
import { Dialog, PlusIcon, Skeleton } from "../../components/ui";
import { clinicApi } from "../../ipc/clinic";
import type { Holiday } from "../../ipc/generated/Holiday";
import type { Scope } from "../../ipc/generated/Scope";
import type { Today } from "../../ipc/generated/Today";
import type { Visit } from "../../ipc/generated/Visit";
import type { VisitKind } from "../../ipc/generated/VisitKind";
import type { VisitView } from "../../ipc/generated/VisitView";
import { TEXT_SIZE_EVENT } from "../../textSize";
import {
  applyMove, byDate, gridHeight, gridHours, hourMarks, kindsShown, minutesToPx, MONTH_ROWS, monthWeeks, movedTo, nowOffset, openingScroll, placeDay,
  rangeTitle, shiftAnchor, slotAt, STEP_MINUTES, suggestStart, viewRange, weekDays, type CalendarView, type Hours, type MoveBy,
} from "./calendarLayout";
import { useCalendar, useClinicSettings, useHolidays, useOptimistic, useReloadClinic, useToday } from "./clinicData";
import {
  dayName, formatDateLong, formatDateShort, formatMonth, formatSpan, formatTime, leftAloneWords, monthOf, VISIT_KIND_LABEL, VISIT_STATUS_LABEL,
  visitTitle, weekday,
} from "./format";
import { NoteDialog } from "./NoteDialog";
import { EmptyState, Pager, PatientName, ReadError, Section, StatusBadge, usePaged, useShownName, VisitRow } from "./parts";
import { VisitActions } from "./VisitActions";
import { VisitDialog, type VisitStart } from "./VisitDialog";
import "./CalendarTab.css";

/** Every cached answer of the calendar: a quick action shows on each range that holds the visit. */
const CALENDARS = ["clinic", "calendar"] as const;
const NO_VISITS: VisitView[] = [];
const NO_HOLIDAYS: Holiday[] = [];

/** The holidays of the range shown, by date (empty when she has them off). */
const HolidaysOf = createContext<Map<string, Holiday[]>>(new Map());

/** The holiday names of a day, in one quiet line; a closed day says so to a screen reader too.
 *  The names are a box of their own, which a day's head too narrow for them hides from the eye
 *  only (CalendarTab.css). */
function HolidayNames({ date, className }: { date: string; className: string }) {
  const list = useContext(HolidaysOf).get(date) ?? NO_HOLIDAYS;
  if (list.length === 0) return null;
  const closed = list.some((h) => h.closed);
  const names = list.map((h) => h.name).join(", ");
  return (
    <span className={`${className}${closed ? " cal-holiday-closed" : ""}`} title={names}>
      <span className="cal-holiday-names">
        {names}
        {closed && <span className="visually-hidden">, יום חג</span>}
      </span>
    </span>
  );
}

const isClosed = (map: Map<string, Holiday[]>, date: string) => (map.get(date) ?? NO_HOLIDAYS).some((h) => h.closed);

/** The weekday in one letter, as a narrow column of the grid shows it: "א׳" for Sunday. */
const SHORT_DAY_NAMES = ["א׳", "ב׳", "ג׳", "ד׳", "ה׳", "ו׳", "ש׳"];

const VIEWS: { value: CalendarView; label: string }[] = [{ value: "day", label: "יום" }, { value: "week", label: "שבוע" }, { value: "month", label: "חודש" }];
const PREVIOUS: Record<CalendarView, string> = { day: "היום הקודם", week: "השבוע הקודם", month: "החודש הקודם" };
const NEXT: Record<CalendarView, string> = { day: "היום הבא", week: "השבוע הבא", month: "החודש הבא" };
const SCOPE_LABEL: Record<Scope, string> = { one: "רק הפגישה הזו", following: "זו וכל הבאות" };

/** The visit she chose to move. `scope`: what a move of a visit of a series applies to, once
 *  she said (`null`: not asked yet). `view`: the visit as it should be now, shown while the
 *  calendar is being read again. */
interface Chosen {
  id: string;
  scope: Scope | null;
  view: VisitView;
}

interface Move {
  visit: Visit;
  to: { date: string; start: number };
  scope: Scope;
}

/** What is open over the tab: one dialog at a time. */
type Open =
  | { kind: "visit"; view: VisitView }
  | { kind: "new"; start?: VisitStart }
  | { kind: "note"; patientId: string; visit: Visit }
  | { kind: "scope"; view: VisitView; by: MoveBy | null; refocus: boolean }
  | null;

const markHeld = (v: VisitView, id: string): VisitView => (v.visit.id === id ? { ...v, visit: { ...v.visit, status: "held" } } : v);
const countText = (n: number) => (n === 0 ? "אין פגישות" : n === 1 ? "פגישה אחת" : `${n} פגישות`);

/** An arrow of the bar. In a right-to-left page the past is to the right: "previous" points there. */
function Arrow({ to }: { to: "right" | "left" }) {
  return (
    <svg width="18" height="18" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round" aria-hidden="true">
      <path d={to === "right" ? "m9 6 6 6-6 6" : "m15 6-6 6 6 6"} />
    </svg>
  );
}

/** Up and down: "move this visit". */
function MoveIcon() {
  return (
    <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2.4" strokeLinecap="round" strokeLinejoin="round" aria-hidden="true">
      <path d="M12 3v18" /><path d="m7 8 5-5 5 5" /><path d="m7 16 5 5 5-5" />
    </svg>
  );
}

export function CalendarTab() {
  const now = useToday();
  const { notify } = useApp();
  const reload = useReloadClinic();
  const [view, setView] = useState<CalendarView>("week");
  const [anchor, setAnchor] = useState(now.date);
  const [chosen, setChosen] = useState<Chosen | null>(null);
  /** Moves on their way to the vault: until they are back, the chosen visit stays as it was shown. */
  const [moving, setMoving] = useState(0);
  const [open, setOpen] = useState<Open>(null);
  const root = useRef<HTMLDivElement>(null);
  /** The visit to give the keyboard back to, once it is drawn in its new place. */
  const refocus = useRef<string | null>(null);
  /** Where the visit being moved will be once the steps on their way are saved. */
  const ahead = useRef<{ id: string; visit: Visit; steps: number } | null>(null);

  const range = viewRange(view, anchor);
  const calendar = useCalendar(range.from, range.to);
  const settings = useClinicSettings();
  const views = calendar.data;
  const s = settings.data;
  const holidaysRead = useHolidays(range.from, range.to, s?.holidays === true);
  const holidays = useMemo(() => {
    const map = new Map<string, Holiday[]>();
    if (s?.holidays !== true) return map;
    for (const h of holidaysRead.data ?? NO_HOLIDAYS) map.set(h.date, [...(map.get(h.date) ?? NO_HOLIDAYS), h]);
    return map;
  }, [holidaysRead.data, s?.holidays]);

  const days = useMemo(() => byDate(views ?? NO_VISITS), [views]);
  const workDays = s?.work_days;
  const shown = useMemo(() => {
    if (view === "day") return [anchor];
    if (view === "week") return weekDays(anchor, workDays ?? [], days.keys());
    return [];
  }, [view, anchor, workDays, days]);
  const dayStart = s?.day_start ?? 480;
  const dayEnd = s?.day_end ?? 1200;
  const hours = useMemo(
    () => gridHours(dayStart, dayEnd, shown.flatMap((d) => (days.get(d) ?? NO_VISITS).map((v) => v.visit))),
    [dayStart, dayEnd, shown, days],
  );
  // The legend explains the kinds on the page: the days shown, or the month's whole page.
  const legendKinds = useMemo(() => kindsShown(views ?? NO_VISITS, view === "month" ? null : shown), [views, shown, view]);

  // A visit marked "held" shows so at once.
  const hold = useOptimistic<VisitView[], Visit>(
    CALENDARS,
    (cached, visit) => cached.map((v) => markHeld(v, visit.id)),
    (visit) => clinicApi.saveVisit({ ...visit, status: "held", status_note: "", charge_missed: false }),
  );
  // A visit that was moved is drawn in its new place at once, and goes back if the vault refuses.
  const move = useOptimistic<VisitView[], Move>(
    CALENDARS,
    (cached, m) => applyMove(cached, m.visit.id, m.to, m.scope),
    async (m) => {
      const saved = await clinicApi.saveVisit({ ...m.visit, date: m.to.date, start: m.to.start }, null, m.scope);
      // Later visits of the series that have a note, or were marked, stayed where they were (D-176).
      const alone = leftAloneWords(saved.left_alone);
      if (alone !== "") notify(`${alone}.`);
    },
  );

  // After a move by the keyboard the visit is drawn elsewhere (another column is another
  // element): the keyboard stays on it. Only when the focus was lost, never when she took it
  // away: by the keyboard (it is on something else then) or by the pointer (any press).
  // Before the page is painted, so the next key still finds the visit.
  useLayoutEffect(() => {
    const id = refocus.current;
    if (id === null) return;
    const el = Array.from(root.current?.querySelectorAll<HTMLElement>("[data-visit]") ?? []).find((b) => b.dataset.visit === id);
    const active = document.activeElement;
    if (!el || active === el) return;
    if (active === null || active === document.body) el.focus();
    else refocus.current = null;
  });
  useEffect(() => {
    const forget = () => {
      refocus.current = null;
    };
    window.addEventListener("pointerdown", forget, true);
    return () => window.removeEventListener("pointerdown", forget, true);
  }, []);

  const live = chosen && view !== "month" ? views?.find((v) => v.visit.id === chosen.id) : undefined;
  // While the calendar is read again (another week, a move on its way) the bar keeps what it showed.
  const picked: VisitView | null = chosen && view !== "month" ? (live ?? (views === undefined || moving > 0 ? chosen.view : null)) : null;

  function show(nextView: CalendarView, nextAnchor: string) {
    refocus.current = null;
    setChosen(null);
    setView(nextView);
    setAnchor(nextAnchor);
  }

  function choose(v: VisitView) {
    setChosen((c) => (c?.id === v.visit.id ? c : { id: v.visit.id, scope: null, view: v }));
  }

  /** One step with a visit. A visit of a series asks first what the step applies to, once. */
  function step(v: VisitView, by: MoveBy, keyboard: boolean, given?: Scope) {
    // Two quick steps: the second starts where the first one put the visit, drawn yet or not.
    const visit = ahead.current?.id === v.visit.id ? ahead.current.visit : v.visit;
    if (visit.status !== "planned") return;
    const to = movedTo(visit, by);
    if (!to) return;
    const scope: Scope | null = visit.series_id === null ? "one" : (given ?? (chosen?.id === visit.id ? chosen.scope : null));
    if (scope === null) {
      setOpen({ kind: "scope", view: v, by, refocus: keyboard });
      return;
    }
    if (keyboard) refocus.current = visit.id;
    setChosen({ id: visit.id, scope: visit.series_id === null ? null : scope, view: { ...v, visit: { ...visit, ...to } } });
    // The page follows the visit: to the next day, or to the week it went to.
    const from = anchor;
    const follows = view === "day" ? to.date !== anchor : to.date < range.from || to.date > range.to;
    if (follows) setAnchor(to.date);
    ahead.current = { id: visit.id, visit: { ...visit, ...to }, steps: (ahead.current?.id === visit.id ? ahead.current.steps : 0) + 1 };
    setMoving((n) => n + 1);
    void move({ visit, to, scope }).then((ok) => {
      setMoving((n) => n - 1);
      const going = ahead.current;
      if (going?.id === visit.id) {
        going.steps -= 1;
        if (going.steps === 0 || !ok) ahead.current = null;
      }
      if (ok) return;
      // Refused: the visit is back where it was, and so is the page.
      setChosen((c) => (c?.id === visit.id ? { ...c, view: v } : c));
      if (follows) {
        if (keyboard) refocus.current = visit.id;
        setAnchor(from);
      }
    });
  }

  function answerScope(scope: Scope) {
    if (open?.kind !== "scope") return;
    const { view: v, by, refocus: keyboard } = open;
    setOpen(null);
    if (by) step(v, by, keyboard, scope);
    else setChosen((c) => (c?.id === v.visit.id ? { ...c, scope } : c));
  }

  const newOn = (date: string) => setOpen({ kind: "new", start: { date, start: suggestStart(date, now, dayStart, dayEnd) } });
  const grid: GridActions = {
    open: (v) => setOpen({ kind: "visit", view: v }),
    choose,
    toggle: (v) => setChosen((c) => (c?.id === v.visit.id ? null : { id: v.visit.id, scope: null, view: v })),
    release: () => {
      refocus.current = null;
      setChosen(null);
    },
    moveKey: (v, by) => step(v, by, true),
    slot: (date, start) => setOpen({ kind: "new", start: { date, start } }),
    add: newOn,
  };

  const error = calendar.error ?? settings.error;
  let body: ReactNode;
  if (error && (!views || !s)) {
    body = <ReadError error={error.message} retry={reload} />;
  } else if (!views || !s) {
    body = <div aria-busy="true"><Skeleton card lines={6} label="טוען את היומן" /></div>;
  } else if (view === "month") {
    body = (
      <>
        <MonthGrid anchor={anchor} days={days} today={now.date} onDay={(date) => show("day", date)} />
        <div className="cal-foot">
          {views.length === 0
            ? <p className="muted cal-empty">אין פגישות בחודש הזה. לחיצה על יום פותחת אותו, ושם אפשר לקבוע פגישה.</p>
            : <KindLegend kinds={legendKinds} />}
        </div>
      </>
    );
  } else {
    const timeGrid = (
      <TimeGrid dates={shown} hours={hours} days={days} now={now} chosenId={picked?.visit.id ?? null} actions={grid}
        opensAt={view === "day" ? "now" : "start"} scrollKey={view === "day" ? `day ${anchor}` : `week ${range.from}`}
        {...(view === "week" ? { onDay: (date: string) => show("day", date) } : {})} />
    );
    const empty = shown.every((d) => (days.get(d) ?? NO_VISITS).length === 0);
    // Under the grid: what the colours mean (or, on an empty page, how to start).
    const line = empty
      ? <p className="muted cal-empty">{`אין פגישות ${view === "day" ? "ביום" : "בשבוע"} הזה. כדי לקבוע פגישה לוחצים על שעה ביומן.`}</p>
      : <KindLegend kinds={legendKinds} />;
    const bar = picked ? (
      <MoveBar picked={picked} scope={chosen?.scope ?? null}
        onStep={(by) => step(picked, by, false)}
        onScope={() => setOpen({ kind: "scope", view: picked, by: null, refocus: false })}
        onOpen={() => setOpen({ kind: "visit", view: picked })}
        onDone={() => setChosen(null)} />
    ) : null;
    // Choosing a visit never moves the grid nor anything under the pointer (D-204 §5). In a week
    // the chosen visit's bar lies over the line under the grid, which stays under it unseen and
    // unread to keep its height. In a day it is on top of the day's list, beside the grid or
    // under it, and pushes the list down (CalendarTab.css).
    const foot = bar !== null && view === "week" ? (
      <>
        <div className="cal-foot-under" aria-hidden="true">{line}</div>
        <div className="cal-move-rail">{bar}</div>
      </>
    ) : line;
    const stage = <div className="cal-stage">{timeGrid}<div className="cal-foot">{foot}</div></div>;
    body = view === "week" ? stage : (
      <div className="cal-day-view">
        {stage}
        <DaySide bar={bar}>
          <Agenda key={anchor} date={anchor} now={now} visits={days.get(anchor) ?? NO_VISITS} onOpen={grid.open} onNew={() => newOn(anchor)}
            onHold={(v) => void hold(v)} onNote={(patientId, visit) => setOpen({ kind: "note", patientId, visit })} />
        </DaySide>
      </div>
    );
  }

  return (
    // A day or a week fills the page under the tabs, the month only takes the height it needs.
    <div className={view === "month" ? "cal" : "cal cal-fill"} ref={root}>
      <header className="cal-bar">
        <Segmented label="תצוגת היומן" value={view} options={VIEWS} onChange={(next) => show(next, anchor)} />
        <div className="row cal-nav">
          <button type="button" className="btn" onClick={() => show(view, now.date)}>היום</button>
          <button type="button" className="btn icon-btn" aria-label={PREVIOUS[view]} title={PREVIOUS[view]} onClick={() => show(view, shiftAnchor(view, anchor, -1))}><Arrow to="right" /></button>
          <button type="button" className="btn icon-btn" aria-label={NEXT[view]} title={NEXT[view]} onClick={() => show(view, shiftAnchor(view, anchor, 1))}><Arrow to="left" /></button>
        </div>
        <h2 className="cal-title" aria-live="polite">{rangeTitle(view, anchor)}</h2>
        {/* The "?" and the new visit are one group: when the bar folds in a narrow window, they go down together. */}
        <div className="cal-actions">
          <HelpTip key={view === "month" ? "month" : "grid"} label={view === "month" ? "איך פותחים יום" : "איך מזיזים פגישה"}>
            {view === "month" ? <p>לחיצה על יום פותחת אותו, ושם אפשר לקבוע פגישה.</p> : (
              <>
                <p>כדי להזיז פגישה בלי לפתוח אותה, מצביעים עליה ולוחצים על החצים שבפינה שלה.</p>
                <p>מהמקלדת: עומדים על הפגישה ולוחצים Alt עם החצים.</p>
                <p>כדי לקבוע פגישה לוחצים על שעה ביומן.</p>
              </>
            )}
          </HelpTip>
          <button type="button" className="btn btn-primary" onClick={() => (view === "day" ? newOn(anchor) : setOpen({ kind: "new" }))}><PlusIcon /> פגישה חדשה</button>
        </div>
      </header>

      <HolidaysOf.Provider value={holidays}>{body}</HolidaysOf.Provider>

      {open?.kind === "visit" && <VisitDialog view={open.view} onClose={() => setOpen(null)} />}
      {open?.kind === "new" && (open.start ? <VisitDialog start={open.start} onClose={() => setOpen(null)} /> : <VisitDialog onClose={() => setOpen(null)} />)}
      {open?.kind === "note" && <NoteDialog patientId={open.patientId} visit={open.visit} onClose={() => setOpen(null)} />}
      {open?.kind === "scope" && (
        <Dialog narrow title="פגישה מסדרה חוזרת" onClose={() => setOpen(null)}
          footer={<>
            <button type="button" className="btn" onClick={() => setOpen(null)}>ביטול</button>
            <span className="grow" />
            <button type="button" className="btn" onClick={() => answerScope("following")}>{SCOPE_LABEL.following}</button>
            <button type="button" className="btn btn-primary" autoFocus onClick={() => answerScope("one")}>{SCOPE_LABEL.one}</button>
          </>}>
          <div className="stack">
            <p className="cal-ask">הפגישה הזו חלק מסדרה חוזרת. מה להזיז?</p>
            <p className="hint">עם "{SCOPE_LABEL.following}", הפגישות הבאות בסדרה שעוד מתוכננות יזוזו יחד איתה.</p>
          </div>
        </Dialog>
      )}
    </div>
  );
}

// ------------------------------------------------------------------ under the grid

/** The bar of a visit chosen for moving (under the grid, or on top of the day's list): what it
 *  is, and the steps it can take. */
function MoveBar({ picked, scope, onStep, onScope, onOpen, onDone }: {
  picked: VisitView; scope: Scope | null;
  onStep: (by: MoveBy) => void; onScope: () => void; onOpen: () => void; onDone: () => void;
}) {
  const v = picked.visit;
  const self = useRef<HTMLDivElement>(null);
  useNoShrink(self, true);
  const can = (by: MoveBy) => movedTo(v, by) !== null;
  const steps: { label: string; keys: string; by: MoveBy }[] = [
    { label: "להקדים ברבע שעה", keys: "Alt וחץ למעלה", by: { minutes: -STEP_MINUTES } },
    { label: "לדחות ברבע שעה", keys: "Alt וחץ למטה", by: { minutes: STEP_MINUTES } },
    { label: "ליום הקודם", keys: "Alt וחץ ימינה", by: { days: -1 } },
    { label: "ליום הבא", keys: "Alt וחץ שמאלה", by: { days: 1 } },
  ];
  return (
    <div ref={self} className="cal-move cal-move-on" role="group" aria-label="הזזת הפגישה שנבחרה">
      <span className="cal-move-what" role="status">
        {v.kind === "block" ? <span className="clinic-name clinic-name-strong">{visitTitle(v)}</span> : <PatientName name={picked.patient_name} strong />}
        <span>{formatDateLong(v.date)}</span>
        <span className="num" dir="ltr">{formatSpan(v.start, v.minutes)}</span>
        {picked.overlaps && <span className="chip chip-warn">חופפת לפגישה אחרת</span>}
      </span>
      {v.status === "planned" ? (
        <>
          {steps.map((st) => (
            <button key={st.label} type="button" className="btn btn-small" title={st.keys} disabled={!can(st.by)} onClick={() => onStep(st.by)}>{st.label}</button>
          ))}
          {v.series_id !== null && scope !== null && (
            <span className="row muted small cal-move-scope">
              <span>ההזזה חלה על: {SCOPE_LABEL[scope]}</span>
              <button type="button" className="link-btn" onClick={onScope}>שינוי</button>
            </span>
          )}
        </>
      ) : (
        <span className="muted">פגישה שכבר סומנה לא מזיזים מכאן. אפשר לשנות אותה בפרטי הפגישה.</span>
      )}
      <button type="button" className="btn btn-small" onClick={onOpen}>פרטי הפגישה</button>
      <button type="button" className="btn btn-small btn-ghost" onClick={onDone}>סיום</button>
    </div>
  );
}

/** What the colour at a visit's side means: only the kinds on the page, in words from the code
 *  (never a visit's own title). Nothing when there is nothing to explain. */
function KindLegend({ kinds }: { kinds: VisitKind[] }) {
  if (kinds.length === 0) return null;
  return (
    <ul className="cal-legend" aria-label="מקרא: צבעי סוגי הפגישות">
      {kinds.map((k) => (
        <li key={k} className={`cal-legend-item cal-kind-${k}`}><span className="cal-swatch" aria-hidden="true" />{VISIT_KIND_LABEL[k]}</li>
      ))}
    </ul>
  );
}

// ------------------------------------------------------------------ the time grid (day, week)

interface GridActions {
  open: (view: VisitView) => void;
  /** The visit the keyboard is on is the chosen one. */
  choose: (view: VisitView) => void;
  /** The arrows at a visit's corner: chosen, or let go. */
  toggle: (view: VisitView) => void;
  release: () => void;
  moveKey: (view: VisitView, by: MoveBy) => void;
  /** A press on an empty half hour. */
  slot: (date: string, start: number) => void;
  /** The "add" control of a day. */
  add: (date: string) => void;
}

/** The hours down the side and a column for each day. The grid scrolls inside itself. */
function TimeGrid({ dates, hours, days, now, chosenId, actions, onDay, opensAt, scrollKey }: {
  dates: string[]; hours: Hours; days: Map<string, VisitView[]>; now: Today; chosenId: string | null; actions: GridActions;
  onDay?: (date: string) => void; opensAt: "start" | "now"; scrollKey: string;
}) {
  const scroller = useRef<HTMLDivElement>(null);
  const height = gridHeight(hours);
  const nowAt = dates.includes(now.date) ? nowOffset(now.minute, hours) : null;
  // A week opens at the top of its grid: the start of her working day, or the hour of an earlier
  // visit that week (D-204). A day opens at the hour of now when it is today.
  const startAt = useRef(0);
  useLayoutEffect(() => {
    startAt.current = openingScroll(opensAt, nowAt);
  });
  useLayoutEffect(() => {
    if (scroller.current) scroller.current.scrollTop = startAt.current;
  }, [scrollKey]);

  return (
    <div className="cal-grid" ref={scroller} role="group" aria-label="היומן לפי שעות" style={{ gridTemplateColumns: `56px repeat(${dates.length}, minmax(112px, 1fr))` }}>
      <div className="cal-gutter" aria-hidden="true">
        <div className="cal-corner" />
        <div className="cal-hours" style={{ height }}>
          {hourMarks(hours).map((m) => (
            <span key={m} className="cal-hour num" dir="ltr" style={{ top: minutesToPx(m - hours.start) }}>{formatTime(m)}</span>
          ))}
        </div>
      </div>
      {dates.map((date) => (
        <DayColumn key={date} date={date} visits={days.get(date) ?? NO_VISITS} hours={hours} height={height} today={date === now.date}
          nowAt={date === now.date ? nowAt : null} chosenId={chosenId} actions={actions} onDay={onDay} />
      ))}
    </div>
  );
}

function DayColumn({ date, visits, hours, height, today, nowAt, chosenId, actions, onDay }: {
  date: string; visits: VisitView[]; hours: Hours; height: number; today: boolean; nowAt: number | null; chosenId: string | null;
  actions: GridActions; onDay?: ((date: string) => void) | undefined;
}) {
  const placed = useMemo(() => new Map(placeDay(visits.map((v) => v.visit), hours).map((p) => [p.id, p])), [visits, hours]);
  const long = formatDateLong(date);
  const closed = isClosed(useContext(HolidaysOf), date);
  // Seen: "ראשון 15.11", or "א׳ 15.11" in a narrow column, where the date is never cut
  // (CalendarTab.css). Read out: "יום ראשון 15.11.2026"; in a narrow column with the letter she
  // sees too, "יום ראשון א׳ 15.11.2026", so the name holds what is on screen (WCAG 2.5.3).
  const wd = weekday(date);
  const name = (
    <>
      {wd !== 6 && <span className="visually-hidden">יום</span>}{" "}
      <span className="cal-day-weekday">{dayName(date)}</span>{" "}
      {wd !== null && <span className="cal-day-weekday-short">{SHORT_DAY_NAMES[wd]}</span>}{" "}
      <span className="num muted small" dir="ltr">{formatDateShort(date)}</span>
      <span className="visually-hidden">.{date.slice(0, 4)}</span>
    </>
  );

  /** A press on the empty part of the day: a new visit at that half hour. */
  function press(e: MouseEvent<HTMLDivElement>) {
    if (e.target !== e.currentTarget) return;
    actions.slot(date, slotAt(e.clientY - e.currentTarget.getBoundingClientRect().top, hours));
  }

  return (
    <section className={["cal-day", today ? "cal-day-today" : "", closed ? "cal-day-closed" : ""].filter(Boolean).join(" ")} aria-label={long}>
      <header className="cal-day-head">
        <h3>
          {onDay
            ? <button type="button" className="cal-day-name" onClick={() => onDay(date)}>{name}<span className="visually-hidden">, לתצוגת היום</span></button>
            : <span className="cal-day-name">{name}</span>}
        </h3>
        {/* A holiday's name takes what is left between the date and "היום", which stays beside "+" either way. */}
        <HolidayNames date={date} className="cal-holiday" />
        {today && <span className="chip">היום</span>}
        <button type="button" className="btn btn-small icon-btn cal-day-add" aria-label={`פגישה חדשה ב${long}`} title="פגישה חדשה ביום הזה" onClick={() => actions.add(date)}><PlusIcon size={15} /></button>
      </header>
      <div className="cal-day-body" style={{ height }} onClick={press}>
        {nowAt !== null && <span className="cal-now" style={{ top: nowAt }} aria-hidden="true" />}
        {visits.length > 0 && (
          <ul className="cal-day-visits" aria-label={`הפגישות של ${long}`}>
            {visits.map((v) => {
              const p = placed.get(v.visit.id);
              if (!p) return null;
              return (
                <GridVisit key={v.visit.id} view={v} chosen={v.visit.id === chosenId} actions={actions}
                  style={{ top: p.top, height: p.height, insetInlineStart: `${(p.column * 100) / p.columns}%`, width: `${100 / p.columns}%` }}
                  short={p.height < 40} />
              );
            })}
          </ul>
        )}
      </div>
    </section>
  );
}

/** One visit on the grid: a button that opens it, and at its corner the arrows that choose it for moving. */
function GridVisit({ view, chosen, short, style, actions }: {
  view: VisitView; chosen: boolean; short: boolean; style: { top: number; height: number; insetInlineStart: string; width: string }; actions: GridActions;
}) {
  const v = view.visit;
  const shownName = useShownName();
  const block = v.kind === "block";
  const who = block ? visitTitle(v) : shownName(view.patient_name);
  function onKey(e: KeyboardEvent<HTMLButtonElement>) {
    if (e.key === "Escape") {
      actions.release();
      return;
    }
    if (!e.altKey) return;
    // Up is earlier; right, in a right-to-left week, is the day before.
    const by: MoveBy | null = e.key === "ArrowUp" ? { minutes: -STEP_MINUTES }
      : e.key === "ArrowDown" ? { minutes: STEP_MINUTES }
      : e.key === "ArrowRight" ? { days: -1 }
      : e.key === "ArrowLeft" ? { days: 1 }
      : null;
    if (!by) return;
    e.preventDefault();
    actions.moveKey(view, by);
  }

  const classes = ["cal-visit", `cal-visit-${v.status}`, `cal-kind-${v.kind}`, block ? "cal-visit-block" : "", view.overlaps ? "cal-visit-overlap" : "", chosen ? "cal-visit-chosen" : "", short ? "cal-visit-short" : ""];
  return (
    <li className={classes.filter(Boolean).join(" ")} style={style} data-status={v.status}>
      {/* Its name is its own text, so what is read out holds what is seen: "09:00–10:30 תמר, מפגש אבחון, מתוכננת". */}
      <button type="button" className="cal-visit-open" data-visit={v.id} onClick={() => actions.open(view)} onFocus={() => actions.choose(view)} onKeyDown={onKey}>
        <span className="num cal-visit-time" dir="ltr">{formatTime(v.start)}<span className="visually-hidden">–{formatTime(v.start + v.minutes)}</span></span>
        {" "}
        <span className="cal-visit-who">{block ? who : <PatientName name={view.patient_name} strong />}</span>
        <span className="visually-hidden">,</span>
        {" "}
        {block ? <span className="visually-hidden">{VISIT_KIND_LABEL.block}</span> : <span className="cal-visit-kind">{visitTitle(v)}</span>}
        {!block && <span className="visually-hidden">,</span>}
        {" "}
        {!block && (v.status === "planned" ? <span className="visually-hidden">{VISIT_STATUS_LABEL.planned}</span> : <StatusBadge status={v.status} />)}
        {view.overlaps && <span className="visually-hidden">,</span>}
        {" "}
        {view.overlaps && <span className="chip chip-warn">חופפת{" "}<span className="visually-hidden">לפגישה אחרת</span></span>}
      </button>
      {v.status === "planned" && (
        // Not a Tab stop of its own: from the keyboard, the visit the focus is on is the chosen one.
        <button type="button" tabIndex={-1} className="cal-visit-move" aria-label={`הזזת הפגישה של ${who}, ${dayName(v.date)} ${formatTime(v.start)}`} title="הזזת הפגישה" aria-pressed={chosen} onClick={() => actions.toggle(view)}>
          <MoveIcon />
        </button>
      )}
    </li>
  );
}

/** While a visit is chosen, the box never gets shorter (D-204 §5). A step can take a line out
 *  of the bar ("חופפת לפגישה אחרת" going, a shorter day's name) or out of the day's list; on a
 *  page scrolled to its end, the page would get shorter, the browser would pull it down to its
 *  new end, and the button under the pointer would move away before the next click. So the box
 *  keeps the tallest height it had (`min-block-size`), and lets go when the bar goes ("סיום"),
 *  the window is resized or the text size changes (then it is measured again two frames later,
 *  once the bar's own height, `--cal-move-h`, has been taken anew). It is kept at its height to
 *  the fraction (`heightOf`), so it neither loses a part of a pixel nor grows by one and is
 *  measured again: it only ever grows, so a box at that height is never measured again, and
 *  nothing loops. (In the day, a bar that changes height sets `--cal-move-h` from inside its
 *  ResizeObserver, which makes the column taller from there: the browser then counts one
 *  "ResizeObserver loop" event, harmless, and measures the column in the next frame.) */
function useNoShrink(ref: RefObject<HTMLElement | null>, on: boolean) {
  useLayoutEffect(() => {
    const el = ref.current;
    if (!el || !on) return;
    let most = 0;
    /** Let go, and not measured until the page has settled (the frame waited for). */
    let settling = 0;
    const keep = () => {
      if (settling) return;
      const h = heightOf(el);
      if (h <= most) return;
      most = h;
      el.style.minBlockSize = `${h}px`;
    };
    const anew = () => {
      most = 0;
      el.style.removeProperty("min-block-size");
      if (typeof requestAnimationFrame !== "function") return keep();
      cancelAnimationFrame(settling);
      settling = requestAnimationFrame(() => {
        settling = requestAnimationFrame(() => {
          settling = 0;
          keep();
        });
      });
    };
    keep();
    const watch = typeof ResizeObserver === "undefined" ? null : new ResizeObserver(keep);
    // Its whole box: the day's column grows by its padding too (the room it makes for the bar).
    watch?.observe(el, { box: "border-box" });
    window.addEventListener("resize", anew);
    window.addEventListener(TEXT_SIZE_EVENT, anew);
    return () => {
      watch?.disconnect();
      window.removeEventListener("resize", anew);
      window.removeEventListener(TEXT_SIZE_EVENT, anew);
      if (settling) cancelAnimationFrame(settling);
      el.style.removeProperty("min-block-size");
    };
  }, [ref, on]);
}

/** A box's height as laid out, to the fraction, in its own pixels (also when the text size
 *  scales the window): its height on the screen divided by its zoom. `offsetHeight` rounds it,
 *  and the computed style keeps six digits: a box kept at such a height could lose a part of a
 *  pixel, or grow by it and be measured again. The browser lays a height out in 1/64 px after
 *  the zoom, rounding down, so a ten-thousandth of a pixel more keeps it exactly as it was. */
function heightOf(el: HTMLElement): number {
  const zoom = (el as HTMLElement & { currentCSSZoom?: number }).currentCSSZoom;
  const shown = el.getBoundingClientRect().height;
  if (zoom !== undefined && zoom > 0 && shown > 0) return shown / zoom + 0.0001;
  return parseFloat(getComputedStyle(el).blockSize) || el.offsetHeight;
}

// ------------------------------------------------------------------ the day's list

/** The day's list, with the chosen visit's bar on top of it (D-204 §5). The bar pushes the list
 *  down, and the grid beside it keeps its height even when it follows a list longer than the
 *  page: the column gives the bar's height back at its end (`--cal-move-h`, CalendarTab.css),
 *  so the row of the day counts the list alone. */
function DaySide({ bar, children }: { bar: ReactNode; children: ReactNode }) {
  const side = useRef<HTMLDivElement>(null);
  const shown = bar !== null;
  useNoShrink(side, shown);
  useLayoutEffect(() => {
    const el = side.current;
    const top = el?.querySelector(":scope > .cal-day-rail > .cal-move");
    if (!el || !shown || !(top instanceof HTMLElement)) return;
    // The bar's height, as it folds.
    const keep = () => el.style.setProperty("--cal-move-h", `${heightOf(top)}px`);
    keep();
    const watch = typeof ResizeObserver === "undefined" ? null : new ResizeObserver(keep);
    watch?.observe(top);
    return () => {
      watch?.disconnect();
      el.style.removeProperty("--cal-move-h");
    };
  }, [shown]);
  return <div className="cal-day-side" ref={side}>{shown && <div className="cal-day-rail">{bar}</div>}{children}</div>;
}

/** The day as a list beside its grid. Each visit offers one step by the time and the rest under
 *  "⋯", as on the front page (`VisitActions`, D-204); her own time has neither. */
function Agenda({ date, now, visits, onOpen, onNew, onHold, onNote }: {
  date: string; now: Today; visits: VisitView[]; onOpen: (view: VisitView) => void; onNew: () => void;
  onHold: (visit: Visit) => void; onNote: (patientId: string, visit: Visit) => void;
}) {
  const { shown, page, pages, setPage } = usePaged(visits, 12);
  return (
    <Section title="סדר היום" aside={<span className="muted small">{visits.length === 0 ? "" : countText(visits.length)}</span>}>
      {visits.length === 0 ? (
        <EmptyState title="אין פגישות ביום הזה" actions={<button type="button" className="btn" onClick={onNew}>לקבוע פגישה</button>}>
          <p>{formatDateLong(date)} עוד פנוי. אפשר גם ללחוץ על שעה ביומן.</p>
        </EmptyState>
      ) : (
        <>
          <ul className="clinic-list clinic-visits">
            {shown.map((view) => {
              const v = view.visit;
              // The key is the visit's and the step keeps its place: after "התקיימה" the focus
              // stays on the same button, now "לכתוב סיכום".
              const pid = v.kind !== "block" ? v.patient_id : null;
              return (
                <VisitRow key={v.id} view={view} onOpen={() => onOpen(view)}
                  actions={pid !== null
                    ? <VisitActions view={view} patientId={pid} now={now} onHold={onHold} onNote={(x) => onNote(pid, x)} onOpen={() => onOpen(view)} />
                    : undefined} />
              );
            })}
          </ul>
          <Pager page={page} pages={pages} onPage={setPage} label="עמודי סדר היום" />
        </>
      )}
    </Section>
  );
}

// ------------------------------------------------------------------ the month

/** The month as a table of weeks, Sunday first. A day lists its first visits and counts the rest. */
function MonthGrid({ anchor, days, today, onDay }: { anchor: string; days: Map<string, VisitView[]>; today: string; onDay: (date: string) => void }) {
  const weeks = useMemo(() => monthWeeks(anchor), [anchor]);
  const month = monthOf(anchor);
  return (
    <div className="cal-month-wrap">
      <table className="cal-month">
        <caption className="visually-hidden">{formatMonth(anchor)}</caption>
        <thead>
          <tr>{(weeks[0] ?? []).map((date) => <th key={date} scope="col">{dayName(date)}</th>)}</tr>
        </thead>
        <tbody>
          {weeks.map((week) => (
            <tr key={week[0]}>
              {week.map((date) => <MonthDay key={date} date={date} visits={days.get(date) ?? NO_VISITS} today={date === today} outside={monthOf(date) !== month} onDay={onDay} />)}
            </tr>
          ))}
        </tbody>
      </table>
    </div>
  );
}

function MonthDay({ date, visits, today, outside, onDay }: { date: string; visits: VisitView[]; today: boolean; outside: boolean; onDay: (date: string) => void }) {
  const more = visits.length - MONTH_ROWS;
  const closed = isClosed(useContext(HolidaysOf), date);
  const classes = ["cal-month-day", today ? "cal-month-today" : "", outside ? "cal-month-outside" : "", closed ? "cal-month-closed" : ""];
  return (
    // The whole day answers a press; the date is its button, for the keyboard (its press reaches the cell).
    <td className={classes.filter(Boolean).join(" ")} onClick={() => onDay(date)}>
      <div className="cal-month-top">
        {/* Seen: "15". Read out: "יום ראשון, 15.11.2026, 4 פגישות". */}
        <button type="button" className="cal-month-date">
          <span className="visually-hidden">{weekday(date) === 6 ? "" : "יום "}{dayName(date)},</span>
          {" "}
          <span className="num">{Number(date.slice(8, 10))}</span>
          <span className="visually-hidden">.{Number(date.slice(5, 7))}.{date.slice(0, 4)}, {countText(visits.length)}</span>
        </button>
        {today && <span className="chip">היום</span>}
        <span className="grow" />
        {more > 0 && (
          <span className="muted small cal-month-more"><span dir="ltr">+{more}</span><span className="visually-hidden"> פגישות נוספות</span></span>
        )}
      </div>
      <HolidayNames date={date} className="cal-holiday cal-month-holiday" />
      {visits.length > 0 && (
        <ul className="clinic-list cal-month-visits">
          {visits.slice(0, MONTH_ROWS).map((view) => {
            const v = view.visit;
            return (
              <li key={v.id} className={`cal-month-visit cal-visit-${v.status} cal-kind-${v.kind}${v.kind === "block" ? " cal-visit-block" : ""}`} data-status={v.status}>
                <span className="num cal-visit-time" dir="ltr">{formatTime(v.start)}</span>
                <span className="cal-visit-who">{v.kind === "block" ? visitTitle(v) : <PatientName name={view.patient_name} />}</span>
                {v.kind !== "block" && v.status !== "planned" && <span className="visually-hidden">, {VISIT_STATUS_LABEL[v.status]}</span>}
              </li>
            );
          })}
        </ul>
      )}
    </td>
  );
}
