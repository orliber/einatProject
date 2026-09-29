import { useEffect, useRef, useState, type DragEvent } from "react";
import { useApp } from "../../App";
import { kindLabel, kindOrder } from "../../i18n/he";
import { ipc, type CaseInput, type ImportPreview, type InputKind, type MaterialRouting, type ScoreSheet } from "../../ipc/client";
import type { FilterOutcome } from "../../ipc/generated/FilterOutcome";
import { ImportDialog } from "../../components/ImportDialog";
import { ScoresDialog } from "../../components/ScoresDialog";
import { Dialog, ErrorLine, Segments, Spinner, UploadIcon } from "../../components/ui";
import type { CaseApi } from "../CaseScreen";
import "./MaterialsView.css";

const ACCEPT = ".docx,.odt,.pdf,.txt,application/vnd.oasis.opendocument.text,application/pdf,application/vnd.openxmlformats-officedocument.wordprocessingml.document,text/plain";

export function MaterialsView({ api }: { api: CaseApi }) {
  const { fail, notify } = useApp();
  const { detail, caseId, reload } = api;
  const [selected, setSelected] = useState<string | null>(detail.inputs[0]?.id ?? null);
  const [previewed, setPreviewed] = useState<{ id: string; outcome: FilterOutcome } | null>(null);
  const [importing, setImporting] = useState<ImportPreview | null>(null);
  const [reading, setReading] = useState<string | null>(null);
  const [writing, setWriting] = useState<{ kind: InputKind; input?: CaseInput } | null>(null);
  const [scoring, setScoring] = useState<{ input?: CaseInput; sheet?: ScoreSheet } | null>(null);
  const [choosing, setChoosing] = useState<CaseInput | null>(null);
  const [dragging, setDragging] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const fileRef = useRef<HTMLInputElement>(null);

  const input = detail.inputs.find((i) => i.id === selected) ?? null;
  const routeOf = (id: string) => detail.routing.find((r) => r.input_id === id);
  const route = input ? routeOf(input.id) : undefined;
  const titleOf = (key: string) => detail.sections.find((s) => s.key === key)?.title ?? key;

  useEffect(() => {
    let alive = true;
    if (input) {
      ipc
        .previewFilter(caseId, input.content)
        .then((outcome) => alive && setPreviewed({ id: input.id, outcome }))
        .catch(() => undefined);
    }
    return () => {
      alive = false;
    };
  }, [caseId, input]);
  const preview = previewed && input && previewed.id === input.id ? previewed.outcome : null;

  async function readFile(file: File) {
    setError(null);
    setReading(file.name);
    try {
      setImporting(await ipc.importDocument(caseId, file));
    } catch (e) {
      setError(fail(e as never));
    } finally {
      setReading(null);
    }
  }

  function onDrop(e: DragEvent) {
    e.preventDefault();
    setDragging(false);
    const file = e.dataTransfer.files[0];
    if (file) void readFile(file);
  }

  /** A score table entered here opens as a table again; any other material opens as text. */
  async function edit(i: CaseInput) {
    if (i.kind === "test_scores") {
      try {
        const sheet = await ipc.scoreSheet(caseId, i.id);
        if (sheet) return setScoring({ input: i, sheet });
      } catch (e) {
        return setError(fail(e as never));
      }
    }
    setWriting({ kind: i.kind, input: i });
  }

  async function remove(i: CaseInput) {
    try {
      await ipc.deleteInput(caseId, i.id);
      setSelected(null);
      await reload();
      notify("החומר נמחק מהתיק.");
    } catch (e) {
      setError(fail(e as never));
    }
  }

  const sorted = [...detail.inputs].sort((a, b) => b.created_at - a.created_at);

  return (
    <div className="view" onDragOver={(e) => { e.preventDefault(); setDragging(true); }} onDragLeave={() => setDragging(false)} onDrop={onDrop}>
      <div className="view-head">
        <div className="stack" style={{ gap: 4 }}>
          <h1>חומרי התיק</h1>
          <p className="muted small">הכל נשמר מוצפן. ל-Claude יוצא רק טקסט אחרי הסתרה, ורק באישורך.</p>
        </div>
        <div className="row">
          <button type="button" className="btn btn-primary" onClick={() => fileRef.current?.click()}><UploadIcon /> העלאת מסמך</button>
          <button type="button" className="btn" onClick={() => setScoring({})}>הזנת ציונים</button>
          <button type="button" className="btn" onClick={() => setWriting({ kind: "session_note" })}>רישום מפגש</button>
          <button type="button" className="btn" onClick={() => setWriting({ kind: "free_text" })}>הדבקת טקסט</button>
          <input ref={fileRef} type="file" accept={ACCEPT} hidden onChange={(e) => { const f = e.target.files?.[0]; e.target.value = ""; if (f) void readFile(f); }} />
        </div>
      </div>
      <ErrorLine error={error} />
      <div className="view-body">
        <section className="materials-list" aria-label="רשימת החומרים">
          {sorted.length === 0 && (
            <div className="card empty-materials">
              <b>עוד אין חומרים בתיק</b>
              <p className="muted small">מתחילים מהחומרים שכבר יש: דוחות של רופאים וקלינאיות, אינטייק, שיחה עם הגננת, תוצאות מבחנים והסיכומים שלך.</p>
              <ol className="start-steps small">
                <li><b>העלאת מסמך</b>: קובץ Word, ‏ODT (LibreOffice / Google Docs) או PDF מהמחשב (אפשר גם לגרור לכאן).</li>
                <li><b>הזנת ציונים</b>: טבלה לכל כלי, עם טווח ואחוזון מחושבים.</li>
                <li><b>רישום מפגש</b>: מה שראית, במילים שלך.</li>
              </ol>
              <button type="button" className="btn btn-primary" onClick={() => fileRef.current?.click()}>העלאת מסמך מהמחשב</button>
            </div>
          )}
          {sorted.map((i) => (
            <button key={i.id} type="button" className={i.id === selected ? "material selected" : "material"} onClick={() => setSelected(i.id)}>
              <span className="row">
                <span className="material-title grow">{i.title || kindLabel[i.kind]}</span>
                <span className={i.kind === "test_scores" ? "chip chip-ok" : i.kind === "session_note" ? "chip chip-sand" : "chip"}>{kindLabel[i.kind]}</span>
              </span>
              <span className="small muted">
                {feedText(routeOf(i.id), titleOf)}{routeOf(i.id)?.sorted && <span className="sorted-mark"> · מוין</span>} · {new Date(i.created_at * 1000).toLocaleDateString("he-IL", { day: "numeric", month: "numeric" })}
              </span>
            </button>
          ))}
          <button type="button" className={dragging ? "dropzone over" : "dropzone"} onClick={() => fileRef.current?.click()}>
            {reading ? <><Spinner /> קוראת את {reading}…</> : <><UploadIcon /> גוררים לכאן קובץ Word, ‏ODT או PDF, או לוחצים לבחירה</>}
          </button>
        </section>

        <section className="card material-view" aria-label="תצוגת החומר">
          {input ? (
            <>
              <div className="material-head">
                <h2 className="grow">{input.title || kindLabel[input.kind]}</h2>
                <span className="small muted">{kindLabel[input.kind]}</span>
                {preview && preview.hidden.length > 0 && <span className="chip chip-warn">{countHidden(preview)} פרטים יוסתרו</span>}
                <button type="button" className="btn btn-small" onClick={() => void edit(input)}>עריכה</button>
                <button type="button" className="btn btn-small" onClick={() => void remove(input)}>מחיקה</button>
              </div>
              {route && (
                <div className="material-feeds">
                  <span className="small grow">
                    <b>מזין בדוח: </b>
                    {route.feeds.length ? route.feeds.map(titleOf).join(" · ") : "אף סעיף"}
                    <span className="muted">
                      {route.sorted
                        ? ` (לפי המיון${route.by_ai ? " של Claude" : ""}: ${route.used_passages} מתוך ${route.passages} קטעים)`
                        : route.added.length || route.removed.length ? " (לפי הבחירה שלך)" : " (לפי סוג החומר)"}
                    </span>
                  </span>
                  <button type="button" className="btn btn-small" onClick={() => setChoosing(input)}>שינוי הסעיפים</button>
                </div>
              )}
              <div className="material-text serif">
                {preview ? <Segments segments={preview.original_segments} side="original" /> : input.content}
              </div>
              <div className="material-foot small muted">
                <span><mark className="mark-real">מסומן</mark> = יוחלף בתפקיד לפני כל שליחה ("הילד", "האם", "לפני שבועיים")</span>
              </div>
            </>
          ) : (
            <p className="muted material-empty">בוחרים חומר מהרשימה כדי לראות אותו, ומה יוסתר ממנו.</p>
          )}
        </section>
      </div>

      {importing && (
        <ImportDialog caseId={caseId} preview={importing} onClose={() => setImporting(null)}
          onSaved={async (id) => { setImporting(null); await reload(); setSelected(id); notify("המסמך נשמר בתיק."); }} />
      )}
      {scoring && (
        <ScoresDialog caseId={caseId} age={detail.meta.age} input={scoring.input} sheet={scoring.sheet}
          onClose={() => setScoring(null)}
          onSaved={async (id) => { setScoring(null); await reload(); setSelected(id); notify("הציונים נשמרו בתיק."); }} />
      )}
      {choosing && routeOf(choosing.id) && (
        <SectionsDialog api={api} input={choosing} route={routeOf(choosing.id) as MaterialRouting} titleOf={titleOf}
          onClose={() => setChoosing(null)}
          onSaved={async () => { setChoosing(null); await reload(); notify("הסעיפים של החומר עודכנו."); }} />
      )}
      {writing && (
        <WriteDialog caseId={caseId} kind={writing.kind} input={writing.input} onClose={() => setWriting(null)}
          onSaved={async (id) => { setWriting(null); await reload(); if (id) setSelected(id); }} />
      )}
    </div>
  );
}

function countHidden(p: FilterOutcome): number {
  return p.original_segments.filter((s) => s.mark === "replaced" || s.mark === "relative").length;
}

function feedText(route: MaterialRouting | undefined, titleOf: (key: string) => string): string {
  const titles = (route?.feeds ?? []).map(titleOf);
  if (!titles.length) return "לא מזין סעיף";
  return `מזין: ${titles.slice(0, 2).join(", ")}${titles.length > 2 ? ` ועוד ${titles.length - 2}` : ""}`;
}

/** Einat picks the sections a material feeds (D-022). Her choice always wins. */
function SectionsDialog(props: { api: CaseApi; input: CaseInput; route: MaterialRouting; titleOf: (key: string) => string; onClose: () => void; onSaved: () => Promise<void> }) {
  const { fail } = useApp();
  const { api, input, route } = props;
  const [chosen, setChosen] = useState<string[]>(route.feeds);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const sortable = api.detail.sections.filter((s) => s.sortable);
  const parts = Array.from(new Set(sortable.map((s) => s.part)));
  const origin = (key: string) =>
    route.sorted ? (route.suggested.includes(key) ? (route.by_ai ? "Claude הציע" : "הוצע במיון") : null) : route.table.includes(key) ? "לפי סוג החומר" : null;

  async function save() {
    setBusy(true);
    try {
      await ipc.setInputSections(api.caseId, input.id, chosen);
      await props.onSaved();
    } catch (e) {
      setError(fail(e as never));
      setBusy(false);
    }
  }

  return (
    <Dialog narrow title="לאילו סעיפים החומר הזה מזין?" subtitle={input.title || kindLabel[input.kind]} onClose={props.onClose}
      footer={<>
        <button type="button" className="btn" onClick={props.onClose}>ביטול</button>
        <button type="button" className="btn btn-primary" disabled={busy} onClick={() => void save()}>שמירה</button>
      </>}>
      <p className="muted small">
        {route.sorted
          ? "מסומנים הסעיפים שנמצאו במיון. לסעיף שתוסיפי יישלח כל החומר; לסעיף שתורידי לא יישלח ממנו דבר."
          : "מסומנים הסעיפים שהחומר מזין לפי סוגו. אפשר להוסיף ולהוריד; הבחירה שלך קובעת."}
      </p>
      <ErrorLine error={error} />
      <div className="sections-pick">
        {parts.map((part) => (
          <fieldset key={part} className="sections-part">
            <legend>{part}</legend>
            {sortable.filter((s) => s.part === part).map((s) => {
              const tag = origin(s.key);
              return (
                <label key={s.key} className="check-row">
                  <input type="checkbox" checked={chosen.includes(s.key)}
                    onChange={(e) => setChosen(e.target.checked ? [...chosen, s.key] : chosen.filter((k) => k !== s.key))} />
                  <span className="grow">{s.title}</span>
                  {tag && <span className="chip small">{tag}</span>}
                </label>
              );
            })}
          </fieldset>
        ))}
      </div>
    </Dialog>
  );
}

/** Typing a session note, pasting text, or editing a saved material. */
function WriteDialog(props: { caseId: string; kind: InputKind; input?: CaseInput | undefined; onClose: () => void; onSaved: (id: string | null) => Promise<void> }) {
  const { fail } = useApp();
  const [kind, setKind] = useState<InputKind>(props.input?.kind ?? props.kind);
  const [title, setTitle] = useState(props.input?.title ?? (props.kind === "session_note" ? `מפגש ${new Date().toLocaleDateString("he-IL")}` : ""));
  const [content, setContent] = useState(props.input?.content ?? "");
  const [error, setError] = useState<string | null>(null);

  async function save() {
    if (!content.trim()) return setError("אין טקסט לשמור.");
    try {
      if (props.input) {
        await ipc.updateInput(props.caseId, props.input.id, title.trim(), content);
        await props.onSaved(props.input.id);
      } else {
        const saved = await ipc.addInput(props.caseId, kind, title.trim(), content);
        await props.onSaved(saved.id);
      }
    } catch (e) {
      setError(fail(e as never));
    }
  }

  return (
    <Dialog title={props.input ? "עריכת חומר" : kind === "session_note" ? "רישום מפגש" : "הוספת טקסט"}
      subtitle="אפשר לכתוב עם שמות אמיתיים. הם נשמרים מוצפנים ומוסתרים לפני כל שליחה."
      onClose={props.onClose}
      footer={<><span className="grow" /><button type="button" className="btn" onClick={props.onClose}>ביטול</button><button type="button" className="btn btn-primary" onClick={() => void save()}>שמירה בתיק</button></>}>
      <div className="stack">
        {!props.input && (
          <div className="field">
            <label htmlFor="w-kind">סוג החומר</label>
            <select id="w-kind" className="select" value={kind} onChange={(e) => setKind(e.target.value as InputKind)}>
              {kindOrder.map((k) => <option key={k} value={k}>{kindLabel[k]}</option>)}
            </select>
          </div>
        )}
        <div className="field">
          <label htmlFor="w-title">כותרת</label>
          <input id="w-title" className="input" value={title} onChange={(e) => setTitle(e.target.value)} />
        </div>
        <div className="field">
          <label htmlFor="w-text">הטקסט</label>
          <textarea id="w-text" className="textarea serif write-area" rows={14} value={content} onChange={(e) => setContent(e.target.value)} autoFocus />
        </div>
        <ErrorLine error={error} />
      </div>
    </Dialog>
  );
}
