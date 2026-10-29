// הסגנון שלי · before the first report (D3 / D-112): the four stages drawn as one path, each
// with a small invented example of what the program learns, so it is clear why it is worth
// starting. Every example below is made up; nothing here comes from a report.
import type { ReactNode } from "react";
import { UploadIcon } from "../../components/ui";
import { RECOMMENDED } from "./labels";

type StartStage = { key: string; title: string; line: string; example: ReactNode };

/** An invented hidden name, drawn the way the filter's tags read. */
const Hidden = ({ tag }: { tag: string }) => <span className="style-path-hidden">{tag}</span>;

export const START_STAGES: StartStage[] = [
  {
    key: "in",
    title: "הדוח נכנס",
    line: "דוח שכתבת בעבר. השמות והפרטים המזהים מוסתרים לפני שהוא נשמר.",
    example: (
      <blockquote className="style-path-quote">
        בבדיקה <Hidden tag="[ילד]" /> עזב משימות באמצע, וחזר אליהן אחרי תיווך קצר.
      </blockquote>
    ),
  },
  {
    key: "read",
    title: "הדוח נקרא",
    line: "התוכנה מוצאת איך את כותבת, ומה את מסיקה מכל ממצא.",
    example: (
      <span className="style-path-learned">
        <span><span className="style-path-tag">איך את מסיקה</span> עוזב משימה באמצע ← קושי בהתמדה</span>
        <span><span className="style-path-tag">ביטוי שחוזר אצלך</span> «ניכר כי»</span>
      </span>
    ),
  },
  {
    key: "approve",
    title: "את מאשרת",
    // D-138: what is learned is used at once; anything can be changed, turned off or removed.
    line: "מה שנלמד נכנס לשימוש. כל פריט אפשר לערוך, לכבות או למחוק, ומה שמחכה לך מאשרים אחד-אחד.",
    example: (
      <span className="style-path-choices">
        <span className="style-path-choice style-path-choice-yes">להשתמש בזה</span>
        <span className="style-path-choice">לערוך</span>
        <span className="style-path-choice">לא צריך</span>
      </span>
    ),
  },
  {
    key: "use",
    title: "בשימוש",
    line: "מעכשיו כל טיוטה נכתבת בסגנון שלך, רק עם מה שמתאים לסעיף ולתיק.",
    example: (
      <div className="style-path-draft">
        <span className="style-path-tag">בטיוטה של תיק חדש</span>
        <blockquote className="style-path-quote">ניכר כי קיים קושי בהתמדה, ונדרש תיווך בתחילת משימה.</blockquote>
      </div>
    ),
  },
];

/** The page's whole body until the first report is in: the path, then one action. */
export function StyleStart({ adding, onAdd }: { adding: boolean; onAdd: () => void }) {
  return (
    <section className="card style-empty" aria-label="איך התוכנה לומדת את הסגנון שלך">
      <ol className="style-path" aria-label="ארבעת השלבים">
        {START_STAGES.map((s, i) => (
          <li key={s.key} className="style-path-step" aria-current={i === 0 ? "step" : undefined}>
            <span className="style-path-node" aria-hidden="true">{i + 1}</span>
            {i < START_STAGES.length - 1 && <span className="style-path-link" aria-hidden="true" />}
            <span className="style-path-head">
              <strong className="style-path-title"><span className="visually-hidden">שלב {i + 1}: </span>{s.title}</strong>
              {i === 0 && <span className="style-path-here">מתחילים כאן</span>}
            </span>
            <span className="style-path-line">{s.line}</span>
            <div className="style-path-example">{s.example}</div>
          </li>
        ))}
      </ol>
      <p className="style-path-note">
        <span className="chip chip-sand">דוגמה בדויה</span>
        {" "}כך זה נראה עם דוח אחד. מומלץ להוסיף {RECOMMENDED} דוחות לפחות: מה שחוזר בכמה דוחות הוא באמת הסגנון שלך.
      </p>
      <div className="style-start">
        <button type="button" className="btn btn-primary btn-big" disabled={adding} onClick={onAdd}>
          <UploadIcon /> הוספת דוח ישן
        </button>
        <p className="hint">דוח שכתבת בעבר, ב-Word, ODT או PDF. נשמר רק הטקסט אחרי הסתרה.</p>
      </div>
    </section>
  );
}
