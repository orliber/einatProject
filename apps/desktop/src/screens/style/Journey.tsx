import type { StyleOverview } from "../../ipc/client";
import { sinceLabel } from "./labels";

// הסגנון שלי · the journey (D-100)

/** The number of things the program knows and uses: the profile's items in use and the approved interpretations. */
function knownCount(o: StyleOverview): number {
  return (o.active?.profile.items.filter((i) => i.enabled).length ?? 0)
    + o.interpretations.filter((i) => i.item.approved && i.item.enabled).length;
}

type Stage = {
  key: "reports" | "analysis" | "approval" | "use";
  title: string;
  figure: string;
  line: string;
  action: string;
  state: "done" | "current" | "warn" | "next";
  run: () => void;
  disabled: boolean;
};

const scrollTo = (id: string) => () => document.getElementById(id)?.scrollIntoView({ behavior: "smooth", block: "start" });

/** The whole process in one strip: my reports, analysis, approval, in use. Each stage says where
 * it stands and what to do next; the current one is marked. */
export function Journey({ o, waiting, analyzed, notRead, onAdd, build }: {
  o: StyleOverview;
  waiting: number;
  analyzed: number;
  notRead: number;
  onAdd: () => void;
  build: { label: string; run: () => void; disabled: boolean } | null;
}) {
  const n = o.sources.length;
  const last = Math.max(...o.sources.map((s) => s.added_at));
  const known = knownCount(o);
  const inUse = o.active !== null && o.enabled;
  const stages: Stage[] = [
    {
      key: "reports", title: "הדוחות שלי", figure: n === 1 ? "דוח אחד" : `${n} דוחות`,
      line: `האחרון נוסף ${sinceLabel(last)}. נשמר רק הטקסט אחרי הסתרה.`,
      action: "+ הוספת דוח", state: "done", run: onAdd, disabled: false,
    },
    {
      key: "analysis", title: "ניתוח", figure: analyzed === n ? (n === 1 ? "נותח" : "כולם נותחו") : `${analyzed} מתוך ${n} נותחו`,
      line: "מכל דוח נלמדים הסגנון, הפרשנות של תוצאות וכללי ההסקה שלך.",
      action: build ? `${build.label} ←` : "הכול מעודכן",
      state: build ? "current" : "done", run: build ? build.run : scrollTo("style-sources"), disabled: build?.disabled ?? false,
    },
    {
      key: "approval", title: "אישור", figure: waiting === 0 ? "הכול מאושר" : waiting === 1 ? "אחד מחכה לך" : `${waiting} מחכים לך`,
      line: "הפרופיל נכנס לשימוש לבד. רק פרשנות של תוצאות מחכה לעין שלך.",
      action: waiting > 0 ? "לאשר עכשיו ↓" : "אין מה לאשר",
      state: waiting > 0 ? "warn" : notRead > 0 && !build ? "next" : "done", run: scrollTo("style-queue-title"), disabled: false,
    },
    {
      key: "use", title: "בשימוש", figure: known === 1 ? "דבר אחד" : `${known} דברים`,
      line: o.active
        ? inUse
          ? `גרסה ${o.active.version} של הפרופיל. נכנסים לכל טיוטה, רק מה שמתאים לסעיף ולתיק.`
          : "הפרופיל כבוי: הניסוח בסגנון ברירת המחדל."
        : "אחרי האישור הראשון הפרופיל ייכנס לכל טיוטה.",
      action: "מה התוכנה יודעת עלייך ↓", state: o.active ? (inUse ? "done" : "next") : "next", run: scrollTo("style-known-title"), disabled: false,
    },
  ];
  // Without a report analyzed there is no analysis to stand on; the first open stage is the current one.
  const current = stages.find((s) => s.state === "current") ?? stages.find((s) => s.state === "warn") ?? stages.find((s) => s.state === "next");
  return (
    <ol className="style-journey" aria-label="המסלול">
      {stages.map((s, i) => (
        <li key={s.key}>
          <button type="button" className={`style-stage style-stage-${s.state}`} aria-current={s === current ? "step" : undefined}
            disabled={s.disabled} onClick={s.run}>
            <span className="style-stage-head">
              <span className={`style-stage-num style-stage-num-${s.state}`} aria-hidden="true">{s.state === "done" ? "✓" : i + 1}</span>
              <strong>{s.title}</strong>
            </span>
            <span className="style-stage-figure">{s.figure}</span>
            <span className="style-stage-line">{s.line}</span>
            <span className="style-stage-action">{s.action}</span>
          </button>
        </li>
      ))}
    </ol>
  );
}
