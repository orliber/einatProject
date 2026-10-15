// The DSM-5 severity (round 43): one level for each domain, as she decides it. It goes with the
// DSM section's request in fixed words, and a paragraph that names another level gets a note.
// The AI never chooses it.
import { useState } from "react";
import { useApp } from "../App";
import { ipc, type DsmSeverity } from "../ipc/client";
import { ErrorLine } from "./ui";

const LEVELS: [number, string][] = [
  [1, "דרגה 1: נדרשת תמיכה"],
  [2, "דרגה 2: נדרשת תמיכה משמעותית"],
  [3, "דרגה 3: נדרשת תמיכה משמעותית מאוד"],
];

const DOMAINS = [
  ["social", "תקשורת חברתית ואינטראקציה חברתית"],
  ["restricted", "דפוסי התנהגות, תחומי עניין ופעילויות מוגבלים וחוזרים"],
] as const;

export function DsmSeverityPanel(props: { caseId: string; severity?: DsmSeverity | undefined; onSaved: () => Promise<void> | void }) {
  const { fail } = useApp();
  const [error, setError] = useState<string | null>(null);
  // What she chose, until the case reloads: a second change goes out with the first one's
  // value for the other domain, and nothing changes while one is being saved.
  const [mine, setMine] = useState<{ social: number | null; restricted: number | null } | null>(null);
  const [saving, setSaving] = useState(false);
  const now = mine ?? { social: props.severity?.social ?? null, restricted: props.severity?.restricted ?? null };
  const change = async (domain: "social" | "restricted", value: string) => {
    setError(null);
    const next = { ...now, [domain]: value ? Number(value) : null };
    setMine(next);
    setSaving(true);
    try {
      await ipc.setDsmSeverity(props.caseId, next.social, next.restricted);
      await props.onSaved();
    } catch (e) {
      setError(fail(e as never));
    } finally {
      setMine(null);
      setSaving(false);
    }
  };
  return (
    <section className="section-guide dsm-severity" aria-label="רמת החומרה">
      <div className="stack guide-body">
        <b className="small">רמת החומרה, כפי שקבעת</b>
        {DOMAINS.map(([key, label]) => (
          <div key={key} className="field">
            <label htmlFor={`dsm-${key}`} className="small">{label}</label>
            <select id={`dsm-${key}`} className="input" value={now[key] ?? ""} disabled={saving} onChange={(e) => void change(key, e.target.value)}>
              <option value="">לא נקבעה</option>
              {LEVELS.map(([n, words]) => <option key={n} value={n}>{words}</option>)}
            </select>
          </div>
        ))}
        <p className="muted small">הדרגה נכתבת בסעיף בדיוק כפי שבחרת. פסקה שכתובה בה דרגה אחרת מקבלת הערה.</p>
        <ErrorLine error={error} />
      </div>
    </section>
  );
}
