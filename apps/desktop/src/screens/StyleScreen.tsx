// "הסגנון שלי" (D-043, round 46 / D-089): the program learns how Einat writes from her past
// reports. The page is three numbers, one queue that asks her about one thing at a time, what
// the program already knows (five rows that open in place) and one line about her reports.
// Every request goes through the same review screen as everything else that leaves the computer.
// The parts live in `./style/`; this file holds the page and the run (O5 / D-111).
import { useEffect, useRef, useState } from "react";
import { useQuery, useQueryClient } from "@tanstack/react-query";
import { useApp } from "../App";
import type { MenuItem } from "../components/Menu";
import { estimateMs, ProgressLine, recordDuration } from "../components/Progress";
import { ReviewDialog } from "../components/ReviewDialog";
import { StyleImportDialog } from "../components/StyleImportDialog";
import { TopBar } from "../components/TopBar";
import { ErrorLine, Skeleton, Spinner } from "../components/ui";
import { ipc, type Prepared, type StyleImportPreview, type StyleSourceView } from "../ipc/client";
import { ConfirmDialog } from "./library/LibraryDialogs";
import "./StyleScreen.css";
import { notSentMessage } from "../sending";
import { ApproveQueue } from "./style/ApproveQueue";
import { Journey } from "./style/Journey";
import { KnownList } from "./style/KnownList";
import { Reports } from "./style/Reports";
import { StyleStart } from "./style/StyleStart";
import { Versions } from "./style/Versions";
import { loadReviewed, storeReviewed, waitingFor } from "./style/waiting";

// What the page exported before the split, still from here.
export { CHAIN_SEP, chainLinks } from "./style/InferenceChain";
export { sinceLabel } from "./style/labels";
export { learnedLines, monthLabel } from "./style/Learned";
export { waitingFor } from "./style/waiting";

type Step = { kind: "analysis"; source: StyleSourceView } | { kind: "profile" } | { kind: "interpret" };

const STEP_TITLE = (step: Step) =>
  step.kind === "analysis" ? `ניתוח סגנון · ${step.source.title}` : step.kind === "interpret" ? "איך פירשת תוצאות" : "בניית פרופיל הסגנון";

export function StyleScreen() {
  const { status, fail, notify } = useApp();
  const qc = useQueryClient();
  const overview = useQuery({ queryKey: ["style"], queryFn: ipc.styleOverview });
  const fileInput = useRef<HTMLInputElement>(null);
  // Whether a run is on, for `runAll`, which is called from an upload that awaited first.
  const runningRef = useRef(false);
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
  useEffect(() => {
    runningRef.current = queue.length > 0 || sending !== null;
  }, [queue, sending]);

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

  /** One upload, and the rest runs by itself (D-138): the suggested parts are kept, the report
   * is read for its style, and the profile is updated. The parts can still be changed from
   * the report's row; nothing of the report leaves without passing the filter and the gate. */
  async function pick(file: File) {
    setError(null);
    setReading(true);
    try {
      const preview = await ipc.importStyleSource(file);
      const suggested = preview.parts.filter((x) => x.included).map((x) => x.index);
      if (suggested.length === 0 || preview.warnings.length > 0) {
        // Nothing stood out as style, or the reading has something to tell her (a part not
        // read, file properties to check for names): she sees the dialog and decides.
        setImporting(preview);
        return;
      }
      const saved = await ipc.saveStyleSource(preview.token, suggested, null);
      notify(`"${saved.title}" נשמר, ועכשיו נקרא ונלמד.`);
      await runAll();
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
        // The profile is used as it came (D-138): any item can be turned off or removed later
        // in "מה התוכנה יודעת עלייך", rather than approving one by one first.
        await ipc.approveStyleDraft();
        notify(view.dropped > 0 ? `הפרופיל עודכן. ${view.dropped} פריטים נפסלו (העתקה או פרט מזהה).` : "הפרופיל עודכן. אפשר לראות ולכבות פריטים ב\"מה התוכנה יודעת עלייך\".");
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

  /** Read every report not read yet and update the profile, from the overview as it is now. */
  async function runAll() {
    // One run at a time: a report added while another is read waits for the button.
    if (runningRef.current) return;
    const fresh = await qc.fetchQuery({ queryKey: ["style"], queryFn: ipc.styleOverview, staleTime: 0 });
    const todo = fresh.sources.filter((x) => !x.analyzed || (x.analysis_demo && !fresh.demo_mode));
    await next([...todo.map((source): Step => ({ kind: "analysis", source })), { kind: "profile" }]);
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
          <p className="style-lead">
            התוכנה לומדת מהדוחות שלך איך את כותבת, ומה את מסיקה מכל ממצא.
            {/* Before the first report the four stages are drawn below (D-112); after it, the journey shows them. */}
            {o.sources.length > 0 && " ארבעה שלבים: הדוח נכנס, נקרא, את מאשרת, וזה משמש בכל טיוטה."}
            {" "}מעלים דוח, והשאר קורה לבד. כל פריט אפשר לכבות או למחוק ב"מה התוכנה יודעת עלייך".
          </p>
          {o.demo_mode && <p className="hint">מצב הדגמה: ניתוח לדוגמה בלבד.</p>}
        </header>

        <input ref={fileInput} type="file" hidden accept=".docx,.doc,.odt,.rtf,.pdf,.txt"
          onChange={(e) => { const f = e.target.files?.[0]; if (f) void pick(f); }} />

        {o.sources.length === 0 ? (
          <StyleStart adding={reading} onAdd={() => fileInput.current?.click()} />
        ) : (
          <Journey o={o} waiting={asks.length} analyzed={analyzed} notRead={waiting} onAdd={() => fileInput.current?.click()} adding={reading || running}
            build={buildDue ? { label: buildLabel, run: () => startBuild(false), disabled: running } : null} />
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
              <ApproveQueue o={o} asks={asks} onReviewed={markReviewed} running={running} buildDue={buildDue} />
              <KnownList o={o} running={running} onReadScores={() => void next([{ kind: "interpret" }])} />
            </div>
            <Reports o={o} adding={reading || running} onAdd={() => fileInput.current?.click()} onDelete={setDeleting} more={more} />
            {showVersions && o.versions.length > 0 && <Versions o={o} />}
          </>
        )}
      </main>

      {importing && (
        <StyleImportDialog preview={importing} sectionTitle={sectionTitle} onClose={() => setImporting(null)}
          onSaved={(s) => { setImporting(null); notify(`"${s.title}" נשמר, ועכשיו נקרא ונלמד.`); void runAll(); }} />
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
