import { useEffect, useRef, useState, type DragEvent } from "react";
import { useApp } from "../../App";
import { kindLabel, kindOrder } from "../../i18n/he";
import { ipc, type CaseInput, type FollowUpView, type ImportPreview, type InputKind, type MaterialRouting, type PriorRecordView, type ScoreSheet, type Setting } from "../../ipc/client";
import type { FilterOutcome } from "../../ipc/generated/FilterOutcome";
import { ImportDialog } from "../../components/ImportDialog";
import { AddMenu } from "../../components/Menu";
import { ScoresView, scoresDraft } from "../../components/ScoresView";
import { useHoldUnsaved } from "../../components/Unsaved";
import { LocalText, type LocalTextHandle } from "../../components/LocalText";
import { Dialog, ErrorLine, Segments, UploadIcon, PlusIcon } from "../../components/ui";
import { sizeBucket, timed, WaitLine } from "../../components/Progress";
import type { CaseApi } from "../CaseScreen";
import "./MaterialsView.css";
import { useAi } from "../../ai";

const ACCEPT = ".docx,.doc,.odt,.rtf,.pdf,.txt,.jpg,.jpeg,.png,.tif,.tiff,.heic,application/vnd.oasis.opendocument.text,application/pdf,application/vnd.openxmlformats-officedocument.wordprocessingml.document,application/msword,application/rtf,text/plain,image/jpeg,image/png,image/tiff";

/** What became of one file of several chosen or dropped together (D-147). */
interface FileResult {
  name: string;
  text: string;
  /** Saved at once: the line offers "ביטול". */
  savedId?: string | undefined;
  failed?: boolean;
}

/** The materials of a case (round 49, D-088): what is still missing, one place to drop a file,
 *  and one list of everything, newest first. `onWrite` goes on to writing (sorting first). */
export function MaterialsView({ api, onWrite }: { api: CaseApi; onWrite?: () => void }) {
  const { fail, notify } = useApp();
  const ai = useAi();
  const { detail, caseId, reload } = api;
  // A material opens in a window when clicked; the screen itself is only the list.
  const [selected, setSelected] = useState<string | null>(null);
  const [previewed, setPreviewed] = useState<{ id: string; outcome: FilterOutcome } | null>(null);
  const [importing, setImporting] = useState<{ preview: ImportPreview; kind?: InputKind | undefined } | null>(null);
  // The slot a file is being chosen for (the "+" of that slot), so it lands in its place.
  const [uploadFor, setUploadFor] = useState<InputKind | undefined>(undefined);
  const [dropOn, setDropOn] = useState<string | null>(null);
  const [reading, setReading] = useState<{ name: string; size: string; at?: string } | null>(null);
  /** D-147: the files of one choice or drop, a line each, as they are read. */
  const [batch, setBatch] = useState<FileResult[] | null>(null);
  const batchRunning = useRef(false);
  /** Resolves when the import window of a file in a batch closes (true: it was saved). */
  const dialogDone = useRef<((saved: boolean) => void) | null>(null);
  const [writing, setWriting] = useState<{ kind: InputKind; input?: CaseInput } | null>(null);
  // Round 54 (D-094): the score table is a view of this stage, not a window. Scores left
  // unsaved on the way to another stage open again on the way back.
  const [scoring, setScoring] = useState<{ input?: CaseInput | undefined; sheet?: ScoreSheet } | null>(() => {
    const d = scoresDraft(caseId);
    return d ? { input: detail.inputs.find((i) => i.id === d.inputId) } : null;
  });
  const [choosing, setChoosing] = useState<CaseInput | null>(null);
  const [dragging, setDragging] = useState(false);
  const [error, setError] = useState<string | null>(null);
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
    setBatch(null);
    setReading({ name: file.name, size: sizeBucket(file.size) });
    try {
      const preview = await timed("read", ipc.importDocument(caseId, file), sizeBucket(file.size));
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

  /** D-147: several files, read one after another through the same road as one file: a file
   *  read in full is saved at once, one that was not opens the import window and the next waits
   *  for it, and one that could not be read says why. Each file gets its own line. */
  async function readFiles(files: File[], kind?: InputKind) {
    if (files.length === 1 && files[0]) return readFile(files[0], kind);
    if (files.length === 0 || batchRunning.current) return;
    batchRunning.current = true;
    setError(null);
    setAutoSaved(null);
    const results: FileResult[] = [];
    setBatch([]);
    try {
      for (const [i, file] of files.entries()) {
        setReading({ name: file.name, size: sizeBucket(file.size), at: `${i + 1} מתוך ${files.length}` });
        let result: FileResult;
        try {
          const preview = await timed("read", ipc.importDocument(caseId, file), sizeBucket(file.size));
          const clean = preview.body.trim() !== "" && preview.left_out.length === 0 && preview.warnings.length === 0;
          const title = preview.title.trim() || file.name;
          if (clean) {
            const saved = await ipc.addInput(caseId, kind ?? preview.suggested_kind, preview.title.trim(), preview.body);
            await reload();
            result = { name: file.name, text: `"${title}" נשמר בתיק.`, savedId: saved.id };
          } else {
            setReading(null);
            const kept = await new Promise<boolean>((resolve) => {
              dialogDone.current = resolve;
              setImporting({ preview, kind });
            });
            result = { name: file.name, text: kept ? `"${title}" נשמר בתיק אחרי הבדיקה בחלון.` : "לא נשמר: החלון נסגר בלי שמירה." };
          }
        } catch (e) {
          result = { name: file.name, text: `לא נקרא: ${fail(e as never)}`, failed: true };
        }
        results.push(result);
        setBatch([...results]);
      }
    } finally {
      setReading(null);
      batchRunning.current = false;
    }
  }

  /** The import window of a file in a batch closed: the next file goes on. */
  function importClosed(saved: boolean) {
    setImporting(null);
    const done = dialogDone.current;
    dialogDone.current = null;
    done?.(saved);
  }

  async function undoBatchItem(r: FileResult) {
    if (!r.savedId) return;
    const id = r.savedId;
    setBatch((b) => b?.map((x) => (x.savedId === id ? { ...x, text: "הייבוא בוטל.", savedId: undefined } : x)) ?? null);
    try {
      await ipc.deleteInput(caseId, id);
      await reload();
    } catch (e) {
      setError(fail(e as never));
    }
  }

  function onDrop(e: DragEvent, kind?: InputKind) {
    e.preventDefault();
    e.stopPropagation();
    setDragging(false);
    setDropOn(null);
    void readFiles(Array.from(e.dataTransfer.files), kind);
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
    setError(null);
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
    setError(null);
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
  // Round 49 (D-088): what the report can be written from, in one sentence.
  const sortable = detail.sections.filter((s) => s.sortable);
  const covered = sortable.filter((s) => s.source_count > 0).length;
  const unread = detail.routing.filter((r) => r.needs_sorting).length;
  const anySorted = detail.routing.some((r) => r.sorted);
  const has = (n: Need) => detail.inputs.some((i) => n.kinds.includes(i.kind));
  const addItems = (n: Need) => [
    { label: n.add === "upload" ? "העלאת קובץ מהמחשב (וורד, PDF)" : n.addLabel, run: () => (n.add === "upload" ? chooseFile(n.kinds[0]) : write(n)) },
    n.add === "upload" ? { label: "הדבקת טקסט", run: () => write(n) } : { label: "העלאת קובץ מהמחשב (וורד, PDF)", run: () => chooseFile(n.kinds[0]) },
  ];
  const dropProps = (key: string, kind?: InputKind) => ({
    onDragOver: (e: DragEvent) => { e.preventDefault(); e.stopPropagation(); setDropOn(key); },
    onDragLeave: () => setDropOn((d) => (d === key ? null : d)),
    onDrop: (e: DragEvent) => onDrop(e, kind),
  });

  if (scoring) {
    return (
      <ScoresView caseId={caseId} age={detail.meta.age} input={scoring.input} sheet={scoring.sheet}
        onClose={() => setScoring(null)}
        onSaved={async () => { setScoring(null); await reload(); notify("הציונים נשמרו בתיק."); }} />
    );
  }

  return (
    <div className={dragging ? "view materials-drop" : "view"} onDragOver={(e) => { e.preventDefault(); setDragging(true); }} onDragLeave={() => setDragging(false)} onDrop={onDrop}>
      <div className="view-head">
        <div className="stack" style={{ gap: 4 }}>
          <h1>מה יש בתיק</h1>
          <p className="muted small">
            {sorted.length === 0 ? "עוד אין חומרים. הכול נשמר מוצפן." : `יש חומר ל-${covered} מתוך ${sortable.length} סעיפים. הכול נשמר מוצפן.`}
          </p>
        </div>
        {sorted.length > 0 && onWrite && (
          <button type="button" className={detail.meta.consent ? "btn btn-primary btn-big" : "btn btn-big"} onClick={onWrite}>לכתיבת הדוח ←</button>
        )}
      </div>
      {/* With a material's window open, its failure is said in the window, not behind it. */}
      {!input && <ErrorLine error={error} />}
      {autoSaved && (
        <div className="note-sand row" role="status">
          <span className="grow">"{autoSaved.title}" נשמר בתיק.</span>
          <button type="button" className="btn btn-small" onClick={() => { setSelected(autoSaved.id); setAutoSaved(null); }}>לבדיקה</button>
          <button type="button" className="btn btn-small" onClick={() => void undoAutoSave()}>ביטול</button>
        </div>
      )}
      {batch && batch.length > 0 && (
        <div className="note-sand stack" role="status" aria-label="הקבצים שנבחרו">
          <ul className="stack" style={{ gap: 4, margin: 0, paddingInlineStart: 18 }}>
            {batch.map((r, i) => (
              <li key={`${i}-${r.name}`} className={r.failed ? "row warn-text" : "row"}>
                <span className="grow"><b>{r.name}</b>: {r.text}</span>
                {r.savedId && <button type="button" className="btn btn-small" onClick={() => void undoBatchItem(r)}>ביטול</button>}
              </li>
            ))}
          </ul>
          {!reading && !importing && (
            <button type="button" className="btn btn-small" onClick={() => setBatch(null)}>סגירה</button>
          )}
        </div>
      )}
      {detail.meta.follows && <FollowUpPanel api={api} />}
      <div className="view-body materials-body">
        <ul className="needs" aria-label="מה עוד חסר">
          {NEEDS.map((n) => {
            const on = has(n);
            return (
              <li key={n.label} className={`need ${on ? "have" : n.required ? "missing" : "optional"}${dropOn === n.label ? " drop" : ""}`} {...dropProps(n.label, n.kinds[0])}>
                {on ? (
                  <span className="need-name"><span className="need-mark" aria-hidden="true">✓</span><span className="visually-hidden">יש: </span>{n.label}</span>
                ) : (
                  <button type="button" className="need-name need-add" title={n.addLabel}
                    onClick={() => (n.add === "upload" ? chooseFile(n.kinds[0]) : write(n))}>
                    <span className="need-mark" aria-hidden="true">○</span>
                    <span>{n.label}{!n.required && <span className="small"> (לא חובה)</span>}<span className="visually-hidden"> ({n.addLabel})</span></span>
                  </button>
                )}
                <AddMenu className="icon-btn need-more" label={`הוספה ל${n.label}`} items={addItems(n)}><PlusIcon /></AddMenu>
              </li>
            );
          })}
        </ul>

        <div className={`dropzone wide${dropOn === "any" ? " over" : ""}`} {...dropProps("any")}>
          <UploadIcon />
          {reading ? (
            <div className="grow"><WaitLine task="read" speed={reading.size} delay={0} label={`קוראת את ${reading.name}${reading.at ? ` (${reading.at})` : ""} ומסתירה פרטים מזהים`} /></div>
          ) : (
            <span>גוררים לכאן Word, ‏PDF או ODT, או <button type="button" className="link-btn" onClick={() => chooseFile()}>בוחרים קובץ</button></span>
          )}
          <input ref={fileRef} type="file" accept={ACCEPT} multiple hidden aria-label="העלאת מסמך" onChange={(e) => { const fs = Array.from(e.target.files ?? []); e.target.value = ""; const k = uploadFor; setUploadFor(undefined); void readFiles(fs, k); }} />
        </div>

        <section className="materials-list" aria-label="רשימת החומרים">
          <div className="materials-list-head small">
            <b>{sorted.length === 1 ? "חומר אחד" : `${sorted.length} חומרים`}</b>
            <span className="muted">
              {unread > 0
                ? `${unread === 1 ? "חומר אחד עוד לא נקרא" : `${unread} חומרים עוד לא נקראו`}. הסיכום יופיע אחרי המיון.`
                : anySorted ? "מוינו לסעיפים לבד, אחרי הסתרה" : ""}
            </span>
          </div>
          {sorted.length === 0 && <p className="materials-empty muted">מתחילים מאינטייק עם ההורים, או גוררים מסמך.</p>}
          {sorted.map((i) => {
            const r = routeOf(i.id);
            return (
              <button key={i.id} type="button" className={i.id === selected ? "material selected" : "material"} onClick={() => setSelected(i.id)}>
                <span className="material-kind">{KIND_TAG[i.kind]}</span>
                <span className="material-main">
                  <span className="material-title">{i.title || kindLabel[i.kind]}</span>
                  {r?.summary && <span className="material-summary">{firstSentence(r.summary)}</span>}
                  <span className="small muted">{feedText(r, titleOf)}</span>
                </span>
                <span className="small muted material-date">{new Date(i.created_at * 1000).toLocaleDateString("he-IL", { day: "numeric", month: "numeric" })}</span>
              </button>
            );
          })}
        </section>
        {sorted.length > 0 && <PriorOrder api={api} titleOf={(id) => detail.inputs.find((x) => x.id === id)?.title ?? ""} open={(id) => setSelected(id)} />}
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
              <ErrorLine error={error} />
              {route?.left_out && (
                <p className="material-feeds small warn-text" role="note">{route.left_out}</p>
              )}
              {route && !route.left_out && (
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
        <ImportDialog caseId={caseId} preview={importing.preview} kind={importing.kind} onClose={() => importClosed(false)}
          onSaved={async () => { importClosed(true); await reload(); notify("המסמך נשמר בתיק."); }} />
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

/** The previous assessments in the order the report lists them (fixed in code, round 31). */
function PriorOrder({ api, titleOf, open }: { api: CaseApi; titleOf: (id: string) => string; open: (id: string) => void }) {
  const { detail } = api;
  const records = detail.prior_order.flatMap((id) => {
    const record = detail.routing.find((r) => r.input_id === id)?.record;
    return record ? [{ id, record }] : [];
  });
  if (!records.length) return null;
  return (
    <section className="prior-order stack" aria-label="אבחונים קודמים, בסדר הדוח">
      <b className="small">אבחונים וטיפולים קודמים, בסדר שבו הם נכתבים בדוח</b>
      <ol className="prior-list">
        {records.map(({ id, record }) => (
          <li key={id}>
            <button type="button" className="link-btn" onClick={() => open(id)}>{titleOf(id) || record.kind}</button>
            <RecordLine record={record} />
          </li>
        ))}
      </ol>
    </section>
  );
}

function feedText(route: MaterialRouting | undefined, titleOf: (key: string) => string): string {
  // A score table of an instrument she does not give (D-098) goes nowhere.
  if (route?.left_out) return "לא נשלח ולא נכנס לדוח";
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
  { label: "דיווח מהמסגרת", kinds: ["kindergarten"], feeds: "סעיף המסגרת החינוכית", required: true, add: "kindergarten", addLabel: "רישום שיחה עם הגננת" },
  { label: "תצפית ומפגשים", kinds: ["session_note", "observation"], feeds: "הופעה והתרשמות, משחק ומישור רגשי", required: true, add: "session_note", addLabel: "רישום מפגש" },
  { label: "ציוני מבחנים", kinds: ["test_scores"], feeds: "כלי האבחון והפרופיל הקוגניטיבי", required: true, add: "test_scores", addLabel: "הזנת ציונים" },
  { label: "אבחונים קודמים", kinds: ["prior_report", "professional"], feeds: "אבחונים וטיפולים (אם יש)", required: false, add: "upload", addLabel: "העלאת מסמך" },
];

/** The short tag of each kind in the list. */
const KIND_TAG: Record<InputKind, string> = {
  intake: "אינטייק", kindergarten: "מסגרת", session_note: "מפגש", observation: "תצפית", test_scores: "ציונים",
  prior_report: "דוח קודם", professional: "איש מקצוע", free_text: "הערה",
};

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
      <div className="checklist-head">
        <button type="button" className="checklist-toggle" aria-expanded={open} onClick={() => setOpen(!open)}>
          <b className="grow">
            אבחון מעקב{view.previous_gone ? " · האבחון הקודם נמחק" : ` · האבחון הקודם: ${view.previous_code}`}
            {view.rows.length > 0 && ` · ${view.rows.length} מדדים להשוואה`}
          </b>
          <span aria-hidden="true">{open ? "▴" : "▾"}</span>
        </button>
        {!view.previous_gone && (
          <button type="button" className="link-btn small" onClick={() => go({ name: "case", id: view.previous_id, view: "report" })}>
            לדוח הקודם
          </button>
        )}
      </div>
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
