// D-163: the closing sections in the right order. The DSM section, the summary, the diagnoses
// and the recommendations are written from what she approved, so their order matters: her
// diagnoses first (hers to choose), then the DSM section and the recommendations (which read
// her diagnoses), then the summary (which reads them all). One compact strip where the closing
// part of the report begins, shown only when the body is mostly approved or she is on a
// closing section, and gone once the three steps are done.
import { useState } from "react";
import { useQuery } from "@tanstack/react-query";
import { useApp } from "../../App";
import { ipc, type CaseDetail } from "../../ipc/client";
import { ErrorLine } from "../../components/ui";
import type { CaseApi } from "../CaseScreen";
import { DRAFT_FROM_APPROVED } from "./instructions";

type Section = CaseDetail["sections"][number];

/** The sections written from the approved ones (the core's `DERIVED_SECTIONS`). */
export const CLOSING_KEYS: readonly string[] = ["dsm", "summary", "diagnoses", "recommendations"];
/** The closing sections a finished report has. The DSM section is not one of them: it is
 *  written only for a child it fits, so an empty one says nothing about the report. */
const REQUIRED: readonly string[] = ["diagnoses", "recommendations", "summary"];

const waits = (s: Section) => s.paragraphs.some((p) => p.status === "proposed");
const written = (s: Section) => s.paragraphs.some((p) => p.status === "approved");
/** Approved, with no draft still waiting. */
const done = (s: Section) => written(s) && !waits(s);

/** The closing sections a finished report has that have no approved text yet. */
export function emptyClosing(sections: Section[]): Section[] {
  return sections.filter((s) => REQUIRED.includes(s.key) && !written(s));
}

/** The sections she writes herself that are still empty: a closing section drafted now
 *  cannot reflect them. */
export function ownEmpty(sections: Section[]): Section[] {
  return sections.filter((s) => s.mode === "self" && !CLOSING_KEYS.includes(s.key) && s.key !== "signature" && !written(s));
}

/** Said beside a closing section's draft while her own sections are empty. */
export function ownEmptyLine(sections: Section[]): string | null {
  const own = ownEmpty(sections);
  if (own.length === 0) return null;
  const names = own.map((s) => `"${s.title}"`).join(", ");
  return own.length === 1
    ? `הסעיף ${names}, שאת כותבת בעצמך, עוד לא נכתב, ולכן לא יבוא לידי ביטוי בניסוח.`
    : `הסעיפים ${names}, שאת כותבת בעצמך, עוד לא נכתבו, ולכן לא יבואו לידי ביטוי בניסוח.`;
}

/** The request of a closing section's draft, with the wish she kept for it. */
export function closingRequest(wish: string | undefined): string | undefined {
  const w = wish?.trim();
  return w ? `${DRAFT_FROM_APPROVED} מה שחשוב לי שייכתב בסעיף: ${w}` : undefined;
}

export type StepState = "done" | "waiting" | "next" | "later";

export interface ClosingStep {
  id: "diagnoses" | "dsm_recommendations" | "summary";
  label: string;
  state: StepState;
  /** The step's sections that the report has. */
  sections: Section[];
  /** Of them, the empty ones the AI drafts. */
  toDraft: Section[];
}

/** The three steps, each with its state; a step whose sections the report does not have is
 *  left out. The first step not done is "next", the ones after it "later". */
export function closingSteps(sections: Section[]): ClosingStep[] {
  const of = (...keys: string[]) => keys.flatMap((k) => sections.filter((s) => s.key === k));
  const raw: { id: ClosingStep["id"]; label: string; sections: Section[]; finished: boolean }[] = [
    { id: "diagnoses", label: "אבחנות", sections: of("diagnoses"), finished: of("diagnoses").every(done) },
    // The recommendations decide; the DSM section only when something was written in it.
    {
      id: "dsm_recommendations",
      label: of("dsm").length && of("recommendations").length ? "DSM והמלצות" : of("dsm").length ? "DSM" : "המלצות",
      sections: of("dsm", "recommendations"),
      finished: of("recommendations").every(done) && of("dsm").every((s) => done(s) || (of("recommendations").length > 0 && s.paragraphs.length === 0)),
    },
    { id: "summary", label: "סיכום", sections: of("summary"), finished: of("summary").every(done) },
  ];
  let nextGiven = false;
  return raw.filter((r) => r.sections.length > 0).map((r) => {
    const state: StepState = r.sections.some(waits) ? "waiting" : r.finished ? "done" : nextGiven ? "later" : "next";
    if (state === "next" || state === "waiting") nextGiven = true;
    return { id: r.id, label: r.label, state, sections: r.sections, toDraft: r.sections.filter((s) => s.paragraphs.length === 0 && s.mode === "ai") };
  });
}

/** Why a closing section drafted from its own button now would be written too early (D-163):
 *  the DSM section and the recommendations before her diagnoses, the summary before them or
 *  before the recommendations. `null` when nothing holds it. The same order the strip keeps;
 *  drafting anyway is her choice ("להמשיך בכל זאת"). */
export function closingHold(sections: Section[], key: string): string | null {
  const steps = closingSteps(sections);
  const open = (id: ClosingStep["id"]) => steps.some((s) => s.id === id && s.state !== "done");
  if (key === "dsm" || key === "recommendations") {
    return open("diagnoses") ? "האבחנות שלך עוד לא אושרו, והסעיף הזה נכתב מתוכן: מה שינוסח עכשיו ייכתב בלעדיהן." : null;
  }
  if (key === "summary") {
    if (open("diagnoses")) return "האבחנות שלך עוד לא אושרו: סיכום שינוסח עכשיו ייכתב בלעדיהן.";
    if (open("dsm_recommendations")) return "ההמלצות עוד לא אושרו: סיכום שינוסח עכשיו לא יכלול אותן.";
  }
  return null;
}

/** Whether the strip has a place on the page now: she is on a closing section, or two thirds
 *  of the body (the sections with material or text, and her own) are approved; never once
 *  every step is done. */
export function showClosing(sections: Section[], current: string | null): boolean {
  const steps = closingSteps(sections);
  if (steps.length === 0 || steps.every((s) => s.state === "done")) return false;
  if (current && CLOSING_KEYS.includes(current)) return true;
  const body = sections.filter((s) => !CLOSING_KEYS.includes(s.key) && s.key !== "signature" && (s.paragraphs.length > 0 || s.source_count > 0 || s.mode === "self"));
  return body.length > 0 && body.filter(done).length >= Math.ceil((body.length * 2) / 3);
}

/** The section before which the strip sits: the first of the summary, the diagnoses and the
 *  recommendations in the report's order (else the DSM section). */
export function closingAnchor(sections: Section[]): string | null {
  return (sections.find((s) => REQUIRED.includes(s.key)) ?? sections.find((s) => s.key === "dsm"))?.key ?? null;
}

const STATE_WORDS: Record<StepState, string> = { done: "נעשה", waiting: "מחכה לאישורך", next: "הבא בתור", later: "אחר כך" };

export function ClosingFlow({ api, current, onPick }: { api: CaseApi; current: string | null; onPick: (key: string, move?: boolean) => void }) {
  const { fail } = useApp();
  const { detail } = api;
  const briefs = useQuery({ queryKey: ["draftBriefs"], queryFn: ipc.draftBriefs, staleTime: 60_000 });
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  /** "להמשיך בכל זאת": the summary before the steps above it are done. */
  const [anyway, setAnyway] = useState(false);
  /** "להמשיך בכל זאת": the DSM section and the recommendations before her diagnoses. */
  const [early, setEarly] = useState(false);
  if (!showClosing(detail.sections, current)) return null;
  const steps = closingSteps(detail.sections);
  const writing = (s: Section) => api.jobs.some((j) => j.section === s.key);
  const before = (id: ClosingStep["id"]) => steps.slice(0, steps.findIndex((s) => s.id === id));
  // She diagnoses: what comes after her diagnoses waits for them, unless she says otherwise.
  const diagPending = steps.some((s) => s.id === "diagnoses" && s.state !== "done");
  const recsPending = steps.some((s) => s.id === "dsm_recommendations" && s.state !== "done");
  const laterOpen = steps.some((s) => (s.id === "dsm_recommendations" || s.id === "summary") && s.state !== "done");

  async function draft(sections: Section[]) {
    if (busy || sections.length === 0) return;
    setError(null);
    setBusy(true);
    try {
      // Side by side: each is prepared, cleared by the gate and written on its own.
      const results = await Promise.allSettled(sections.map((s) => api.draft(s.key, closingRequest(briefs.data?.sections[s.key]))));
      const failed = results.find((r) => r.status === "rejected");
      if (failed) setError(fail(failed.reason as never));
    } finally {
      setBusy(false);
    }
  }

  // Her own sections still empty: said here when she drafts from the strip (on a closing
  // section the same line is under its own box).
  const own = current && CLOSING_KEYS.includes(current) ? null : ownEmptyLine(detail.sections);
  return (
    <div className="closing-flow" role="group" aria-label="סעיפי הסיום, לפי הסדר">
      <span className="closing-flow-title">סעיפי הסיום, לפי הסדר</span>
      <ol className="closing-steps">
        {steps.map((step, i) => {
          const first = step.sections.find(waits) ?? step.sections[0];
          const active = step.toDraft.filter((s) => !writing(s));
          const inWork = busy || step.sections.some(writing);
          // The summary reads the recommendations (and the DSM section): it waits for them
          // unless she says otherwise.
          const held = step.id === "summary" && !anyway && before("summary").some((s) => (s.id === "dsm_recommendations" || s.id === "diagnoses") && s.state !== "done");
          const heldForDiagnoses = step.id === "dsm_recommendations" && diagPending && !early;
          return (
            <li key={step.id} className={`closing-step st-${step.state}`}>
              <span className="closing-n" aria-hidden="true">{step.state === "done" ? "✓" : i + 1}</span>
              <span className="closing-label">{step.label}</span>
              <span className="closing-state">{inWork && step.state !== "done" && step.state !== "waiting" ? "נכתב עכשיו" : STATE_WORDS[step.state]}</span>
              {step.state === "waiting" && first && (
                <button type="button" className="link-btn" onClick={() => onPick(first.key, true)}>לאישור</button>
              )}
              {step.id === "diagnoses" && step.state !== "done" && step.state !== "waiting" && first && (
                <button type="button" className="link-btn" onClick={() => onPick(first.key, true)}>לבחירת האבחנות</button>
              )}
              {step.id === "dsm_recommendations" && step.state !== "done" && step.state !== "waiting" && (
                active.length > 0 && heldForDiagnoses ? (
                  <button type="button" className="link-btn closing-anyway" onClick={() => setEarly(true)}>להמשיך בכל זאת</button>
                ) : active.length > 0 ? (
                  <button type="button" className="link-btn" disabled={inWork} onClick={() => void draft(active)}>
                    ✦ לנסח {active.length === 1 ? `את "${active[0]?.title ?? ""}"` : "את שניהם"}
                  </button>
                ) : first && !inWork && <button type="button" className="link-btn" onClick={() => onPick(first.key, true)}>לסעיף</button>
              )}
              {step.id === "summary" && step.state !== "done" && step.state !== "waiting" && (
                held ? (
                  <button type="button" className="link-btn closing-anyway" onClick={() => setAnyway(true)}>להמשיך בכל זאת</button>
                ) : active.length > 0 ? (
                  <button type="button" className="link-btn" disabled={inWork} onClick={() => void draft(active)}>✦ לנסח את הסיכום</button>
                ) : first && !inWork && <button type="button" className="link-btn" onClick={() => onPick(first.key, true)}>לסעיף</button>
              )}
            </li>
          );
        })}
      </ol>
      {diagPending && laterOpen && !early && !anyway && (
        <span className="small muted">שאר סעיפי הסיום נכתבים אחרי שהאבחנות שלך מאושרות, כדי שיתבססו עליהן.</span>
      )}
      {diagPending && laterOpen && (early || anyway) && (
        <span className="small muted">האבחנות עוד לא אושרו: מה שינוסח עכשיו ייכתב בלעדיהן.</span>
      )}
      {steps.some((s) => s.id === "summary" && s.state !== "done") && !anyway && recsPending && (!diagPending || early) && (
        <span className="small muted">הסיכום נכתב אחרי שההמלצות מאושרות, כדי שיכלול אותן.</span>
      )}
      {own && <span className="small muted">{own}</span>}
      <ErrorLine error={error} />
    </div>
  );
}
