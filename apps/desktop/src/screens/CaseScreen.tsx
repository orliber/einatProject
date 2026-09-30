import { useCallback, useEffect, useRef, useState } from "react";
import { useApp } from "../App";
import { ipc, type CaseDetail, type Prepared, type SortResult } from "../ipc/client";
import { ReviewDialog } from "../components/ReviewDialog";
import { ExportDialog } from "../components/ExportDialog";
import { ReportRunDialog } from "../components/ReportRunDialog";
import { ProgressLine, estimateMs, recordDuration, type Task } from "../components/Progress";
import { ErrorLine } from "../components/ui";
import { ActionsMenu, type MenuItem } from "../components/Menu";
import { ageWords } from "../components/AgeField";
import { MaterialsView } from "./case/MaterialsView";
import { SectionWork } from "./case/SectionWork";
import { DetailsView } from "./case/DetailsView";
import { ReportView } from "./case/ReportView";
import { FinishView } from "./case/FinishView";
import "./CaseScreen.css";

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
  track: <T>(job: { label: string; task: Task; section?: string; approval?: string }, run: () => Promise<T>) => Promise<T>;
  /** A draft for a section, from anywhere (the report page, the section): prepare, the
   *  review, and the writing with its progress. Resolves when it is in, or she went back. */
  draft: (sectionKey: string, instruction?: string, replaces?: string) => Promise<boolean>;
}

let jobSeq = 0;

/** One line for the result of a sorting, in plain words. */
export function sortSummary(r: SortResult): string {
  const lead = r.demo ? "מצב הדגמה: המיון נעשה במחשב לפי מילות מפתח, ושום דבר לא נשלח. " : "";
  if (r.sorted === 0) return `${lead}לא נמצאו קטעים לשייך. החומרים ממשיכים להזין את הסעיפים לפי ברירת המחדל.`;
  const what = r.sorted === 1 ? "חומר אחד מוין" : `${r.sorted} חומרים מוינו`;
  const rest = r.unchanged > 0 ? ` ${r.unchanged} נשארו לפי ברירת המחדל.` : "";
  return `${lead}${what} לסעיפים. אפשר לבדוק ולשנות בכל כרטיס חומר.${rest}`;
}

type SectionState = "approved" | "pending" | "empty";

function sectionState(s: CaseDetail["sections"][number]): SectionState {
  if (s.paragraphs.some((p) => p.status === "proposed")) return "pending";
  if (s.approved) return "approved";
  return "empty";
}

export function CaseScreen({ caseId, view }: { caseId: string; view: string }) {
  const { go, fail, lockNow, status, notify } = useApp();
  const [detail, setDetail] = useState<CaseDetail | null>(null);
  const detailRef = useRef<CaseDetail | null>(null);
  useEffect(() => {
    detailRef.current = detail;
  }, [detail]);
  const [error, setError] = useState<string | null>(null);
  const [review, setReview] = useState<{ r: ReviewRequest; done: (sent: boolean) => void; failed: (e: unknown) => void } | null>(null);
  const [jobs, setJobs] = useState<Job[]>([]);
  const [exporting, setExporting] = useState(false);
  const [fullDraft, setFullDraft] = useState(false);
  /** A section to bring into view on the report page. */
  const [focus, setFocus] = useState<{ key: string; at: number } | null>(null);

  const reload = useCallback(async () => {
    try {
      setDetail(await ipc.caseDetail(caseId));
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
  const [failures, setFailures] = useState<{ id: number; label: string; section?: string; message: string }[]>([]);

  const track = useCallback(
    async <T,>(job: { label: string; task: Task; section?: string; approval?: string }, run: () => Promise<T>): Promise<T> => {
      const j: Job = { ...job, id: ++jobSeq, started: Date.now(), estimate: estimateMs(job.task, status.speed, status.demo_mode) };
      setJobs((js) => [...js, j]);
      try {
        const out = await run();
        if (!status.demo_mode) recordDuration(job.task, status.speed, Date.now() - j.started);
        const title = detailRef.current?.sections.find((s) => s.key === job.section)?.title;
        if (job.section && viewRef.current !== job.section && viewRef.current !== "report" && title) notify(`הטיוטה ל"${title}" מוכנה לאישור. היא מחכה בסעיף.`);
        return out;
      } catch (e) {
        // Never lost silently, wherever she is by then.
        setFailures((f) => [...f, { id: j.id, label: job.label, ...(job.section ? { section: job.section } : {}), message: fail(e as never) }]);
        throw e;
      } finally {
        setJobs((js) => js.filter((x) => x.id !== j.id));
      }
    },
    [status.speed, status.demo_mode, notify, fail],
  );

  const startReview = useCallback(
    (r: ReviewRequest) =>
      new Promise<boolean>((resolve, reject) => {
        const clean = r.prepared.approval_id && r.prepared.suspects.length === 0 && r.prepared.blocked.length === 0;
        if (status.review_only_suspect && clean && r.prepared.approval_id) {
          r.onSend(r.prepared.approval_id).then(() => resolve(true), reject);
          return;
        }
        // One review at a time: one that is replaced counts as "went back", so whoever
        // waited for it is never left waiting (and never stuck "busy").
        setReview((prev) => {
          if (prev) queueMicrotask(() => prev.done(false));
          return { r, done: resolve, failed: reject };
        });
      }),
    [status.review_only_suspect],
  );

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
          await track({ label: `Claude כותב את "${title}"`, task: "draft", section: key, approval: id }, () => ipc.sendSection(id));
          await reload();
        },
      });
    },
    [caseId, startReview, track, reload],
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
          const result = await track({ label: "Claude קורא את החומרים ומשייך קטעים לסעיפים", task: "sort", approval: id }, () => ipc.sendSort(id));
          await reload();
          notify(sortSummary(result));
        },
      });
    } catch (e) {
      setError(fail(e as never));
    }
  };

  if (!detail) {
    return <main className="center-note">{error ? <p className="error">{error}</p> : <span aria-busy="true" />}</main>;
  }

  const api: CaseApi = { caseId, detail, reload, review: startReview, sort: startSort, jobs, track, draft };
  const unsorted = detail.routing.filter((r) => r.needs_sorting).length;
  const fed = detail.sections.filter((s) => s.source_count > 0);
  const pending = detail.sections.filter((s) => s.paragraphs.some((p) => p.status === "proposed"));
  const undrafted = fed.filter((s) => s.paragraphs.length === 0);
  const hasMaterials = detail.inputs.length > 0;
  const pendingCount = pending.reduce((n, s) => n + s.paragraphs.filter((p) => p.status === "proposed").length, 0);
  const child = detail.identities.find((i) => i.role === "child")?.value ?? "";
  const age = detail.meta.age ? `גיל ${detail.meta.age.years}:${detail.meta.age.months}` : "";
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
        ? { title: "לפני שליחה ל-Claude צריך לרשום את הסכמת ההורים.", note: "אפשר להמשיך לאסוף חומרים גם בלי זה.", action: "רישום ההסכמה", run: () => go({ name: "case", id: caseId, view: "details" }) }
        : stage === 1
          ? hasMaterials
            ? { title: fed.length ? `יש חומר ל-${fed.length} סעיפים` : `${detail.inputs.length} חומרים בתיק`, note: unsorted > 0 ? "בדרך לכתיבה Claude יקרא את החומרים (אחרי הסתרה) ויסמן לכל סעיף רק את הקטעים שלו." : "הצעד הבא: טיוטה לכל סעיף, ואת מאשרת על הדף.", action: "לכתיבת הדוח ←", run: () => void toWriting() }
            : null
          : pendingCount > 0
            ? { title: pendingCount === 1 ? "טיוטה אחת מחכה לאישור שלך" : `${pendingCount} פסקאות מחכות לאישור שלך`, note: "רק מה שאישרת נכנס לקובץ. לחיצה על פסקה פותחת אותה לעריכה.", action: `לטיוטה ב"${pending[0]?.title ?? ""}" ←`, run: () => toReport(pending[0]?.key) }
            : unsorted > 0
              ? { title: unsorted === 1 ? "חומר אחד עוד לא מוין לסעיפים" : `${unsorted} חומרים עוד לא מוינו לסעיפים`, note: "Claude יקרא אותם אחרי הסתרה, ויסמן לכל סעיף רק את הקטעים שלו.", action: "מיון החומרים", run: () => void startSort() }
              : undrafted.length > 0
                ? { title: `${undrafted.length} סעיפים עם חומר עוד לא נכתבו`, note: "כל סעיף נכתב בנפרד, רק מהחומרים שלו, ואת רואה בדיוק מה יוצא.", action: `✦ לכתוב את ${undrafted.length} הסעיפים`, run: () => setFullDraft(true) }
                : fed.length > 0 && fed.every((s) => s.approved)
                  ? { title: "כל הסעיפים שיש להם חומר אושרו", note: "הדוח יוצא כקובץ Word מוגן בסיסמה.", action: "להוצאת הדוח ←", run: () => go({ name: "case", id: caseId, view: "finish" }) }
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
    { label: "כתיבת כל הדוח עם Claude", run: () => setFullDraft(true) },
    { label: "נעילה (Ctrl+L)", run: () => void lockNow() },
  ];

  return (
    <div className="case case-v2">
      <header className="case-bar">
        <button type="button" className="case-back" onClick={() => go({ name: "cases" })}>→ תיקים</button>
        <button type="button" className="case-id" title="פרטים ושמות להסתרה" onClick={() => go({ name: "case", id: caseId, view: "details" })}>
          <span className="case-child">{child || detail.meta.code}</span>
          {age && <span className="case-age" title={detail.meta.age ? ageWords(detail.meta.age) : undefined}>{age}</span>}
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
        <button type="button" className="btn" onClick={() => go({ name: "consult", caseId })}>התייעצות</button>
        <ActionsMenu items={more} label="עוד פעולות: פרטים ושמות, כתיבת כל הדוח, נעילה" />
      </header>
      <ErrorLine error={error} />
      {(elsewhere.length > 0 || failures.length > 0) && (
        <section className="jobs-strip" aria-label="Claude עובד">
          {elsewhere.map((j) => <ProgressLine key={j.id} started={j.started} estimate={j.estimate} label={j.label} approval={j.approval} />)}
          {failures.map((f) => (
            <div key={f.id} className="job-failed" role="alert">
              <span className="grow"><b>{f.label}: לא הצליח.</b> {f.message} שום דבר לא נכנס לדוח; אפשר לנסות שוב.</span>
              {f.section && f.section !== view && (
                <button type="button" className="btn btn-small" onClick={() => toReport(f.section)}>לסעיף</button>
              )}
              <button type="button" className="icon-btn" aria-label="סגירה" onClick={() => setFailures((all) => all.filter((x) => x.id !== f.id))}>×</button>
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
  const writable = empty.filter((s) => s.source_count > 0 || !s.sortable).length;
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
