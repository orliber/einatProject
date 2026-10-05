import { useEffect, useMemo, useState } from "react";
import { useApp } from "../App";
import { ipc, type CaseInput, type Instrument, type ScoreSheet } from "../ipc/client";
import type { Age } from "../ipc/generated/Age";
import type { Band } from "../ipc/generated/Band";
import type { Measure } from "../ipc/generated/Measure";
import { Dialog, ErrorLine, Spinner } from "./ui";
import "./ScoresDialog.css";
import { useAi } from "../ai";

type Entry = { value: string; note: string };

/** "29,5" and "29.5" are the same score; anything else that is not a number is not a score. */
function parse(raw: string): number | null {
  const t = raw.trim().replace(",", ".");
  if (!t) return null;
  const n = Number(t);
  return Number.isFinite(n) ? n : null;
}

function bandOf(m: Measure, v: number): Band | null {
  return m.bands.find((b) => v >= b.min && v <= b.max) ?? null;
}

function ageText(months: number): string {
  return `${Math.floor(months / 12)}:${months % 12}`;
}

function toEntries(sheet: ScoreSheet | null | undefined): Record<string, Entry> {
  const out: Record<string, Entry> = {};
  for (const e of sheet?.entries ?? []) out[e.measure] = { value: String(e.value), note: e.note };
  return out;
}

/**
 * The score table. The range of each score comes from the fixed tables in the core, and the
 * preview on the side is the exact text that will be stored and later sent (after filtering).
 */
export function ScoresDialog(props: {
  caseId: string;
  age: Age | null;
  input?: CaseInput | undefined;
  sheet?: ScoreSheet | null | undefined;
  onClose: () => void;
  onSaved: (id: string) => Promise<void>;
}) {
  const { fail } = useApp();
  const ai = useAi();
  const [instruments, setInstruments] = useState<Instrument[] | null>(null);
  const [key, setKey] = useState(props.sheet?.instrument ?? "wppsi_iv");
  const [entries, setEntries] = useState<Record<string, Entry>>(() => toEntries(props.sheet));
  const [module, setModule] = useState(props.sheet?.module ?? "");
  const [cutoff, setCutoff] = useState(props.sheet?.cutoff != null ? String(props.sheet.cutoff) : "");
  const [notes, setNotes] = useState(props.sheet?.notes ?? "");
  const [preview, setPreview] = useState<{ text: string; error: string | null }>({ text: "", error: null });
  const [saving, setSaving] = useState(false);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    let alive = true;
    ipc
      .scoreInstruments()
      .then((list) => alive && setInstruments(list))
      .catch((e: unknown) => alive && setError(fail(e as never)));
    return () => {
      alive = false;
    };
  }, [fail]);

  const instrument = instruments?.find((i) => i.key === key) ?? null;

  const sheet: ScoreSheet | null = useMemo(() => {
    if (!instrument) return null;
    const list = instrument.measures.flatMap((m) => {
      const e = entries[m.key];
      const v = e ? parse(e.value) : null;
      return v === null ? [] : [{ measure: m.key, value: v, note: e?.note.trim() ?? "" }];
    });
    return { instrument: instrument.key, module: module.trim(), cutoff: parse(cutoff), entries: list, notes: notes.trim() };
  }, [instrument, entries, module, cutoff, notes]);

  useEffect(() => {
    if (!sheet || sheet.entries.length === 0) return;
    let alive = true;
    const t = window.setTimeout(() => {
      ipc
        .previewScores(sheet)
        .then((text) => alive && setPreview({ text, error: null }))
        .catch((e: unknown) => alive && setPreview((p) => ({ text: p.text, error: (e as { message: string }).message })));
    }, 200);
    return () => {
      alive = false;
      window.clearTimeout(t);
    };
  }, [sheet]);

  const ageMonths = props.age ? props.age.years * 12 + props.age.months : null;
  const outOfAge =
    instrument && ageMonths !== null && (ageMonths < instrument.min_age_months || ageMonths > instrument.max_age_months);
  const groups = instrument ? Array.from(new Set(instrument.measures.map((m) => m.group))) : [];
  const invalid = instrument?.measures.filter((m) => {
    const raw = entries[m.key]?.value ?? "";
    if (!raw.trim()) return false;
    const v = parse(raw);
    return v === null || v < m.min || v > m.max;
  }) ?? [];
  const filled = sheet?.entries.length ?? 0;
  const isAdos = key === "ados_2";

  function set(measure: string, patch: Partial<Entry>) {
    setEntries((all) => ({ ...all, [measure]: { value: "", note: "", ...all[measure], ...patch } }));
  }

  async function save() {
    if (!sheet || filled === 0) return setError("עוד לא הוזנו ציונים.");
    if (invalid.length) return setError("יש ציונים מחוץ לטווח האפשרי. כדאי לבדוק את השורות המסומנות.");
    setSaving(true);
    setError(null);
    try {
      const saved = await ipc.saveScores(props.caseId, props.input?.id ?? null, sheet);
      await props.onSaved(saved.id);
    } catch (e) {
      setError(fail(e as never));
    } finally {
      setSaving(false);
    }
  }

  return (
    <Dialog wide title={props.input ? "עריכת ציונים" : "הזנת ציונים"}
      subtitle={`הטווח והאחוזון מחושבים בתוכנה לפי טבלה קבועה. ${ai} מקבל את הטקסט המוכן ואינו מפרש ציונים בעצמו.`}
      onClose={props.onClose}
      footer={
        <>
          <span className="small muted grow">{filled ? `${filled} ציונים הוזנו` : "ממלאים רק את מה שהועבר. שורות ריקות לא נשמרות."}</span>
          <button type="button" className="btn" onClick={props.onClose}>ביטול</button>
          <button type="button" className="btn btn-primary" disabled={saving || filled === 0} onClick={() => void save()}>
            {saving ? <><Spinner /> שומרת…</> : "שמירה בתיק"}
          </button>
        </>
      }>
      {!instruments ? (
        <p className="muted"><Spinner /> טוענת את הכלים…</p>
      ) : (
        <div className="scores">
          <div className="scores-main stack">
            <div className="field">
              <span className="label" id="inst-label">הכלי</span>
              <div className="inst-picker" role="radiogroup" aria-labelledby="inst-label">
                {instruments.map((i) => (
                  <button key={i.key} type="button" role="radio" aria-checked={i.key === key}
                    className={i.key === key ? "inst on" : "inst"}
                    disabled={Boolean(props.input) && i.key !== key}
                    onClick={() => setKey(i.key)}>
                    {i.name}
                  </button>
                ))}
              </div>
              {instrument && (
                <p className="small muted">
                  {instrument.description_he}
                  {outOfAge && (
                    <span className="age-warn" role="note">
                      {" "}· גיל הילד/ה ({ageText(ageMonths ?? 0)}) מחוץ לטווח הגילים של הכלי ({ageText(instrument.min_age_months)}–{ageText(instrument.max_age_months)})
                    </span>
                  )}
                </p>
              )}
            </div>

            {isAdos && (
              <div className="row ados-row">
                <div className="field">
                  <label htmlFor="ados-module">מודול</label>
                  <input id="ados-module" className="input" value={module} onChange={(e) => setModule(e.target.value)} placeholder="1, 2, 3 או T" />
                </div>
                <div className="field">
                  <label htmlFor="ados-cutoff">נקודת החתך לספקטרום (מהפרוטוקול)</label>
                  <input id="ados-cutoff" className="input num" inputMode="decimal" dir="ltr" value={cutoff} onChange={(e) => setCutoff(e.target.value)} />
                </div>
              </div>
            )}

            {instrument && groups.map((g) => (
              <table key={g} className="score-table">
                <caption>{g}</caption>
                <thead>
                  <tr>
                    <th scope="col">מדד</th>
                    <th scope="col" className="col-score">ציון</th>
                    <th scope="col" className="col-band">טווח</th>
                    <th scope="col">הערה (לא חובה)</th>
                  </tr>
                </thead>
                <tbody>
                  {instrument.measures.filter((m) => m.group === g).map((m) => {
                    const e = entries[m.key];
                    const raw = e?.value ?? "";
                    const v = parse(raw);
                    const bad = raw.trim() !== "" && (v === null || v < m.min || v > m.max);
                    const band = v !== null && !bad ? bandOf(m, v) : null;
                    const id = `score-${m.key}`;
                    return (
                      <tr key={m.key} className={raw.trim() ? "filled" : undefined}>
                        <th scope="row">
                          <label htmlFor={id}>{m.name_he}</label>
                          {m.abbr && <span className="abbr">{m.abbr}</span>}
                        </th>
                        <td className="col-score">
                          <input id={id} className={bad ? "input num score-input bad" : "input num score-input"} inputMode="decimal" dir="ltr"
                            aria-invalid={bad} aria-describedby={bad ? `${id}-err` : undefined}
                            value={raw} onChange={(ev) => set(m.key, { value: ev.target.value })} />
                        </td>
                        <td className="col-band">
                          {bad ? (
                            <span id={`${id}-err`} className="band band-bad">טווח אפשרי {m.min}–{m.max}</span>
                          ) : band ? (
                            <span className={`band band-${band.level}`}>{band.label}</span>
                          ) : m.scale === "raw_cutoff" && m.key === "total" && v !== null && parse(cutoff) !== null ? (
                            <span className={v >= (parse(cutoff) ?? 0) ? "band band-2" : "band band-0"}>
                              {v >= (parse(cutoff) ?? 0) ? "בנקודת החתך או מעליה" : "מתחת לנקודת החתך"}
                            </span>
                          ) : null}
                        </td>
                        <td>
                          <input className="input note-input" aria-label={`הערה ל${m.name_he}`} value={e?.note ?? ""}
                            onChange={(ev) => set(m.key, { note: ev.target.value })} />
                        </td>
                      </tr>
                    );
                  })}
                </tbody>
              </table>
            ))}

            <div className="field">
              <label htmlFor="score-notes">הערות על ההעברה (שיתוף פעולה, קשב, תנאים)</label>
              <textarea id="score-notes" className="textarea" rows={3} value={notes} onChange={(e) => setNotes(e.target.value)} />
            </div>
            <ErrorLine error={error} />
          </div>

          <aside className="scores-preview" aria-label="מה יישמר בתיק">
            <h3>מה יישמר בתיק</h3>
            <p className="small muted">זה הטקסט שיזין את הסעיפים. שמות בהערות יוסתרו לפני כל שליחה.</p>
            {filled === 0 ? (
              <p className="muted small preview-empty">ממלאים ציון אחד לפחות כדי לראות כאן את הניסוח.</p>
            ) : (
              <pre className="preview-text serif">{preview.text}</pre>
            )}
            {preview.error && filled > 0 && <p className="error small">{preview.error}</p>}
          </aside>
        </div>
      )}
    </Dialog>
  );
}
