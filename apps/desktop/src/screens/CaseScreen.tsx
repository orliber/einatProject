import { useCallback, useEffect, useState } from "react";
import { useApp } from "../App";
import { ipc, type CaseDetail, type Prepared } from "../ipc/client";
import { ReviewDialog } from "../components/ReviewDialog";
import { ExportDialog } from "../components/ExportDialog";
import { FullDraftDialog } from "../components/FullDraftDialog";
import { LockIcon, ErrorLine } from "../components/ui";
import { ageWords } from "../components/AgeField";
import { MaterialsView } from "./case/MaterialsView";
import { SectionWork } from "./case/SectionWork";
import { DetailsView } from "./case/DetailsView";
import "./CaseScreen.css";

export interface ReviewRequest {
  title: string;
  prepared: Prepared;
  reprepare: () => Promise<Prepared>;
  onSend: (approvalId: string) => Promise<void>;
}

/** What a case view needs from the screen. */
export interface CaseApi {
  caseId: string;
  detail: CaseDetail;
  reload: () => Promise<void>;
  /** Show "what leaves the computer" (or send right away when the policy allows). */
  review: (r: ReviewRequest) => Promise<void>;
}

type SectionState = "approved" | "pending" | "empty";

function sectionState(s: CaseDetail["sections"][number]): SectionState {
  if (s.paragraphs.some((p) => p.status === "proposed")) return "pending";
  if (s.approved) return "approved";
  return "empty";
}

export function CaseScreen({ caseId, view }: { caseId: string; view: string }) {
  const { go, fail, lockNow, status } = useApp();
  const [detail, setDetail] = useState<CaseDetail | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [review, setReview] = useState<ReviewRequest | null>(null);
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

  const startReview = useCallback(
    async (r: ReviewRequest) => {
      const clean = r.prepared.approval_id && r.prepared.suspects.length === 0 && r.prepared.blocked.length === 0;
      if (status.review_only_suspect && clean && r.prepared.approval_id) {
        await r.onSend(r.prepared.approval_id);
        return;
      }
      setReview(r);
    },
    [status.review_only_suspect],
  );

  if (!detail) {
    return <main className="center-note">{error ? <p className="error">{error}</p> : <span aria-busy="true" />}</main>;
  }

  const api: CaseApi = { caseId, detail, reload, review: startReview };
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
  const viewTitle = view === "materials" ? "חומרי התיק" : view === "details" ? "פרטי התיק ושמות" : current?.title ?? "";

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
                  return (
                    <button key={s.key} type="button" className={`side-section st-${st}`}
                      aria-current={view === s.key ? "page" : undefined}
                      onClick={() => go({ name: "case", id: caseId, view: s.key })}>
                      <span className="dot" aria-hidden="true" />
                      <span className="grow">{s.title}</span>
                      {pending > 0 && <span className="side-pending">{pending} ממתינות</span>}
                      <span className="visually-hidden">{st === "approved" ? "אושר" : st === "pending" ? "ממתין" : "ריק"}</span>
                    </button>
                  );
                })}
              </div>
            );
          })}
        </div>
        <button type="button" className="side-draft" onClick={() => setFullDraft(true)}>הכנת טיוטה לכל הדוח</button>
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
        {current && <SectionWork key={current.key} api={api} section={current} />}
      </div>

      {review && (
        <ReviewDialog title={review.title} caseId={caseId} prepared={review.prepared} reprepare={review.reprepare}
          onClose={() => setReview(null)}
          onSend={async (id) => {
            await review.onSend(id);
            setReview(null);
          }} />
      )}
      {exporting && <ExportDialog api={api} onClose={() => setExporting(false)} />}
      {fullDraft && <FullDraftDialog api={api} onClose={() => setFullDraft(false)} />}
    </div>
  );
}
