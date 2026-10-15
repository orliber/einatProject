import { useCallback, useEffect, useRef, useState } from "react";
import { useApp } from "../App";
import { ipc, type CaseDetail, type Prepared, type SortResult, type UiError } from "../ipc/client";
import { ReviewDialog } from "../components/ReviewDialog";
import { ExportDialog } from "../components/ExportDialog";
import { ReportRunDialog } from "../components/ReportRunDialog";
import { ProgressLine, estimateMs, recordDuration, type Task } from "../components/Progress";
import { ErrorLine, CloseIcon, Skeleton } from "../components/ui";
import { ActionsMenu, type MenuItem } from "../components/Menu";
import { SampleGuide } from "./case/SampleGuide";
import { CaseSearch } from "./case/CaseSearch";
import { ageOf } from "../components/AgeField";
import { MaterialsView } from "./case/MaterialsView";
import { SectionWork } from "./case/SectionWork";
import { DetailsView } from "./case/DetailsView";
import { SentHistoryDialog } from "../components/SentHistory";
import { ReportView } from "./case/ReportView";
import { FinishView } from "./case/FinishView";
import { isCombo, KEYS } from "../shortcuts";
import { useKept } from "../kept";
import "./CaseScreen.css";
import { useAi } from "../ai";
import { notSentMessage } from "../sending";

export interface ReviewRequest {
  title: string;
  prepared: Prepared;
  reprepare: () => Promise<Prepared>;
  onSend: (approvalId: string) => Promise<void>;
}

/** A request Claude is working on: shown with how long it has taken and about how long is left. */
export interface Job {
  id: number;
  label: string;
  task: Task;
  /** The section it writes, if any. */
  section?: string;
  /** The approved request on its way: its words written so far are shown. */
  approval?: string;
  started: number;
  estimate: number;
}

/** What a case view needs from the screen. */
export interface CaseApi {
  caseId: string;
  detail: CaseDetail;
  reload: () => Promise<void>;
  /** Show "what leaves the computer" (or send right away when the policy allows). Resolves
   *  once the answer is in (true) or the psychologist went back without sending (false); the
   *  review closes as soon as she sends, and the job shows its progress meanwhile. */
  review: (r: ReviewRequest) => Promise<boolean>;
  /** Sort the materials not sorted yet into sections (D-022). */
  sort: () => Promise<void>;
  /** Requests on their way to Claude and back. */
  jobs: Job[];
  /** `then`: what the screen does with the answer. A failure that offers a retry can then be
   *  sent again from the strip, and its answer is taken in the same way (F6). */
  track: <T>(job: { label: string; task: Task; section?: string; approval?: string }, run: () => Promise<T>, then?: (out: T) => Promise<void> | void) => Promise<T>;
  /** A draft for a section, from anywhere (the report page, the section): prepare, the
   *  review, and the writing with its progress. Resolves when it is in, or she went back. */
  draft: (sectionKey: string, instruction?: string, replaces?: string) => Promise<boolean>;
}

let jobSeq = 0;
/** "לנסות שוב" for a failure in the strip, by its id: memory only, so it survives leaving the
 *  case and coming back (as the strip does), but not a reload. */
const retries = new Map<number, () => void>();

/** The screen's `track`, for a retry from the strip after she left the case and came back. */
let currentTrack: CaseApi["track"] | null = null;
/** The screen's `review`, for "לנסות שוב" on a request that did not go (built again). */
let currentReview: CaseApi["review"] | null = null;

/** One line for the result of a sorting, in plain words. */
export function sortSummary(r: SortResult): string {
  const lead = r.demo ? "מצב הדגמה: מיון לפי מילות מפתח, בלי שליחה. " : "";
  if (r.sorted === 0) return `${lead}לא נמצאו קטעים לשייך. החומרים נשארים בסעיפי ברירת המחדל.`;
  const what = r.sorted === 1 ? "חומר אחד מוין" : `${r.sorted} חומרים מוינו`;
  const rest = r.unchanged > 0 ? ` ${r.unchanged} נשארו לפי ברירת המחדל.` : "";
  return `${lead}${what} לסעיפים. אפשר לשנות בכרטיס החומר.${rest}`;
}

type SectionState = "approved" | "pending" | "empty";

function sectionState(s: CaseDetail["sections"][number]): SectionState {
  if (s.paragraphs.some((p) => p.status === "proposed")) return "pending";
  if (s.approved) return "approved";
  return "empty";
}

export function CaseScreen({ caseId, view }: { caseId: string; view: string }) {
  const { go, fail, lockNow, status, notify, showShortcuts } = useApp();
  const ai = useAi();
  const [detail, setDetail] = useState<CaseDetail | null>(null);
  const detailRef = useRef<CaseDetail | null>(null);
  useEffect(() => {
    detailRef.current = detail;
  }, [detail]);
  const [error, setError] = useState<string | null>(null);
  const [review, setReview] = useState<{ r: ReviewRequest; done: (sent: boolean) => void; failed: (e: unknown) => void } | null>(null);
  // Kept while she is in another case or screen: the requests still running there, so they
  // show (and one section is not asked twice) when she comes back.
  const [jobs, setJobs] = useKept<Job[]>(`case:${caseId}:jobs`, []);
  const [exporting, setExporting] = useState(false);
  const [fullDraft, setFullDraft] = useState(false);
  /** "מה יצא מהמחשב": open for one section (its key), or the whole case (""). */
  const [sentFor, setSentFor] = useState<string | null>(null);
  /** A section to bring into view on the report page. */
  const [focus, setFocus] = useState<{ key: string; at: number } | null>(null);
  const [searching, setSearching] = useState(false);
  const [throwing, setThrowing] = useState(false);

  // Sections written side by side reload at about the same time: an answer that arrives
  // after a newer one is older, and never puts back a section that is already there.
  const reloadSeq = useRef(0);
  const reload = useCallback(async () => {
    const seq = ++reloadSeq.current;
    try {
      const d = await ipc.caseDetail(caseId);
      if (seq === reloadSeq.current) setDetail(d);
    } catch (e) {
      setError(fail(e as never));
    }
  }, [caseId, fail]);

  useEffect(() => {
    let alive = true;
    ipc
      .caseDetail(caseId)
      .then((d) => alive && setDetail(d))
      .catch((e: unknown) => alive && setError(fail(e as never)));
    return () => {
      alive = false;
    };
  }, [caseId, fail]);

  // Where she is now: a job that ends while she works elsewhere says so where she is.
  const viewRef = useRef(view);
  useEffect(() => {
    viewRef.current = view;
  }, [view]);
  // Alt+↓ / Alt+↑: the next or previous section, on the report page or section by section (UX-5).
  const focusRef = useRef(focus);
  useEffect(() => {
    focusRef.current = focus;
  }, [focus]);
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      const down = isCombo(e, KEYS.nextSection);
      if (!down && !isCombo(e, KEYS.prevSection)) return;
      const d = detailRef.current;
      const v = viewRef.current;
      const keys = (d?.sections ?? []).filter((s) => s.key !== "signature").map((s) => s.key);
      if (!keys.length || (v !== "report" && !keys.includes(v))) return;
      e.preventDefault();
      const from = keys.includes(v) ? v : focusRef.current?.key;
      const at = from ? keys.indexOf(from) + (down ? 1 : -1) : 0;
      const key = keys[Math.max(0, Math.min(keys.length - 1, at))] ?? keys[0] ?? "";
      if (v === "report") {
        setFocus({ key, at: Date.now() });
      } else {
        go({ name: "case", id: caseId, view: key });
      }
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [caseId, go]);

  const [failures, setFailures] = useKept<{ id: number; label: string; section?: string; approval?: string; again?: string; message: string }[]>(`case:${caseId}:failures`, []);
  const track = useCallback(
    async <T,>(job: { label: string; task: Task; section?: string; approval?: string }, run: () => Promise<T>, then?: (out: T) => Promise<void> | void): Promise<T> => {
      const j: Job = { ...job, id: ++jobSeq, started: Date.now(), estimate: estimateMs(job.task, status.speed, status.demo_mode) };
      setJobs((js) => [...js, j]);
      let answered = false;
      try {
        const out = await run();
        answered = true;
        if (!status.demo_mode) recordDuration(job.task, status.speed, Date.now() - j.started);
        const title = detailRef.current?.sections.find((s) => s.key === job.section)?.title;
        if (job.section && viewRef.current !== job.section && viewRef.current !== "report" && title) notify(`הטיוטה ל"${title}" מוכנה לאישור.`);
        // The same request sent again and answered: its earlier failure is over.
        if (job.approval) setFailures((f) => f.filter((x) => x.approval !== job.approval));
        await then?.(out);
        return out;
      } catch (e) {
        // A failure taking in the answer is the screen's, not the request's.
        if (answered) throw e;
        // Never lost silently, wherever she is by then. A send the core kept the approval
        // for can go again from the strip, and its answer is taken in by the same `then` (F6).
        const err = e as UiError;
        const action = err.action;
        if (action?.kind === "retry") retries.set(j.id, () => void currentTrack?.(job, run, then).catch(() => undefined));
        // The monthly ceiling: one click raises it and sends the same request (round 26).
        if (action?.kind === "raise_cap") {
          retries.set(j.id, () => void ipc.setMonthlyCap(action.to_usd).then(() => currentTrack?.(job, run, then)).catch(() => undefined));
        }
        const again = action?.kind === "raise_cap" ? { again: `להעלות ל-$${action.to_usd} ולהמשיך` } : {};
        setFailures((f) => [...f, { id: j.id, label: job.label, ...(job.section ? { section: job.section } : {}), ...(job.approval ? { approval: job.approval } : {}), ...again, message: fail(e as never) }]);
        throw e;
      } finally {
        setJobs((js) => js.filter((x) => x.id !== j.id));
      }
    },
    [status.speed, status.demo_mode, notify, fail, setJobs, setFailures],
  );
  useEffect(() => {
    currentTrack = track;
  }, [track]);

  const startReview = useCallback(
    (r: ReviewRequest) =>
      new Promise<boolean>((resolve, reject) => {
        // Round 26: what the gate cleared goes at once; the review only when she asked for it.
        if (!status.review_always) {
          if (r.prepared.approval_id) {
            r.onSend(r.prepared.approval_id).then(() => resolve(true), reject);
            return;
          }
          // Refused after the fix loop: a line in the strip, with "לנסות שוב" (built again).
          const id = ++jobSeq;
          retries.set(id, () => void r.reprepare().then((prepared) => currentReview?.({ ...r, prepared })).catch((e: unknown) => {
            setFailures((f) => [...f, { id: ++jobSeq, label: r.title, message: fail(e as never) }]);
          }));
          setFailures((f) => [...f, { id, label: r.title, message: notSentMessage(r.prepared) }]);
          resolve(false);
          return;
        }
        // One review at a time: one that is replaced counts as "went back", so whoever
        // waited for it is never left waiting (and never stuck "busy").
        setReview((prev) => {
          if (prev) queueMicrotask(() => prev.done(false));
          return { r, done: resolve, failed: reject };
        });
      }),
    [status.review_always, setFailures, fail],
  );
  useEffect(() => {
    currentReview = startReview;
  }, [startReview]);

  const draft = useCallback(
    async (key: string, instruction?: string, replaces?: string) => {
      const section = detailRef.current?.sections.find((s) => s.key === key);
      const title = section?.title ?? key;
      const derivedKey = ["summary", "diagnoses", "recommendations", "dsm"].includes(key);
      const text = instruction ?? (derivedKey ? "כתבי טיוטה לסעיף מתוך הסעיפים שאושרו." : "כתבי טיוטה לסעיף מתוך המקורות, עם מקור לכל פסקה.");
      const prepare = () => ipc.prepareSection(caseId, key, text, replaces);
      const prepared = await prepare();
      return startReview({
        title: `טיוטה לסעיף ${title}`,
        prepared,
        reprepare: prepare,
        onSend: async (id) => {
          await track({ label: `${ai} כותב את "${title}"`, task: "draft", section: key, approval: id }, () => ipc.sendSection(id), () => reload());
        },
      });
    },
    [caseId, startReview, track, reload, ai],
  );

  const startSort = async () => {
    setError(null);
    try {
      const prepared = await ipc.prepareSort(caseId);
      await startReview({
        title: "מיון החומרים לסעיפים",
        prepared,
        reprepare: () => ipc.prepareSort(caseId),
        onSend: async (id) => {
          await track({ label: `${ai} קורא את החומרים ומשייך קטעים לסעיפים`, task: "sort", approval: id }, () => ipc.sendSort(id), async (result) => {
            await reload();
            notify(sortSummary(result));
          });
        },
      });
    } catch (e) {
      setError(fail(e as never));
    }
  };

  if (!detail) {
    return error ? (
      <main className="center-note"><ErrorLine error={error} retry={() => { setError(null); void reload(); }} /></main>
    ) : (
      <main className="skeleton-page" aria-label="טוען את התיק">
        <Skeleton lines={2} label="טוען את התיק" />
        <Skeleton card lines={5} label="טוען" />
        <Skeleton card lines={3} label="טוען" />
      </main>
    );
  }

  const api: CaseApi = { caseId, detail, reload, review: startReview, sort: startSort, jobs, track, draft };
  const unsorted = detail.routing.filter((r) => r.needs_sorting).length;
  const fed = detail.sections.filter((s) => s.source_count > 0);
  const pending = detail.sections.filter((s) => s.paragraphs.some((p) => p.status === "proposed"));
  // What "write the whole report" writes: sections from materials with nothing written yet.
  const undrafted = fed.filter((s) => s.sortable && s.paragraphs.length === 0);
  const hasMaterials = detail.inputs.length > 0;
  const pendingCount = pending.reduce((n, s) => n + s.paragraphs.filter((p) => p.status === "proposed").length, 0);
  const child = detail.identities.find((i) => i.role === "child")?.value ?? "";
  const age = ageOf(detail.meta.age, detail.meta.child_gender);
  const current = detail.sections.find((s) => s.key === view);
  // Three stages: materials, writing (the report page and each section), taking the report out.
  const stage = view === "materials" ? 1 : view === "finish" ? 3 : view === "details" ? 0 : 2;
  const toReport = (key?: string) => {
    if (key) setFocus({ key, at: Date.now() });
    go({ name: "case", id: caseId, view: "report" });
  };
  const toWriting = async () => {
    // Sorting happens on the way to writing, so each section gets only its passages.
    if (unsorted > 0 && detail.meta.consent) await startSort();
    toReport();
  };
  const next: { title: string; note: string; action: string; run: () => void } | null =
    stage === 0 || stage === 3
      ? null
      : !detail.meta.consent
        ? { title: `לפני שליחה ל-${ai} צריך לרשום את הסכמת ההורים.`, note: "אפשר להמשיך לאסוף חומרים בינתיים.", action: "רישום ההסכמה", run: () => go({ name: "case", id: caseId, view: "details" }) }
        : stage === 1
          ? hasMaterials
            ? { title: fed.length ? `יש חומר ל-${fed.length} סעיפים` : `${detail.inputs.length} חומרים בתיק`, note: unsorted > 0 ? `${ai} ימיין את החומרים (אחרי הסתרה) לסעיפים.` : "הבא: טיוטה לכל סעיף, ואת מאשרת.", action: "לכתיבת הדוח ←", run: () => void toWriting() }
            : null
          : pendingCount > 0
            ? { title: pendingCount === 1 ? "טיוטה אחת מחכה לאישור שלך" : `${pendingCount} פסקאות מחכות לאישור שלך`, note: "רק מה שאישרת נכנס לקובץ.", action: `לטיוטה ב"${pending[0]?.title ?? ""}" ←`, run: () => toReport(pending[0]?.key) }
            : unsorted > 0
              ? { title: unsorted === 1 ? "חומר אחד עוד לא מוין לסעיפים" : `${unsorted} חומרים עוד לא מוינו לסעיפים`, note: `${ai} ימיין אותם (אחרי הסתרה) לסעיפים.`, action: "מיון החומרים", run: () => void startSort() }
              : undrafted.length > 0
                ? { title: `${undrafted.length} סעיפים עם חומר עוד לא נכתבו`, note: "כל סעיף נכתב רק מהחומרים שלו, ואת רואה מה יוצא.", action: `✦ לכתוב את ${undrafted.length} הסעיפים`, run: () => setFullDraft(true) }
                : fed.length > 0 && fed.every((s) => s.approved)
                  ? { title: "כל הסעיפים שיש להם חומר אושרו", note: "הדוח יוצא כ-Word מוגן בסיסמה.", action: "להוצאת הדוח ←", run: () => go({ name: "case", id: caseId, view: "finish" }) }
                  : null;
  // A section shows its own job, and so does the report page; the rest run in the strip.
  const elsewhere = jobs.filter((j) => !j.section || (j.section !== view && view !== "report"));
  const stages: { n: number; label: string; view: string; done: boolean }[] = [
    { n: 1, label: "חומרים", view: "materials", done: hasMaterials && unsorted === 0 },
    { n: 2, label: "כתיבה", view: "report", done: fed.length > 0 && fed.every((s) => s.approved) && pendingCount === 0 },
    { n: 3, label: "הוצאת הדוח", view: "finish", done: false },
  ];
  const more: MenuItem[] = [
    { label: "פרטים ושמות להסתרה", run: () => go({ name: "case", id: caseId, view: "details" }) },
    { label: current ? `מה יצא מהמחשב בסעיף "${current.title}"` : "מה יצא מהמחשב בתיק הזה", run: () => setSentFor(current?.key ?? "") },
    { label: `כתיבת כל הדוח עם ${ai}`, run: () => setFullDraft(true) },
    ...(showShortcuts ? [{ label: "קיצורי מקלדת (F1)", run: showShortcuts }] : []),
    { label: "נעילה (Ctrl+L)", run: () => void lockNow() },
  ];

  return (
    <div className="case case-v2">
      <header className="case-bar">
        <button type="button" className="case-back" onClick={() => go({ name: "cases" })}>→ תיקים</button>
        <button type="button" className="case-id" title="פרטים ושמות להסתרה" onClick={() => go({ name: "case", id: caseId, view: "details" })}>
          <span className="case-child" title={child || detail.meta.code}>{child || detail.meta.code}</span>
          {age && <span className="case-age">{age}</span>}
          <span className={detail.meta.consent ? "chip chip-ok" : "chip chip-warn"}>{detail.meta.consent ? "✓ הסכמה" : "חסרה הסכמה"}</span>
          {detail.meta.follows && <span className="chip chip-sand">מעקב</span>}
        </button>
        <nav className="stages" aria-label="שלבי התיק">
          {stages.map((st) => (
            <button key={st.n} type="button" className={stage === st.n ? "stage on" : "stage"} aria-current={stage === st.n ? "step" : undefined}
              onClick={() => (st.n === 2 ? toReport() : go({ name: "case", id: caseId, view: st.view }))}>
              <span className={st.done && stage !== st.n ? "stage-dot done" : "stage-dot"} aria-hidden="true">{st.done && stage !== st.n ? "✓" : st.n}</span>
              {st.label}
            </button>
          ))}
        </nav>
        {status.demo_mode && <span className="chip chip-sand" title="לא הוגדר מפתח API בהגדרות">מצב הדגמה</span>}
        <button type="button" className="btn" onClick={() => setSearching(true)}>חיפוש</button>
        <button type="button" className="btn" onClick={() => go({ name: "consult", caseId })}>התייעצות</button>
        <ActionsMenu items={more} label="עוד פעולות: פרטים ושמות, כתיבת כל הדוח, נעילה" />
      </header>
      <ErrorLine error={error} />
      {detail.sample && (
        <SampleGuide view={view} busy={throwing}
          onNext={(from) => (from === "details" ? go({ name: "case", id: caseId, view: "materials" }) : from === "materials" ? toReport() : go({ name: "case", id: caseId, view: "finish" }))}
          onDelete={() => {
            setThrowing(true);
            ipc.deleteCase(caseId).then(() => go({ name: "cases" }), (e: unknown) => { setThrowing(false); setError(fail(e as never)); });
          }} />
      )}
      {(elsewhere.length > 0 || failures.length > 0) && (
        <section className="jobs-strip" aria-label={`${ai} עובד`}>
          {elsewhere.map((j) => <ProgressLine key={j.id} started={j.started} estimate={j.estimate} label={j.label} approval={j.approval} />)}
          {failures.map((f) => (
            <div key={f.id} className="job-failed" role="alert">
              <span className="grow"><b>{f.label}: לא הצליח.</b> {f.message} שום דבר לא נכנס לדוח.</span>
              {f.section && f.section !== view && (
                <button type="button" className="btn btn-small" onClick={() => toReport(f.section)}>לסעיף</button>
              )}
              {retries.has(f.id) && (
                <button type="button" className="btn btn-small" onClick={() => {
                  const again = retries.get(f.id);
                  retries.delete(f.id);
                  setFailures((all) => all.filter((x) => x.id !== f.id));
                  again?.();
                }}>{f.again ?? "לנסות שוב"}</button>
              )}
              <button type="button" className="icon-btn" aria-label="סגירה" onClick={() => {
                retries.delete(f.id);
                setFailures((all) => all.filter((x) => x.id !== f.id));
              }}><CloseIcon /></button>
            </div>
          ))}
        </section>
      )}

      <div className="case-body">
        {stage === 2 && <Contents detail={detail} jobs={jobs} view={view} onOpen={(key) => toReport(key)} onWriteAll={() => setFullDraft(true)} />}
        <div className="work">
          {view === "materials" && <MaterialsView api={api} />}
          {view === "details" && <DetailsView api={api} />}
          {view === "report" && <ReportView api={api} onWriteAll={() => setFullDraft(true)} focus={focus} />}
          {view === "finish" && <FinishView api={api} onExport={() => setExporting(true)} onOpen={(key) => toReport(key)} />}
          {searching && (
            <CaseSearch detail={detail} onClose={() => setSearching(false)}
              onOpen={(hit) => {
                setSearching(false);
                if (hit.where === "paragraph") toReport(hit.target);
                else go({ name: "case", id: caseId, view: "materials" });
              }} />
          )}
          {current && (
            <>
              <button type="button" className="link-small back-to-report" onClick={() => toReport(current.key)}>→ חזרה לדוח</button>
              <SectionWork key={current.key} api={api} section={current} />
            </>
          )}
        </div>
      </div>

      {next && (
        <footer className="next-bar" aria-label="הצעד הבא">
          <div className="grow stack" style={{ gap: 2 }}>
            <b className="next-title">{next.title}</b>
            <span className="small muted">{next.note}</span>
          </div>
          <button type="button" className="btn btn-primary btn-big" onClick={next.run}>{next.action}</button>
        </footer>
      )}

      {exporting && <ExportDialog api={api} onClose={() => setExporting(false)} />}
      {fullDraft && <ReportRunDialog api={api} onClose={() => setFullDraft(false)} />}
      {sentFor !== null && <SentHistoryDialog caseId={caseId} {...(sentFor ? { place: sentFor } : {})} onClose={() => setSentFor(null)} />}
      {/* Last, so it opens above the dialog that asked for it. */}
      {review && (
        <ReviewDialog title={review.r.title} caseId={caseId} prepared={review.r.prepared} reprepare={review.r.reprepare}
          onClose={() => {
            review.done(false);
            setReview(null);
          }}
          onSend={async (id) => {
            // The review closes as soon as she sends; the progress shows where the work is.
            const current = review;
            setReview(null);
            current.r.onSend(id).then(() => current.done(true), current.failed);
          }} />
      )}
    </div>
  );
}

/** The report's table of contents: every section with its state, one click to it on the page. */
function Contents(props: { detail: CaseDetail; jobs: Job[]; view: string; onOpen: (key: string) => void; onWriteAll: () => void }) {
  const { detail, jobs, view } = props;
  const content = detail.sections.filter((s) => s.key !== "signature");
  const approved = content.filter((s) => s.approved && !s.paragraphs.some((p) => p.status === "proposed")).length;
  const waiting = content.filter((s) => s.paragraphs.some((p) => p.status === "proposed")).length;
  const empty = content.filter((s) => s.paragraphs.length === 0);
  // The same sections "write the whole report" writes (the summary and the like come after
  // approval, from the approved sections).
  const writable = empty.filter((s) => s.sortable && s.source_count > 0).length;
  const parts = Array.from(new Set(detail.sections.map((s) => s.part)));
  return (
    <aside className="contents" aria-label="תוכן הדוח">
      <div className="contents-head">
        <span className="contents-label">תוכן הדוח</span>
        <div className="contents-bar" aria-hidden="true"><div style={{ width: `${Math.round((approved / Math.max(content.length, 1)) * 100)}%` }} /></div>
        <span className="small muted">{approved} מאושרים · {waiting} לאישור · {empty.length} ריקים</span>
      </div>
      <nav className="contents-list">
        {parts.map((part) => (
          <div key={part} className="contents-part">
            <span className="contents-part-title">{part}</span>
            {detail.sections.filter((s) => s.part === part).map((s) => {
              const writing = jobs.some((j) => j.section === s.key);
              const st = writing ? "writing" : sectionState(s);
              return (
                <button key={s.key} type="button" className={`contents-item st-${st}`} aria-current={view === s.key ? "page" : undefined} onClick={() => props.onOpen(s.key)}>
                  <span className="dot" aria-hidden="true" />
                  <span className="grow">{s.title}</span>
                  {st === "pending" && <span className="contents-tag">לאישור</span>}
                  {st === "writing" && <span className="contents-tag writing">כותב…</span>}
                  <span className="visually-hidden">{st === "approved" ? "אושר" : st === "pending" ? "ממתין לאישור" : st === "writing" ? "נכתב עכשיו" : "ריק"}</span>
                </button>
              );
            })}
          </div>
        ))}
      </nav>
      {writable > 0 && (
        <button type="button" className="btn contents-write" onClick={props.onWriteAll}>✦ לכתוב את {writable} הריקים</button>
      )}
    </aside>
  );
}
