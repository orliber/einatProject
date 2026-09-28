import { useCallback, useEffect, useState } from "react";
import { useApp } from "../App";
import { he } from "../i18n/he";
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
        <div className="side-brand">
          <LockIcon size={24} color="#9cc9c3" />
          <span>{he.appName}</span>
        </div>
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
          <span className="crumbs">
            <button type="button" className="crumb" onClick={() => go({ name: "cases" })}>תיקים</button>
            <span aria-hidden="true">‹</span> {detail.meta.code} <span aria-hidden="true">‹</span> <b>{viewTitle}</b>
          </span>
          <span className="encrypted"><LockIcon size={13} /> {he.encrypted}</span>
          {status.demo_mode && <span className="chip chip-sand" title="לא הוגדר מפתח API בהגדרות">מצב הדגמה</span>}
          <button type="button" className="btn" onClick={() => go({ name: "consult", caseId })}>התייעצות</button>
          <button type="button" className="btn btn-primary" onClick={() => setExporting(true)}>הפקת דוח Word</button>
          <button type="button" className="btn icon-btn" aria-label="נעילה" title="נעילה" onClick={() => void lockNow()}><LockIcon /></button>
        </header>
        <ErrorLine error={error} />
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
