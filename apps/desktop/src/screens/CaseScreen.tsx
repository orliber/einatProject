import { useCallback, useEffect, useRef, useState } from "react";
import { useApp } from "../App";
import { sortInParts } from "./case/sortParts";
import { ipc, type CaseDetail, type Prepared, type SortResult, type UiError } from "../ipc/client";
import { ReviewDialog } from "../components/ReviewDialog";
import { ExportDialog } from "../components/ExportDialog";
import { ReportRunDialog } from "../components/ReportRunDialog";
import { ProgressLine, estimateMs, recordDuration, timed, WaitLine, type Task } from "../components/Progress";
import { ErrorLine, CloseIcon, Skeleton, dialogOpen } from "../components/ui";
import { UpdateChip } from "../components/Update";
import { ActionsMenu, type MenuItem } from "../components/Menu";
import { SampleGuide } from "./case/SampleGuide";
import { CaseSearch } from "./case/CaseSearch";
import { ageOf } from "../components/AgeField";
import { MaterialsView } from "./case/MaterialsView";
import { SectionWork } from "./case/SectionWork";
import { DetailsView } from "./case/DetailsView";
import { SentHistoryDialog } from "../components/SentHistory";
import { ReportView, firstFocus } from "./case/ReportView";
import { FinishView } from "./case/FinishView";
import { isCombo, isTyping, KEYS, MOD } from "../shortcuts";
import { useKept } from "../kept";
import "./CaseScreen.css";
import { useAi } from "../ai";
import { notSentMessage } from "../sending";
import { useQuery } from "@tanstack/react-query";
import { forgetRun, isRunning, runLine, runWholeReport, showingCase, stopRun, useRun } from "./case/reportRun";

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
  draft: (sectionKey: string, instruction?: string, replaces?: string, points?: boolean) => Promise<boolean>;
}

let jobSeq = 0;
/** "דף מלא", kept while the program runs. */
let fullPageOn = false;
/** "לנסות שוב" for a failure in the strip, by its id: memory only, so it survives leaving the
 *  case and coming back (as the strip does), but not a reload. */
const retries = new Map<number, () => void>();

/** The screen's `track`, for a retry from the strip after she left the case and came back. */
let currentTrack: CaseApi["track"] | null = null;
/** The screen's `review`, for "לנסות שוב" on a request that did not go (built again). */
let currentReview: CaseApi["review"] | null = null;
/** Where she is now, in which case: an answer that lands after she left says so where she is. */
let showing: { caseId: string; view: string } | null = null;
/** How long the line of a finished run stays (it then gives its place back to the page). */
const RUN_LINE_MS = 8000;

/** One line for the result of a sorting, in plain words. */
export function sortSummary(r: SortResult): string {
  const lead = r.demo ? "מצב הדגמה: מיון לפי מילות מפתח, בלי שליחה. " : "";
  if (r.sorted === 0) return `${lead}לא נמצאו קטעים לשייך. החומרים נשארים בסעיפי ברירת המחדל.`;
  const what = r.sorted === 1 ? "חומר אחד מוין" : `${r.sorted} חומרים מוינו`;
  const rest = r.unchanged > 0 ? ` ${r.unchanged} נשארו לפי ברירת המחדל.` : "";
  // A line the check left out (D-082) may be why a material stayed where it was.
  const left = r.left_out ? " קטע מהחומרים לא נשלח, כי הבדיקה לא הייתה בטוחה שאין בו שם." : "";
  return `${lead}${what} לסעיפים. אפשר לשנות בכרטיס החומר.${rest}${left}`;
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
  const [review, setReview] = useState<{ seq: number; r: ReviewRequest; done: (sent: boolean) => void; failed: (e: unknown) => void } | null>(null);
  // Kept while she is in another case or screen: the requests still running there, so they
  // show (and one section is not asked twice) when she comes back.
  const [jobs, setJobs] = useKept<Job[]>(`case:${caseId}:jobs`, []);
  const [exporting, setExporting] = useState(false);
  const [fullDraft, setFullDraft] = useState(false);
  /** "מה יצא מהמחשב": open for one section (its key), or the whole case (""). */
  const [sentFor, setSentFor] = useState<string | null>(null);
  /** The section in focus on the report page (round 48); `move` brings it into view. */
  const [focus, setFocus] = useState<{ key: string; at: number; move: boolean } | null>(null);
  const [searching, setSearching] = useState(false);
  const [throwing, setThrowing] = useState(false);
  // Round 42: "full page" hides the contents beside the report, for every case until she
  // turns it off (memory only).
  const [fullPage, setFullPageState] = useState(fullPageOn);
  const setFullPage = (on: boolean) => {
    fullPageOn = on;
    setFullPageState(on);
  };

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
    showing = { caseId, view };
    return () => {
      showing = null;
    };
  }, [caseId, view]);
  // Alt+↓ / Alt+↑: the next or previous section, on the report page or section by section (UX-5).
  const focusRef = useRef(focus);
  useEffect(() => {
    focusRef.current = focus;
  }, [focus]);
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      const down = isCombo(e, KEYS.nextSection);
      if (!down && !isCombo(e, KEYS.prevSection)) return;
      // Not under a dialog, and not while she types in a field.
      if (dialogOpen() || isTyping(e.target)) return;
      const d = detailRef.current;
      const v = viewRef.current;
      const keys = (d?.sections ?? []).filter((s) => s.key !== "signature").map((s) => s.key);
      if (!keys.length || (v !== "report" && !keys.includes(v))) return;
      e.preventDefault();
      const from = keys.includes(v) ? v : focusRef.current?.key ?? firstFocus(d?.sections ?? []);
      const at = from ? keys.indexOf(from) + (down ? 1 : -1) : 0;
      const key = keys[Math.max(0, Math.min(keys.length - 1, at))] ?? keys[0] ?? "";
      if (v === "report") {
        setFocus({ key, at: Date.now(), move: true });
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
        const here = showing?.caseId === caseId ? showing.view : null;
        if (job.section && here !== job.section && here !== "report" && title) notify(`הטיוטה ל"${title}" מוכנה לאישור.`);
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
    [caseId, status.speed, status.demo_mode, notify, fail, setJobs, setFailures],
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
          return { seq: ++jobSeq, r, done: resolve, failed: reject };
        });
      }),
    [status.review_always, setFailures, fail],
  );
  useEffect(() => {
    currentReview = startReview;
  }, [startReview]);

  const draft = useCallback(
    async (key: string, instruction?: string, replaces?: string, points = false) => {
      const section = detailRef.current?.sections.find((s) => s.key === key);
      const title = section?.title ?? key;
      const derivedKey = ["summary", "diagnoses", "recommendations", "dsm"].includes(key);
      const text = instruction ?? (derivedKey ? "כתבי טיוטה לסעיף מתוך הסעיפים שאושרו." : "כתבי טיוטה לסעיף מתוך המקורות, עם מקור לכל פסקה.");
      // Her own section (round 30): only her points go, worded in her style (D-140).
      const prepare = () => points ? ipc.prepareFromPoints(caseId, key, text) : ipc.prepareSection(caseId, key, text, replaces);
      const prepared = await timed("prepare", prepare(), "section");
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

  /** D-139: every clean draft approved in one call; a draft with a note waits for her eye.
   *  The core checks again which ones are still clean; a second press waits for the first. */
  const [approvingAll, setApprovingAll] = useState(false);
  const approveAll = async (ids: string[], held: number) => {
    if (approvingAll) return;
    setError(null);
    setApprovingAll(true);
    try {
      const n = await ipc.approveCleanParagraphs(caseId, ids);
      await reload();
      // A finished run's line ("N סעיפים מחכים לאישורך") is no longer true; one with something
      // to fix (a failure, a check that did not run) keeps its "לנסות שוב".
      if (runOver) forgetRun(caseId);
      const waiting = ids.length + held - n;
      notify(waiting > 0 ? `${n} פסקאות אושרו. ${waiting} מחכות לעין שלך.` : `${n} פסקאות אושרו.`);
    } catch (e) {
      setError(fail(e as never));
    } finally {
      setApprovingAll(false);
    }
  };

  /** `quiet`: the reading that runs by itself after a material was added (round 31): when
   *  there is nothing to read, or it cannot be built now, it says nothing; the button stays. */
  const [preparingSort, setPreparingSort] = useState(false);
  /** Resolves true when the sorting was sent (or the run does it), false when she went back. */
  const startSort = async (quiet = false): Promise<boolean> => {
    // While the whole report is written, its run does the sorting: never a second request.
    if (isRunning(caseId)) return true;
    if (!quiet) setError(null);
    setPreparingSort(true);
    try {
      // D-134: one request per material, side by side; the materials fill in as each lands.
      const { sent, result } = await sortInParts(
        { caseId, review: startReview, track, reload },
        { title: "מיון החומרים לסעיפים", label: `${ai} קורא את החומרים ומשייך קטעים לסעיפים`, quiet, prepared: () => setPreparingSort(false) },
      ).finally(() => setPreparingSort(false));
      if (sent) notify(sortSummary(result));
      return sent;
    } catch (e) {
      if (!quiet) setError(fail(e as never));
      return false;
    }
  };

  // Round 31: a material added while she works is read (sorted and summed up) by itself, when
  // requests go without the review screen. Not on opening a case: only after an addition. A
  // material added while one reading runs (several files at once, D-147) is read right after
  // it, so the report page has everything sorted when she gets there.
  const reading = useRef<{ count: number; busy: boolean; again: boolean }>({ count: -1, busy: false, again: false });
  const inputCount = detail?.inputs.length ?? -1;
  const unreadNow = detail?.routing.filter((r) => r.needs_sorting).length ?? 0;
  const consented = Boolean(detail?.meta.consent);
  useEffect(() => {
    const before = reading.current.count;
    reading.current.count = inputCount;
    if (before < 0 || inputCount <= before) return;
    if (status.review_always || !consented || unreadNow === 0) return;
    if (reading.current.busy) {
      reading.current.again = true;
      return;
    }
    const readAll = () => {
      reading.current.busy = true;
      reading.current.again = false;
      void startSort(true).finally(() => {
        reading.current.busy = false;
        // What was added meanwhile; with nothing left to read the core says so quietly.
        if (reading.current.again) readAll();
      });
    };
    readAll();
    // startSort is rebuilt on every render; the count decides when this runs.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [inputCount]);

  // Round 41: "write the whole report" runs in the background, in one line.
  const verifyOn = useQuery({ queryKey: ["verify-stage"], queryFn: ipc.verifyStageOn, enabled: !status.review_always }).data === true;
  const run = useRun(caseId);
  const api: CaseApi | null = detail ? { caseId, detail, reload, review: startReview, sort: () => startSort().then(() => undefined), jobs, track, draft } : null;
  // The run sees this screen, wherever it was started from (she may have left and come back).
  useEffect(() => {
    if (!api) return;
    showingCase(caseId, api);
    return () => showingCase(caseId, null);
  });
  // The page follows the run: each section written, each check and each phase reloads it, on
  // the screen showing the case now (not the one the run was started from).
  const runStep = run ? `${run.phase}:${run.done}:${run.checked}` : "";
  useEffect(() => {
    if (runStep) void reload();
  }, [runStep, reload]);
  // A finished run's line steps aside after a moment; one with something to fix stays.
  const runOver = run !== undefined && !isRunning(caseId) && run.failed === 0 && run.uncheckedFailed === 0 && run.error === null;
  useEffect(() => {
    if (!runOver) return;
    const t = setTimeout(() => forgetRun(caseId), RUN_LINE_MS);
    return () => clearTimeout(t);
  }, [runOver, caseId]);

  if (!detail || !api) {
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

  // With "show me every request" on (advanced), the window that shows each one; otherwise the
  // background run.
  const writeAll = () => (status.review_always ? setFullDraft(true) : void runWholeReport(api, { verify: verifyOn, ai }));
  const running = isRunning(caseId);
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
    if (key) setFocus({ key, at: Date.now(), move: true });
    go({ name: "case", id: caseId, view: "report" });
  };
  const toWriting = async () => {
    // Sorting happens on the way to writing, so each section gets only its passages.
    // "חזרה לעריכה" in the review means: stay here, nothing was sent.
    if (unsorted > 0 && detail.meta.consent && !(await startSort())) return;
    toReport();
  };
  // The section in focus on the report page: the one she chose, else the first draft waiting.
  const focusKey = focus && detail.sections.some((s) => s.key === focus.key) ? focus.key : firstFocus(detail.sections);
  const next: { title: string; note: string; action: string; run: () => void; sends?: boolean } | null =
    stage === 0 || stage === 3
      ? null
      : !detail.meta.consent
        ? { title: `לפני שליחה ל-${ai} צריך לרשום את הסכמת ההורים.`, note: "", action: "רישום ההסכמה", run: () => go({ name: "case", id: caseId, view: "details" }) }
        // Round 49: on the materials page, "לכתיבת הדוח ←" sits at the top of the page; round 48:
        // on the report page what is next is in the contents beside it (in "דף מלא", with the
        // contents hidden, the bar comes back).
        : stage === 1 || (view === "report" && !fullPage)
          ? null
          : pendingCount > 0
            ? { title: pendingCount === 1 ? "טיוטה אחת מחכה לאישור שלך" : `${pendingCount} פסקאות מחכות לאישור שלך`, note: "", action: `לטיוטה ב"${pending[0]?.title ?? ""}" ←`, run: () => toReport(pending[0]?.key) }
            : unsorted > 0
              ? { title: unsorted === 1 ? "חומר אחד עוד לא מוין לסעיפים" : `${unsorted} חומרים עוד לא מוינו לסעיפים`, note: "", action: "מיון החומרים", run: () => void startSort(), sends: true }
              : undrafted.length > 0
                ? { title: `${undrafted.length} סעיפים עם חומר עוד לא נכתבו`, note: "", action: `✦ לכתוב את ${undrafted.length} הסעיפים`, run: writeAll, sends: true }
                : fed.length > 0 && fed.every((s) => s.approved)
                  ? { title: "כל הסעיפים שיש להם חומר אושרו", note: "", action: "להוצאת הדוח ←", run: () => go({ name: "case", id: caseId, view: "finish" }) }
                  : null;
  // A section shows its own job, and so does the report page; the rest run in the strip.
  // While the whole report is written, its one line stands for its requests.
  const elsewhere = running ? [] : jobs.filter((j) => !j.section || (j.section !== view && view !== "report"));
  const stages: { n: number; label: string; view: string; done: boolean }[] = [
    { n: 1, label: "חומרים", view: "materials", done: hasMaterials && unsorted === 0 },
    { n: 2, label: "כתיבה", view: "report", done: fed.length > 0 && fed.every((s) => s.approved) && pendingCount === 0 },
    { n: 3, label: "הוצאת הדוח", view: "finish", done: false },
  ];
  const more: MenuItem[] = [
    { label: "פרטים ושמות להסתרה", run: () => go({ name: "case", id: caseId, view: "details" }) },
    { label: current ? `מה יצא מהמחשב בסעיף "${current.title}"` : "מה יצא מהמחשב בתיק הזה", run: () => setSentFor(current?.key ?? "") },
    { label: `כתיבת כל הדוח עם ${ai}`, run: writeAll },
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
        {status.demo_mode && <span className="chip chip-sand case-demo" title="לא הוגדר מפתח API בהגדרות">מצב הדגמה</span>}
        <UpdateChip />
        {stage === 2 && (
          <button type="button" className="btn" aria-pressed={fullPage} title={fullPage ? "להחזיר את תוכן הדוח שבצד" : "להסתיר את תוכן הדוח שבצד"} onClick={() => setFullPage(!fullPage)}>
            דף מלא
          </button>
        )}
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
      {(elsewhere.length > 0 || failures.length > 0 || run || preparingSort) && (
        <section className="jobs-strip" aria-label={`${ai} עובד`}>
          {run && (
            <div className="run-line" role="status">
              <span className="grow">{runLine(run)}</span>
              {running && (
                <button type="button" className="btn btn-small" disabled={run.stopping} onClick={() => stopRun(caseId)}>לעצור</button>
              )}
              {!running && run.error && (
                <button type="button" className="btn btn-small" onClick={() => {
                  forgetRun(caseId);
                  writeAll();
                }}>לנסות שוב</button>
              )}
              {!running && (
                <button type="button" className="icon-btn" aria-label="סגירה" onClick={() => forgetRun(caseId)}><CloseIcon /></button>
              )}
            </div>
          )}
          {preparingSort && <WaitLine task="prepare" speed="sort" label="מכינה את בקשת המיון: מסתירה שמות ובודקת מה יוצא" />}
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
        {stage === 2 && !fullPage && (
          <Contents detail={detail} jobs={jobs} view={view} current={view === "report" ? focusKey : null} onOpen={(key) => toReport(key)} onWriteAll={running ? undefined : writeAll}
            onSort={running ? undefined : () => void startSort()} onConsent={() => go({ name: "case", id: caseId, view: "details" })}
            onFinish={() => go({ name: "case", id: caseId, view: "finish" })}
            onApproveAll={(ids, held) => void approveAll(ids, held)} approvingAll={approvingAll} />
        )}
        <div className="work">
          {view === "materials" && <MaterialsView api={api} onWrite={() => void toWriting()} />}
          {view === "details" && <DetailsView api={api} />}
          {view === "report" && <ReportView api={api} onWriteAll={writeAll} focus={focus} current={focusKey}
            onPick={(key, move = false) => setFocus({ key, at: Date.now(), move })} />}
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
            {next.note && <span className="small muted">{next.note}</span>}
          </div>
          {/* While the whole report is written, nothing here sends a second request. */}
          <button type="button" className="btn btn-primary btn-big" disabled={running && next.sends === true} onClick={next.run}>{next.action}</button>
        </footer>
      )}

      {exporting && <ExportDialog api={api} onClose={() => setExporting(false)} />}
      {fullDraft && <ReportRunDialog api={api} onClose={() => setFullDraft(false)} />}
      {sentFor !== null && <SentHistoryDialog caseId={caseId} {...(sentFor ? { place: sentFor } : {})} onClose={() => setSentFor(null)} />}
      {/* Last, so it opens above the dialog that asked for it. */}
      {review && (
        <ReviewDialog key={review.seq} title={review.r.title} caseId={caseId} prepared={review.r.prepared} reprepare={review.r.reprepare}
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

/** The report's table of contents (round 48): every section with its state, one click to it on
 *  the page. At the top how far the report is, at the bottom what is left and the two keys. */
function Contents(props: {
  detail: CaseDetail;
  jobs: Job[];
  view: string;
  /** The section in focus on the report page. */
  current: string | null;
  onOpen: (key: string) => void;
  /** Absent while the whole report is written: nothing here sends a second request. */
  onWriteAll: (() => void) | undefined;
  onSort: (() => void) | undefined;
  onConsent: () => void;
  onFinish: () => void;
  /** Every draft waiting, approved in one call: the ones with nothing to look at (D-139). */
  onApproveAll: (ids: string[], held: number) => void;
  approvingAll: boolean;
}) {
  const { detail, jobs, view } = props;
  const ai = useAi();
  const content = detail.sections.filter((s) => s.key !== "signature");
  const approved = content.filter((s) => s.approved && !s.paragraphs.some((p) => p.status === "proposed")).length;
  const waiting = content.filter((s) => s.paragraphs.some((p) => p.status === "proposed")).length;
  const empty = content.filter((s) => s.paragraphs.length === 0);
  // The same sections "write the whole report" writes (the summary and the like come after
  // approval, from the approved sections).
  const writable = empty.filter((s) => s.sortable && s.source_count > 0).length;
  const unsorted = detail.routing.filter((r) => r.needs_sorting).length;
  const fed = detail.sections.filter((s) => s.source_count > 0);
  const done = fed.length > 0 && fed.every((s) => s.approved) && waiting === 0;
  // A new wording beside an approved paragraph, or a paragraph with a note (from the check
  // against the sources, or a name the answer held), still waits for her eyes.
  const proposed = content.flatMap((s) => s.paragraphs.filter((p) => p.status === "proposed" && !p.replaces));
  const readyIds = proposed.filter((p) => p.warnings.length === 0).map((p) => p.id);
  const held = proposed.length - readyIds.length;
  // Counted as the line above counts them ("N טיוטות מחכות לך"): one draft per section, however
  // many paragraphs it has; the message after the press counts the paragraphs.
  const readyDrafts = content.filter((s) => s.paragraphs.some((p) => readyIds.includes(p.id))).length;
  const parts = Array.from(new Set(detail.sections.map((s) => s.part)));
  // Sections marked "חסר חומר" now; sorting may still give some of them material.
  const missing = empty.filter((s) => s.sortable && s.source_count === 0 && s.mode !== "self").length;
  return (
    <aside className="contents" aria-label="תוכן הדוח">
      <div className="contents-head">
        <span className="contents-label">תוכן הדוח</span>
        <span className="contents-count"><b>{approved}</b> / {content.length} מאושרים</span>
        <div className="contents-bar" aria-hidden="true"><div style={{ width: `${Math.round((approved / Math.max(content.length, 1)) * 100)}%` }} /></div>
        {waiting > 0 && <span className="contents-waiting">{waiting === 1 ? "טיוטה אחת מחכה לך" : `${waiting} טיוטות מחכות לך`}</span>}
      </div>
      <nav className="contents-list">
        {parts.map((part) => (
          <div key={part} className="contents-part">
            <span className="contents-part-title">{part}</span>
            {detail.sections.filter((s) => s.part === part).map((s) => {
              const writing = jobs.some((j) => j.section === s.key);
              const state = sectionState(s);
              const st = writing ? "writing" : state === "empty" && s.mode === "self" ? "mine" : state;
              const noMaterial = st === "empty" && s.sortable && s.source_count === 0;
              const here = view === s.key || props.current === s.key;
              return (
                <button key={s.key} type="button" className={`contents-item st-${st}`} aria-current={here ? "location" : undefined} onClick={() => props.onOpen(s.key)}>
                  <span className="dot" aria-hidden="true" />
                  <span className="grow">{s.title}</span>
                  {st === "pending" && <span className="contents-tag">לאישור</span>}
                  {st === "writing" && <span className="contents-tag writing">כותב…</span>}
                  {noMaterial && <span className="contents-tag missing">חסר חומר</span>}
                  <span className="visually-hidden">{st === "approved" ? "אושר" : st === "pending" ? "ממתין לאישור" : st === "writing" ? "נכתב עכשיו" : st === "mine" ? "את כותבת" : "ריק"}</span>
                </button>
              );
            })}
          </div>
        ))}
      </nav>
      <div className="contents-foot">
        {/* D-114: material not sorted yet goes into no draft, so it is said above the writing,
            in words, with what to do (not a small link under it). While the whole report is
            written its run sorts first, so the alert steps aside. */}
        {unsorted > 0 && props.onSort && (
          <div className="contents-alert" role="status">
            <b>{unsorted === 1 ? "חומר אחד עוד לא מוין לסעיפים" : `${unsorted} חומרים עוד לא מוינו לסעיפים`}</b>
            {detail.meta.consent ? (
              <span>
                חומר שלא מוין לא נכנס לטיוטות.
                {missing > 0 ? " אחרי המיון יתברר גם איפה באמת חסר חומר." : ""}
              </span>
            ) : (
              <span>המיון שולח את החומרים, בלי שמות, ל-{ai}. קודם רושמים את הסכמת ההורים.</span>
            )}
            <button type="button" className="btn btn-small" onClick={detail.meta.consent ? props.onSort : props.onConsent}>
              {detail.meta.consent ? "למיין את החומרים" : "רישום ההסכמה"}
            </button>
          </div>
        )}
        {readyDrafts > 1 && (
          <button type="button" className="btn btn-primary" disabled={props.approvingAll} onClick={() => props.onApproveAll(readyIds, held)}>
            ✓ לאשר את כל הטיוטות ({readyDrafts})
          </button>
        )}
        {writable > 0 && props.onWriteAll && (
          <button type="button" className="btn contents-write" onClick={props.onWriteAll}>✦ לכתוב את {writable} הריקים</button>
        )}
        {done && <button type="button" className="btn btn-primary" onClick={props.onFinish}>להוצאת הדוח ←</button>}
        <p className="contents-keys">
          <span><kbd>Enter</kbd> לאשר ולעבור הלאה</span>
          <span><kbd>{MOD}+Enter</kbd> לשנות</span>
        </p>
      </div>
    </aside>
  );
}
