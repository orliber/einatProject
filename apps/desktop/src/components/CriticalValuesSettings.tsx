// Settings → המבחנים → חוזקות וחולשות יחסיות (D-190): the manuals' critical values (.05) she
// copies for each subtest. With every value of a set entered, a sheet of that set is read by
// the publisher's method (each primary subtest against the mean of the primary subtests);
// otherwise the sheet keeps the 3-point rule of thumb and says so. Folded: one line per set.
import { useQuery, useQueryClient } from "@tanstack/react-query";
import { useState } from "react";
import { useApp } from "../App";
import { ipc } from "../ipc/client";
import type { CriticalValue } from "../ipc/generated/CriticalValue";
import type { CriticalValuesView } from "../ipc/generated/CriticalValuesView";
import type { RelativeSet } from "../ipc/generated/RelativeSet";
import { ErrorLine } from "./ui";

/** "3.12" → 3.12; empty → null (no value); anything else → NaN (refused before saving). */
export function parseCritical(text: string): number | null {
  const t = text.trim().replace(",", ".");
  if (t === "") return null;
  return /^\d+(\.\d{1,2})?$/.test(t) ? Number(t) : Number.NaN;
}

function SetForm({ set, values, names, onSave }: {
  set: RelativeSet;
  values: CriticalValue[];
  names: Record<string, string>;
  onSave: (mine: CriticalValue[]) => Promise<boolean>;
}) {
  const stored = (m: string) => values.find((v) => v.set === set.key && v.measure === m)?.value;
  const [texts, setTexts] = useState<Record<string, string>>(() =>
    Object.fromEntries(set.measures.map((m) => [m, stored(m)?.toString() ?? ""])));
  const [bad, setBad] = useState(false);
  const count = set.measures.filter((m) => stored(m) !== undefined).length;
  const complete = count === set.measures.length;

  async function save() {
    const parsed = set.measures.map((m) => [m, parseCritical(texts[m] ?? "")] as const);
    if (parsed.some(([, v]) => v !== null && (Number.isNaN(v) || v <= 0 || v > 9))) {
      setBad(true);
      return;
    }
    setBad(false);
    await onSave(parsed.flatMap(([m, v]) => (v === null ? [] : [{ set: set.key, measure: m, value: v }])));
  }

  return (
    <details className="critical-set">
      <summary>
        <span dir="ltr">{set.title_he}</span>
        <span className="hint">{complete ? " · לפי שיטת המדריך" : count === 0 ? " · לא הוזנו ערכים" : ` · הוזנו ${count} מתוך ${set.measures.length}`}</span>
      </summary>
      <div className="critical-grid">
        {set.measures.map((m) => {
          const id = `crit-${set.key}-${m}`;
          return (
            <label key={m} htmlFor={id} className="critical-row">
              <span>{names[m] ?? m}</span>
              <input id={id} className="input num" inputMode="decimal" dir="ltr" value={texts[m] ?? ""}
                onChange={(e) => setTexts({ ...texts, [m]: e.target.value })} />
            </label>
          );
        })}
      </div>
      {bad && <p className="warn-text" role="alert">ערך קריטי הוא מספר בין 0 ל-9, עם עד שתי ספרות אחרי הנקודה.</p>}
      <button type="button" className="btn btn-small" onClick={() => void save()}>שמירה</button>
    </details>
  );
}

export function CriticalValuesSettings() {
  const { fail, notify } = useApp();
  const qc = useQueryClient();
  const view = useQuery({ queryKey: ["critical-values"], queryFn: ipc.criticalValues });
  const instruments = useQuery({ queryKey: ["instruments"], queryFn: ipc.instrumentApprovals });
  const [error, setError] = useState<string | null>(null);

  const names: Record<string, Record<string, string>> = {};
  for (const a of instruments.data ?? []) {
    names[a.instrument.key] = Object.fromEntries(a.instrument.measures.map((m) => [m.key, `${m.name_he} (${m.abbr})`]));
  }

  async function save(set: RelativeSet, mine: CriticalValue[]): Promise<boolean> {
    setError(null);
    const others = (view.data?.values ?? []).filter((v) => v.set !== set.key);
    try {
      qc.setQueryData<CriticalValuesView>(["critical-values"], await ipc.setCriticalValues([...others, ...mine]));
      notify("הערכים נשמרו. בתיק קיים, דף הציונים מתעדכן כששומרים את טבלת הציונים שוב.");
      return true;
    } catch (e) {
      setError(fail(e as never));
      return false;
    }
  }

  const data = view.data;
  return (
    <section className="card setting" aria-labelledby="s-critical">
      <h2 id="s-critical">חוזקות וחולשות יחסיות</h2>
      <p className="hint">
        התוכנה משווה כל תת-מבחן ראשי לממוצע של תתי-המבחנים הראשיים, ומסמנת חוזקה או חולשה רק כשההפרש מגיע
        לערך הקריטי שבמדריך (מובהקות .05). את הערכים מעתיקים מהמדריך, לכל תת-מבחן. עד שכל הערכים של מבחן הוזנו,
        דף הציונים משתמש בכלל אצבע של 3 נקודות ואומר זאת במפורש.
      </p>
      <ErrorLine error={view.error ? fail(view.error as never) : error} />
      {data && data.sets.map((set) => (
        <SetForm key={`${set.key}-${data.values.filter((v) => v.set === set.key).map((v) => v.value).join(",")}`}
          set={set} values={data.values} names={names[set.instrument] ?? {}}
          onSave={(mine) => save(set, mine)} />
      ))}
    </section>
  );
}
