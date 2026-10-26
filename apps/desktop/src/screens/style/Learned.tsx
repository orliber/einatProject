import { useAi } from "../../ai";
import type { FitMonth, StyleLearned, StyleOverview } from "../../ipc/client";

// הסגנון שלי · what was measured

/** Her usual figures, in words: what the program measured, numbers only. */
export function learnedLines(l: StyleLearned): string[] {
  const out: string[] = [];
  const round = (x: number) => Math.round(x);
  if (l.words_per_sentence != null) out.push(`בממוצע ${round(l.words_per_sentence)} מילים במשפט`);
  if (l.sentences_per_paragraph != null) out.push(`בדרך כלל ${round(l.sentences_per_paragraph)} משפטים בפסקה`);
  if (l.commas_per_sentence != null) out.push(`כ-${l.commas_per_sentence.toFixed(1)} פסיקים במשפט`);
  if (l.hedges != null) out.push(`הסתייגות («נראה», «ניכר») ב-${round(l.hedges)} מכל 100 משפטים`);
  if (l.contrasts != null) out.push(`מילת ניגוד («אך», «עם זאת») ב-${round(l.contrasts)} מכל 100 משפטים`);
  if (l.first_person != null) out.push(`גוף ראשון («התרשמתי») ב-${round(l.first_person)} מכל 100 משפטים`);
  return out;
}

const MONTHS_HE = ["ינואר", "פברואר", "מרץ", "אפריל", "מאי", "יוני", "יולי", "אוגוסט", "ספטמבר", "אוקטובר", "נובמבר", "דצמבר"];

/** "2026-10" → ["אוק׳ 26" as a short label, "אוקטובר 2026" in full]. */
export function monthLabel(month: string): { short: string; full: string } {
  const [y = "", m = "1"] = month.split("-");
  const name = MONTHS_HE[Number(m) - 1] ?? month;
  return { short: `${Number(m)}.${y.slice(2)}`, full: `${name} ${y}` };
}

/** ST12: the average closeness by month, oldest first: is drafting getting closer to her? */
function FitTrend({ months }: { months: FitMonth[] }) {
  return (
    <div className="fit-trend">
      <span className="small muted">לאורך זמן (ממוצע לחודש, מתוך 100):</span>
      <ol className="fit-bars">
        {months.map((m) => {
          const { short, full } = monthLabel(m.month);
          return (
            <li key={m.month} className="fit-bar" aria-label={`${full}: ${m.average} (${m.count} פסקאות)`}>
              <span className="fit-value" aria-hidden="true">{m.average}</span>
              <span className="fit-fill" style={{ height: `${Math.max(3, Math.round(m.average * 0.8))}px` }} aria-hidden="true" />
              <span className="fit-month" aria-hidden="true">{short}</span>
            </li>
          );
        })}
      </ol>
    </div>
  );
}

export function Learned({ o }: { o: StyleOverview }) {
  const ai = useAi();
  const l = o.learned;
  const lines = learnedLines(l);
  const from = [
    l.from_reports > 0 && `${l.from_reports} פסקאות מהדוחות הישנים`,
    l.from_edits > 0 && `${l.from_edits} מהתיקונים שלך`,
    l.finished_reports > 0 && (l.finished_reports === 1 ? "דוח אחד שסיימת בתוכנה" : `${l.finished_reports} דוחות שסיימת בתוכנה`),
  ].filter(Boolean) as string[];
  if (from.length === 0 && l.fit_count === 0 && o.omissions.length === 0) return null;
  return (
    <div className="stack style-measured">
      <h3 className="small">מה התוכנה מדדה בכתיבה שלך</h3>
      <p className="hint">נמדד במחשב שלך, ונשמרים רק מספרים. {from.length > 0 && `נלמד מ${from.join(", ")}.`}</p>
      {lines.length > 0 ? (
        <ul className="style-learned">{lines.map((t) => <li key={t}>{t}</li>)}</ul>
      ) : (
        <p className="muted small">עוד אין מספיק פסקאות כדי לומר מה אופייני לך (צריך לפחות 12).</p>
      )}
      {l.fit_recent != null && (
        <p className="style-fit">
          כמה הפסקאות ש-{ai} כתב קרובות לסגנון שלך: <b>{l.fit_recent} מתוך 100</b>
          {l.fit_earlier != null
            ? ` ב-30 הפסקאות האחרונות, לעומת ${l.fit_earlier} ב-30 שלפניהן.`
            : ` (${l.fit_count} פסקאות עד עכשיו).`}
        </p>
      )}
      {l.fit_by_month.length >= 2 && <FitTrend months={l.fit_by_month} />}
      {o.omissions.length > 0 && (
        <ul className="style-learned" aria-label="מה את לא כותבת">
          {o.omissions.map((t) => <li key={t}>{t}</li>)}
        </ul>
      )}
    </div>
  );
}
