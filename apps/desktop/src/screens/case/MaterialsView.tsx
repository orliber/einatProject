import { useEffect, useRef, useState, type DragEvent } from "react";
import { useApp } from "../../App";
import { kindLabel, kindOrder } from "../../i18n/he";
import { ipc, type CaseInput, type FollowUpView, type ImportPreview, type InputKind, type MaterialRouting, type PriorRecordView, type ScoreSheet, type Setting } from "../../ipc/client";
import type { FilterOutcome } from "../../ipc/generated/FilterOutcome";
import { ImportDialog } from "../../components/ImportDialog";
import { AddMenu } from "../../components/Menu";
import { ScoresDialog } from "../../components/ScoresDialog";
import { useHoldUnsaved } from "../../components/Unsaved";
import { LocalText, type LocalTextHandle } from "../../components/LocalText";
import { Dialog, ErrorLine, Segments, Spinner, UploadIcon, PlusIcon, EditIcon } from "../../components/ui";
import type { CaseApi } from "../CaseScreen";
import "./MaterialsView.css";
import { useAi } from "../../ai";

const ACCEPT = ".docx,.doc,.odt,.rtf,.pdf,.txt,.jpg,.jpeg,.png,.tif,.tiff,.heic,application/vnd.oasis.opendocument.text,application/pdf,application/vnd.openxmlformats-officedocument.wordprocessingml.document,application/msword,application/rtf,text/plain,image/jpeg,image/png,image/tiff";

/** Materials shown in a slot before "עוד" (R5). */
const SLOT_SHOWN = 8;

export function MaterialsView({ api }: { api: CaseApi }) {
  const { fail, notify } = useApp();
  const ai = useAi();
  const { detail, caseId, reload } = api;
  // A material opens in a window when clicked; the screen itself is only the slots.
  const [selected, setSelected] = useState<string | null>(null);
  const [previewed, setPreviewed] = useState<{ id: string; outcome: FilterOutcome } | null>(null);
  const [importing, setImporting] = useState<{ preview: ImportPreview; kind?: InputKind | undefined } | null>(null);
  // The slot a file is being chosen for (the "+" of that slot), so it lands in its place.
  const [uploadFor, setUploadFor] = useState<InputKind | undefined>(undefined);
  const [dropOn, setDropOn] = useState<string | null>(null);
  const [reading, setReading] = useState<string | null>(null);
  const [writing, setWriting] = useState<{ kind: InputKind; input?: CaseInput } | null>(null);
  const [scoring, setScoring] = useState<{ input?: CaseInput; sheet?: ScoreSheet } | null>(null);
  const [choosing, setChoosing] = useState<CaseInput | null>(null);
  const [dragging, setDragging] = useState(false);
  const [error, setError] = useState<string | null>(null);
  // R5: a slot with many materials shows the newest first, the rest on request.
  const [opened, setOpened] = useState<Set<string>>(() => new Set());
  /** A clean import saved without the review window, with a way back (F7). */
  const [autoSaved, setAutoSaved] = useState<{ id: string; title: string } | null>(null);
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

  async function readFile(file: File, kind?: InputKind) {
    setError(null);
    setReading(file.name);
    try {
      const preview = await ipc.importDocument(caseId, file);
      // Everything read: saved at once (encrypted, nothing leaves the computer) with "ביטול"
      // beside it. What the filter will hide is not a question (round 26); only a file that
      // was not read in full opens the window, to say what was left out.
      const clean = preview.body.trim() !== "" && preview.left_out.length === 0 && preview.warnings.length === 0;
      if (clean) {
        const saved = await ipc.addInput(caseId, kind ?? preview.suggested_kind, preview.title.trim(), preview.body);
        setAutoSaved({ id: saved.id, title: preview.title.trim() || file.name });
        await reload();
      } else {
        setAutoSaved(null);
        setImporting({ preview, kind });
      }
    } catch (e) {
      setError(fail(e as never));
    } finally {
      setReading(null);
    }
  }

  function onDrop(e: DragEvent, kind?: InputKind) {
    e.preventDefault();
    e.stopPropagation();
    setDragging(false);
    setDropOn(null);
    const file = e.dataTransfer.files[0];
    if (file) void readFile(file, kind);
  }

  /** Choose a file for one slot (or for none: the kind is then suggested from the file). */
  function chooseFile(kind?: InputKind) {
    setUploadFor(kind);
    fileRef.current?.click();
  }

  function write(g: Need) {
    if (g.add === "test_scores") setScoring({});
    else if (g.add === "upload") setWriting({ kind: g.kinds[0] ?? "free_text" });
    else if (g.add) setWriting({ kind: g.add });
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
      notify("החומר נמחק מהתיק.", { label: "ביטול", run: () => void restore(i) });
    } catch (e) {
      setError(fail(e as never));
    }
  }

  async function restore(i: CaseInput) {
    try {
      await ipc.restoreInput(caseId, i.id);
      await reload();
      notify("החומר הוחזר לתיק.");
    } catch (e) {
      setError(fail(e as never));
    }
  }

  async function undoAutoSave() {
    if (!autoSaved) return;
    const { id } = autoSaved;
    setAutoSaved(null);
    try {
      await ipc.deleteInput(caseId, id);
      await reload();
      notify("הייבוא בוטל.");
    } catch (e) {
      setError(fail(e as never));
    }
  }

  const sorted = [...detail.inputs].sort((a, b) => b.created_at - a.created_at);
  // Materials in their places: what an assessment rests on, and what is still missing.
  const groups = [...NEEDS, OTHER].map((n) => ({ ...n, items: sorted.filter((i) => n.kinds.includes(i.kind)) }))
    .filter((g) => g !== undefined && (g.kinds !== OTHER.kinds || g.items.length > 0));

  return (
    <div className={dragging ? "view materials-drop" : "view"} onDragOver={(e) => { e.preventDefault(); setDragging(true); }} onDragLeave={() => setDragging(false)} onDrop={onDrop}>
      <div className="view-head">
        <div className="stack" style={{ gap: 4 }}>
          <h1>מה יש בתיק</h1>
          <p className="muted small">גוררים לכאן Word, ‏PDF או ODT. הכל נשמר מוצפן.</p>
        </div>
        <div className="row">
          <button type="button" className="btn btn-primary btn-big" disabled={reading !== null} onClick={() => chooseFile()}>
            {reading ? <><Spinner /> קוראת את {reading}…</> : <><UploadIcon /> העלאת מסמך</>}
          </button>
          <input ref={fileRef} type="file" accept={ACCEPT} hidden onChange={(e) => { const f = e.target.files?.[0]; e.target.value = ""; const k = uploadFor; setUploadFor(undefined); if (f) void readFile(f, k); }} />
        </div>
      </div>
      <ErrorLine error={error} />
      {autoSaved && (
        <div className="note-sand row" role="status">
          <span className="grow">"{autoSaved.title}" נשמר בתיק.</span>
          <button type="button" className="btn btn-small" onClick={() => { setSelected(autoSaved.id); setAutoSaved(null); }}>לבדיקה</button>
          <button type="button" className="btn btn-small" onClick={() => void undoAutoSave()}>ביטול</button>
        </div>
      )}
      {detail.meta.follows && <FollowUpPanel api={api} />}
      {detail.inputs.length > 0 && <MaterialsReading api={api} titleOf={(id) => detail.inputs.find((x) => x.id === id)?.title ?? ""} open={(id) => setSelected(id)} />}
      <div className="view-body">
        <section className="materials-list" aria-label="רשימת החומרים">
          {groups.map((g) => (
            <div key={g.label} className={`slot ${g.items.length ? "have" : g.required ? "missing" : "optional"}${dropOn === g.label ? " drop" : ""}`}
              onDragOver={(e) => { e.preventDefault(); e.stopPropagation(); setDropOn(g.label); }}
              onDragLeave={() => setDropOn((d) => (d === g.label ? null : d))}
              onDrop={(e) => onDrop(e, g.kinds[0])}>
              <div className="slot-head">
                <span className="check-mark" aria-hidden="true">{g.items.length ? "✓" : g.required ? "!" : "+"}</span>
                <span className="grow stack" style={{ gap: 0 }}>
                  <b>{g.label}{g.items.length > 1 && ` (${g.items.length})`}</b>
                  <span className="small muted">{dropOn === g.label ? "לשחרר כאן" : g.items.length ? "" : g.required ? `חסר · נחוץ ל: ${g.feeds}` : `לא חובה · ${g.feeds}`}</span>
                </span>
                {g.items.length ? (
                  <AddMenu className="icon-btn slot-add" label={`הוספה ל${g.label}`} items={[
                    { label: g.add === "upload" ? "העלאת קובץ מהמחשב (וורד, PDF)" : g.addLabel, run: () => (g.add === "upload" ? chooseFile(g.kinds[0]) : write(g)) },
                    g.add === "upload" ? { label: "הדבקת טקסט", run: () => write(g) } : { label: "העלאת קובץ מהמחשב (וורד, PDF)", run: () => chooseFile(g.kinds[0]) },
                  ]}><PlusIcon /></AddMenu>
                ) : (
                  <span className="slot-actions">
                    <button type="button" className="btn btn-small" aria-label={`${g.addLabel} (${g.label})`}
                      onClick={() => (g.add === "upload" ? chooseFile(g.kinds[0]) : write(g))}>{g.addLabel}</button>
                    <button type="button" className="icon-btn slot-upload" aria-label={g.add === "upload" ? `הדבקת טקסט (${g.label})` : `העלאת קובץ (${g.label})`}
                      title={g.add === "upload" ? "הדבקת טקסט" : "העלאת קובץ Word, ‏PDF או ODT"}
                      onClick={() => (g.add === "upload" ? write(g) : chooseFile(g.kinds[0]))}>
                      {g.add === "upload" ? <EditIcon /> : <UploadIcon />}
                    </button>
                  </span>
                )}
              </div>
              {(opened.has(g.label) ? g.items : g.items.slice(0, SLOT_SHOWN)).map((i) => (
                <button key={i.id} type="button" className={i.id === selected ? "material selected" : "material"} onClick={() => setSelected(i.id)}>
                  <span className="material-title">{i.title || kindLabel[i.kind]}</span>
                  <span className="small muted">
                    {feedText(routeOf(i.id), titleOf)}{routeOf(i.id)?.sorted && <span className="sorted-mark"> · מוין</span>} · {new Date(i.created_at * 1000).toLocaleDateString("he-IL", { day: "numeric", month: "numeric" })}
                  </span>
                  {routeOf(i.id)?.summary && <span className="material-summary small">{firstSentence(routeOf(i.id)?.summary ?? "")}</span>}
                </button>
              ))}
              {g.items.length > SLOT_SHOWN && !opened.has(g.label) && (
                <button type="button" className="link-btn slot-more" onClick={() => setOpened((o) => new Set(o).add(g.label))}>
                  עוד {g.items.length - SLOT_SHOWN} ב{g.label}
                </button>
              )}
            </div>
          ))}
        </section>

      </div>

      {input && (
        <Dialog title={input.title || kindLabel[input.kind]} subtitle={kindLabel[input.kind]} onClose={() => setSelected(null)}>
          <div className="material-view">
<>
              <div className="material-head">
                <h2 className="grow">{input.title || kindLabel[input.kind]}</h2>
                <span className="small muted">{kindLabel[input.kind]}</span>
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
                        ? ` (לפי המיון${route.by_ai ? ` של ${ai}` : ""}: ${route.used_passages} מתוך ${route.passages} קטעים)`
                        : route.added.length || route.removed.length ? " (לפי הבחירה שלך)" : " (לפי סוג החומר)"}
                    </span>
                  </span>
                  <button type="button" className="btn btn-small" onClick={() => setChoosing(input)}>שינוי הסעיפים</button>
                </div>
              )}
              {route?.summary && <ReadingView route={route} ai={ai} />}
              <div className="material-text serif">
                {preview ? <Segments segments={preview.original_segments} side="original" /> : input.content}
              </div>
              <div className="material-foot small muted">
                <span><mark className="mark-real">מסומן</mark> = יוחלף לפני שליחה ("הילד", "לפני שבועיים")</span>
              </div>
            </>
          </div>
        </Dialog>
      )}
      {importing && (
        <ImportDialog caseId={caseId} preview={importing.preview} kind={importing.kind} onClose={() => setImporting(null)}
          onSaved={async () => { setImporting(null); await reload(); notify("המסמך נשמר בתיק."); }} />
      )}
      {scoring && (
        <ScoresDialog caseId={caseId} age={detail.meta.age} input={scoring.input} sheet={scoring.sheet}
          onClose={() => setScoring(null)}
          onSaved={async () => { setScoring(null); await reload(); notify("הציונים נשמרו בתיק."); }} />
      )}
      {choosing && routeOf(choosing.id) && (
        <SectionsDialog api={api} input={choosing} route={routeOf(choosing.id) as MaterialRouting} titleOf={titleOf}
          onClose={() => setChoosing(null)}
          onSaved={async () => { setChoosing(null); await reload(); notify("הסעיפים של החומר עודכנו."); }} />
      )}
      {writing && (
        <WriteDialog caseId={caseId} kind={writing.kind} input={writing.input} onClose={() => setWriting(null)}
          onSaved={async () => { setWriting(null); await reload(); notify("נשמר בתיק."); }} />
      )}
    </div>
  );
}

/** The first sentence of a summary, for the row. */
/** The summary's first sentence; with no sentence mark, its first line, cut at about 160
 *  letters at a space (the review of stage 3). */
function firstSentence(text: string): string {
  const m = /^.*?[.!?](?=\s|$)/s.exec(text.trim());
  const first = (m ? m[0] : text.trim().split("\n")[0] ?? "").trim();
  if (first.length <= 160) return first;
  const cut = first.lastIndexOf(" ", 160);
  return `${first.slice(0, cut > 80 ? cut : 160)}…`;
}

const SETTING: Record<Setting, string> = { institute: "במכון", community: "בקהילה", unknown: "לא צוין איפה" };

/** One previous assessment, as the reading recorded it. Plain text only. */
function RecordLine({ record }: { record: PriorRecordView }) {
  const head = [record.date, record.kind, record.by && `על ידי ${record.by}`, SETTING[record.setting]].filter(Boolean).join(" · ");
  return (
    <div className="stack prior-record">
      <b className="small">{head}</b>
      {record.findings && <span className="small"><b>ממצאים: </b>{record.findings}</span>}
      {record.recommendations && <span className="small"><b>המלצות: </b>{record.recommendations}</span>}
    </div>
  );
}

/** What the reading said about one material (round 31). */
function ReadingView({ route, ai }: { route: MaterialRouting; ai: string }) {
  return (
    <section className="material-reading stack" aria-label="מה כתוב בחומר">
      <b className="small">מה כתוב בו, בקיצור ({ai} קרא, אחרי הסתרה)</b>
      <p className="serif">{route.summary}</p>
      {route.record && <RecordLine record={route.record} />}
      {route.summary_notes.length > 0 && (
        <ul className="small reading-notes">{route.summary_notes.map((n, i) => <li key={i}>{n}</li>)}</ul>
      )}
    </section>
  );
}

/** "סיכום החומרים": what no material feeds yet, and the previous assessments in the order
 *  the report lists them (fixed in code, round 31). */
function MaterialsReading({ api, titleOf, open }: { api: CaseApi; titleOf: (id: string) => string; open: (id: string) => void }) {
  const { detail } = api;
  const records = detail.prior_order.flatMap((id) => {
    const record = detail.routing.find((r) => r.input_id === id)?.record;
    return record ? [{ id, record }] : [];
  });
  const unread = detail.routing.filter((r) => r.needs_sorting).length;
  return (
    <section className="materials-reading stack" aria-label="סיכום החומרים">
      <b>סיכום החומרים</b>
      {detail.missing.length > 0 ? (
        <p className="small">אין עדיין חומר ל: {detail.missing.join(" · ")}</p>
      ) : (
        <p className="small muted">לכל סעיף שנכתב מהחומרים יש חומר.</p>
      )}
      {unread > 0 && <p className="small muted">{unread === 1 ? "חומר אחד עוד לא נקרא." : `${unread} חומרים עוד לא נקראו.`} הסיכום שלהם יופיע אחרי המיון.</p>}
      {records.length > 0 && (
        <div className="stack">
          <span className="small muted">אבחונים וטיפולים קודמים, בסדר שבו הם נכתבים בדוח:</span>
          <ol className="prior-list">
            {records.map(({ id, record }) => (
              <li key={id}>
                <button type="button" className="link-btn" onClick={() => open(id)}>{titleOf(id) || record.kind}</button>
                <RecordLine record={record} />
              </li>
            ))}
          </ol>
        </div>
      )}
    </section>
  );
}

function feedText(route: MaterialRouting | undefined, titleOf: (key: string) => string): string {
  const titles = (route?.feeds ?? []).map(titleOf);
  if (!titles.length) return "לא מזין סעיף";
  return `מזין: ${titles.slice(0, 2).join(", ")}${titles.length > 2 ? ` ועוד ${titles.length - 2}` : ""}`;
}

/** Einat picks the sections a material feeds (D-022). Her choice always wins. */
function SectionsDialog(props: { api: CaseApi; input: CaseInput; route: MaterialRouting; titleOf: (key: string) => string; onClose: () => void; onSaved: () => Promise<void> }) {
  const { fail } = useApp();
  const ai = useAi();
  const { api, input, route } = props;
  const [chosen, setChosen] = useState<string[]>(route.feeds);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const sortable = api.detail.sections.filter((s) => s.sortable);
  const parts = Array.from(new Set(sortable.map((s) => s.part)));
  const origin = (key: string) =>
    route.sorted ? (route.suggested.includes(key) ? (route.by_ai ? `${ai} הציע` : "הוצע במיון") : null) : route.table.includes(key) ? "לפי סוג החומר" : null;

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
          ? "מסומנים הסעיפים מהמיון. הבחירה שלך קובעת."
          : "מסומנים לפי סוג החומר. הבחירה שלך קובעת."}
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
  // The text being typed lives in its box (F1a): `content` is its debounced copy (for the lock),
  // and `changed` / `empty` follow the box only when they flip (for "closing would lose it").
  const [content, setContent] = useState(props.input?.content ?? "");
  const [changed, setChanged] = useState(false);
  const [empty, setEmpty] = useState(() => !(props.input?.content ?? "").trim());
  const box = useRef<LocalTextHandle>(null);
  const [error, setError] = useState<string | null>(null);
  const [saving, setSaving] = useState(false);
  // A long session note must survive Escape and the automatic lock.
  const holdLive = useHoldUnsaved(props.caseId, title.trim() || kindLabel[kind], changed && !empty ? content : null, "materials");

  async function save() {
    // What is in the box now, not the debounced copy.
    const text = box.current?.value() ?? content;
    if (!text.trim()) return setError("אין טקסט לשמור.");
    setSaving(true);
    try {
      if (props.input) {
        await ipc.updateInput(props.caseId, props.input.id, title.trim(), text);
        await props.onSaved(props.input.id);
      } else {
        const saved = await ipc.addInput(props.caseId, kind, title.trim(), text);
        await props.onSaved(saved.id);
      }
    } catch (e) {
      setError(fail(e as never));
      setSaving(false);
    }
  }

  return (
    <Dialog title={props.input ? "עריכת חומר" : kind === "session_note" ? "רישום מפגש" : "הוספת טקסט"}
      subtitle="אפשר לכתוב עם שמות. הם מוסתרים לפני כל שליחה."
      onClose={props.onClose} dirty={!saving && changed && !empty}
      footer={<><span className="grow" /><button type="button" className="btn" onClick={props.onClose}>ביטול</button><button type="button" className="btn btn-primary" disabled={saving} onClick={() => void save()}>שמירה בתיק</button></>}>
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
          <LocalText id="w-text" ref={box} className="textarea serif write-area" rows={14} initial={props.input?.content ?? ""} autoFocus
            onText={setContent} onLive={holdLive} onEditedChange={setChanged} onEmptyChange={setEmpty} />
        </div>
        <ErrorLine error={error} />
      </div>
    </Dialog>
  );
}

type Need = {
  label: string;
  kinds: InputKind[];
  /** What it gives the report, in her words. */
  feeds: string;
  required: boolean;
  add: InputKind | "upload" | null;
  addLabel: string;
};

/** What an assessment usually rests on (docs/REPORT_STRUCTURE.md), deterministic. */
const NEEDS: Need[] = [
  { label: "אינטייק עם ההורים", kinds: ["intake"], feeds: "רקע, התפתחות ותיאור ההורים", required: true, add: "intake", addLabel: "רישום אינטייק" },
  { label: "דיווח מהמסגרת החינוכית", kinds: ["kindergarten"], feeds: "סעיף המסגרת החינוכית", required: true, add: "kindergarten", addLabel: "רישום שיחה עם הגננת" },
  { label: "ציוני מבחנים", kinds: ["test_scores"], feeds: "כלי האבחון והפרופיל הקוגניטיבי", required: true, add: "test_scores", addLabel: "הזנת ציונים" },
  { label: "המפגשים והתצפית שלך", kinds: ["session_note", "observation"], feeds: "הופעה והתרשמות, משחק ומישור רגשי", required: true, add: "session_note", addLabel: "רישום מפגש" },
  { label: "דוחות ואבחונים קודמים", kinds: ["prior_report", "professional"], feeds: "אבחונים וטיפולים (אם יש)", required: false, add: "upload", addLabel: "העלאת מסמך" },
];

/** Anything else (notes, pasted text). */
const OTHER: Need = { label: "הערות וטקסט חופשי", kinds: ["free_text"], feeds: "לפי המיון", required: false, add: "free_text", addLabel: "הדבקת טקסט" };

/** D-029: the scores of this follow-up next to the assessment before, by fixed rules. */
function FollowUpPanel({ api }: { api: CaseApi }) {
  const { fail, notify, go } = useApp();
  const { caseId, detail, reload } = api;
  const [view, setView] = useState<FollowUpView | null>(null);
  const [open, setOpen] = useState(true);
  const [error, setError] = useState<string | null>(null);
  const scoreCount = detail.inputs.filter((i) => i.kind === "test_scores").length;

  useEffect(() => {
    let alive = true;
    ipc.followUp(caseId).then((v) => alive && setView(v)).catch((e: unknown) => alive && setError(fail(e as never)));
    return () => {
      alive = false;
    };
  }, [caseId, scoreCount, fail]);

  if (!view) return <ErrorLine error={error} />;
  const num = (v: number) => (Number.isInteger(v) ? String(v) : v.toFixed(1));
  return (
    <section className="followup" aria-label="השוואה לאבחון הקודם">
      <button type="button" className="checklist-head" aria-expanded={open} onClick={() => setOpen(!open)}>
        <b className="grow">
          אבחון מעקב{view.previous_gone ? " · האבחון הקודם נמחק" : ` · האבחון הקודם: ${view.previous_code}`}
          {view.rows.length > 0 && ` · ${view.rows.length} מדדים להשוואה`}
        </b>
        {!view.previous_gone && (
          <span role="link" tabIndex={0} className="link-small" onClick={(e) => { e.stopPropagation(); go({ name: "case", id: view.previous_id, view: "report" }); }}
            onKeyDown={(e) => { if (e.key === "Enter") go({ name: "case", id: view.previous_id, view: "report" }); }}>
            לדוח הקודם
          </span>
        )}
        <span aria-hidden="true">{open ? "▴" : "▾"}</span>
      </button>
      {open && (
        <div className="followup-body">
          {view.rows.length === 0 ? (
            <p className="muted small">
              {view.previous_gone ? "אין עם מה להשוות." : "ההשוואה תופיע כשיוזנו ציונים במבחן שנמדד גם קודם (למשל FSIQ)."}
            </p>
          ) : (
            <>
              <table className="compare-table">
                <thead>
                  <tr><th>מדד</th><th>קודם</th><th>עכשיו</th><th>שינוי</th><th>טווח</th></tr>
                </thead>
                <tbody>
                  {view.rows.map((r) => (
                    <tr key={r.abbr + r.measure} className={r.notable ? "notable" : undefined}>
                      <td>{r.measure} <span className="muted">({r.abbr})</span></td>
                      <td className="num">{num(r.before)}</td>
                      <td className="num">{num(r.after)}</td>
                      <td className="num"><b dir="ltr">{r.change > 0 ? "+" : ""}{num(r.change)}</b>{r.notable && <span className="small"> · ששווה לבדוק</span>}</td>
                      <td className="small">{r.before_band} ← {r.after_band}</td>
                    </tr>
                  ))}
                </tbody>
              </table>
              <p className="small muted">
                הפרש של פחות מ-10 בציון תקן (3 במותאם) הוא לרוב טעות מדידה.
                {view.rows.some((r) => r.before_instrument !== r.after_instrument) && ` המבחנים שונים (${view.rows[0]?.before_instrument} ← ${view.rows[0]?.after_instrument}), ולכן ההשוואה זהירה.`}
              </p>
              <div className="row">
                <button type="button" className="btn btn-small"
                  onClick={async () => {
                    setError(null);
                    try {
                      await ipc.addComparisonMaterial(caseId);
                      await reload();
                      setView(await ipc.followUp(caseId));
                      notify("ההשוואה נוספה לחומרי התיק.");
                    } catch (e) {
                      setError(fail(e as never));
                    }
                  }}>
                  {view.in_materials ? "עדכון ההשוואה בחומרי התיק" : "הוספת ההשוואה לחומרי התיק"}
                </button>
              </div>
            </>
          )}
          <ErrorLine error={error} />
        </div>
      )}
    </section>
  );
}
