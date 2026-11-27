// "תמונת מצב" (D-200): how the clinic is doing over a stretch of whole months. One row to pick
// the months (four ready stretches, or from a month to a month), then the stretch in figures:
// four tiles compared with the stretch just before, the months as two small column charts,
// cancellations and no-shows (and on which weekday), the assessment pathway's times, where
// new families come from, today's debts by age, visits by kind and income by method.
//
// Numbers only: no name and no id reach this screen. The one text of hers is where families
// were referred from; like the card and the waiting list, discreet mode hides it.
// The core computes every figure; this screen only lays them out. Each chart is drawn with
// plain elements (no library), hidden from a screen reader, and has its figures as text
// beside it or in a table.
import { useId, useMemo, useState, type ReactNode } from "react";
import { Segmented } from "../../components/Switch";
import { Skeleton, WarnIcon } from "../../components/ui";
import type { IncomeMonth } from "../../ipc/generated/IncomeMonth";
import type { Insights } from "../../ipc/generated/Insights";
import type { WeekdayCount } from "../../ipc/generated/WeekdayCount";
import { useInsights, useReloadClinic, useToday } from "./clinicData";
import { formatMoney, formatMonth, monthOf, PAY_METHOD_LABEL, VISIT_KIND_LABEL } from "./format";
import { barPercent } from "./MoneyTab";
import { EmptyState, Money, ReadError, Section, SelectField, useDiscreet } from "./parts";
// The months' table has the look of the money tab's tables.
import "./MoneyTab.css";
import "./InsightsTab.css";

// ------------------------------------------------------------------ months and stretches

/** The longest stretch the core gives a picture of. */
export const MAX_MONTHS = 24;
/** The first year a date of the clinic may have. */
const FIRST_YEAR = 2000;
/** How many years back the month lists reach. */
const YEARS_BACK = 5;

export interface MonthRange {
  from: string;
  to: string;
}

/** "2026-11" → one number (year × 12 + month − 1), so a stretch is a plain range. */
export function monthIndex(month: string): number {
  const [y = FIRST_YEAR, m = 1] = month.split("-").map(Number);
  return y * 12 + m - 1;
}

/** The month of `monthIndex`'s number; never before the clinic's first year. */
export function monthAt(index: number): string {
  const at = Math.max(FIRST_YEAR * 12, index);
  return `${Math.floor(at / 12)}-${String((at % 12) + 1).padStart(2, "0")}`;
}

/** The ready stretches, each ending with this month. */
export type Preset = "month" | "quarter" | "year" | "twelve";

export const PRESETS: { value: Preset; label: string }[] = [
  { value: "month", label: "החודש" },
  { value: "quarter", label: "3 החודשים האחרונים" },
  { value: "year", label: "מתחילת השנה" },
  { value: "twelve", label: "12 החודשים האחרונים" },
];

/** The months of a ready stretch, for `thisMonth` ("2026-11"). */
export function presetRange(preset: Preset, thisMonth: string): MonthRange {
  const at = monthIndex(thisMonth);
  const first = preset === "month" ? at : preset === "quarter" ? at - 2 : preset === "year" ? at - (at % 12) : at - 11;
  return { from: monthAt(first), to: thisMonth };
}

/** The stretch with a new first month: the last one moves only as far as it must (not
 *  before the first, and at most `MAX_MONTHS` months after it). */
export function withFrom(range: MonthRange, from: string): MonthRange {
  const f = monthIndex(from);
  const t = Math.min(Math.max(monthIndex(range.to), f), f + MAX_MONTHS - 1);
  return { from, to: monthAt(t) };
}

/** The stretch with a new last month: the first one moves only as far as it must. */
export function withTo(range: MonthRange, to: string): MonthRange {
  const t = monthIndex(to);
  const f = Math.max(Math.min(monthIndex(range.from), t), t - MAX_MONTHS + 1);
  return { from: monthAt(f), to };
}

/** "נובמבר 2026", "ספטמבר–נובמבר 2026", "דצמבר 2025–נובמבר 2026". */
export function periodName(from: string, to: string): string {
  if (from === to) return formatMonth(from);
  if (from.slice(0, 4) === to.slice(0, 4)) return `${formatMonth(from).split(" ")[0] ?? ""}–${formatMonth(to)}`;
  return `${formatMonth(from)}–${formatMonth(to)}`;
}

/** A share per thousand as a percent: 83 → "8.3%", 250 → "25%". */
export function percentText(perThousand: number): string {
  return `${(perThousand / 10).toFixed(1).replace(/\.0$/, "")}%`;
}

/** A round top for an axis, just above the largest value: 7 → 8, 34 → 40, 1,234,500 → 1,500,000. */
export function niceTop(max: number): number {
  if (max <= 0) return 0;
  const step = 10 ** Math.floor(Math.log10(max));
  for (const f of [1, 1.5, 2, 2.5, 3, 4, 5, 6, 8, 10]) if (f * step >= max) return f * step;
  return 10 * step;
}

/** 1234 → "1,234". */
const countText = (n: number) => String(n).replace(/\B(?=(\d{3})+(?!\d))/g, ",");

type Period = { preset: Preset } | { preset: "custom"; from: string; to: string };

/** The stretch she was on stays while the program is open (in memory only), so a look at
 *  another tab and back does not start the page over. A ready stretch follows the calendar. */
let periodNow: Period = { preset: "quarter" };

/** For the tests: each starts on "3 החודשים האחרונים". */
export function forgetInsightsPeriod() {
  periodNow = { preset: "quarter" };
}

// ------------------------------------------------------------------ the tab

export function InsightsTab() {
  const now = useToday();
  const thisMonth = monthOf(now.date);
  const [period, setPeriodState] = useState<Period>(periodNow);
  const range = period.preset === "custom" ? { from: period.from, to: period.to } : presetRange(period.preset, thisMonth);
  const insights = useInsights(range.from, range.to, now.date);
  const reload = useReloadClinic();
  // While another stretch is read, the last picture stays, dimmed: no jump, no flash.
  const [last, setLast] = useState<Insights | null>(null);
  if (insights.data !== undefined && insights.data !== last) setLast(insights.data);
  const shown = insights.data ?? last;

  function setPeriod(next: Period) {
    periodNow = next;
    setPeriodState(next);
  }

  return (
    <div className="stack insights">
      <header className="stack insights-head">
        <div className="insights-title">
          <h2>תמונת מצב</h2>
          <p className="muted insights-period" aria-live="polite">{periodName(range.from, range.to)}</p>
        </div>
        <PeriodPicker period={period} range={range} thisMonth={thisMonth} onChange={setPeriod} />
      </header>

      {insights.error && insights.data === undefined ? <ReadError error={insights.error.message} retry={reload} />
        : shown === null ? (
          <div className="stack" aria-busy="true">
            <Skeleton card lines={2} label="טוען את תמונת המצב" />
            <Skeleton card lines={6} label="טוען את החודשים" />
          </div>
        ) : (
          <div className={insights.data === undefined ? "stack insights-body insights-stale" : "stack insights-body"} aria-busy={insights.data === undefined || undefined}>
            <Picture i={shown} thisMonth={thisMonth} />
          </div>
        )}

      <p className="hint insights-foot">מספרים בלבד, בלי שמות. הכל מחושב במחשב הזה ולא נשלח לשום מקום.</p>
    </div>
  );
}

/** The ready stretches, and from a month to a month. */
function PeriodPicker({ period, range, thisMonth, onChange }: { period: Period; range: MonthRange; thisMonth: string; onChange: (p: Period) => void }) {
  const id = useId();
  // From January five years back to this month, the newest first (and the months shown, if
  // a stretch kept from before reaches past them).
  const options = useMemo(() => {
    const low = Math.min(monthIndex(range.from), monthIndex(`${Math.max(FIRST_YEAR, Number(thisMonth.slice(0, 4)) - YEARS_BACK)}-01`));
    const high = Math.max(monthIndex(range.to), monthIndex(thisMonth));
    return Array.from({ length: high - low + 1 }, (_, i) => monthAt(high - i)).map((m) => ({ value: m, label: formatMonth(m) }));
  }, [range.from, range.to, thisMonth]);
  return (
    <div className="insights-picker">
      <Segmented<Period["preset"]> label="תקופה" value={period.preset} options={PRESETS} onChange={(p) => onChange(p === "custom" ? { preset: "custom", ...range } : { preset: p })} />
      <div className="row insights-months" role="group" aria-label="מחודש עד חודש">
        <SelectField id={`${id}-from`} label="מחודש" value={range.from} options={options} className="insights-month"
          onChange={(from) => onChange({ preset: "custom", ...withFrom(range, from) })} />
        <SelectField id={`${id}-to`} label="עד חודש" value={range.to} options={options} className="insights-month"
          onChange={(to) => onChange({ preset: "custom", ...withTo(range, to) })} />
        <p className="hint insights-months-hint">עד {MAX_MONTHS} חודשים בבת אחת.</p>
      </div>
    </div>
  );
}

// ------------------------------------------------------------------ the picture

function Picture({ i, thisMonth }: { i: Insights; thisMonth: string }) {
  const before = periodName(i.before.from_month, i.before.to_month);
  const quiet = i.held + i.cancelled + i.no_show === 0 && i.charged === 0 && i.received === 0 && i.new_patients === 0 && i.waiting_now === 0
    && i.debt.every((d) => d.patients === 0 && d.amount === 0)
    && i.before.held === 0 && i.before.received === 0 && i.before.new_patients === 0;
  if (quiet) {
    return (
      <EmptyState title="אין עדיין נתונים לתקופה הזו">
        <p>כשיירשמו פגישות, מטופלים ותקבולים, תמונת המצב תתמלא מעצמה.</p>
      </EmptyState>
    );
  }
  // This month is not over: its figures will still grow.
  const open = i.from_month <= thisMonth && thisMonth <= i.to_month;
  return (
    <>
      <ul className="insights-tiles" aria-label="התקופה במספרים">
        <Tile label="פגישות שהתקיימו" value={<Num n={i.held} />}
          foot={<Change diff={i.held - i.before.held} amount={<Num n={Math.abs(i.held - i.before.held)} />} before={before} />} />
        <Tile label="התקבל" value={<Money agorot={i.received} />}
          foot={<Change diff={i.received - i.before.received} amount={<Money agorot={Math.abs(i.received - i.before.received)} />} before={before} />} />
        <Tile label="מטופלים חדשים" value={<Num n={i.new_patients} />}
          foot={<Change diff={i.new_patients - i.before.new_patients} amount={<Num n={Math.abs(i.new_patients - i.before.new_patients)} />} before={before} />} />
        <Tile label="תפוסה" value={i.work_minutes === 0 ? <None /> : <span className="num" dir="ltr">{percentText(i.occupancy)}</span>}
          foot={i.work_minutes === 0 ? "אין שעות עבודה בתקופה הזו"
            : <><Hours minutes={i.held_minutes} /> שעות פגישה מתוך <Hours minutes={i.work_minutes} /> שעות עבודה</>} />
      </ul>
      {open && <p className="hint">{formatMonth(thisMonth)} עוד לא הסתיים, ולכן ההשוואה לתקופה שלפני עוד לא שלמה.</p>}

      <MonthsSection months={i.months} />

      <div className="insights-columns">
        <Cancellations i={i} />
        <Pathway i={i} />
      </div>
      <div className="insights-columns">
        <Referrals i={i} />
        <Debts i={i} />
      </div>
      <div className="insights-columns">
        <Section title="לפי סוג פגישה" aside={<span className="muted small">פגישות שהתקיימו</span>}>
          {i.by_kind.length === 0 ? <p className="muted clinic-quiet">עוד לא התקיימו פגישות בתקופה הזו.</p>
            : <Ranked label="פגישות שהתקיימו לפי סוג" rows={i.by_kind.map((k) => ({ key: k.kind, name: VISIT_KIND_LABEL[k.kind], value: k.count, figure: <Num n={k.count} /> }))} />}
        </Section>
        <Section title="התקבל לפי אמצעי תשלום">
          {i.by_method.length === 0 ? <p className="muted clinic-quiet">עוד לא התקבלו תשלומים בתקופה הזו.</p>
            : <Ranked label="התקבל לפי אמצעי תשלום"
              rows={[...i.by_method].sort((a, b) => b.amount - a.amount).map((m) => ({ key: m.method, name: PAY_METHOD_LABEL[m.method], value: m.amount, figure: <Money agorot={m.amount} /> }))} />}
        </Section>
      </div>
    </>
  );
}

// ------------------------------------------------------------------ small pieces

/** A whole number, in its own direction: "1,234". */
function Num({ n }: { n: number }) {
  return <span className="num" dir="ltr">{countText(n)}</span>;
}

/** What a median, or a figure, shows when there is nothing to count yet. */
function None() {
  return <span className="insights-none">אין עדיין נתונים</span>;
}

/** Minutes as hours, to one decimal: 2490 → "41.5". */
function Hours({ minutes }: { minutes: number }) {
  return <span className="num" dir="ltr">{countText(Math.round(minutes / 6) / 10)}</span>;
}

/** Days in her words: "באותו יום", "יום אחד", "יומיים", "45 ימים (כ-6 שבועות)"; none: "אין עדיין נתונים". */
function Days({ n }: { n: number | null }) {
  if (n === null) return <None />;
  const words = n === 0 ? "באותו יום" : n === 1 ? "יום אחד" : n === 2 ? "יומיים" : null;
  return (
    <span>
      {words ?? <><Num n={n} /> ימים</>}
      {n >= 14 && <span className="muted"> (כ-<Num n={Math.round(n / 7)} /> שבועות)</span>}
    </span>
  );
}

/** "משפחה אחת", "3 משפחות". */
function Families({ n }: { n: number }) {
  return n === 1 ? <>משפחה אחת</> : <><Num n={n} /> משפחות</>;
}

/** One figure of the stretch: what it is, its value, and a line under it. */
function Tile({ label, value, foot }: { label: string; value: ReactNode; foot: ReactNode }) {
  return (
    <li className="card insights-tile">
      <span className="insights-tile-label">{label}</span>
      <span className="insights-tile-value">{value}</span>
      <span className="insights-tile-foot">{foot}</span>
    </li>
  );
}

/** The change from the stretch before: "▲ 12 לעומת יוני–אוגוסט 2026", said as words to a
 *  screen reader ("עלייה של 12"). Up is good for every tile that has one. */
function Change({ diff, amount, before }: { diff: number; amount: ReactNode; before: string }) {
  if (diff === 0) return <span className="insights-change">ללא שינוי לעומת {before}</span>;
  const up = diff > 0;
  return (
    <span className={up ? "insights-change insights-change-up" : "insights-change"}>
      <span aria-hidden="true">{up ? "▲" : "▼"}</span>
      <span className="visually-hidden">{up ? "עלייה של" : "ירידה של"}</span> {amount} לעומת {before}
    </span>
  );
}

/** Rows of a name, a bar and a figure, the bars on one baseline, the largest the longest.
 *  The bar is for the eye; the figure beside it is the data. */
function Ranked({ label, rows }: { label: string; rows: { key: string; name: ReactNode; value: number; figure: ReactNode; warn?: boolean }[] }) {
  const most = Math.max(0, ...rows.map((r) => r.value));
  return (
    <ul className="clinic-list insights-ranked" aria-label={label}>
      {rows.map((r) => (
        <li key={r.key} className={r.warn ? "insights-rank insights-rank-warn" : "insights-rank"}>
          <span className="insights-rank-name">{r.name}</span>
          <span className="insights-rank-bar" aria-hidden="true">
            {r.value > 0 && <span className="insights-hbar" style={{ width: `${barPercent(r.value, most)}%` }} />}
          </span>
          <span className="insights-rank-figure">{r.figure}</span>
        </li>
      ))}
    </ul>
  );
}

// ------------------------------------------------------------------ the months

const SHORT_MONTHS = ["ינו׳", "פבר׳", "מרץ", "אפר׳", "מאי", "יוני", "יולי", "אוג׳", "ספט׳", "אוק׳", "נוב׳", "דצמ׳"] as const;

const heldWords = (n: number) => (n === 1 ? "פגישה אחת" : `${countText(n)} פגישות`);

function MonthsSection({ months }: { months: IncomeMonth[] }) {
  const [asTable, setAsTable] = useState(false);
  const caption = months.length > 0 ? `${periodName(months[0]?.month ?? "", months[months.length - 1]?.month ?? "")} לפי חודשים` : "לפי חודשים";
  return (
    <Section title="לפי חודשים" aside={<button type="button" className="btn btn-small btn-ghost" aria-pressed={asTable} onClick={() => setAsTable(!asTable)}>הצגה כטבלה</button>}>
      {!asTable && (
        <div className="insights-charts">
          <Columns title="פגישות שהתקיימו" months={months} value={(m) => m.held} tick={countText} tip={(m) => heldWords(m.held)} />
          <Columns title="התקבל" months={months} value={(m) => m.received} tick={formatMoney} tip={(m) => formatMoney(m.received)} />
        </div>
      )}
      {/* The charts' figures, for a screen reader always, and on the screen when she asks. */}
      <div className={asTable ? "insights-table" : "visually-hidden"}>
        <table className="clinic-table">
          <caption>{caption}</caption>
          <thead>
            <tr>
              <th scope="col">חודש</th>
              <th scope="col" className="clinic-num">התקיימו</th>
              <th scope="col" className="clinic-num">בוטלו</th>
              <th scope="col" className="clinic-num">לא הגיעו</th>
              <th scope="col" className="clinic-num">חויב</th>
              <th scope="col" className="clinic-num">התקבל</th>
            </tr>
          </thead>
          <tbody>
            {months.map((m) => (
              <tr key={m.month}>
                <th scope="row">{formatMonth(m.month)}</th>
                <td className="clinic-num"><Num n={m.held} /></td>
                <td className="clinic-num"><Num n={m.cancelled} /></td>
                <td className="clinic-num"><Num n={m.no_show} /></td>
                <td className="clinic-num"><Money agorot={m.charged} /></td>
                <td className="clinic-num"><Money agorot={m.received} /></td>
              </tr>
            ))}
          </tbody>
          {months.length > 1 && (
            <tfoot>
              <tr>
                <th scope="row">סך התקופה</th>
                <td className="clinic-num"><Num n={months.reduce((s, m) => s + m.held, 0)} /></td>
                <td className="clinic-num"><Num n={months.reduce((s, m) => s + m.cancelled, 0)} /></td>
                <td className="clinic-num"><Num n={months.reduce((s, m) => s + m.no_show, 0)} /></td>
                <td className="clinic-num"><Money agorot={months.reduce((s, m) => s + m.charged, 0)} /></td>
                <td className="clinic-num"><Money agorot={months.reduce((s, m) => s + m.received, 0)} /></td>
              </tr>
            </tfoot>
          )}
        </table>
      </div>
    </Section>
  );
}

/** One measure by month as columns, the first month at the right (the way the page reads):
 *  one series, so its title names it and there is no legend. The axis has its top and 0, the
 *  highest month carries its value, and each month shows its figure on hover. Hidden from a
 *  screen reader: the table of the months says the same. */
function Columns({ title, months, value, tick, tip }: {
  title: string; months: IncomeMonth[]; value: (m: IncomeMonth) => number; tick: (n: number) => string; tip: (m: IncomeMonth) => string;
}) {
  const values = months.map(value);
  const max = Math.max(0, ...values);
  const top = niceTop(max);
  const peak = values.indexOf(max);
  // Past a year, every second or third month is named under the axis (the last one always).
  const every = months.length > 18 ? 3 : months.length > 12 ? 2 : 1;
  const manyYears = months.length > 0 && months[0]?.month.slice(0, 4) !== months[months.length - 1]?.month.slice(0, 4);
  return (
    <figure className="insights-chart">
      <figcaption className="insights-chart-title">{title}</figcaption>
      {max === 0 ? <p className="muted insights-chart-empty">אין עדיין נתונים בתקופה הזו.</p> : (
        <div className="insights-plot" aria-hidden="true">
          <div className="insights-y">
            <span className="insights-y-top">{tick(top)}</span>
            <span className="insights-y-zero">0</span>
          </div>
          <div className="insights-area">
            <div className="insights-grid" />
            <div className="insights-cols">
              {months.map((m, at) => {
                const v = values[at] ?? 0;
                const [year = "", month = "1"] = m.month.split("-");
                const named = (months.length - 1 - at) % every === 0;
                const edge = at === 0 ? " insights-peak-start" : at === months.length - 1 ? " insights-peak-end" : "";
                return (
                  <div key={m.month} className="insights-col">
                    <div className="insights-col-plot">
                      {v > 0 && (
                        <span className="insights-col-bar" style={{ height: `${(v / top) * 100}%` }}>
                          {at === peak && <span className={`insights-peak${edge}`}>{tick(v)}</span>}
                        </span>
                      )}
                      <span className="insights-tip">
                        <strong>{tip(m)}</strong>
                        <span>{formatMonth(m.month)}</span>
                      </span>
                    </div>
                    <span className="insights-col-label">
                      {named ? SHORT_MONTHS[Number(month) - 1] : ""}
                      {manyYears && (at === 0 || month === "01") && <span className="insights-col-year">{year}</span>}
                    </span>
                  </div>
                );
              })}
            </div>
          </div>
        </div>
      )}
    </figure>
  );
}

// ------------------------------------------------------------------ cancellations and no-shows

const DAY_NAMES = ["ראשון", "שני", "שלישי", "רביעי", "חמישי", "שישי", "שבת"] as const;

function Cancellations({ i }: { i: Insights }) {
  const total = i.held + i.cancelled + i.no_show;
  return (
    <Section title="ביטולים ואי-הגעות" aside={total > 0 ? <span className="muted small">מתוך <Num n={total} /> פגישות</span> : undefined}>
      {total === 0 ? <p className="muted clinic-quiet">אין עדיין נתונים. עוד לא סומנו פגישות בתקופה הזו.</p> : (
        <>
          <dl className="insights-rates">
            <div>
              <dt>בוטלו</dt>
              <dd><span className="insights-rate num" dir="ltr">{percentText(i.cancel_rate)}</span> <span className="muted">({heldWords(i.cancelled)})</span></dd>
            </div>
            <div>
              <dt>לא הגיעו</dt>
              <dd><span className="insights-rate num" dir="ltr">{percentText(i.no_show_rate)}</span> <span className="muted">({heldWords(i.no_show)})</span></dd>
            </div>
          </dl>
          <Weekdays days={i.by_weekday} />
        </>
      )}
    </Section>
  );
}

/** Each weekday with visits: one bar as long as its visits (against the busiest day), split
 *  into those held and those cancelled or missed, and the same in words beside it. */
function Weekdays({ days }: { days: WeekdayCount[] }) {
  const id = useId();
  const rows = days.map((d) => ({ ...d, total: d.held + d.cancelled + d.no_show })).filter((d) => d.total > 0);
  if (rows.length === 0) return null;
  const most = Math.max(...rows.map((d) => d.total));
  return (
    <div className="stack insights-weekdays">
      <div className="row insights-weekdays-head">
        <h3 id={id}>לפי יום בשבוע</h3>
        <span className="grow" />
        <ul className="row insights-legend" aria-hidden="true">
          <li><span className="insights-swatch" />התקיימו</li>
          <li><span className="insights-swatch insights-swatch-missed" />בוטלו או לא הגיעו</li>
        </ul>
      </div>
      <ul className="clinic-list insights-days" aria-labelledby={id}>
        {rows.map((d) => {
          const missed = d.cancelled + d.no_show;
          return (
            <li key={d.weekday} className="insights-day">
              <span className="insights-day-name">{DAY_NAMES[d.weekday] ?? ""}</span>
              <span className="insights-day-bar" aria-hidden="true">
                <span className="insights-stack" style={{ width: `${barPercent(d.total, most)}%` }}>
                  {d.held > 0 && <span className="insights-seg" style={{ flexGrow: d.held }} />}
                  {missed > 0 && <span className="insights-seg insights-seg-missed" style={{ flexGrow: missed }} />}
                </span>
              </span>
              <span className="insights-day-text">
                <Num n={d.held} /> התקיימו
                {missed > 0 && <> · <Num n={missed} /> בוטלו או לא הגיעו (<span className="num" dir="ltr">{percentText(Math.round((missed * 1000) / d.total))}</span>)</>}
              </span>
            </li>
          );
        })}
      </ul>
    </div>
  );
}

// ------------------------------------------------------------------ the assessment pathway

function Pathway({ i }: { i: Insights }) {
  return (
    <Section title="מסלול האבחון">
      <dl className="insights-facts">
        <div className="insights-fact">
          <dt>אבחונים שהסתיימו <span className="muted small">(שיחות משוב שהתקיימו בתקופה)</span></dt>
          <dd><Num n={i.assessments_done} /></dd>
        </div>
        <div className="insights-fact">
          <dt>מפגישת ההיכרות עד שיחת המשוב</dt>
          <dd><Days n={i.intake_to_feedback_days} /></dd>
        </div>
        <div className="insights-fact">
          <dt>מהמפגש האחרון של האבחון עד שיחת המשוב</dt>
          <dd><Days n={i.assessment_to_feedback_days} /></dd>
        </div>
        <div className="insights-fact">
          <dt>המתנה עד הפגישה הראשונה</dt>
          <dd><Days n={i.waited_days} /></dd>
        </div>
        <div className="insights-fact">
          <dt>ברשימת ההמתנה היום</dt>
          <dd>
            {i.waiting_now === 0 ? "אין משפחות שממתינות" : <Families n={i.waiting_now} />}
            {i.waiting_now_days !== null && <span className="muted small"> · ממתינות <Days n={i.waiting_now_days} /></span>}
          </dd>
        </div>
      </dl>
      <p className="hint">הזמנים הם חציון: לחצי מהמשפחות זה לקח פחות, ולחצי יותר.</p>
    </Section>
  );
}

// ------------------------------------------------------------------ referrals

function Referrals({ i }: { i: Insights }) {
  const discreet = useDiscreet();
  const total = i.referrals.reduce((s, r) => s + r.count, 0);
  return (
    <Section title="מאיפה מגיעים" aside={<span className="muted small">{i.new_patients === 1 ? "מטופל חדש אחד" : <><Num n={i.new_patients} /> מטופלים חדשים</>}</span>}>
      {i.referrals.length === 0 ? <p className="muted clinic-quiet">לא נפתחו כרטיסים חדשים בתקופה הזו.</p>
        // Her words for who referred a family may name a person: hidden while a family may look on.
        : discreet ? <p className="muted clinic-quiet">מקורות ההפניה מוסתרים במצב דיסקרטי.</p> : (
        <>
          <Ranked label="מקורות ההפניה" rows={i.referrals.map((r, n) => ({
            // Her own words may repeat the "אחרים" row's name: the place keeps the key unique.
            key: `${n}-${r.source}`,
            name: r.source,
            value: r.count,
            figure: <><Num n={r.count} /> <span className="muted">(<span className="num" dir="ltr">{percentText(Math.round((r.count * 1000) / Math.max(1, total)))}</span>)</span></>,
          }))} />
          <p className="hint">לפי השדה "מי הפנה" בכרטיסים שנפתחו בתקופה.</p>
        </>
      )}
    </Section>
  );
}

// ------------------------------------------------------------------ debts

function Debts({ i }: { i: Insights }) {
  const total = i.debt.reduce((s, d) => ({ patients: s.patients + d.patients, amount: s.amount + d.amount }), { patients: 0, amount: 0 });
  const oldest = Math.max(...i.debt.map((d) => d.from_days));
  return (
    <Section title="חובות לפי ותק" aside={<span className="muted small">נכון להיום</span>}>
      {total.amount === 0 ? <p className="muted clinic-quiet">אין חובות פתוחים היום. כל מה שחויב שולם.</p> : (
        <>
          <Ranked label="חובות לפי ותק" rows={i.debt.map((d) => {
            const warn = d.from_days === oldest && d.amount > 0;
            return {
              key: d.label,
              name: <>{warn && <WarnIcon />}{d.label}</>,
              value: d.amount,
              warn,
              figure: d.patients === 0 ? <span className="muted">אין</span> : <><Families n={d.patients} /> · <Money agorot={d.amount} /></>,
            };
          })} />
          <p className="insights-debt-total">סך הכל: <Families n={total.patients} />, <Money agorot={total.amount} tone="owed" /></p>
          <p className="hint">כל משפחה נספרת לפי החיוב הוותיק ביותר שעוד לא שולם. הפירוט בלשונית "כספים".</p>
        </>
      )}
    </Section>
  );
}

