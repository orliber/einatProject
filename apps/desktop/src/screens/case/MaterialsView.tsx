import { useEffect, useRef, useState, type DragEvent } from "react";
import { useApp } from "../../App";
import { kindLabel, kindOrder } from "../../i18n/he";
import { ipc, type CaseInput, type ImportPreview, type InputKind } from "../../ipc/client";
import type { FilterOutcome } from "../../ipc/generated/FilterOutcome";
import { ImportDialog } from "../../components/ImportDialog";
import { Dialog, ErrorLine, Segments, Spinner } from "../../components/ui";
import type { CaseApi } from "../CaseScreen";
import "./MaterialsView.css";

const ACCEPT = ".docx,.pdf,.txt,application/pdf,application/vnd.openxmlformats-officedocument.wordprocessingml.document,text/plain";

export function MaterialsView({ api }: { api: CaseApi }) {
  const { fail, notify } = useApp();
  const { detail, caseId, reload } = api;
  const [selected, setSelected] = useState<string | null>(detail.inputs[0]?.id ?? null);
  const [previewed, setPreviewed] = useState<{ id: string; outcome: FilterOutcome } | null>(null);
  const [importing, setImporting] = useState<ImportPreview | null>(null);
  const [reading, setReading] = useState<string | null>(null);
  const [writing, setWriting] = useState<{ kind: InputKind; input?: CaseInput } | null>(null);
  const [dragging, setDragging] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const fileRef = useRef<HTMLInputElement>(null);

  const input = detail.inputs.find((i) => i.id === selected) ?? null;

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
          <button type="button" className="btn btn-primary" onClick={() => fileRef.current?.click()}>העלאת מסמך</button>
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
            </div>
          )}
          {sorted.map((i) => (
            <button key={i.id} type="button" className={i.id === selected ? "material selected" : "material"} onClick={() => setSelected(i.id)}>
              <span className="row">
                <span className="material-title grow">{i.title || kindLabel[i.kind]}</span>
                <span className={i.kind === "test_scores" ? "chip chip-ok" : i.kind === "session_note" ? "chip chip-sand" : "chip"}>{kindLabel[i.kind]}</span>
              </span>
              <span className="small muted">
                {feedText(api, i.kind)} · {new Date(i.created_at * 1000).toLocaleDateString("he-IL", { day: "numeric", month: "numeric" })}
              </span>
            </button>
          ))}
          <button type="button" className={dragging ? "dropzone over" : "dropzone"} onClick={() => fileRef.current?.click()}>
            {reading ? <><Spinner /> קוראת את {reading}…</> : "גוררים לכאן קובץ Word או PDF"}
          </button>
        </section>

        <section className="card material-view" aria-label="תצוגת החומר">
          {input ? (
            <>
              <div className="material-head">
                <h2 className="grow">{input.title || kindLabel[input.kind]}</h2>
                <span className="small muted">{kindLabel[input.kind]}</span>
                {preview && preview.hidden.length > 0 && <span className="chip chip-warn">{countHidden(preview)} פרטים יוסתרו</span>}
                <button type="button" className="btn btn-small" onClick={() => setWriting({ kind: input.kind, input })}>עריכה</button>
                <button type="button" className="btn btn-small" onClick={() => void remove(input)}>מחיקה</button>
              </div>
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

function feedText(api: CaseApi, kind: InputKind): string {
  const titles = api.detail.sections.filter((s) => sectionInputs(api, s.key).includes(kind)).map((s) => s.title);
  if (!titles.length) return "לא מזין סעיף";
  return `מזין: ${titles.slice(0, 2).join(", ")}${titles.length > 2 ? ` ועוד ${titles.length - 2}` : ""}`;
}

/** The kinds that feed each section (mirrors templates/report_structure.json). */
const SECTION_INPUTS: Record<string, InputKind[]> = {
  referral: ["intake", "free_text"],
  background: ["intake"],
  parents_view: ["intake"],
  kindergarten: ["kindergarten"],
  prior_assessments: ["prior_report", "professional"],
  tools: ["test_scores", "free_text"],
  appearance: ["observation", "session_note", "free_text"],
  cognitive: ["test_scores", "observation", "free_text"],
  adaptive: ["test_scores", "free_text"],
  communication: ["observation", "session_note", "prior_report", "test_scores", "free_text"],
  dsm: ["free_text"],
  emotional: ["observation", "session_note", "free_text"],
  diagnoses: ["free_text"],
  recommendations: ["free_text"],
};

function sectionInputs(_api: CaseApi, key: string): InputKind[] {
  return SECTION_INPUTS[key] ?? [];
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
