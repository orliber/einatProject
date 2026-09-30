import { useEffect, useMemo, useState } from "react";
import { useApp } from "../App";
import { ipc, type Prepared } from "../ipc/client";
import type { CaseApi } from "../screens/CaseScreen";
import { Dialog, ErrorLine, Segments, Spinner } from "./ui";
import { aboutLeft, estimateMs, ProgressLine } from "./Progress";
import "./ReportRunDialog.css";

/** How many sections are written at the same time. */
const PARALLEL = 3;

type RowState = "ready" | "question" | "waiting" | "writing" | "done" | "failed";

/**
 * "Write the whole report with Claude", in one place: what will happen and about how long it
 * takes, the exact text that leaves (per section, after hiding), then every section written
 * side by side with its own progress. Nothing enters the report without her approval.
 */
export function ReportRunDialog({ api, onClose }: { api: CaseApi; onClose: () => void }) {
  const { fail, status, go } = useApp();
  const [items, setItems] = useState<[string, Prepared][] | null>(null);
  const [state, setState] = useState<Record<string, RowState>>({});
  const [started, setStarted] = useState<Record<string, number>>({});
  const [open, setOpen] = useState<string | null>(null);
  const [running, setRunning] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const unsorted = api.detail.routing.filter((r) => r.needs_sorting).length;
  const draftMs = estimateMs("draft", status.speed, status.demo_mode);

  function apply(list: [string, Prepared][]) {
    setItems(list);
    // A section still being written (the window was closed and opened again) is not sent twice.
    const busy = new Set(api.jobs.map((j) => j.section));
    setState(Object.fromEntries(list.map(([k, p]) => [k, busy.has(k) ? "writing" : p.approval_id ? "ready" : "question"])));
  }

  async function load() {
    setError(null);
    try {
      apply(await ipc.prepareFullDraft(api.caseId));
    } catch (e) {
      setError(fail(e as never));
    }
  }

  useEffect(() => {
    // Opening starts the work: sorting first when needed (its review shows what leaves),
    // then the drafts are prepared (after sorting, `load` runs again).
    if (unsorted > 0) void sortFirst();
    else ipc.prepareFullDraft(api.caseId).then(apply).catch((e: unknown) => setError(fail(e as never)));
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  const title = (key: string) => api.detail.sections.find((s) => s.key === key)?.title ?? key;
  /** A section written by an earlier opening of this window is done once its job ends. */
  const stateOf = (key: string): RowState => {
    const st = state[key] ?? "ready";
    return st === "writing" && started[key] === undefined && !api.jobs.some((j) => j.section === key) ? "done" : st;
  };
  const rows = items ?? [];
  const ready = rows.filter(([k]) => stateOf(k) === "ready");
  const done = rows.filter(([k]) => stateOf(k) === "done").length;
  const finished = items !== null && rows.length > 0 && rows.every(([k]) => stateOf(k) === "done" || stateOf(k) === "failed");
  const totalLeft = useMemo(() => Math.ceil(ready.length / PARALLEL) * draftMs, [ready.length, draftMs]);

  async function sortFirst() {
    setError(null);
    try {
      const prepared = await ipc.prepareSort(api.caseId);
      const sent = await api.review({
        title: "שלב 1: מיון החומרים לסעיפים",
        prepared,
        reprepare: () => ipc.prepareSort(api.caseId),
        onSend: async (id) => {
          await api.track({ label: "Claude קורא את החומרים ומשייך קטעים לסעיפים", task: "sort" }, () => ipc.sendSort(id));
          await api.reload();
        },
      });
      if (sent) await load();
    } catch (e) {
      setError(fail(e as never));
    }
  }

  async function writeOne(key: string, approval: string) {
    setState((s) => ({ ...s, [key]: "writing" }));
    setStarted((s) => ({ ...s, [key]: Date.now() }));
    try {
      await api.track({ label: `Claude כותב את "${title(key)}"`, task: "draft", section: key }, () => ipc.sendSection(approval));
      setState((s) => ({ ...s, [key]: "done" }));
    } catch (e) {
      setError(fail(e as never));
      setState((s) => ({ ...s, [key]: "failed" }));
    }
  }

  async function writeAll() {
    setRunning(true);
    setError(null);
    const queue = ready.map(([k, p]) => [k, p.approval_id ?? ""] as const);
    setState((s) => ({ ...s, ...Object.fromEntries(queue.map(([k]) => [k, "waiting"])) }));
    const worker = async () => {
      for (let next = queue.shift(); next; next = queue.shift()) await writeOne(next[0], next[1]);
    };
    await Promise.all(Array.from({ length: Math.min(PARALLEL, queue.length) }, worker));
    await api.reload();
    setRunning(false);
  }

  /** A section with a question: decide in the usual review, then it is written like the rest. */
  async function answer(key: string, prepared: Prepared) {
    setError(null);
    try {
      await api.review({
        title: `טיוטה לסעיף ${title(key)}`,
        prepared,
        reprepare: () => ipc.prepareSection(api.caseId, key, ""),
        onSend: async (id) => {
          setState((s) => ({ ...s, [key]: "waiting" }));
          await writeOne(key, id);
          await api.reload();
        },
      });
    } catch (e) {
      setError(fail(e as never));
    }
  }

  const stateLabel: Record<RowState, string> = {
    ready: "מוכן לשליחה",
    question: "יש שאלה לפני שליחה",
    waiting: "בתור",
    writing: "Claude כותב…",
    done: "✓ טיוטה מוכנה לאישורך",
    failed: "לא הצליח. אפשר לנסות שוב מהסעיף",
  };

  return (
    <Dialog title="כתיבת כל הדוח עם Claude" onClose={onClose}
      subtitle="כל סעיף נשלח בנפרד, רק עם הקטעים שלו ואחרי הסתרת השמות. הטיוטות נכנסות לדוח רק אחרי שתאשרי."
      footer={
        <>
          <span className="grow small muted">
            {finished ? `${done} סעיפים מוכנים לאישור` : running ? `נכתבו ${done} מתוך ${rows.length}` : ready.length ? `${ready.length} סעיפים · ${aboutLeft(totalLeft).replace("נותרו", "ייקח").replace("נותרה", "תיקח")}` : ""}
          </span>
          {finished ? (
            <>
              <button type="button" className="btn" onClick={() => { onClose(); go({ name: "case", id: api.caseId, view: "report" }); }}>לתצוגת הדוח</button>
              <button type="button" className="btn btn-primary" onClick={() => { onClose(); go({ name: "case", id: api.caseId, view: "report" }); }}>לאישור הטיוטות</button>
            </>
          ) : (
            <>
              <button type="button" className="btn" onClick={onClose}>{running ? "סגירה (הכתיבה ממשיכה)" : "ביטול"}</button>
              <button type="button" className="btn btn-primary" hidden={items === null} disabled={!ready.length || running} onClick={() => void writeAll()}>
                {running ? <><Spinner /> כותב…</> : `שליחה וכתיבת ${ready.length} סעיפים`}
              </button>
            </>
          )}
        </>
      }>
      <div className="stack run">
        <ol className="run-steps">
          <li className={unsorted > 0 ? "current" : "done"}>
            <b>מיון החומרים</b>
            <span>{unsorted > 0 ? `${unsorted} חומרים עוד לא מוינו. Claude יקרא אותם (אחרי הסתרה) ויחליט איזה קטע שייך לאיזה סעיף.` : "כל החומרים ממוינים לסעיפים."}</span>
          </li>
          <li className={unsorted > 0 ? "" : finished ? "done" : "current"}>
            <b>כתיבת הטיוטות</b>
            <span>כל סעיף מקבל רק את הקטעים שלו. {PARALLEL} סעיפים נכתבים במקביל.</span>
          </li>
          <li className={finished ? "current" : ""}>
            <b>האישור שלך</b>
            <span>עוברים סעיף אחר סעיף: מאשרים, עורכים או מבקשים ניסוח אחר.</span>
          </li>
        </ol>

        {unsorted > 0 && (
          <div className="run-sort">
            <span className="grow">קודם ממיינים, כדי שכל סעיף יקבל רק את מה ששייך אליו. אחרי המיון הטיוטות מוכנות לשליחה כאן.</span>
            <button type="button" className="btn btn-primary" onClick={() => void sortFirst()}>מיון החומרים</button>
          </div>
        )}
        {unsorted === 0 && !items && !error && <p className="muted"><Spinner /> מכינה את הבקשות ובודקת מה יוצא…</p>}
        {items && items.length === 0 && <p className="muted">אין סעיפים שמחכים לטיוטה: לכל סעיף עם חומרים כבר יש טיוטה, או שעוד אין חומרים. מוסיפים חומרים ב"חומרי התיק".</p>}

        {rows.map(([key, p]) => {
          const st = stateOf(key);
          return (
            <div key={key} className={`card run-row st-${st}`}>
              <div className="run-row-head">
                <b className="grow">{title(key)}</b>
                <span className="small run-state">{stateLabel[st]}</span>
                {st === "question" && <button type="button" className="btn btn-small" onClick={() => void answer(key, p)}>לענות</button>}
                {(st === "ready" || st === "question") && (
                  <button type="button" className="link-small" aria-expanded={open === key} onClick={() => setOpen(open === key ? null : key)}>
                    {open === key ? "הסתרה" : "מה יוצא?"}
                  </button>
                )}
                {st === "done" && <button type="button" className="link-small" onClick={() => { onClose(); go({ name: "case", id: api.caseId, view: "report" }); }}>לדוח</button>}
              </div>
              {st === "writing" && started[key] !== undefined && <ProgressLine started={started[key]} estimate={draftMs} label="" />}
              {open === key && (
                <div className="run-row-body serif">
                  <p className="small muted">{p.hidden.length ? `יוסתרו: ${p.hidden.join(", ")}` : "אין פרטים מזהים"}</p>
                  {p.parts.map((part, i) => (
                    <p key={i}><span className="small muted">{part.label}: </span><Segments segments={part.outgoing} side="outgoing" /></p>
                  ))}
                </div>
              )}
            </div>
          );
        })}
        <ErrorLine error={error} />
      </div>
    </Dialog>
  );
}
