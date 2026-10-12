// The made-up case (U3): a short explanation of the page she is on, a way on to the next
// stage, and a way to throw the case away. Everything in it is invented.
import { useState } from "react";

const STEPS: Record<string, { text: string; next?: string }> = {
  details: {
    text: "פרטי הילד, ההסכמה והשמות להסתרה. «תמר» כבר רשומה, ולכן נשלחת כ-[ילד].",
    next: "לחומרים",
  },
  materials: {
    text: "שלושה חומרים בדויים. בתיק אמיתי מעלים כאן שיחות, דוחות וציונים.",
    next: "לכתיבת הדוח",
  },
  report: {
    text: "מבקשים טיוטה לסעיף. רואים מה יוצא, וכל פסקה מחכה לאישורך.",
    next: "לסיום",
  },
  finish: {
    text: "מה עוד לא אושר, והפקת הדוח ל-Word. אחר כך אפשר למחוק את תיק הדוגמה.",
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
