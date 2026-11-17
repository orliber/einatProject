// Money, in two views. "יומן הכנסות": a month day by day (sessions, charges, payments), with
// what was voided still there and struck through, what was changed marked, the month's
// totals, and under it who still owes. "סיכום שנתי": the twelve months of a year as a table
// with a plain bar beside each. The page says, always, that the program is not registered
// with the tax authority (D-172). Nothing here is sent. One quiet button writes the payments
// of a month or a year to a file for the accountant, with no name in it (D-194): the core
// makes the file and the shell saves it; the window only says where.
import { useMemo, useRef, useState, type ReactNode } from "react";
import { useApp } from "../../App";
import { Segmented } from "../../components/Switch";
import { Dialog, ErrorLine, Skeleton } from "../../components/ui";
import { asUiError, ipc } from "../../ipc/client";
import type { Alert } from "../../ipc/generated/Alert";
import type { DiaryDay } from "../../ipc/generated/DiaryDay";
import type { DiaryLine } from "../../ipc/generated/DiaryLine";
import type { DiaryMonth } from "../../ipc/generated/DiaryMonth";
import type { IncomeYear } from "../../ipc/generated/IncomeYear";
import type { PatientRow } from "../../ipc/generated/PatientRow";
import type { DiaryErased } from "../../ipc/generated/DiaryErased";
import { useClinicSettings, useDiary, useHistory, useIncome, useOverview, usePatients, useReloadClinic, useToday, useYearErased } from "./clinicData";
import { formatDate, formatDateLong, formatMoney, formatMonth, formatTime, monthOf, PAY_METHOD_LABEL, VISIT_KIND_LABEL } from "./format";
import { EmptyState, HistoryEntries, Money, Pager, PatientName, ReadError, Section, StatusBadge, useClinicNav, useDiscreet, usePaged, useShownName } from "./parts";
import { SendMenu } from "./SendMenu";
import "./MoneyTab.css";

// ------------------------------------------------------------------ pure pieces

/** The first and the last month (and year) a date of the clinic may have. */
const FIRST_YEAR = 2000;
const LAST_YEAR = 2200;

/** "2026-11" moved by `by` months; `null` past the years the clinic keeps. */
export function shiftMonth(month: string, by: number): string | null {
  const [y = 0, m = 1] = month.split("-").map(Number);
  const at = y * 12 + (m - 1) + by;
  const year = Math.floor(at / 12);
  if (year < FIRST_YEAR || year > LAST_YEAR) return null;
  return `${year}-${String((at % 12) + 1).padStart(2, "0")}`;
}

/** A payment that counts and has no receipt number yet. */
export const lacksReceipt = (line: DiaryLine) => line.received > 0 && !line.voided && line.receipt.trim() === "";

/** The days of a month with only the payments that have no receipt number. */
export function daysLackingReceipts(days: DiaryDay[]): DiaryDay[] {
  return days.map((d) => ({ ...d, lines: d.lines.filter(lacksReceipt) })).filter((d) => d.lines.length > 0);
}

/** How many payments of the days shown have no receipt number. */
export function missingReceipts(days: DiaryDay[]): number {
  return days.reduce((n, d) => n + d.lines.filter(lacksReceipt).length, 0);
}

export interface OpenBalance {
  row: PatientRow;
  /** The date of the oldest charge still uncovered, when the clinic already points the debt out. */
  since: string | null;
}

/** Who owes: the debts the clinic points out first (the oldest first), then the fresher
 *  ones, the largest first. */
export function openBalances(rows: PatientRow[], alerts: Alert[]): OpenBalance[] {
  const since = new Map<string, string>();
  for (const a of alerts) if (a.kind === "debt" && a.patient_id !== null) since.set(a.patient_id, a.since);
  return rows
    .filter((r) => r.due > 0)
    .map((row) => ({ row, since: since.get(row.patient.id) ?? null }))
    .sort((a, b) => {
      if (a.since !== null && b.since !== null) return a.since < b.since ? -1 : a.since > b.since ? 1 : b.row.due - a.row.due;
      if (a.since !== null) return -1;
      if (b.since !== null) return 1;
      return b.row.due - a.row.due || (a.row.patient.id < b.row.patient.id ? -1 : 1);
    });
}

/** The share of `value` in `max`, as a whole percent for a bar's length (0 when there is no max). */
export function barPercent(value: number, max: number): number {
  return max <= 0 || value <= 0 ? 0 : Math.max(1, Math.round((value / max) * 100));
}

const visitsWord = (n: number) => (n === 1 ? "פגישה אחת" : `${n} פגישות`);

/** What the file for the accountant covers: a month, or a whole year. */
export type ExportKind = "month" | "year";

/** The period the core is asked for (D-194): "2026-11" for a month, "2026" for its year. */
export function exportPeriod(kind: ExportKind, month: string): string {
  return kind === "month" ? month : month.slice(0, 4);
}

type View = "diary" | "year";

/** The view she was on stays while the program is open (in memory only), so a look at a
 *  card and back does not start the page over. */
let viewNow: View = "diary";

/** For the tests: each starts on the diary. */
export function forgetMoneyView() {
  viewNow = "diary";
}

// ------------------------------------------------------------------ the tab

export function MoneyTab() {
  const now = useToday();
  const [view, setView] = useState<View>(viewNow);
  return (
    <div className="stack clinic-money-tab">
      {/* Above both views: the yearly summary is no more a registered program than the diary (D-172). */}
      <p className="note-sand clinic-tax-note" role="note">
        היומן שומר כל שינוי שנעשה בפגישות, בחיובים ובתקבולים. התוכנה אינה רשומה ברשות המסים כתוכנה לניהול 'יומן רופא', ואת הקבלות מפיקים בתוכנת החשבוניות. כאן רושמים את מספר הקבלה.
      </p>
      <div className="row clinic-money-switch">
        <Segmented<View> label="תצוגת הכספים" value={view} onChange={(v) => { viewNow = v; setView(v); }}
          options={[{ value: "diary", label: "יומן הכנסות" }, { value: "year", label: "סיכום שנתי" }]} />
      </div>
      {view === "diary" ? <DiaryView today={now.date} /> : <YearView today={now.date} />}
    </div>
  );
}

// ------------------------------------------------------------------ the income diary

function DiaryView({ today }: { today: string }) {
  const thisMonth = monthOf(today);
  const [month, setMonth] = useState(thisMonth);
  const [onlyMissing, setOnlyMissing] = useState(false);
  const [history, setHistory] = useState<DiaryLine | null>(null);
  const diary = useDiary(month);
  const patients = usePatients(today);
  const reload = useReloadClinic();
  const balancesAt = useRef<HTMLDivElement>(null);
  const prev = shiftMonth(month, -1);
  const next = shiftMonth(month, 1);

  // A diary line carries the child's name, not the card's id: the name leads to the card
  // only when exactly one card has it.
  const idByName = useMemo(() => {
    const seen = new Map<string, string | null>();
    for (const r of patients.data ?? []) seen.set(r.patient.name, seen.has(r.patient.name) ? null : r.patient.id);
    return seen;
  }, [patients.data]);

  function go(to: string | null) {
    if (to === null) return;
    setMonth(to);
    setOnlyMissing(false);
  }

  const d = diary.data;
  const days = useMemo(() => (d ? (onlyMissing ? daysLackingReceipts(d.days) : d.days) : []), [d, onlyMissing]);
  const paged = usePaged(days, 10, `${month}|${onlyMissing ? "missing" : "all"}`);

  return (
    <div className="stack clinic-diary">
      <div className="row clinic-period-bar">
        <nav className="row clinic-period" aria-label="בחירת חודש">
          <button type="button" className="btn btn-small" disabled={prev === null} onClick={() => go(prev)}>החודש הקודם</button>
          <h2 className="clinic-period-name" aria-live="polite">{formatMonth(month)}</h2>
          <button type="button" className="btn btn-small" disabled={next === null} onClick={() => go(next)}>החודש הבא</button>
          {month !== thisMonth && <button type="button" className="link-btn" onClick={() => go(thisMonth)}>לחודש הנוכחי</button>}
        </nav>
        <IncomeExport kind="month" month={month} />
      </div>

      {diary.error && !d ? <ReadError error={diary.error.message} retry={reload} />
        : !d ? (
          <div className="stack" aria-busy="true">
            <Skeleton card lines={2} label="טוען את סיכום החודש" />
            <Skeleton card lines={6} label="טוען את יומן ההכנסות" />
          </div>
        ) : (
          <>
            <MonthTotals diary={d} onlyMissing={onlyMissing} onOnlyMissing={setOnlyMissing}
              onBalances={() => balancesAt.current?.scrollIntoView?.({ block: "start" })} />

            {d.days.length === 0 ? (
              <EmptyState title={`אין רישומים בחודש ${formatMonth(month)}`}>
                <p>פגישות, חיובים ותקבולים של החודש יופיעו כאן לפי ימים, ברגע שיירשמו ביומן או בכרטיס של מטופל.</p>
              </EmptyState>
            ) : (
              <section className="stack clinic-diary-days" aria-label={onlyMissing ? "תקבולים בלי מספר קבלה" : `ימי החודש ${formatMonth(month)}`}>
                {onlyMissing && (
                  <p className="row clinic-filter-note" role="status">
                    <span>מוצגים רק תקבולים בלי מספר קבלה. את המספר רושמים בכרטיס של המטופל, בתקבול עצמו.</span>
                    <button type="button" className="link-btn" onClick={() => setOnlyMissing(false)}>להציג את כל היומן</button>
                  </p>
                )}
                {paged.shown.map((day) => (
                  <DayTable key={day.date} day={day} totals={!onlyMissing} idOf={(name) => idByName.get(name) ?? null} onHistory={setHistory} />
                ))}
                <Pager page={paged.page} pages={paged.pages} onPage={paged.setPage} label="עמודים של ימי החודש" />
              </section>
            )}
          </>
        )}

      <div ref={balancesAt}>
        <Balances today={today} rows={patients.data} error={patients.error?.message ?? null} />
      </div>

      {history && <HistoryDialog line={history} onClose={() => setHistory(null)} />}
    </div>
  );
}

/** The month in figures: charged, received (and how), and the payments that lack a receipt number. */
function MonthTotals({ diary, onlyMissing, onOnlyMissing, onBalances }: {
  diary: DiaryMonth; onlyMissing: boolean; onOnlyMissing: (on: boolean) => void; onBalances: () => void;
}) {
  // Counted from the lines the days show: the core's figure also counts the payments of cards
  // that were erased (D-179), which have no line here to show or to complete.
  const missing = useMemo(() => missingReceipts(diary.days), [diary.days]);
  return (
    <section className="card clinic-month-totals" aria-label={`סיכום ${formatMonth(diary.month)}`}>
      <dl className="clinic-totals">
        <div>
          <dt>חויב בחודש</dt>
          <dd><Money agorot={diary.charged} /></dd>
        </div>
        <div>
          <dt>התקבל בחודש</dt>
          <dd><Money agorot={diary.received} tone="ok" /></dd>
        </div>
      </dl>
      <div className="stack clinic-by-method">
        <span className="muted small" id="clinic-by-method">התקבל לפי אמצעי תשלום</span>
        {diary.by_method.length === 0
          ? <span className="muted">עוד לא התקבל תשלום בחודש הזה.</span>
          : (
            <ul className="row clinic-methods" aria-labelledby="clinic-by-method">
              {diary.by_method.map((m) => <li key={m.method} className="chip chip-sand">{PAY_METHOD_LABEL[m.method]}: {formatMoney(m.amount)}</li>)}
            </ul>
          )}
      </div>
      <div className="stack clinic-receipts">
        {missing === 0
          ? <span className="muted">לכל התקבולים של החודש יש מספר קבלה.</span>
          : (
            <>
              <span className="clinic-overdue">{missing === 1 ? "תקבול אחד בלי מספר קבלה" : `${missing} תקבולים בלי מספר קבלה`}</span>
              <button type="button" className="btn btn-small" aria-pressed={onlyMissing} onClick={() => onOnlyMissing(!onlyMissing)}>להציג רק אותם</button>
            </>
          )}
        <button type="button" className="link-btn" onClick={onBalances}>ליתרות הפתוחות</button>
      </div>
      {diary.erased !== null && (
        <ErasedLine line={diary.erased}>הסכומים האלה כלולים בסיכום החודש, כמספרים בלבד: בלי שם ובלי תאריך, ולכן הם לא מופיעים באף יום.</ErasedLine>
      )}
    </section>
  );
}

/** How many visits, in her words: "פגישה אחת התקיימה", "3 פגישות התקיימו". */
const visitsThat = (count: number, one: string, many: string) => (count === 1 ? `פגישה אחת ${one}` : `${count} פגישות ${many}`);

/** What the anonymous line counts, as one line of text: only the parts that are not zero. */
export function erasedWords(line: DiaryErased): string {
  return [
    line.held > 0 ? visitsThat(line.held, "התקיימה", "התקיימו") : "",
    line.cancelled > 0 ? visitsThat(line.cancelled, "בוטלה", "בוטלו") : "",
    line.no_show > 0 ? (line.no_show === 1 ? "לפגישה אחת לא הגיעו" : `ל-${line.no_show} פגישות לא הגיעו`) : "",
    line.charged !== 0 ? `חויב ${formatMoney(line.charged)}` : "",
    line.received !== 0 ? `התקבל ${formatMoney(line.received)}` : "",
  ].filter(Boolean).join(" · ");
}

/** The one anonymous line of the cards that were erased (D-179): its name as the core gives
 *  it, what it counts, and one sentence that says what it is. Totals only: there is nothing
 *  here to open, and no name to show. */
function ErasedLine({ line, children }: { line: DiaryErased; children: ReactNode }) {
  return (
    <div className="stack clinic-erased" role="group" aria-label={line.label}>
      <span className="row clinic-erased-head">
        <span className="chip chip-sand">{line.label}</span>
        <span>{erasedWords(line)}</span>
      </span>
      <span className="muted small">{children}</span>
    </div>
  );
}

/** One day of the diary as a table: its lines, and under them what the day came to. */
function DayTable({ day, totals, idOf, onHistory }: { day: DiaryDay; totals: boolean; idOf: (name: string) => string | null; onHistory: (line: DiaryLine) => void }) {
  const shown = useShownName();
  const discreet = useDiscreet();
  return (
    <div className="card clinic-day">
      <table className="clinic-table clinic-diary-table">
        <caption>{formatDateLong(day.date)}</caption>
        <thead>
          <tr>
            <th scope="col">שעה</th>
            <th scope="col">ילד או ילדה</th>
            <th scope="col">מה</th>
            <th scope="col">מצב</th>
            <th scope="col" className="clinic-num">חיוב</th>
            <th scope="col" className="clinic-num">התקבל</th>
            <th scope="col">אמצעי</th>
            <th scope="col">קבלה</th>
            <th scope="col">מי שילם</th>
            <th scope="col">הערה</th>
          </tr>
        </thead>
        <tbody>
          {day.lines.map((line) => (
            <tr key={line.record_id} className={line.voided ? "clinic-diary-void" : undefined}>
              <td className="num clinic-struck">{line.time === null ? "" : <span dir="ltr">{formatTime(line.time)}</span>}</td>
              <th scope="row" className="clinic-struck"><PatientName name={line.patient_name} id={idOf(line.patient_name)} /></th>
              <td className="clinic-struck">{line.what}</td>
              <td>
                {line.voided ? <span className="chip chip-fail">בוטל</span> : line.status !== null ? <StatusBadge status={line.status} /> : null}
              </td>
              <td className="clinic-num clinic-struck">{line.charged !== 0 && (line.voided ? <Money agorot={line.charged} tone="muted" /> : <Money agorot={line.charged} />)}</td>
              <td className="clinic-num clinic-struck">{line.received !== 0 && <Money agorot={line.received} tone={line.voided ? "muted" : "ok"} />}</td>
              <td className="clinic-struck">{line.method === null ? "" : PAY_METHOD_LABEL[line.method]}</td>
              <td className="clinic-struck">
                {lacksReceipt(line) ? <span className="chip chip-warn">בלי מספר קבלה</span> : <span dir="ltr">{line.receipt}</span>}
              </td>
              {/* Whoever paid may be a person: in discreet mode, initials. */}
              <td className="clinic-struck">{line.payer === "" ? "" : shown(line.payer)}</td>
              <td className="clinic-diary-note">
                {/* Her own words stay in the card while a family may look on. */}
                {!discreet && <span>{line.note}</span>}
                {line.changed && (
                  <button type="button" className="chip chip-sand clinic-changed" aria-label={`שונה. היסטוריית השינויים של ${line.what}, ${shown(line.patient_name)}`}
                    onClick={() => onHistory(line)}>שונה</button>
                )}
              </td>
            </tr>
          ))}
        </tbody>
        {/* A day with nothing charged and nothing received (visits still ahead) has no total to show. */}
        {totals && (day.charged !== 0 || day.received !== 0) && (
          <tfoot>
            <tr>
              <th scope="row" colSpan={4}>סך היום</th>
              <td className="clinic-num"><Money agorot={day.charged} /></td>
              <td className="clinic-num"><Money agorot={day.received} tone="ok" /></td>
              <td colSpan={4} />
            </tr>
          </tfoot>
        )}
      </table>
    </div>
  );
}

/** Every saved state of a diary line, in a dialog. */
function HistoryDialog({ line, onClose }: { line: DiaryLine; onClose: () => void }) {
  const shown = useShownName();
  const history = useHistory(line.record_id);
  const reload = useReloadClinic();
  return (
    <Dialog title="היסטוריית שינויים" subtitle={`${line.what} · ${shown(line.patient_name)}`} onClose={onClose}>
      {history.data ? <HistoryEntries entries={history.data} />
        : history.error ? <ReadError error={history.error.message} retry={reload} />
        : <Skeleton lines={4} label="טוען את ההיסטוריה" />}
    </Dialog>
  );
}

// ------------------------------------------------------------------ open balances

function Balances({ today, rows, error }: { today: string; rows: PatientRow[] | undefined; error: string | null }) {
  const overview = useOverview(today);
  const settings = useClinicSettings();
  const nav = useClinicNav();
  const shown = useShownName();
  const reload = useReloadClinic();
  const alerts = overview.data?.alerts;
  const list = useMemo(() => openBalances(rows ?? [], alerts ?? []), [rows, alerts]);
  const total = useMemo(() => list.reduce((sum, b) => sum + b.row.due, 0), [list]);
  const paged = usePaged(list, 20);
  const debtDays = settings.data?.debt_days;

  /** Since when: the date when the clinic knows it (a debt it already points out), else that it is a recent one. */
  const sinceWords = (b: OpenBalance) => (b.since !== null ? `מאז ${formatDate(b.since)}`
    : alerts === undefined || debtDays === undefined ? ""
    : debtDays <= 1 ? "מהיום האחרון" : `מ-${debtDays} הימים האחרונים`);

  return (
    <Section title="יתרות פתוחות" aside={list.length > 0 ? <span className="row clinic-balances-total"><span className="muted small">{list.length === 1 ? "משפחה אחת" : `${list.length} משפחות`}</span><Money agorot={total} tone="owed" /></span> : undefined}>
      {error && !rows ? <ReadError error={error} retry={reload} />
        : !rows ? <Skeleton lines={3} label="טוען את היתרות" />
        : list.length === 0 ? <p className="muted clinic-quiet">אין יתרות פתוחות. כל מה שחויב שולם.</p>
        : (
          <>
            <ul className="clinic-list clinic-balances" aria-label="משפחות עם יתרה לתשלום">
              {paged.shown.map((b) => (
                <li key={b.row.patient.id} className="clinic-balance">
                  <div className="grow stack clinic-balance-who">
                    <PatientName name={b.row.patient.name} id={b.row.patient.id} strong />
                    <span className={b.since !== null ? "small clinic-overdue" : "small muted"}>{sinceWords(b)}</span>
                  </div>
                  <Money agorot={b.row.due} tone="owed" />
                  <div className="row clinic-balance-actions" role="group" aria-label={`פעולות עבור ${shown(b.row.patient.name)}`}>
                    <SendMenu kind="payment" patientId={b.row.patient.id} label="הודעת תשלום" />
                    <button type="button" className="btn btn-small" onClick={() => nav.openPatient(b.row.patient.id)}>לכרטיס</button>
                  </div>
                </li>
              ))}
            </ul>
            <Pager page={paged.page} pages={paged.pages} onPage={paged.setPage} label="עמודים של יתרות פתוחות" />
            <p className="hint">"הודעת תשלום" ממלאת את הנוסח מההגדרות ומעתיקה אותו. התוכנה לא שולחת כלום: את מדביקה את ההודעה בעצמך.</p>
          </>
        )}
    </Section>
  );
}

// ------------------------------------------------------------------ the year

function YearView({ today }: { today: string }) {
  const thisYear = Number(today.slice(0, 4));
  const [year, setYear] = useState(thisYear);
  const income = useIncome(year);
  const erased = useYearErased(year);
  const reload = useReloadClinic();
  const y = income.data;
  return (
    <div className="stack clinic-year">
      <div className="row clinic-period-bar">
        <nav className="row clinic-period" aria-label="בחירת שנה">
          <button type="button" className="btn btn-small" disabled={year <= FIRST_YEAR} onClick={() => setYear(year - 1)}>השנה הקודמת</button>
          <h2 className="clinic-period-name num" aria-live="polite">{year}</h2>
          <button type="button" className="btn btn-small" disabled={year >= LAST_YEAR} onClick={() => setYear(year + 1)}>השנה הבאה</button>
          {year !== thisYear && <button type="button" className="link-btn" onClick={() => setYear(thisYear)}>לשנה הנוכחית</button>}
        </nav>
        {/* The month the dialog offers first, if she switches to one: this month's, in the year shown. */}
        <IncomeExport kind="year" month={`${year}${monthOf(today).slice(4)}`} />
      </div>
      {income.error && !y ? <ReadError error={income.error.message} retry={reload} />
        : !y ? <Skeleton card lines={8} label="טוען את סיכום השנה" />
        : <YearTable income={y} erased={erased.data ?? null} unsure={erased.error !== null && erased.data === undefined} />}
    </div>
  );
}

/** `erased`: what the cards that were erased counted for in this year, already inside the
 *  figures of `income`; `unsure`: it could not be read, so the table cannot say which part
 *  of its figures they are. */
function YearTable({ income, erased, unsure }: { income: IncomeYear; erased: DiaryErased | null; unsure: boolean }) {
  const sums = useMemo(() => income.months.reduce(
    (s, m) => ({ held: s.held + m.held, cancelled: s.cancelled + m.cancelled, noShow: s.noShow + m.no_show, most: Math.max(s.most, m.received) }),
    { held: 0, cancelled: 0, noShow: 0, most: 0 },
  ), [income.months]);
  const quiet = sums.held + sums.cancelled + sums.noShow === 0 && income.charged === 0 && income.received === 0 && income.new_patients === 0;
  if (quiet) {
    return (
      <EmptyState title={`אין רישומים בשנת ${income.year}`}>
        <p>פגישות, חיובים ותקבולים של השנה יסוכמו כאן לפי חודשים.</p>
      </EmptyState>
    );
  }
  return (
    <>
      <div className="card clinic-day">
        <table className="clinic-table clinic-year-table">
          <caption>{`סיכום ${income.year} לפי חודשים`}</caption>
          <thead>
            <tr>
              <th scope="col">חודש</th>
              <th scope="col" className="clinic-num">התקיימו</th>
              <th scope="col" className="clinic-num">בוטלו</th>
              <th scope="col" className="clinic-num">לא הגיעו</th>
              <th scope="col" className="clinic-num">חויב</th>
              <th scope="col" className="clinic-num">התקבל</th>
              {/* The bars repeat the "התקבל" column for the eye; the numbers are the data. */}
              <td className="clinic-bar-cell" aria-hidden="true" />
            </tr>
          </thead>
          <tbody>
            {income.months.map((m) => (
              <tr key={m.month}>
                <th scope="row">{formatMonth(m.month).split(" ")[0]}</th>
                <td className="clinic-num num">{m.held}</td>
                <td className="clinic-num num">{m.cancelled}</td>
                <td className="clinic-num num">{m.no_show}</td>
                <td className="clinic-num"><Money agorot={m.charged} /></td>
                <td className="clinic-num"><Money agorot={m.received} /></td>
                <td className="clinic-bar-cell" aria-hidden="true">
                  <span className="clinic-bar-track" title={`${formatMonth(m.month)}: התקבל ${formatMoney(m.received)}`}>
                    <span className="clinic-bar" style={{ width: `${barPercent(m.received, sums.most)}%` }} />
                  </span>
                </td>
              </tr>
            ))}
          </tbody>
          <tfoot>
            <tr>
              <th scope="row">סך השנה</th>
              <td className="clinic-num num">{sums.held}</td>
              <td className="clinic-num num">{sums.cancelled}</td>
              <td className="clinic-num num">{sums.noShow}</td>
              <td className="clinic-num"><Money agorot={income.charged} /></td>
              <td className="clinic-num"><Money agorot={income.received} tone="ok" /></td>
              <td className="clinic-bar-cell" aria-hidden="true" />
            </tr>
          </tfoot>
        </table>
        <p className="hint clinic-bar-legend">הפס שליד כל חודש מראה כמה התקבל בו, ביחס לחודש שבו התקבל הכי הרבה.</p>
        {erased !== null && (
          <ErasedLine line={erased}>המספרים האלה כלולים בסיכום השנה. הם נשארו מכרטיסים שנמחקו, בלי שם ובלי תאריך, כדי שסיכום של שנה שעברה לא ישתנה.</ErasedLine>
        )}
        {unsure && <p className="hint">אם נמחקו כרטיסים, מה שנשאר מהם כלול בסיכום הזה. הפירוט שלהם לא נקרא כרגע.</p>}
      </div>

      <div className="clinic-columns">
        <Section title="פגישות שהתקיימו, לפי סוג" aside={<span className="muted small">{visitsWord(sums.held)}</span>}>
          {income.by_kind.length === 0
            ? <p className="muted clinic-quiet">עוד לא התקיימו פגישות בשנה הזו.</p>
            : (
              <ul className="clinic-list clinic-kinds">
                {income.by_kind.map((k) => (
                  <li key={k.kind} className="row clinic-kind-count">
                    <span className="grow">{VISIT_KIND_LABEL[k.kind]}</span>
                    <span className="num">{k.count}</span>
                  </li>
                ))}
              </ul>
            )}
        </Section>
        <Section title="מטופלים חדשים בשנה">
          <p className="clinic-quiet"><span className="clinic-figure-value">{income.new_patients}</span></p>
          <p className="hint">כרטיסים שנפתחו בשנת {income.year}, כולל משפחות ברשימת ההמתנה, וכרטיסים שנמחקו אחרי תקופת השמירה.</p>
        </Section>
      </div>
    </>
  );
}

// ------------------------------------------------------------------ the file for the accountant

/** How the file names the anonymous line of the erased cards (the core's own label, D-179). */
const ERASED_WORDS = "רשומות של כרטיסים שנמחקו";

/** "ייצוא לרואה החשבון" (D-194): a quiet button beside the month or the year, and its dialog.
 *  `kind` and `month`: what the dialog offers first (the month or the year she is looking at). */
function IncomeExport({ kind, month }: { kind: ExportKind; month: string }) {
  const [open, setOpen] = useState(false);
  return (
    <>
      <button type="button" className="btn btn-small btn-ghost clinic-export-open" onClick={() => setOpen(true)}>ייצוא לרואה החשבון</button>
      {open && <IncomeExportDialog kind={kind} month={month} onClose={() => setOpen(false)} />}
    </>
  );
}

/** Picks a month or a whole year, and has the core write the file. What the file holds is
 *  said before it is made; where it was saved is said after (a toast). A month with payments
 *  that lack a receipt number says so first, so she can complete them before sending. */
function IncomeExportDialog({ kind: first, month: firstMonth, onClose }: { kind: ExportKind; month: string; onClose: () => void }) {
  const { notify, fail } = useApp();
  const [kind, setKind] = useState<ExportKind>(first);
  const [month, setMonth] = useState(firstMonth);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const diary = useDiary(month);
  // A year moves by twelve months, so the month she may come back to stays in the year shown.
  const step = kind === "month" ? 1 : 12;
  const prev = shiftMonth(month, -step);
  const next = shiftMonth(month, step);
  const period = exportPeriod(kind, month);
  const shown = kind === "month" ? formatMonth(month) : `שנת ${period}`;
  // Counted from the lines of the month, as the month's figures count them (D-179).
  const missing = kind === "month" && diary.data ? missingReceipts(diary.data.days) : 0;

  async function save() {
    setBusy(true);
    setError(null);
    try {
      const where = await ipc.exportIncome(period);
      // The path is isolated, so its slashes do not mix with the Hebrew around it.
      notify(`הקובץ לרואה החשבון נשמר: \u2066${where}\u2069`);
      onClose();
    } catch (e) {
      setError(fail(asUiError(e)));
      setBusy(false);
    }
  }

  return (
    <Dialog narrow title="ייצוא לרואה החשבון" subtitle="התקבולים בקובץ אחד, בלי שמות" onClose={onClose}
      footer={<>
        <span className="grow" />
        <button type="button" className="btn" onClick={onClose}>ביטול</button>
        <button type="button" className="btn btn-primary" disabled={busy} onClick={() => void save()}>שמירת הקובץ</button>
      </>}>
      <div className="stack clinic-export">
        <Segmented<ExportKind> label="מה לייצא" value={kind} onChange={setKind}
          options={[{ value: "month", label: "חודש" }, { value: "year", label: "שנה שלמה" }]} />
        <div className="row clinic-period" role="group" aria-label={kind === "month" ? "החודש לייצוא" : "השנה לייצוא"}>
          <button type="button" className="btn btn-small" disabled={prev === null} onClick={() => prev !== null && setMonth(prev)}>
            {kind === "month" ? "החודש הקודם" : "השנה הקודמת"}
          </button>
          <span className="clinic-export-period" aria-live="polite">{shown}</span>
          <button type="button" className="btn btn-small" disabled={next === null} onClick={() => next !== null && setMonth(next)}>
            {kind === "month" ? "החודש הבא" : "השנה הבאה"}
          </button>
        </div>
        <p className="clinic-export-what">
          בקובץ: התאריך, מספר הקבלה, הסכום, אמצעי התשלום והמצב (תקין או בוטל) של כל תקבול, ואחריהם מה שהתקבל בכל חודש לפי אמצעי תשלום, כולל {ERASED_WORDS}, כסכומים בלבד. אין בו שמות, טלפונים, הערות או סיבות ביטול.
        </p>
        {missing > 0 && (
          <p className="clinic-overdue" role="status">
            {missing === 1 ? "בחודש הזה יש תקבול אחד בלי מספר קבלה" : `בחודש הזה יש ${missing} תקבולים בלי מספר קבלה`}. כדאי להשלים את המספר בכרטיס של המטופל לפני ששולחים. הקובץ סופר אותם בסופו.
          </p>
        )}
        <p className="hint">הקובץ נשמר במחשב, בתיקייה שלא מסונכרנת לענן (בדרך כלל ההורדות). התוכנה לא שולחת אותו: את שולחת אותו לרואה החשבון בעצמך.</p>
        <ErrorLine error={error} />
      </div>
    </Dialog>
  );
}
