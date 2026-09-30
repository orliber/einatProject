import { useCallback, useEffect, useRef, useState } from "react";
import { useApp } from "../App";
import { ipc, type CaseDetail, type Prepared, type SortResult } from "../ipc/client";
import { ReviewDialog } from "../components/ReviewDialog";
import { ExportDialog } from "../components/ExportDialog";
import { ReportRunDialog } from "../components/ReportRunDialog";
import { ProgressLine, estimateMs, recordDuration, type Task } from "../components/Progress";
import { LockIcon, ErrorLine } from "../components/ui";
import { ageWords } from "../components/AgeField";
import { MaterialsView } from "./case/MaterialsView";
import { SectionWork } from "./case/SectionWork";
import { DetailsView } from "./case/DetailsView";
import { ReportView } from "./case/ReportView";
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
  track: <T>(job: { label: string; task: Task; section?: string }, run: () => Promise<T>) => Promise<T>;
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
  const [openParts, setOpenParts] = useState<Record<string, boolean>>({});

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
    async <T,>(job: { label: string; task: Task; section?: string }, run: () => Promise<T>): Promise<T> => {
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
          await track({ label: `Claude כותב את "${title}"`, task: "draft", section: key }, () => ipc.sendSection(id));
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
          const result = await track({ label: "Claude קורא את החומרים ומשייך קטעים לסעיפים", task: "sort" }, () => ipc.sendSort(id));
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
  const hasDraft = detail.sections.some((s) => s.paragraphs.length > 0);
  const allApproved = fed.length > 0 && fed.every((s) => s.approved) && pending.length === 0;
  const steps: { label: string; done: boolean }[] = [
    { label: "חומרים", done: hasMaterials },
    { label: "טיוטה", done: hasDraft },
    { label: "אישור הפסקאות", done: allApproved },
    { label: "דוח Word", done: false },
  ];
  const currentStep = steps.findIndex((st) => !st.done);
  const next: { title: string; note: string; action: string; run: () => void } | null = !detail.meta.consent
    ? { title: "לפני שליחה ל-Claude צריך לרשום את הסכמת ההורים.", note: "אפשר להמשיך לאסוף חומרים גם בלי זה.", action: "רישום ההסכמה", run: () => go({ name: "case", id: caseId, view: "details" }) }
    : !hasMaterials
      ? null
      : pending.length > 0
        ? { title: `${pending.reduce((n, s) => n + s.paragraphs.filter((p) => p.status === "proposed").length, 0)} פסקאות מחכות לאישור שלך.`, note: `הראשונה בסעיף "${pending[0]?.title ?? ""}".`, action: "לאישור הפסקאות", run: () => go({ name: "case", id: caseId, view: pending[0]?.key ?? "materials" }) }
        : unsorted > 0
          ? { title: unsorted === 1 ? "חומר אחד עוד לא מוין לסעיפים." : `${unsorted} חומרים עוד לא מוינו לסעיפים.`, note: "Claude יקרא את החומרים אחרי הסתרת השמות, ויציע לכל סעיף רק את הקטעים שנוגעים אליו. את רואה בדיוק מה יוצא, ואפשר לשנות אחר כך.", action: "מיון החומרים לסעיפים", run: () => void startSort() }
        : undrafted.length > 0
          ? { title: `יש חומר ל-${undrafted.length} סעיפים. אפשר להכין להם טיוטה.`, note: "לפני שמשהו נשלח ל-Claude תראי בדיוק מה יוצא, ושמות יוחלפו בתפקידים.", action: `הכנת טיוטה ל-${undrafted.length} סעיפים`, run: () => setFullDraft(true) }
          : allApproved
            ? { title: "כל הסעיפים שיש להם חומר אושרו.", note: "הדוח יוצא כקובץ Word מוגן בסיסמה.", action: "הפקת דוח Word", run: () => setExporting(true) }
            : null;
  const child = detail.identities.find((i) => i.role === "child")?.value ?? "";
  const age = detail.meta.age ? `גיל ${detail.meta.age.years}:${detail.meta.age.months}` : "";
  const approvedCount = detail.sections.filter((s) => s.approved).length;
  const total = detail.sections.length;
  const parts = Array.from(new Set(detail.sections.map((s) => s.part)));
  const current = detail.sections.find((s) => s.key === view);
  const viewTitle = view === "materials" ? "חומרי התיק" : view === "details" ? "פרטי התיק ושמות" : view === "report" ? "הדוח" : current?.title ?? "";
  // A section shows its own job; everything else runs in the strip under the header.
  const elsewhere = jobs.filter((j) => !j.section || j.section !== view);

  return (
    <div className="case">
      <aside className="side" aria-label="התיק">
        <button type="button" className="side-back" onClick={() => go({ name: "cases" })}>→ כל התיקים</button>
        <button type="button" className="side-case" onClick={() => go({ name: "case", id: caseId, view: "details" })}>
          <span className="side-code">{detail.meta.code}</span>
          <span className="side-child" title={detail.meta.age ? ageWords(detail.meta.age) : undefined}>{child}{age && ` · ${age}`}</span>
          <span className={detail.meta.consent ? "side-consent" : "side-consent missing"}>
            {detail.meta.consent ? `הסכמת הורים נרשמה · ${new Date(detail.meta.consent.given_on).toLocaleDateString("he-IL")}` : "חסרה הסכמת הורים"}
          </span>
        </button>
        <nav className="side-nav" aria-label="חלקי התיק">
          <button type="button" className="side-report" aria-current={view === "report" ? "page" : undefined} onClick={() => go({ name: "case", id: caseId, view: "report" })}>
            <span>הדוח (כמו בוורד)</span><span className="side-count">{approvedCount}/{total}</span>
          </button>
          <button type="button" aria-current={view === "materials" ? "page" : undefined} onClick={() => go({ name: "case", id: caseId, view: "materials" })}>
            <span>חומרי התיק</span><span className="side-count">{detail.inputs.length}</span>
          </button>
          <button type="button" aria-current={view === "details" ? "page" : undefined} onClick={() => go({ name: "case", id: caseId, view: "details" })}>
            <span>פרטים ושמות להסתרה</span><span className="side-count">{detail.identities.length}</span>
          </button>

        </nav>
        <div className="side-progress">
          <div className="side-progress-label"><span>הדוח</span><span>{approvedCount} מתוך {total} סעיפים</span></div>
          <div className="side-bar"><div style={{ width: `${Math.round((approvedCount / Math.max(total, 1)) * 100)}%` }} /></div>
        </div>
        <div className="side-sections">
          {parts.map((part) => {
            const sections = detail.sections.filter((s) => s.part === part);
            const hasCurrent = sections.some((s) => s.key === view);
            const open = openParts[part] ?? true;
            return (
              <div key={part} className="side-part">
                <button type="button" className="side-part-title" aria-expanded={open || hasCurrent}
                  onClick={() => setOpenParts({ ...openParts, [part]: !open })}>
                  <span>{part}</span>
                  <span>{sections.filter((s) => s.approved).length}/{sections.length}</span>
                </button>
                {(open || hasCurrent) && sections.map((s) => {
                  const st = sectionState(s);
                  const pending = s.paragraphs.filter((p) => p.status === "proposed").length;
                  const writing = jobs.some((j) => j.section === s.key);
                  return (
                    <button key={s.key} type="button" className={`side-section st-${st}`}
                      aria-current={view === s.key ? "page" : undefined}
                      onClick={() => go({ name: "case", id: caseId, view: s.key })}>
                      <span className="dot" aria-hidden="true" />
                      <span className="grow">{s.title}</span>
                      {writing ? <span className="side-pending">כותב…</span> : pending > 0 && <span className="side-pending">טיוטה לאישור</span>}
                      <span className="visually-hidden">{st === "approved" ? "אושר" : st === "pending" ? "ממתין" : "ריק"}</span>
                    </button>
                  );
                })}
              </div>
            );
          })}
        </div>
        <button type="button" className="side-draft" onClick={() => setFullDraft(true)}>כתיבת כל הדוח עם Claude</button>
      </aside>

      <div className="work">
        <header className="work-head">
          <ol className="case-steps" aria-label="שלבי התיק">
            {steps.map((st, i) => (
              <li key={st.label} className={st.done ? "cs done" : i === currentStep ? "cs current" : "cs"} aria-current={i === currentStep ? "step" : undefined}>
                <span className="cs-dot" aria-hidden="true">{st.done ? "✓" : i + 1}</span>{st.label}
                {st.done && <span className="visually-hidden"> (הושלם)</span>}
              </li>
            ))}
          </ol>
          <span className="visually-hidden">{viewTitle}</span>
          {status.demo_mode && <span className="chip chip-sand" title="לא הוגדר מפתח API בהגדרות">מצב הדגמה</span>}
          <button type="button" className="btn" onClick={() => go({ name: "consult", caseId })}>התייעצות</button>
          <button type="button" className="btn btn-primary" onClick={() => setExporting(true)}>הפקת דוח Word</button>
          <button type="button" className="btn" title="נעילה (Ctrl+L)" onClick={() => void lockNow()}><LockIcon size={15} /> נעילה</button>
        </header>
        <ErrorLine error={error} />
        {(elsewhere.length > 0 || failures.length > 0) && (
          <section className="jobs-strip" aria-label="Claude עובד">
            {elsewhere.map((j) => <ProgressLine key={j.id} started={j.started} estimate={j.estimate} label={j.label} />)}
            {failures.map((f) => (
              <div key={f.id} className="job-failed" role="alert">
                <span className="grow"><b>{f.label}: לא הצליח.</b> {f.message} שום דבר לא נכנס לדוח; אפשר לנסות שוב.</span>
                {f.section && f.section !== view && (
                  <button type="button" className="btn btn-small" onClick={() => go({ name: "case", id: caseId, view: f.section ?? "materials" })}>לסעיף</button>
                )}
                <button type="button" className="icon-btn" aria-label="סגירה" onClick={() => setFailures((all) => all.filter((x) => x.id !== f.id))}>×</button>
              </div>
            ))}
          </section>
        )}
        {view === "materials" && next && (
          <section className="next-step" aria-label="הצעד הבא">
            <div className="grow stack" style={{ gap: 4 }}>
              <span className="next-eyebrow">הצעד הבא</span>
              <b className="next-title">{next.title}</b>
              <span className="muted">{next.note}</span>
            </div>
            <button type="button" className="btn btn-primary btn-big" onClick={next.run}>{next.action}</button>
          </section>
        )}
        {view === "materials" && <MaterialsView api={api} />}
        {view === "details" && <DetailsView api={api} />}
        {view === "report" && <ReportView api={api} onWriteAll={() => setFullDraft(true)} />}
        {current && <SectionWork key={current.key} api={api} section={current} />}
      </div>

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
