// The made-up case (U3): a short explanation of the page she is on, a way on to the next
// stage, and a way to throw the case away. Everything in it is invented.
import { useState } from "react";

const STEPS: Record<string, { text: string; next?: string }> = {
  details: {
    text: "כאן רושמים את פרטי הילד, את הסכמת ההורים ואת השמות להסתרה. השם «תמר» כבר רשום, ולכן בכל מה שנשלח הוא מופיע כ-[ילד].",
    next: "לחומרים",
  },
  materials: {
    text: "אלה שלושה חומרים בדויים. בתיק אמיתי מדביקים או מעלים כאן שיחות, דוחות, תצפיות וציונים. אחר כך עוברים לכתיבת הדוח.",
    next: "לכתיבת הדוח",
  },
  report: {
    text: "בוחרים סעיף ומבקשים טיוטה. לפני כל שליחה רואים בדיוק מה יוצא, עם השמות מוסתרים, וכל פסקה מחכה לאישור שלך לפני שהיא נכנסת לדוח.",
    next: "לסיום",
  },
  finish: {
    text: "כאן רואים מה עוד לא אושר, ומפיקים את הדוח לקובץ Word. כשסיימת להתנסות אפשר למחוק את תיק הדוגמה.",
  },
};

/** Which explanation fits the page: a section of the report counts as the report. */
export function sampleStep(view: string): string {
  return view in STEPS ? view : "report";
}

export function SampleGuide({ view, busy, onNext, onDelete }: {
  view: string;
  busy: boolean;
  onNext: (from: string) => void;
  onDelete: () => void;
}) {
  const [hidden, setHidden] = useState(false);
  if (hidden) return null;
  const step = sampleStep(view);
  const { text, next } = STEPS[step] ?? { text: "" };
  return (
    <section className="sample-guide" aria-label="תיק לדוגמה">
      <p><b>תיק לדוגמה.</b> {text}</p>
      <div className="row wrap-row">
        {next && <button type="button" className="btn btn-small btn-primary" onClick={() => onNext(step)}>{next}</button>}
        <button type="button" className="btn btn-small" disabled={busy} onClick={onDelete}>למחוק את תיק הדוגמה</button>
        <button type="button" className="btn btn-small btn-ghost" onClick={() => setHidden(true)}>להסתיר את ההסבר</button>
      </div>
    </section>
  );
}
