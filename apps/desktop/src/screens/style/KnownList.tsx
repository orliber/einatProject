import { useState } from "react";
import { useQueryClient } from "@tanstack/react-query";
import { useApp } from "../../App";
import { Switch } from "../../components/Switch";
import { ErrorLine } from "../../components/ui";
import { ipc, type StyleItem, type StyleKind, type StyleOverview } from "../../ipc/client";
import { ItemText } from "./InferenceChain";
import { Interpretations } from "./Interpretations";
import { Learned } from "./Learned";
import { ProfileEditor } from "./ProfileEditor";

// הסגנון שלי · what the program knows

type Known = "writing" | "order" | "phrases" | "scores" | "inference" | "argument";
const KNOWN: { key: Known; title: string; line: string; kinds: StyleKind[] }[] = [
  { key: "writing", title: "איך את כותבת", line: "כללי כתיבה, מה את לא כותבת, ומה התוכנה מדדה", kinds: ["rule", "avoid", "example"] },
  { key: "order", title: "סדר הנושאים בכל סעיף", line: "הסדר שבו את כותבת כל סעיף", kinds: [] },
  { key: "phrases", title: "משפטים שחוזרים אצלך", line: "ביטויים ותבניות משפט קבועות", kinds: ["phrase", "template"] },
  { key: "scores", title: "איך את מפרשת ציונים", line: "מה את כותבת על תוצאה של כל תת-מבחן", kinds: [] },
  { key: "inference", title: "איך את מסיקה", line: "מהממצא למסקנה: תצפית, דיווח או דפוס תוצאות, ומה נובע מהם", kinds: ["inference"] },
  { key: "argument", title: "איך את בונה טיעון", line: "חיבור בין כלים, וניסוח קריטריון DSM", kinds: ["argument", "integration", "criterion"] },
];

const INFERENCE_HINT = "נלמד מהדוחות שלך: מאיזה ממצא (התנהגות בבדיקה, דיווח ההורים או הגן, הרקע, אבחון קודם) את מגיעה לאיזו מסקנה, איך את מנסחת אותה ואיזו המלצה נובעת ממנה. "
  + "דפוסי תוצאות (פער בין מדדים, המדד הנמוך ביותר, פיזור, מסתגל מול קוגניטיבי) מזוהים בקוד, ולכל דפוס נשמרת הקריאה שלך. "
  + "בטיוטה נשלחים רק הכללים שהממצא שלהם מופיע בחומרים של הסעיף, או שהדפוס שלהם נמצא בתוצאות של הילד.";

export function KnownList({ o, running, onReadScores }: { o: StyleOverview; running: boolean; onReadScores: () => void }) {
  const { fail, notify } = useApp();
  const qc = useQueryClient();
  // One open at a time: each editor holds the whole profile, so two open ones would overwrite each other.
  const [open, setOpen] = useState<Known | null>(null);
  const [error, setError] = useState<string | null>(null);
  const items = o.active?.profile.items.filter((i) => i.enabled) ?? [];
  const count = (k: (typeof KNOWN)[number]) =>
    k.key === "order" ? o.skeletons.length
      : k.key === "scores" ? o.interpretations.filter((i) => i.item.approved && i.item.enabled).length
        : items.filter((i) => k.kinds.includes(i.kind)).length;

  async function toggleUse(on: boolean) {
    setError(null);
    try {
      await ipc.setStyleEnabled(on);
      notify(on ? "הפרופיל חזר לשימוש." : "הניסוח חזר לסגנון ברירת המחדל.");
      await qc.invalidateQueries({ queryKey: ["style"] });
    } catch (e) {
      setError(fail(e as never));
    }
  }

  return (
    <section className="card style-known" aria-labelledby="style-known-title">
      <h2 id="style-known-title">מה התוכנה יודעת עלייך</h2>
      {o.active && (
        <div className="style-known-use">
          <Switch id="style-use" label="להשתמש במה שאישרת בכל ניסוח" checked={o.enabled} onChange={(on) => void toggleUse(on)} />
        </div>
      )}
      <ErrorLine error={error} />
      <ul className="style-known-rows">
        {KNOWN.map((k) => {
          const on = open === k.key;
          return (
            <li key={k.key}>
              <button type="button" className="style-know" aria-expanded={on} aria-controls={`known-${k.key}`}
                onClick={() => setOpen(on ? null : k.key)}>
                <span className="style-know-text"><strong>{k.title}</strong><span className="muted">{k.line}</span></span>
                <span className="style-know-count">{count(k)} <span aria-hidden="true">{on ? "↓" : "←"}</span></span>
              </button>
              {on && (
                <div id={`known-${k.key}`} className="style-known-open" role="region" aria-label={k.title}>
                  {k.key === "order" ? <Skeletons o={o} />
                    : k.key === "scores" ? <Interpretations o={o} running={running} onRead={onReadScores} />
                      : (
                        <>
                          {k.key === "inference" && <p className="hint">{INFERENCE_HINT}</p>}
                          {o.active
                            ? o.draft
                              ? <ReadOnlyItems items={o.active.profile.items.filter((i) => k.kinds.includes(i.kind))} o={o} />
                              : <ProfileEditor key={o.active.id} view={o.active} o={o} kinds={k.kinds} />
                            : <p className="muted small">עוד אין פרופיל מאושר. מה שתאשרי בצד יופיע כאן.</p>}
                          {k.key === "writing" && <Learned o={o} />}
                        </>
                      )}
                </div>
              )}
            </li>
          );
        })}
      </ul>
    </section>
  );
}

/** While a draft waits, the profile in use is shown and not changed: one place to approve. */
function ReadOnlyItems({ items, o }: { items: StyleItem[]; o: StyleOverview }) {
  if (items.length === 0) return <p className="muted small">עוד אין כאן פריטים.</p>;
  const title = (key: string | null) => (key ? (o.sections.find((s) => s.key === key)?.title ?? key) : "כל הדוח");
  return (
    <>
      <p className="hint">אפשר לשנות כאן אחרי שתסיימי לאשר את הטיוטה.</p>
      <ul className="style-items">
        {items.map((i) => (
          <li key={i.id} className={i.enabled ? "style-item" : "style-item style-item-off"}>
            <span className="style-item-text grow"><ItemText item={i} /></span>
            <span className="chip">{title(i.section)}</span>
          </li>
        ))}
      </ul>
    </>
  );
}

function Skeletons({ o }: { o: StyleOverview }) {
  if (o.skeletons.length === 0) return <p className="muted small">עוד לא נמצא סדר קבוע. הוא נלמד מ-3 דוחות לפחות שיש בהם אותו סעיף.</p>;
  return (
    <div className="stack style-skeletons">
      <ul className="style-learned">
        {o.skeletons.map((k) => (
          <li key={`${k.section}-${k.band ?? ""}`}>
            <b>{k.title}{k.band ? ` (${k.band})` : ""}:</b> {k.topics.join(" · ")}{" "}
            <span className="muted small">(מ-{k.reports} דוחות)</span>
          </li>
        ))}
      </ul>
      <p className="hint">כל עוד לא כתבת הנחיה משלך לסעיף, זו ההנחיה שלו בטיוטה.</p>
    </div>
  );
}
