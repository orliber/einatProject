import { useEffect, useMemo, useRef, useState, type ClipboardEvent, type KeyboardEvent } from "react";
import { useApp } from "../App";
import { ipc, type CaseInput, type Instrument, type ScoreSheet } from "../ipc/client";
import type { Age } from "../ipc/generated/Age";
import type { Band } from "../ipc/generated/Band";
import type { Measure } from "../ipc/generated/Measure";
import { ageWords } from "./AgeField";
import { PROFILE_SCALES, ScoreChart } from "./ScoreChart";
import { ErrorLine, Spinner } from "./ui";
import "./ScoresView.css";
import { useAi } from "../ai";

type Entry = { value: string; note: string };

/** "29,5" and "29.5" are the same score; anything else that is not a number is not a score. */
export function parse(raw: string): number | null {
  const t = raw.trim().replace(",", ".");
  if (!t) return null;
  const n = Number(t);
  return Number.isFinite(n) ? n : null;
}

/** The band of a score in the measure's fixed table (deterministic, never the model). */
export function bandOf(m: Measure, v: number): Band | null {
  return m.bands.find((b) => v >= b.min && v <= b.max) ?? null;
}

/** A typed score that is not a possible score of its measure. Empty is not wrong. */
export function isBad(m: Measure, raw: string): boolean {
  if (!raw.trim()) return false;
  const v = parse(raw);
  return v === null || v < m.min || v > m.max;
}

/** The measures in the order of the table: grouped as the instrument groups them (the
 *  order of its record form), each group in the instrument's order. */
export function tableOrder(instrument: Instrument): Measure[] {
  const groups = Array.from(new Set(instrument.measures.map((m) => m.group)));
  return groups.flatMap((g) => instrument.measures.filter((m) => m.group === g));
}

/** What is still missing: the tables say nothing of which subtests make up an index, so
 *  "missing" is every measure not entered yet in a group she has started (an untouched group,
 *  such as the ancillary indexes, is not asked for). Nothing entered: nothing is missing yet. */
export function missingOf(instrument: Instrument, entries: Record<string, Entry>): Measure[] {
  const has = (m: Measure) => (entries[m.key]?.value ?? "").trim() !== "";
  const started = new Set(instrument.measures.filter(has).map((m) => m.group));
  return tableOrder(instrument).filter((m) => started.has(m.group) && !has(m));
}

/** A column (or a row) copied from a spreadsheet: one value per line or per tab. A single
 *  value is an ordinary paste (`null`). A trailing empty line, as Excel copies, is dropped. */
export function pasteCells(text: string): string[] | null {
  const cells = text.replace(/\r/g, "").split(/[\n\t]/).map((c) => c.trim());
  while (cells.length > 0 && cells[cells.length - 1] === "") cells.pop();
  return cells.length > 1 ? cells : null;
}

function toEntries(sheet: ScoreSheet | null | undefined): Record<string, Entry> {
  const out: Record<string, Entry> = {};
  for (const e of sheet?.entries ?? []) out[e.measure] = { value: String(e.value), note: e.note };
  return out;
}

/** "מ-6 שנים עד 16 שנים ו-11 חודשים". */
export function ageSpan(minMonths: number, maxMonths: number): string {
  const w = (n: number) => ageWords({ years: Math.floor(n / 12), months: n % 12 });
  return `מ-${w(minMonths)} עד ${w(maxMonths)}`;
}

type Draft = {
  inputId: string | null;
  key: string;
  entries: Record<string, Entry>;
  module: string;
  cutoff: string;
  notes: string;
  initialEntries: string;
};

/** Scores typed and not saved yet, by case, while she goes to another stage and back. In
 *  the page's memory only: never stored, and gone with the page when the vault locks. */
const drafts = new Map<string, Draft>();

/** The scores left unsaved in this case, to open the table again where she was. */
export function scoresDraft(caseId: string): { inputId: string | null } | null {
  const d = drafts.get(caseId);
  return d ? { inputId: d.inputId } : null;
}

/** For tests: every draft forgotten. */
export function forgetScoreDrafts() {
  drafts.clear();
}

/** The instrument a new table opens on: the first whose ages (from the core's tables) hold the child's age. */
function firstFit(list: Instrument[], age: Age | null): string | null {
  if (!age) return null;
  const months = age.years * 12 + age.months;
  return list.find((i) => months >= i.min_age_months && months <= i.max_age_months)?.key ?? null;
}

/**
 * The score table, a view of the materials stage (round 54, D-094): the table in the order
 * of the record form, the band of each score in words beside it, a profile that follows
 * the typing, and the text that will be stored. The range of each score comes from the fixed
 * tables in the core, and the preview is the exact text stored and later sent (after filtering).
 * Enter goes to the next score, Tab to the note, and a column pasted from a spreadsheet
 * fills the scores from the field it is pasted into, in order.
 */
export function ScoresView(props: {
  caseId: string;
  age: Age | null;
  input?: CaseInput | undefined;
  sheet?: ScoreSheet | null | undefined;
  onClose: () => void;
  onSaved: (id: string) => Promise<void>;
}) {
  const { fail } = useApp();
  const ai = useAi();
  const inputId = props.input?.id ?? null;
  const [draft] = useState(() => {
    const d = drafts.get(props.caseId);
    return d && d.inputId === inputId ? d : null;
  });
  const [instruments, setInstruments] = useState<Instrument[] | null>(null);
  /** A new table opens on the instrument for the child's age (null: kept or edited table). */
  const opensFor = useRef(draft || props.sheet ? null : props.age);
  /** Instruments whose tables still await her approval in Settings → המבחנים (D-098). */
  const [awaiting, setAwaiting] = useState<Set<string>>(() => new Set());
  const [key, setKey] = useState(draft?.key ?? props.sheet?.instrument ?? "wppsi_iv");
  const [entries, setEntries] = useState<Record<string, Entry>>(() => draft?.entries ?? toEntries(props.sheet));
  const [module, setModule] = useState(draft?.module ?? props.sheet?.module ?? "");
  const [cutoff, setCutoff] = useState(draft?.cutoff ?? (props.sheet?.cutoff != null ? String(props.sheet.cutoff) : ""));
  const [notes, setNotes] = useState(draft?.notes ?? props.sheet?.notes ?? "");
  const [preview, setPreview] = useState<{ text: string; error: string | null }>({ text: "", error: null });
  const [saving, setSaving] = useState(false);
  /** Saved: nothing here is unsaved any more, even before the view closes. */
  const [done, setDone] = useState(false);
  // Everything she can type: the scores, their notes, the module, the cut-off and the notes.
  const [initialEntries] = useState(() => draft?.initialEntries ?? JSON.stringify([entries, module, cutoff, notes]));
  const [error, setError] = useState<string | null>(null);
  const [pasted, setPasted] = useState<string | null>(null);
  const [confirming, setConfirming] = useState(false);
  const fields = useRef(new Map<string, HTMLInputElement>());
  const notesRef = useRef<HTMLTextAreaElement>(null);
  const radios = useRef(new Map<string, HTMLButtonElement>());
  const focused = useRef(false);

  useEffect(() => {
    let alive = true;
    ipc
      .scoreInstruments()
      .then((list) => {
        if (!alive) return;
        const given = list.map((a) => a.instrument);
        setInstruments(given);
        setAwaiting(new Set(list.filter((a) => a.status === "awaiting").map((a) => a.instrument.key)));
        // A new table opens at an instrument she gives, the first whose ages hold the child's.
        const fit = firstFit(given, opensFor.current);
        if (fit) setKey(fit);
        else if (!props.input && !draft) setKey((k) => (given.some((i) => i.key === k) ? k : given[0]?.key ?? k));
      })
      .catch((e: unknown) => alive && setError(fail(e as never)));
    return () => {
      alive = false;
    };
    // Loaded once, when the view opens.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [fail]);

  const instrument = instruments?.find((i) => i.key === key) ?? null;
  const order = useMemo(() => (instrument ? tableOrder(instrument) : []), [instrument]);

  // Once the table is there: a new table starts at the instrument, an edited one at its first score.
  useEffect(() => {
    if (!instrument || focused.current) return;
    focused.current = true;
    if (props.input || draft) fields.current.get(order[0]?.key ?? "")?.focus();
    else radios.current.get(instrument.key)?.focus();
  }, [instrument, order, props.input, draft]);

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

  const dirty = !saving && !done && JSON.stringify([entries, module, cutoff, notes]) !== initialEntries;
  // Unsaved scores wait in memory while she is on another stage (see `drafts`).
  useEffect(() => {
    if (dirty) drafts.set(props.caseId, { inputId, key, entries, module, cutoff, notes, initialEntries });
    else drafts.delete(props.caseId);
  }, [dirty, props.caseId, inputId, key, entries, module, cutoff, notes, initialEntries]);

  const ageMonths = props.age ? props.age.years * 12 + props.age.months : null;
  const outOfAge =
    instrument && props.age && ageMonths !== null && (ageMonths < instrument.min_age_months || ageMonths > instrument.max_age_months);
  const groups = instrument ? Array.from(new Set(instrument.measures.map((m) => m.group))) : [];
  const invalid = instrument?.measures.filter((m) => isBad(m, entries[m.key]?.value ?? "")) ?? [];
  const filled = sheet?.entries.length ?? 0;
  const missing = instrument ? missingOf(instrument, entries) : [];
  const isAdos = key === "ados_2";
  const charts = PROFILE_SCALES.flatMap((p) => {
    const ms = order.filter((m) => m.scale === p.scale);
    if (!ms.length) return [];
    const rows = ms.flatMap((m) => {
      const raw = entries[m.key]?.value ?? "";
      const v = parse(raw);
      return v === null || isBad(m, raw) ? [] : [{ measure: m, value: v }];
    });
    return [{ ...p, rows }];
  });

  function set(measure: string, patch: Partial<Entry>) {
    setEntries((all) => ({ ...all, [measure]: { value: "", note: "", ...all[measure], ...patch } }));
  }

  function close() {
    if (dirty) setConfirming(true);
    else leave();
  }

  function leave() {
    drafts.delete(props.caseId);
    props.onClose();
  }

  /** Enter: the next score in the table; after the last one, the notes on the session. */
  function next(e: KeyboardEvent<HTMLInputElement>, at: number) {
    if (e.key !== "Enter" || e.nativeEvent.isComposing) return;
    e.preventDefault();
    const m = order[at + 1];
    if (m) fields.current.get(m.key)?.focus();
    else notesRef.current?.focus();
  }

  /** A column from a spreadsheet fills this score and the ones after it, in the table's order.
   *  An empty cell leaves its field as it was; what does not fit is said, not dropped silently. */
  function paste(e: ClipboardEvent<HTMLInputElement>, at: number) {
    const cells = pasteCells(e.clipboardData.getData("text/plain"));
    if (!cells) return;
    e.preventDefault();
    const targets = order.slice(at, at + cells.length);
    setEntries((all) => {
      const out = { ...all };
      targets.forEach((m, i) => {
        const c = cells[i] ?? "";
        if (c !== "") out[m.key] = { note: "", ...all[m.key], value: c };
      });
      return out;
    });
    const over = cells.length - targets.length;
    const put = cells.slice(0, targets.length).filter((c) => c !== "").length;
    setPasted(over > 0
      ? `הודבקו ${put} ציונים. ${over === 1 ? "ערך אחד לא נכנס" : `${over} ערכים לא נכנסו`}: אין אחרי זה עוד שדות.`
      : `הודבקו ${put} ציונים, לפי הסדר.`);
    const after = order[at + targets.length];
    (after ? fields.current.get(after.key) : fields.current.get(targets[targets.length - 1]?.key ?? ""))?.focus();
  }

  /** The instruments as one group of choices: the arrows move between them (right to left). */
  function pick(e: KeyboardEvent<HTMLButtonElement>) {
    if (!instruments || props.input) return;
    const step = e.key === "ArrowLeft" || e.key === "ArrowDown" ? 1 : e.key === "ArrowRight" || e.key === "ArrowUp" ? -1 : 0;
    if (!step) return;
    e.preventDefault();
    const i = instruments.findIndex((x) => x.key === key);
    const to = instruments[(i + step + instruments.length) % instruments.length];
    if (!to) return;
    setKey(to.key);
    radios.current.get(to.key)?.focus();
  }

  async function save() {
    if (!sheet || filled === 0) return setError("עוד לא הוזנו ציונים.");
    if (invalid.length) return setError("יש ציונים מחוץ לטווח האפשרי. כדאי לבדוק את השורות המסומנות.");
    setSaving(true);
    setError(null);
    try {
      const saved = await ipc.saveScores(props.caseId, inputId, sheet);
      setDone(true);
      drafts.delete(props.caseId);
      await props.onSaved(saved.id);
    } catch (e) {
      setError(fail(e as never));
    } finally {
      setSaving(false);
    }
  }

  const title = props.input ? "עריכת ציונים" : "הזנת ציונים";

  return (
    <div className="view scores-view" aria-label={title} role="region"
      onKeyDown={(e) => {
        if (e.key !== "Escape" || e.defaultPrevented) return;
        e.preventDefault();
        if (confirming) setConfirming(false);
        else close();
      }}>
      <div className="view-head">
        <div className="stack" style={{ gap: 4 }}>
          <button type="button" className="link-btn small" onClick={close}>→ חזרה לחומרים</button>
          <h1>{title}</h1>
          <p className="muted small">הטווח והאחוזון מחושבים בתוכנה. {ai} לא מפרש ציונים.</p>
        </div>
      </div>
      <div className="view-body scores-body">
        {!instruments ? (
          <>
            <p className="muted"><Spinner /> טוענת את הכלים…</p>
            <ErrorLine error={error} />
          </>
        ) : (
          <div className="scores">
            <div className="scores-main stack">
              <div className="field">
                <span className="label" id="inst-label">הכלי</span>
                <div className="inst-picker" role="radiogroup" aria-labelledby="inst-label">
                  {instruments.map((i) => (
                    <button key={i.key} type="button" role="radio" aria-checked={i.key === key}
                      ref={(el) => { if (el) radios.current.set(i.key, el); else radios.current.delete(i.key); }}
                      tabIndex={i.key === key ? 0 : -1}
                      className={i.key === key ? "inst on" : "inst"}
                      disabled={Boolean(props.input) && i.key !== key}
                      onKeyDown={pick}
                      onClick={() => setKey(i.key)}>
                      {i.name}
                      {awaiting.has(i.key) && <span className="inst-draft" title="הטבלאות מחכות לאישור שלך בהגדרות"> · טיוטה</span>}
                    </button>
                  ))}
                </div>
                {!instrument && props.input && (
                  <p className="small warn-text" role="note">
                    סימנת בהגדרות שאת לא מעבירה את הכלי הזה. הציונים נשארים בתיק כמו שנשמרו, אבל לא נשלחים ולא נכנסים לדוח.
                    כדי לערוך אותם, סמני אותו שוב כמאושר בהגדרות ← המבחנים.
                  </p>
                )}
                {instrument && awaiting.has(instrument.key) && (
                  <p className="small warn-text" role="note">
                    הטבלאות של {instrument.name} עוד מחכות לאישור שלך (הגדרות ← המבחנים). עד אז, רק בתיקים בדויים.
                  </p>
                )}
                {instrument && (
                  <p className="small muted">
                    {instrument.description_he}
                    {outOfAge && props.age && (
                      <span className="age-warn" role="note">
                        {" "}· גיל הילד/ה ({ageWords(props.age)}) מחוץ לטווח הגילים של הכלי ({ageSpan(instrument.min_age_months, instrument.max_age_months)})
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
                      const at = order.indexOf(m);
                      const e = entries[m.key];
                      const raw = e?.value ?? "";
                      const v = parse(raw);
                      const bad = isBad(m, raw);
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
                              ref={(el) => { if (el) fields.current.set(m.key, el); else fields.current.delete(m.key); }}
                              aria-invalid={bad} aria-describedby={bad ? `${id}-err` : undefined}
                              enterKeyHint="next"
                              value={raw} onChange={(ev) => set(m.key, { value: ev.target.value })}
                              onKeyDown={(ev) => next(ev, at)} onPaste={(ev) => paste(ev, at)} />
                            {bad && (
                              <span id={`${id}-err`} className="score-err">
                                {v === null ? "רק מספר" : <>אפשרי <span dir="ltr">{m.min}–{m.max}</span></>}
                              </span>
                            )}
                          </td>
                          <td className="col-band">
                            {band ? (
                              <span className={`band band-${band.level}`}>{band.label}</span>
                            ) : m.scale === "raw_cutoff" && m.key === "total" && v !== null && !bad && parse(cutoff) !== null ? (
                              <span className={v >= (parse(cutoff) ?? 0) ? "band band-2" : "band band-0"}>
                                {v >= (parse(cutoff) ?? 0) ? "בנקודת החתך או מעליה" : "מתחת לנקודת החתך"}
                              </span>
                            ) : null}
                          </td>
                          <td>
                            <input className="input note-input" aria-label={`הערה ל${m.name_he}`} value={e?.note ?? ""}
                              onChange={(ev) => set(m.key, { note: ev.target.value })}
                              onKeyDown={(ev) => next(ev, at)} />
                          </td>
                        </tr>
                      );
                    })}
                  </tbody>
                </table>
              ))}

              <p className="scores-missing small" aria-live="polite">
                {missing.length > 0 && (
                  <>
                    <b>עוד לא הוזנו: </b>
                    {missing.slice(0, 8).map((m) => m.name_he).join(", ")}
                    {missing.length > 8 && ` ועוד ${missing.length - 8}`}
                  </>
                )}
              </p>
              {pasted && <p className="small muted" role="status">{pasted}</p>}

              <div className="field">
                <label htmlFor="score-notes">הערות על ההעברה (שיתוף פעולה, קשב, תנאים)</label>
                <textarea ref={notesRef} id="score-notes" className="textarea" rows={3} value={notes} onChange={(e) => setNotes(e.target.value)} />
              </div>
              <ErrorLine error={error} />
              <section className="scores-preview" aria-label="מה יישמר בתיק">
                <h3>מה יישמר בתיק</h3>
                <p className="small muted">הטקסט שיזין את הסעיפים.</p>
                {filled === 0 ? (
                  <p className="muted small preview-empty">ממלאים ציון כדי לראות את הניסוח.</p>
                ) : (
                  <pre className="preview-text serif">{preview.text}</pre>
                )}
                {preview.error && filled > 0 && <p className="error small">{preview.error}</p>}
              </section>
            </div>

            <aside className="scores-side stack" aria-label="פרופיל הציונים">
              {charts.length > 0 ? (
                charts.map((c) => (
                  <section key={c.scale} className="scores-chart" aria-label={c.title}>
                    <h3>{c.title}</h3>
                    {c.rows.length ? (
                      <ScoreChart title={c.title} mean={c.mean} sd={c.sd} rows={c.rows} />
                    ) : (
                      <p className="muted small">ממלאים ציון והוא מופיע כאן.</p>
                    )}
                  </section>
                ))
              ) : (
                <p className="muted small scores-chart">לכלי הזה אין פרופיל: הסולם שלו לא נועד להשוואה בין עמודות.</p>
              )}
            </aside>
          </div>
        )}
      </div>
      {confirming ? (
        <footer className="scores-foot" role="alertdialog" aria-label="לצאת בלי לשמור">
          <span className="grow">מה שהקלדת עדיין לא נשמר. לצאת בכל זאת?</span>
          <button type="button" className="btn btn-primary" autoFocus onClick={() => setConfirming(false)}>להמשיך לערוך</button>
          <button type="button" className="btn" onClick={leave}>לצאת בלי לשמור</button>
        </footer>
      ) : (
        <footer className="scores-foot">
          <span className="small muted grow">{filled ? `${filled} ציונים הוזנו` : "ממלאים רק את מה שהועבר."}</span>
          <button type="button" className="btn" onClick={close}>ביטול</button>
          <button type="button" className="btn btn-primary" disabled={saving || filled === 0} onClick={() => void save()}>
            {saving ? <><Spinner /> שומרת…</> : "שמירה בתיק"}
          </button>
        </footer>
      )}
    </div>
  );
}
