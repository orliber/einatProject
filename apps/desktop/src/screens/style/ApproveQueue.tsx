import { useState, type ReactNode } from "react";
import { useQueryClient } from "@tanstack/react-query";
import { useApp } from "../../App";
import { useAi } from "../../ai";
import { ErrorLine } from "../../components/ui";
import { ipc, type StyleItem, type StyleOverview, type StyleSuggestion } from "../../ipc/client";
import { InferenceChain } from "./InferenceChain";
import { KIND_TAG, RECOMMENDED, SUGGESTION_TAG } from "./labels";
import { ProfileEditor } from "./ProfileEditor";
import type { Waiting } from "./waiting";

// הסגנון שלי · one at a time

function itemWhy(item: StyleItem, reports: number, o: StyleOverview): string {
  const title = item.section ? o.sections.find((s) => s.key === item.section)?.title : undefined;
  const where = item.section ? `בסעיף ${title ?? item.section}` : "בכל הדוח";
  if (item.origin === "edits") return `נלמד מהתיקונים שלך, ${where}.`;
  if (item.origin === "manual") return `כתבת את זה בעצמך, ${where}.`;
  if (item.support === 1) return `מופיע באחד הדוחות שלך, ${where}.`;
  if (item.support > 1) return `מופיע ב-${item.support} ${reports >= item.support ? `מתוך ${reports} ` : "מ"}הדוחות שלך, ${where}.`;
  return `מהניתוח של הדוחות שלך, ${where}.`;
}

function suggestionWhy(s: StyleSuggestion, o: StyleOverview): string {
  const title = s.section ? o.sections.find((x) => x.key === s.section)?.title : undefined;
  const where = title ? `, בסעיף ${title}` : "";
  if (s.kind === "replace" || s.kind === "avoid") return `שינית את זה ${s.count === 1 ? "פעם אחת" : `${s.count} פעמים`}${where}.`;
  const of = o.sources.length >= s.count ? ` מתוך ${o.sources.length} ` : " מ";
  return `מופיע ב-${s.count}${of}הדוחות שלך${where}.`;
}

function SuggestionText({ s }: { s: StyleSuggestion }) {
  switch (s.kind) {
    case "replace":
      return <>במקום <q>{s.from}</q> את כותבת <q>{s.to}</q></>;
    case "opening":
    case "closing":
      return <>{s.from}…</>;
    default:
      return <>{s.from}</>;
  }
}

/** One thing at a time, in the report's font, with "to use it", "to edit" and "not needed".
 * What she decides goes at once; the next thing comes up by itself. */
export function ApproveQueue({ o, asks, onReviewed, running, buildDue }: {
  o: StyleOverview;
  asks: Waiting[];
  onReviewed: (id: string) => void;
  running: boolean;
  /** A report waits to be read: the analysis starts from the journey's "ניתוח" stage (D-100). */
  buildDue: boolean;
}) {
  const { fail, notify } = useApp();
  const ai = useAi();
  const qc = useQueryClient();
  const [done, setDone] = useState(0);
  const [busy, setBusy] = useState(false);
  const [editing, setEditing] = useState<string | null>(null);
  const [text, setText] = useState("");
  const [error, setError] = useState<string | null>(null);
  const [whole, setWhole] = useState(false);
  const current = asks[0];

  async function act(f: () => Promise<unknown>) {
    if (busy) return;
    setBusy(true);
    setError(null);
    try {
      await f();
      setEditing(null);
      // The count starts again once the queue is empty, so what comes later reads "1 of 1".
      setDone((d) => (asks.length <= 1 ? 0 : d + 1));
      await qc.invalidateQueries({ queryKey: ["style"] });
    } catch (e) {
      setError(fail(e as never));
    } finally {
      setBusy(false);
    }
  }

  /** Her word on one item of the draft. The last one makes the draft the profile in use. */
  async function decide(item: StyleItem, change: Partial<StyleItem>) {
    const draft = o.draft;
    if (!draft) return;
    const items = draft.profile.items.map((x) => (x.id === item.id ? { ...x, ...change } : x));
    const left = asks.filter((a) => a.kind === "item" && a.item.id !== item.id).length;
    const changed = Object.entries(change).some(([k, v]) => item[k as keyof StyleItem] !== v);
    if (changed || left === 0) await ipc.saveStyleDraft({ items, reports: draft.profile.reports });
    if (left > 0) {
      onReviewed(item.id);
      return;
    }
    await finish(items.some((i) => i.enabled));
  }

  async function finish(any: boolean) {
    if (any) {
      await ipc.approveStyleDraft();
      notify(`הפרופיל אושר. מעכשיו ${ai} כותב בסגנון שלך.`);
    } else {
      await ipc.discardStyleDraft();
      notify("לא נשאר פריט לשימוש, והטיוטה נמחקה.");
    }
  }

  if (!current) {
    return (
      <section className="card style-queue style-queue-empty" aria-labelledby="style-queue-title">
        {buildDue ? (
          <>
            <h2 id="style-queue-title">ללמוד מהדוחות</h2>
            <p className="style-queue-lead">
              {o.sources.length < RECOMMENDED ? `מומלץ ${RECOMMENDED} דוחות לפחות. ` : ""}
              הניתוח מתחיל בשלב «ניתוח» למעלה. אחריו כל דבר שהתוכנה למדה יחכה כאן לאישור שלך.
            </p>
          </>
        ) : (
          <>
            <h2 id="style-queue-title">הכול מאושר</h2>
            <p className="style-queue-lead">
              {o.active ? `הפרופיל בשימוש, גרסה ${o.active.version}.` : "אין כרגע מה לאשר."}
              {" "}דבר חדש שהתוכנה תלמד מהתיקונים שלך יחכה כאן.
            </p>
          </>
        )}
      </section>
    );
  }

  const total = done + asks.length;
  const isEditing = editing === current.key;
  // One click for the draft's remaining items, so a long queue is never a chore (D-100): what
  // she already said no to stays off, the rest goes into use as the draft holds it.
  const rest = asks.filter((a) => a.kind === "item").length;
  const useRest = async () => {
    const items = o.draft?.profile.items ?? [];
    await finish(items.some((i) => i.enabled));
  };
  const canEdit = current.kind === "item" || current.kind === "interpretation";
  const startEdit = () => {
    setEditing(current.key);
    setText(current.kind === "item" ? current.item.text : current.kind === "interpretation" ? current.view.item.text : "");
  };

  let tag: string;
  let body: ReactNode;
  let why: string;
  let use: () => Promise<unknown>;
  let save: (t: string) => Promise<unknown> = async () => undefined;
  let skip: () => Promise<unknown>;
  switch (current.kind) {
    case "item": {
      const { item } = current;
      tag = KIND_TAG[item.kind];
      body = item.kind === "inference" ? <InferenceChain text={item.text} /> : item.text;
      why = itemWhy(item, o.draft?.profile.reports ?? 0, o)
        + (item.kind === "inference" ? " בטיוטה: כשבחומרים יש ממצא כזה, הניסוח מסיק ומנסח לפי הכלל; בלי ממצא כזה הכלל לא משמש." : "");
      use = () => decide(item, { enabled: true });
      save = (t) => decide(item, { enabled: true, text: t });
      skip = () => decide(item, { enabled: false });
      break;
    }
    case "draft":
      tag = "הפרופיל המעודכן";
      body = `${current.items} פריטים, כולם כבר אושרו בגרסה הקודמת.`;
      why = "אין בטיוטה הזאת דבר חדש. אישור מכניס אותה לשימוש כגרסה חדשה.";
      use = () => finish(current.items > 0);
      skip = () => ipc.discardStyleDraft();
      break;
    case "suggestion": {
      const { s } = current;
      tag = SUGGESTION_TAG[s.kind];
      body = <SuggestionText s={s} />;
      why = suggestionWhy(s, o);
      use = () => ipc.acceptStyleSuggestion(s.id).then(() => notify("נוסף לפרופיל."));
      skip = () => ipc.dismissStyleSuggestion(s.id);
      break;
    }
    case "interpretation": {
      const { item, instrument_name, measure_name } = current.view;
      tag = "איך את מפרשת ציון";
      body = item.text;
      why = `${instrument_name} · ${measure_name}, ${item.case}${item.support > 1 ? `. כך כתבת ב-${item.support} דוחות.` : "."}`;
      use = () => ipc.updateInterpretation(item.id, null, true, null);
      save = (t) => ipc.updateInterpretation(item.id, t, true, null);
      // "Not needed" keeps it, off, among what the program knows: she can turn it on there.
      skip = () => ipc.updateInterpretation(item.id, null, true, false);
      break;
    }
  }

  return (
    <section className="card style-queue" aria-labelledby="style-queue-title">
      <div className="style-queue-head">
        <h2 id="style-queue-title">לאשר אחד-אחד</h2>
        <span className="muted">{done + 1} מתוך {total}</span>
      </div>
      <div className="style-progress" role="progressbar" aria-label="כמה כבר אישרת" aria-valuemin={0} aria-valuemax={total} aria-valuenow={done}>
        <span style={{ width: `${Math.round((done / total) * 100)}%` }} />
      </div>
      <span className="style-tag">{tag}</span>
      {isEditing ? (
        <textarea className="textarea style-quote-edit" aria-label="הנוסח" rows={3} maxLength={600} autoFocus value={text}
          onChange={(e) => setText(e.target.value)}
          onKeyDown={(e) => {
            if (e.key === "Escape") {
              e.preventDefault();
              setEditing(null);
            } else if (e.key === "Enter" && (e.ctrlKey || e.metaKey) && text.trim()) {
              e.preventDefault();
              void act(() => save(text.trim()));
            }
          }} />
      ) : (
        <blockquote className="style-quote">{body}</blockquote>
      )}
      <p className="style-why">{why}</p>
      <ErrorLine error={error} />
      <div className="style-queue-actions">
        {isEditing ? (
          <>
            <button type="button" className="btn btn-primary" disabled={busy || !text.trim()} onClick={() => void act(() => save(text.trim()))}>שמירה ושימוש</button>
            <button type="button" className="btn" disabled={busy} onClick={() => setEditing(null)}>ביטול</button>
          </>
        ) : (
          <>
            <button type="button" className="btn btn-primary" disabled={busy || running} onClick={() => void act(use)}>להשתמש בזה</button>
            {canEdit && <button type="button" className="btn" disabled={busy || running} onClick={startEdit}>לערוך</button>}
            <button type="button" className="btn btn-ghost" disabled={busy || running} onClick={() => void act(skip)}>לא צריך</button>
          </>
        )}
      </div>
      {o.draft && (current.kind === "item" || current.kind === "draft") && (
        <>
          <div className="style-queue-rest">
            {rest > 1 && !isEditing && (
              <button type="button" className="btn btn-ghost btn-small" disabled={busy || running} onClick={() => void act(useRest)}>
                להשתמש בכל {rest} שנשארו
              </button>
            )}
            <button type="button" className="btn btn-ghost btn-small style-whole" aria-expanded={whole} onClick={() => setWhole((w) => !w)}>
              {whole ? "להסתיר את הטיוטה" : "לעבור על כל הטיוטה בבת אחת"}
            </button>
          </div>
          {whole && <ProfileEditor key={o.draft.id} view={o.draft} o={o} />}
        </>
      )}
    </section>
  );
}
