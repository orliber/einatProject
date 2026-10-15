// "הסגנון שלי" (D-043, round 46 / D-089): the program learns how Einat writes from her past
// reports. The page is three numbers, one queue that asks her about one thing at a time, what
// the program already knows (five rows that open in place) and one line about her reports.
// Every request goes through the same review screen as everything else that leaves the computer.
import { useMemo, useRef, useState, type ReactNode } from "react";
import { useQuery, useQueryClient } from "@tanstack/react-query";
import { useApp } from "../App";
import { ActionsMenu, type MenuItem } from "../components/Menu";
import { estimateMs, ProgressLine, recordDuration } from "../components/Progress";
import { ReviewDialog } from "../components/ReviewDialog";
import { StyleImportDialog } from "../components/StyleImportDialog";
import { Switch } from "../components/Switch";
import { TopBar } from "../components/TopBar";
import { ErrorLine, Skeleton, Spinner, UploadIcon, CloseIcon } from "../components/ui";
import {
  ipc,
  type FitMonth,
  type InterpretationView,
  type Prepared,
  type StyleImportPreview,
  type StyleItem,
  type StyleKind,
  type StyleLearned,
  type StyleOverview,
  type StyleProfile,
  type StyleProfileView,
  type StyleSourceView,
  type StyleSuggestion,
} from "../ipc/client";
import { ConfirmDialog } from "./library/LibraryDialogs";
import "./StyleScreen.css";
import { useAi } from "../ai";
import { notSentMessage } from "../sending";

const KIND_LABEL: Record<StyleKind, string> = {
  rule: "כללי כתיבה",
  phrase: "ביטויים אופייניים",
  avoid: "מה את לא כותבת",
  template: "תבניות משפט",
  example: "דוגמאות בדויות",
  argument: "מבנה הטיעון",
  integration: "חיבור בין כלים וסעיפים",
  criterion: "ניסוח קריטריונים (DSM-5)",
};
const KIND_HINT: Record<StyleKind, string> = {
  rule: "",
  phrase: "עד 5 מילים.",
  avoid: "",
  template: "במקום התוכן באים סוגריים מסולסלים, למשל {תחום}.",
  example: "פסקאות על ילד בדוי. הן מראות ל-AI את הסגנון, ואין בהן שום דבר מהדוחות.",
  argument: "השלבים של פסקה, לפי הסדר.",
  integration: "",
  criterion: "איך את מנסחת קריטריון, ואיזה סוג ממצא את קושרת אליו.",
};
/** The tag of one item in the queue: what kind of thing she is asked about, in one or two words. */
const KIND_TAG: Record<StyleKind, string> = {
  rule: "כלל כתיבה",
  phrase: "ביטוי שחוזר אצלך",
  avoid: "משהו שאת לא כותבת",
  template: "תבנית משפט",
  example: "דוגמה בדויה",
  argument: "מבנה הטיעון",
  integration: "חיבור בין כלים",
  criterion: "ניסוח קריטריון",
};
const SUGGESTION_TAG: Record<StyleSuggestion["kind"], string> = {
  replace: "מילה שאת מחליפה",
  avoid: "משהו שאת מוחקת",
  phrase: "ביטוי שחוזר אצלך",
  opening: "איך את פותחת סעיף",
  closing: "איך את מסיימת סעיף",
  formula: "משפט שחוזר אצלך",
};
const KINDS: StyleKind[] = ["rule", "phrase", "avoid", "template", "argument", "integration", "criterion", "example"];
const RECOMMENDED = 3;

type Step = { kind: "analysis"; source: StyleSourceView } | { kind: "profile" } | { kind: "interpret" };

const STEP_TITLE = (step: Step) =>
  step.kind === "analysis" ? `ניתוח סגנון · ${step.source.title}` : step.kind === "interpret" ? "איך פירשת תוצאות" : "בניית פרופיל הסגנון";

function dateOf(unix: number): string {
  return new Date(unix * 1000).toLocaleDateString("he-IL", { day: "numeric", month: "long", year: "numeric" });
}

/** "היום", "לפני שבוע", "ב-3 במרץ 2026": when the last report was added, in words. */
export function sinceLabel(unix: number, now = Date.now()): string {
  const days = Math.floor((now / 1000 - unix) / 86_400);
  if (days <= 0) return "היום";
  if (days === 1) return "אתמול";
  if (days < 7) return `לפני ${days} ימים`;
  if (days < 14) return "לפני שבוע";
  if (days < 31) return `לפני ${Math.floor(days / 7)} שבועות`;
  return `ב-${dateOf(unix)}`;
}

// ------------------------------------------------------------------ what waits for her

/** The draft items she already said something about, so leaving the page does not ask again. */
const REVIEWED_KEY = "dv.style.reviewed";
function loadReviewed(): string[] {
  try {
    const v: unknown = JSON.parse(localStorage.getItem(REVIEWED_KEY) ?? "[]");
    return Array.isArray(v) ? v.filter((x): x is string => typeof x === "string") : [];
  } catch {
    return [];
  }
}
function storeReviewed(ids: string[]) {
  try {
    if (ids.length === 0) localStorage.removeItem(REVIEWED_KEY);
    else localStorage.setItem(REVIEWED_KEY, JSON.stringify(ids));
  } catch {
    // Without storage she is only asked again about what she already saw; nothing is used unasked.
  }
}

type Waiting =
  | { kind: "item"; key: string; item: StyleItem }
  | { kind: "draft"; key: string; items: number }
  | { kind: "suggestion"; key: string; s: StyleSuggestion }
  | { kind: "interpretation"; key: string; view: InterpretationView };

const sameItem = (a: StyleItem, b: StyleItem) => a.kind === b.kind && a.section === b.section && a.text === b.text;

/** Everything that waits for her yes, in one line: the draft's new items, then what was learned
 * from her edits, then how she interprets scores. An item the profile in use already holds is
 * not asked about again. */
export function waitingFor(o: StyleOverview, reviewed: string[]): Waiting[] {
  const out: Waiting[] = [];
  if (o.draft) {
    const known = (o.active?.profile.items ?? []).filter((i) => i.enabled);
    const fresh = o.draft.profile.items.filter((i) => !reviewed.includes(i.id) && !known.some((k) => sameItem(k, i)));
    if (fresh.length > 0) out.push(...fresh.map((item): Waiting => ({ kind: "item", key: `item:${item.id}`, item })));
    else out.push({ kind: "draft", key: `draft:${o.draft.id}`, items: o.draft.profile.items.filter((i) => i.enabled).length });
  }
  out.push(...o.suggestions.map((s): Waiting => ({ kind: "suggestion", key: `sugg:${s.id}`, s })));
  out.push(...o.interpretations.filter((i) => !i.item.approved).map((view): Waiting => ({ kind: "interpretation", key: `int:${view.item.id}`, view })));
  return out;
}

export function StyleScreen() {
  const { status, fail, notify } = useApp();
  const qc = useQueryClient();
  const overview = useQuery({ queryKey: ["style"], queryFn: ipc.styleOverview });
  const fileInput = useRef<HTMLInputElement>(null);
  const [importing, setImporting] = useState<StyleImportPreview | null>(null);
  const [reading, setReading] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [deleting, setDeleting] = useState<StyleSourceView | null>(null);
  const [resetting, setResetting] = useState(false);
  const [forgetting, setForgetting] = useState(false);
  const [showVersions, setShowVersions] = useState(false);
  const [reviewed, setReviewed] = useState<string[]>(loadReviewed);
  // The run: what is left to send, the review on screen, and the request on its way.
  const [queue, setQueue] = useState<Step[]>([]);
  const [review, setReview] = useState<{ step: Step; prepared: Prepared } | null>(null);
  const [sending, setSending] = useState<{ label: string; approval: string; started: number } | null>(null);

  const o = overview.data;
  const reload = () => qc.invalidateQueries({ queryKey: ["style"] });
  const sectionTitle = (key: string | null) =>
    key ? (o?.sections.find((s) => s.key === key)?.title ?? key) : "לא זוהה סעיף";

  // What she said about the draft's items lives as long as that draft; a new draft asks afresh.
  const markReviewed = (id: string | null) => {
    const ids = o?.draft?.profile.items.map((i) => i.id) ?? [];
    setReviewed((r) => {
      const next = id === null ? [] : [...r.filter((x) => ids.includes(x)), id];
      storeReviewed(next);
      return next;
    });
  };

  async function pick(file: File) {
    setError(null);
    setReading(true);
    try {
      setImporting(await ipc.importStyleSource(file));
    } catch (e) {
      setError(fail(e as never));
    } finally {
      setReading(false);
      if (fileInput.current) fileInput.current.value = "";
    }
  }

  // ---------------------------------------------------------------- the run

  async function prepare(step: Step) {
    if (step.kind === "analysis") return ipc.prepareStyleAnalysis(step.source.id);
    return step.kind === "interpret" ? ipc.prepareInterpretations() : ipc.prepareStyleProfile();
  }

  async function next(steps: Step[]) {
    setQueue(steps);
    const step = steps[0];
    if (!step) {
      await reload();
      return;
    }
    setError(null);
    try {
      const prepared = await prepare(step);
      // Round 26: what the gate cleared goes at once; the review only when she asked for it.
      if (status.review_always) setReview({ step, prepared });
      else if (prepared.approval_id) await send(step, prepared.approval_id, steps);
      else {
        setError(notSentMessage(prepared));
        setQueue([]);
      }
    } catch (e) {
      setError(fail(e as never));
      setQueue([]);
    }
  }

  async function send(step: Step, approval: string, steps: Step[]) {
    const label = step.kind === "analysis"
      ? `מנתח את הסגנון ב"${step.source.title}"`
      : step.kind === "interpret" ? "לומד מהדוחות איך פירשת תוצאות" : "בונה את פרופיל הסגנון מכל הניתוחים";
    const started = Date.now();
    setSending({ label, approval, started });
    try {
      if (step.kind === "analysis") {
        const r = await ipc.sendStyleAnalysis(approval);
        if (r.dropped > 0) notify(`${r.kept} פריטים נשמרו, ${r.dropped} נפסלו (העתקה או פרט מזהה).`);
      } else if (step.kind === "interpret") {
        const r = await ipc.sendInterpretations(approval);
        notify(r.kept === 0
          ? "לא נמצאה פרשנות חדשה בדוחות."
          : `${r.kept === 1 ? "פרשנות אחת מחכה" : `${r.kept} פרשנויות מחכות`} לאישור שלך.${r.dropped > 0 ? ` ${r.dropped} נפסלו (העתקה או פרט מזהה).` : ""}`);
      } else {
        const view = await ipc.sendStyleProfile(approval);
        markReviewed(null);
        notify(view.dropped > 0 ? `הטיוטה מוכנה. ${view.dropped} פריטים נפסלו (העתקה או פרט מזהה).` : "הטיוטה מוכנה. אפשר לאשר אחד-אחד.");
      }
      recordDuration("draft", status.speed, Date.now() - started);
      setSending(null);
      await reload();
      await next(steps.slice(1));
    } catch (e) {
      setSending(null);
      setError(fail(e as never));
      setQueue([]);
      await reload();
    }
  }

  function startBuild(analyzeAll: boolean) {
    if (!o) return;
    const todo = o.sources.filter((s) => analyzeAll || !s.analyzed || (s.analysis_demo && !o.demo_mode));
    void next([...todo.map((source): Step => ({ kind: "analysis", source })), { kind: "profile" }]);
  }

  if (!o) {
    return (
      <div className="page">
        <TopBar active="style" />
        <main className="page-main style">{overview.error
          ? <ErrorLine error={fail(overview.error as never)} retry={() => void qc.invalidateQueries({ queryKey: ["style"] })} />
          : <div className="stack" style={{ gap: 16 }}><Skeleton card lines={4} label="טוען את הסגנון" /><Skeleton card lines={3} label="טוען" /></div>}</main>
      </div>
    );
  }

  const waiting = o.sources.filter((s) => !s.analyzed || (s.analysis_demo && !o.demo_mode)).length;
  const running = queue.length > 0 || sending !== null;
  const asks = waitingFor(o, reviewed);
  // A build is due when a report was not read yet, or nothing was built from the reports read.
  const buildDue = o.sources.length > 0 && (waiting > 0 || (!o.draft && !o.active));
  const analyzed = o.sources.filter((s) => s.analyzed).length;
  const buildLabel = analyzed === 0
    ? `ניתוח ${o.sources.length === 1 ? "הדוח" : `${o.sources.length} הדוחות`} ובניית הפרופיל`
    : waiting > 0 ? `ניתוח ${waiting === 1 ? "הדוח החדש" : `${waiting} הדוחות החדשים`} ועדכון הפרופיל` : "בניית הפרופיל";
  const canForget = o.learned.from_edits > 0 || o.learned.finished_reports > 0 || o.learned.fit_count > 0;

  const more: MenuItem[] = [
    ...(o.sources.some((s) => s.analyzed) ? [{ label: "ניתוח מחדש של כל הדוחות", run: () => startBuild(true), disabled: running }] : []),
    ...(o.versions.length > 0 ? [{ label: showVersions ? "להסתיר את הגרסאות" : "גרסאות קודמות של הפרופיל", run: () => setShowVersions((v) => !v) }] : []),
    ...(canForget ? [{ label: "לשכוח את מה שנלמד מהתיקונים ומהדוחות שסיימתי", run: () => setForgetting(true), danger: true }] : []),
    ...(o.versions.length > 0 || o.draft ? [{ label: "מחיקת הפרופיל ומה שנלמד", run: () => setResetting(true), danger: true }] : []),
  ];

  return (
    <div className="page">
      <TopBar active="style" />
      <main className="page-main style">
        <header className="style-head">
          <h1>הסגנון שלי</h1>
          <p className="style-lead">התוכנה לומדת מהדוחות שלך איך את כותבת. שום דבר לא נכנס לשימוש לפני שאישרת אותו.</p>
          {o.demo_mode && <p className="hint">מצב הדגמה: ניתוח לדוגמה בלבד.</p>}
        </header>

        <input ref={fileInput} type="file" hidden accept=".docx,.doc,.odt,.rtf,.pdf,.txt"
          onChange={(e) => { const f = e.target.files?.[0]; if (f) void pick(f); }} />

        {o.sources.length === 0 ? (
          <div className="style-start">
            <button type="button" className="btn btn-primary btn-big" disabled={reading} onClick={() => fileInput.current?.click()}>
              <UploadIcon /> הוספת דוח ישן
            </button>
            <p className="hint">דוח שכתבת בעבר, ב-Word, ODT או PDF. נשמר רק הטקסט אחרי הסתרה.</p>
          </div>
        ) : (
          <StyleNumbers o={o} waiting={asks.length} />
        )}

        {reading && <p className="muted"><Spinner /> קורא את הדוח ומסתיר פרטים מזהים…</p>}
        {sending && (
          <ProgressLine started={sending.started} estimate={estimateMs("draft", status.speed, status.demo_mode)} label={sending.label} approval={sending.approval} />
        )}
        {queue.length > 1 && !sending && !review && <p className="muted small">נשארו {queue.length} שלבים.</p>}
        <ErrorLine error={error} />

        {o.sources.length > 0 && (
          <>
            <div className="style-pair">
              <ApproveQueue o={o} asks={asks} onReviewed={markReviewed} running={running}
                build={buildDue && asks.length === 0 ? { label: buildLabel, run: () => startBuild(false) } : null} />
              <KnownList o={o} running={running} onReadScores={() => void next([{ kind: "interpret" }])} />
            </div>
            <Reports o={o} adding={reading || running} onAdd={() => fileInput.current?.click()} onDelete={setDeleting} more={more}
              build={buildDue && asks.length > 0 ? { label: buildLabel, run: () => startBuild(false), disabled: running } : null} />
            {showVersions && o.versions.length > 0 && <Versions o={o} />}
          </>
        )}
      </main>

      {importing && (
        <StyleImportDialog preview={importing} sectionTitle={sectionTitle} onClose={() => setImporting(null)}
          onSaved={(s) => { setImporting(null); notify(`"${s.title}" נשמר ב"הדוחות שלי".`); void reload(); }} />
      )}
      {review && (
        <ReviewDialog title={STEP_TITLE(review.step)}
          caseId={null} prepared={review.prepared} reprepare={() => prepare(review.step)}
          onClose={() => { setReview(null); setQueue([]); }}
          onSend={async (id) => { const step = review.step; setReview(null); await send(step, id, queue); }} />
      )}
      {deleting && (
        <ConfirmDialog danger title="מחיקת דוח" action="מחיקה"
          text={`"${deleting.title}" יימחק. הפרופיל שאושר לא משתנה עד בנייה מחדש.`}
          onClose={() => setDeleting(null)}
          onConfirm={async () => { await ipc.deleteStyleSource(deleting.id); setDeleting(null); await reload(); }} />
      )}
      {forgetting && (
        <ConfirmDialog danger title="לשכוח את מה שנלמד" action="לשכוח"
          text="מה שנלמד מהתיקונים שלך יימחק. הדוחות הישנים והפרופיל נשארים."
          onClose={() => setForgetting(false)}
          onConfirm={async () => { await ipc.forgetStyleLearning(); setForgetting(false); notify("מה שנלמד נמחק."); await reload(); }} />
      )}
      {resetting && (
        <ConfirmDialog danger title="מחיקת הפרופיל" action="מחיקת הפרופיל והלמידה"
          text="הפרופיל ומה שנלמד מהתיקונים יימחקו. הדוחות הישנים נשארים."
          onClose={() => setResetting(false)}
          onConfirm={async () => { await ipc.resetStyle(); setResetting(false); markReviewed(null); await reload(); }} />
      )}
    </div>
  );
}

// ------------------------------------------------------------------ the three numbers

function StyleNumbers({ o, waiting }: { o: StyleOverview; waiting: number }) {
  const known = (o.active?.profile.items.filter((i) => i.enabled).length ?? 0)
    + o.interpretations.filter((i) => i.item.approved && i.item.enabled).length;
  return (
    <ul className="style-numbers" aria-label="במספרים">
      <li className="style-number"><b>{o.sources.length}</b><span>{o.sources.length === 1 ? "דוח שלך נקרא" : "דוחות שלך נקראו"}</span></li>
      <li className="style-number"><b>{known}</b><span>דברים שהתוכנה יודעת עלייך</span></li>
      <li className={waiting > 0 ? "style-number warn" : "style-number"}><b>{waiting}</b><span>מחכים לאישור שלך</span></li>
    </ul>
  );
}

// ------------------------------------------------------------------ one at a time

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
function ApproveQueue({ o, asks, onReviewed, running, build }: {
  o: StyleOverview;
  asks: Waiting[];
  onReviewed: (id: string) => void;
  running: boolean;
  build: { label: string; run: () => void } | null;
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
        {build ? (
          <>
            <h2 id="style-queue-title">ללמוד מהדוחות</h2>
            <p className="style-queue-lead">
              {o.sources.length < RECOMMENDED ? `מומלץ ${RECOMMENDED} דוחות לפחות. ` : ""}
              אחרי הניתוח כל דבר שהתוכנה למדה יחכה כאן לאישור שלך.
            </p>
            <button type="button" className="btn btn-primary btn-big" disabled={running} onClick={build.run}>{build.label}</button>
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
      body = item.text;
      why = itemWhy(item, o.draft?.profile.reports ?? 0, o);
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
          <button type="button" className="btn btn-ghost btn-small style-whole" aria-expanded={whole} onClick={() => setWhole((w) => !w)}>
            {whole ? "להסתיר את הטיוטה" : "לעבור על כל הטיוטה בבת אחת"}
          </button>
          {whole && <ProfileEditor key={o.draft.id} view={o.draft} o={o} />}
        </>
      )}
    </section>
  );
}

// ------------------------------------------------------------------ what the program knows

type Known = "writing" | "order" | "phrases" | "scores" | "argument";
const KNOWN: { key: Known; title: string; line: string; kinds: StyleKind[] }[] = [
  { key: "writing", title: "איך את כותבת", line: "כללי כתיבה, מה את לא כותבת, ומה התוכנה מדדה", kinds: ["rule", "avoid", "example"] },
  { key: "order", title: "סדר הנושאים בכל סעיף", line: "הסדר שבו את כותבת כל סעיף", kinds: [] },
  { key: "phrases", title: "משפטים שחוזרים אצלך", line: "ביטויים ותבניות משפט קבועות", kinds: ["phrase", "template"] },
  { key: "scores", title: "איך את מפרשת ציונים", line: "מה את כותבת על תוצאה של כל תת-מבחן", kinds: [] },
  { key: "argument", title: "איך את בונה טיעון", line: "חיבור בין כלים, וניסוח קריטריון DSM", kinds: ["argument", "integration", "criterion"] },
];

function KnownList({ o, running, onReadScores }: { o: StyleOverview; running: boolean; onReadScores: () => void }) {
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
            <span className="style-item-text grow">{i.text}</span>
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

// ------------------------------------------------------------------ my reports

function Reports(props: {
  o: StyleOverview;
  adding: boolean;
  onAdd: () => void;
  onDelete: (s: StyleSourceView) => void;
  more: MenuItem[];
  build: { label: string; run: () => void; disabled: boolean } | null;
}) {
  const { o } = props;
  const [list, setList] = useState(false);
  const last = Math.max(...o.sources.map((s) => s.added_at));
  const notRead = o.sources.filter((s) => !s.analyzed).length;
  return (
    <section className="card style-reports" aria-labelledby="style-sources">
      <div className="style-reports-row">
        <h2 id="style-sources">הדוחות שלי</h2>
        <span className="muted grow">
          {o.sources.length === 1 ? "דוח אחד" : `${o.sources.length} דוחות`}, האחרון נוסף {sinceLabel(last)}.
          {notRead > 0 && ` ${notRead === 1 ? "אחד עוד לא נותח" : `${notRead} עוד לא נותחו`}.`}
          {" "}נשמר רק הטקסט אחרי הסתרה, לא הקובץ.
        </span>
        <div className="style-reports-actions">
          {props.build && <button type="button" className="btn" disabled={props.build.disabled} onClick={props.build.run}>{props.build.label}</button>}
          <button type="button" className="btn btn-ghost" aria-expanded={list} aria-controls="style-source-list" onClick={() => setList((v) => !v)}>
            {list ? "להסתיר את הרשימה" : "לראות את הרשימה"}
          </button>
          <button type="button" className="btn" disabled={props.adding} onClick={props.onAdd}><UploadIcon /> הוספת דוח</button>
          {props.more.length > 0 && <ActionsMenu label="עוד פעולות על הסגנון" items={props.more} />}
        </div>
      </div>
      {list && (
        <ul id="style-source-list" className="style-sources">
          {o.sources.map((s) => (
            <li key={s.id} className="style-source">
              <span className="fmt">{s.format.toUpperCase()}</span>
              <div className="stack grow" style={{ gap: 4 }}>
                <strong>{s.title}</strong>
                <span className="muted small">
                  נוסף ב-{dateOf(s.added_at)} · {s.words.toLocaleString("he-IL")} מילים · {s.headings.join(" · ")}
                </span>
              </div>
              {s.analyzed
                ? s.analysis_demo
                  ? <span className="chip chip-sand" title="נותח במצב הדגמה">נותח כדוגמה</span>
                  : <span className="chip chip-ok">נותח · {s.analysis_items} פריטים</span>
                : <span className="chip chip-warn">עוד לא נותח</span>}
              <button type="button" className="btn btn-small btn-danger-quiet" onClick={() => props.onDelete(s)}>מחיקה</button>
            </li>
          ))}
        </ul>
      )}
    </section>
  );
}

// ------------------------------------------------------------------ what was measured

/** Her usual figures, in words: what the program measured, numbers only. */
export function learnedLines(l: StyleLearned): string[] {
  const out: string[] = [];
  const round = (x: number) => Math.round(x);
  if (l.words_per_sentence != null) out.push(`בממוצע ${round(l.words_per_sentence)} מילים במשפט`);
  if (l.sentences_per_paragraph != null) out.push(`בדרך כלל ${round(l.sentences_per_paragraph)} משפטים בפסקה`);
  if (l.commas_per_sentence != null) out.push(`כ-${l.commas_per_sentence.toFixed(1)} פסיקים במשפט`);
  if (l.hedges != null) out.push(`הסתייגות («נראה», «ניכר») ב-${round(l.hedges)} מכל 100 משפטים`);
  if (l.contrasts != null) out.push(`מילת ניגוד («אך», «עם זאת») ב-${round(l.contrasts)} מכל 100 משפטים`);
  if (l.first_person != null) out.push(`גוף ראשון («התרשמתי») ב-${round(l.first_person)} מכל 100 משפטים`);
  return out;
}

const MONTHS_HE = ["ינואר", "פברואר", "מרץ", "אפריל", "מאי", "יוני", "יולי", "אוגוסט", "ספטמבר", "אוקטובר", "נובמבר", "דצמבר"];

/** "2026-10" → ["אוק׳ 26" as a short label, "אוקטובר 2026" in full]. */
export function monthLabel(month: string): { short: string; full: string } {
  const [y = "", m = "1"] = month.split("-");
  const name = MONTHS_HE[Number(m) - 1] ?? month;
  return { short: `${Number(m)}.${y.slice(2)}`, full: `${name} ${y}` };
}

/** ST12: the average closeness by month, oldest first: is drafting getting closer to her? */
function FitTrend({ months }: { months: FitMonth[] }) {
  return (
    <div className="fit-trend">
      <span className="small muted">לאורך זמן (ממוצע לחודש, מתוך 100):</span>
      <ol className="fit-bars">
        {months.map((m) => {
          const { short, full } = monthLabel(m.month);
          return (
            <li key={m.month} className="fit-bar" aria-label={`${full}: ${m.average} (${m.count} פסקאות)`}>
              <span className="fit-value" aria-hidden="true">{m.average}</span>
              <span className="fit-fill" style={{ height: `${Math.max(3, Math.round(m.average * 0.8))}px` }} aria-hidden="true" />
              <span className="fit-month" aria-hidden="true">{short}</span>
            </li>
          );
        })}
      </ol>
    </div>
  );
}

function Learned({ o }: { o: StyleOverview }) {
  const ai = useAi();
  const l = o.learned;
  const lines = learnedLines(l);
  const from = [
    l.from_reports > 0 && `${l.from_reports} פסקאות מהדוחות הישנים`,
    l.from_edits > 0 && `${l.from_edits} מהתיקונים שלך`,
    l.finished_reports > 0 && (l.finished_reports === 1 ? "דוח אחד שסיימת בתוכנה" : `${l.finished_reports} דוחות שסיימת בתוכנה`),
  ].filter(Boolean) as string[];
  if (from.length === 0 && l.fit_count === 0 && o.omissions.length === 0) return null;
  return (
    <div className="stack style-measured">
      <h3 className="small">מה התוכנה מדדה בכתיבה שלך</h3>
      <p className="hint">נמדד במחשב שלך, ונשמרים רק מספרים. {from.length > 0 && `נלמד מ${from.join(", ")}.`}</p>
      {lines.length > 0 ? (
        <ul className="style-learned">{lines.map((t) => <li key={t}>{t}</li>)}</ul>
      ) : (
        <p className="muted small">עוד אין מספיק פסקאות כדי לומר מה אופייני לך (צריך לפחות 12).</p>
      )}
      {l.fit_recent != null && (
        <p className="style-fit">
          כמה הפסקאות ש-{ai} כתב קרובות לסגנון שלך: <b>{l.fit_recent} מתוך 100</b>
          {l.fit_earlier != null
            ? ` ב-30 הפסקאות האחרונות, לעומת ${l.fit_earlier} ב-30 שלפניהן.`
            : ` (${l.fit_count} פסקאות עד עכשיו).`}
        </p>
      )}
      {l.fit_by_month.length >= 2 && <FitTrend months={l.fit_by_month} />}
      {o.omissions.length > 0 && (
        <ul className="style-learned" aria-label="מה את לא כותבת">
          {o.omissions.map((t) => <li key={t}>{t}</li>)}
        </ul>
      )}
    </div>
  );
}

// ------------------------------------------------------------------ interpretation (round 33)

/** How she interprets each subtest's score, learned from her reports; nothing is used before she approves it. */
function Interpretations({ o, running, onRead }: { o: StyleOverview; running: boolean; onRead: () => void }) {
  const { fail } = useApp();
  const qc = useQueryClient();
  const [editing, setEditing] = useState<string | null>(null);
  const [draft, setDraft] = useState("");
  const [error, setError] = useState<string | null>(null);
  // The item being changed: its buttons wait, so a double click does it once (the review of stage 4).
  const [busyId, setBusyId] = useState<string | null>(null);
  const [deleting, setDeleting] = useState<InterpretationView | null>(null);
  if (o.scored_measures === 0 && o.interpretations.length === 0) {
    return <p className="muted small">בדוחות שלך עוד לא נמצאה תוצאה ליד שם של תת-מבחן.</p>;
  }
  const waiting = o.interpretations.filter((i) => !i.item.approved).length;
  const groups = new Map<string, InterpretationView[]>();
  for (const i of o.interpretations) {
    const key = `${i.instrument_name} · ${i.measure_name}`;
    groups.set(key, [...(groups.get(key) ?? []), i]);
  }

  async function act(id: string, f: () => Promise<unknown>) {
    if (busyId !== null) return;
    setError(null);
    setBusyId(id);
    try {
      await f();
      setEditing(null);
      await qc.invalidateQueries({ queryKey: ["style"] });
    } catch (e) {
      setError(fail(e as never));
    } finally {
      setBusyId(null);
    }
  }
  const save = (id: string) => void act(id, () => ipc.updateInterpretation(id, draft, true, null));

  return (
    <div className="stack style-interpret">
      <p className="hint">
        מהמשפטים בדוחות הישנים שבהם כתבת על תוצאה של תת-מבחן. נשלחים רק המשפטים האלה, בלי פרטים מזהים, ונשמרת פרשנות כללית.
        שום פרשנות לא משמשת בדוח לפני שאישרת אותה.
      </p>
      <div className="row">
        <button type="button" className="btn btn-small" disabled={running || o.scored_measures === 0} onClick={() => { setError(null); onRead(); }}>
          {o.interpretations.length === 0 ? "ללמוד מהדוחות איך פירשתי תוצאות" : "לקרוא שוב את הדוחות"}
        </button>
        {o.scored_measures > 0 && <span className="hint">{o.scored_measures === 1 ? "תת-מבחן אחד" : `${o.scored_measures} תת-מבחנים`} עם תוצאות בדוחות.</span>}
        {waiting > 0 && <span className="chip">{waiting === 1 ? "אחת מחכה" : `${waiting} מחכות`} לאישור</span>}
      </div>
      <ErrorLine error={error} />
      {[...groups.entries()].map(([title, items]) => (
        <div key={title} className="style-group" role="group" aria-label={title}>
          <h3>{title}</h3>
          <ul className="style-items">
            {items.map((view) => {
              const { item } = view;
              const busy = busyId === item.id;
              return (
                <li key={item.id} className={item.enabled ? "style-item" : "style-item style-item-off"}>
                  <span className="chip">{item.case}</span>{" "}
                  {editing === item.id ? (
                    <span className="stack grow">
                      {/* One line: the core keeps a single paragraph of up to 600 characters. */}
                      <textarea className="textarea" aria-label="הפרשנות" value={draft} rows={2} maxLength={600} autoFocus
                        onChange={(e) => setDraft(e.target.value)}
                        onKeyDown={(e) => {
                          if (e.key === "Escape") {
                            e.preventDefault();
                            setEditing(null);
                          } else if (e.key === "Enter" && (e.ctrlKey || e.metaKey)) {
                            e.preventDefault();
                            save(item.id);
                          }
                        }} />
                      <span className="row">
                        <button type="button" className="btn btn-small btn-primary" disabled={busy || !draft.trim()} onClick={() => save(item.id)}>שמירה ואישור</button>
                        <button type="button" className="btn btn-small" onClick={() => setEditing(null)}>ביטול</button>
                      </span>
                    </span>
                  ) : (
                    <span className="stack grow">
                      <span className="style-item-text">{item.text}</span>
                      <span className="row">
                        {!item.approved && <span className="chip chip-warn">מחכה לאישור</span>}
                        {item.support > 1 && <span className="hint">ב-{item.support} דוחות</span>}
                        {!item.approved && <button type="button" className="btn btn-small btn-primary" disabled={busy} onClick={() => void act(item.id, () => ipc.updateInterpretation(item.id, null, true, null))}>אישור</button>}
                        {/* One editor at a time: what she typed in the open one is never dropped silently. */}
                        <button type="button" className="btn btn-small" disabled={busy || editing !== null}
                          title={editing !== null ? "קודם לשמור או לבטל את העריכה הפתוחה" : undefined}
                          onClick={() => { setEditing(item.id); setDraft(item.text); }}>עריכה</button>
                        {item.approved && (
                          <button type="button" className="btn btn-small" disabled={busy} onClick={() => void act(item.id, () => ipc.updateInterpretation(item.id, null, null, !item.enabled))}>
                            {item.enabled ? "כיבוי" : "הדלקה"}
                          </button>
                        )}
                        <button type="button" className="btn btn-small btn-danger-quiet" disabled={busy} onClick={() => setDeleting(view)}>מחיקה</button>
                      </span>
                    </span>
                  )}
                </li>
              );
            })}
          </ul>
        </div>
      ))}
      {deleting && (
        <ConfirmDialog danger title="מחיקת פרשנות" action="מחיקה"
          text={`"${deleting.item.text}" יימחק, וכבר לא ישמש בטיוטות.`}
          onClose={() => setDeleting(null)}
          onConfirm={async () => {
            await ipc.deleteInterpretation(deleting.item.id);
            setDeleting(null);
            setError(null);
            await qc.invalidateQueries({ queryKey: ["style"] });
          }} />
      )}
    </div>
  );
}

// ------------------------------------------------------------------ the profile

/** The profile's items, to check, change, delete or add. `kinds`: only these are shown (a row of
 * "what the program knows"); the rest of the profile is kept as it is when she saves. */
function ProfileEditor({ view, o, kinds }: { view: StyleProfileView; o: StyleOverview; kinds?: StyleKind[] }) {
  const { fail, notify } = useApp();
  const ai = useAi();
  const qc = useQueryClient();
  const [items, setItems] = useState<StyleItem[]>(view.profile.items);
  const [dirty, setDirty] = useState(false);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [editing, setEditing] = useState<string | null>(null);
  const isDraft = view.status === "draft";
  const shownKinds = kinds ?? KINDS;

  const groups = useMemo(() => {
    const keys: (string | null)[] = [null, ...o.sections.map((s) => s.key)];
    return keys
      .map((key) => ({ key, title: key ? (o.sections.find((s) => s.key === key)?.title ?? key) : "כל הדוח", items: items.filter((i) => i.section === key && shownKinds.includes(i.kind)) }))
      .filter((g) => g.key === null || g.items.length > 0);
  }, [items, o.sections, shownKinds]);

  const change = (f: (all: StyleItem[]) => StyleItem[]) => {
    setItems(f);
    setDirty(true);
  };
  const profile = (): StyleProfile => ({ items, reports: view.profile.reports });

  async function act(f: () => Promise<unknown>, done: string) {
    setBusy(true);
    setError(null);
    try {
      await f();
      notify(done);
      setDirty(false);
      await qc.invalidateQueries({ queryKey: ["style"] });
    } catch (e) {
      setError(fail(e as never));
    } finally {
      setBusy(false);
    }
  }

  const enabledCount = items.filter((i) => i.enabled).length;

  return (
    <section id="style-profile" className={isDraft ? "style-profile style-draft" : "style-profile"} aria-label={isDraft ? "טיוטת פרופיל הסגנון" : "פרופיל הסגנון"}>
      {isDraft && (
        <div className="row style-profile-head">
          <span className="chip chip-warn">טיוטה · עוד לא בשימוש</span>
          {view.profile.reports > 0 && <span className="chip chip-sand">נבנה מ-{view.profile.reports} דוחות</span>}
          {view.demo && <span className="chip chip-sand">נבנה במצב הדגמה</span>}
        </div>
      )}
      <p className="hint">
        {isDraft
          ? "רק מה שמסומן ייכנס, ורק אחרי האישור."
          : "כל שינוי משפיע רק אחרי אישור."}
        {isDraft && view.dropped > 0 && ` ${view.dropped} פריטים נפסלו (העתקה או פרט מזהה).`}
      </p>

      {groups.map((g) => (
        <div key={g.key ?? "general"} className="style-group">
          <h3>{g.title}</h3>
          {shownKinds.map((kind) => {
            const list = g.items.filter((i) => i.kind === kind);
            if (list.length === 0) return null;
            return (
              <div key={kind} className="style-kind">
                <span className="style-kind-title">{KIND_LABEL[kind]}{KIND_HINT[kind] && <span className="muted small"> · {KIND_HINT[kind]}</span>}</span>
                <ul className="style-items">
                  {list.map((item) => (
                    <li key={item.id} className={item.enabled ? "style-item" : "style-item style-item-off"}>
                      <input type="checkbox" aria-label="להשתמש בפריט" checked={item.enabled}
                        onChange={() => change((all) => all.map((x) => (x.id === item.id ? { ...x, enabled: !x.enabled } : x)))} />
                      {editing === item.id ? (
                        <textarea className="textarea grow" rows={kind === "example" ? 4 : 2} autoFocus defaultValue={item.text}
                          onBlur={(e) => { const text = e.target.value.trim(); setEditing(null); if (text && text !== item.text) change((all) => all.map((x) => (x.id === item.id ? { ...x, text } : x))); }} />
                      ) : (
                        <button type="button" className={kind === "example" ? "style-item-text style-example grow" : "style-item-text grow"} title="עריכה" onClick={() => setEditing(item.id)}>
                          {item.text}
                        </button>
                      )}
                      {item.origin === "edits" && <span className="chip chip-sand">מהתיקונים שלך</span>}
                      {item.origin === "manual" && <span className="chip chip-sand">שלך</span>}
                      {item.origin === "reports" && item.support > 1 && <span className="chip">ב-{item.support} דוחות</span>}
                      <button type="button" className="btn icon-btn" aria-label="מחיקה" onClick={() => change((all) => all.filter((x) => x.id !== item.id))}><CloseIcon /></button>
                    </li>
                  ))}
                </ul>
              </div>
            );
          })}
          <AddItem kinds={shownKinds} onAdd={(kind, text) => change((all) => [...all, { id: "", section: g.key, kind, text, enabled: true, origin: "manual", support: 0 }])} />
        </div>
      ))}

      <ErrorLine error={error} />
      <div className="row style-profile-foot">
        <span className="muted small grow">{enabledCount} פריטים מסומנים</span>
        {isDraft && (
          <button type="button" className="btn btn-danger-quiet" disabled={busy} onClick={() => void act(() => ipc.discardStyleDraft(), "הטיוטה נמחקה.")}>מחיקת הטיוטה</button>
        )}
        {dirty && (
          <button type="button" className="btn" disabled={busy} onClick={() => void act(() => ipc.saveStyleDraft(profile()), "השינויים נשמרו בטיוטה.")}>שמירה כטיוטה</button>
        )}
        {(isDraft || dirty) && (
          <button type="button" className="btn btn-primary" disabled={busy || enabledCount === 0}
            onClick={() => void act(async () => { if (dirty) await ipc.saveStyleDraft(profile()); await ipc.approveStyleDraft(); }, `הפרופיל אושר. מעכשיו ${ai} כותב בסגנון שלך.`)}>
            אישור הפרופיל
          </button>
        )}
      </div>
    </section>
  );
}

function AddItem({ kinds, onAdd }: { kinds: StyleKind[]; onAdd: (kind: StyleKind, text: string) => void }) {
  const choices = kinds.filter((k) => k !== "example");
  const [open, setOpen] = useState(false);
  const [kind, setKind] = useState<StyleKind>(choices[0] ?? "rule");
  const [text, setText] = useState("");
  if (!open) {
    return <button type="button" className="btn btn-ghost btn-small style-add" onClick={() => setOpen(true)}>+ הוספת פריט משלך</button>;
  }
  return (
    <form className="row style-add-form" onSubmit={(e) => { e.preventDefault(); if (text.trim()) { onAdd(kind, text.trim()); setText(""); setOpen(false); } }}>
      <select className="select" aria-label="סוג" value={kind} onChange={(e) => setKind(e.target.value as StyleKind)}>
        {choices.map((k) => <option key={k} value={k}>{KIND_LABEL[k]}</option>)}
      </select>
      <input className="input grow" autoFocus placeholder="למשל: פסקת הסיכום פותחת בחוזקות של הילד" value={text} onChange={(e) => setText(e.target.value)} />
      <button type="submit" className="btn btn-small" disabled={!text.trim()}>הוספה</button>
      <button type="button" className="btn btn-small btn-ghost" onClick={() => setOpen(false)}>ביטול</button>
    </form>
  );
}

// ------------------------------------------------------------------ versions

function Versions({ o }: { o: StyleOverview }) {
  const { fail, notify } = useApp();
  const qc = useQueryClient();
  const [error, setError] = useState<string | null>(null);
  return (
    <section className="card style-card" aria-labelledby="style-versions">
      <h2 id="style-versions">גרסאות</h2>
      <ul className="style-versions">
        {o.versions.map((v) => (
          <li key={v.id} className="row">
            <span className="grow">
              גרסה {v.version} · {dateOf(v.created_at)} · {v.items} פריטים{v.reports > 0 ? ` · מ-${v.reports} דוחות` : ""}
            </span>
            {v.status === "active"
              ? <span className="chip chip-ok">בשימוש</span>
              : (
                <button type="button" className="btn btn-small btn-ghost"
                  onClick={() => { setError(null); ipc.restoreStyleVersion(v.id).then(() => { notify(`גרסה ${v.version} חזרה לשימוש (כגרסה חדשה).`); return qc.invalidateQueries({ queryKey: ["style"] }); }).catch((e) => setError(fail(e as never))); }}>
                  חזרה לגרסה הזו
                </button>
              )}
          </li>
        ))}
      </ul>
      <ErrorLine error={error} />
    </section>
  );
}
