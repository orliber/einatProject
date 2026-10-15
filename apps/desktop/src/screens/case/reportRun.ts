// Round 41: "write the whole report" as one piece of work in the background (master plan
// 7.2). One click sorts what is not sorted yet, writes every section that has materials and no
// text, three at a time, each showing up in the report as soon as it is in, and then checks
// the new drafts against their sources. The screen shows one line: "the program is working,
// 3 of 9", with "stop". Nothing enters the report without her approval.
//
// The run outlives the case screen (she may go elsewhere) and a lock: a request cut by the lock
// is not lost, the run waits, and opening the case again goes on from what is still missing
// (a fresh request for each section, since an approval does not outlive the lock). Only the
// case id and counts are kept here, never text.
//
// When she asked to see every request first (advanced), the old window stays instead.
import { useSyncExternalStore } from "react";
import { ipc, type ParagraphCheck, type Prepared, type UiError } from "../../ipc/client";
import { setKept } from "../../kept";
import type { CaseApi } from "../CaseScreen";

/** How many sections are written at the same time. */
const PARALLEL = 3;
/** The request each section is sent with (the same as the core's full draft). */
const DRAFT_INSTRUCTION = "כתוב/י טיוטה לסעיף מתוך המקורות, עם מקור לכל פסקה.";

export type RunPhase = "sort" | "write" | "check" | "done" | "stopped" | "paused";

export interface RunState {
  phase: RunPhase;
  /** Sections written, and how many were to be written. */
  done: number;
  total: number;
  /** Sections checked against their sources, and how many will be. */
  checked: number;
  toCheck: number;
  /** Sections whose request failed (in the strip, with "לנסות שוב"). */
  failed: number;
  /** "לעצור" was pressed: what is being written finishes, nothing new is sent. */
  stopping: boolean;
}

const runs = new Map<string, RunState>();
const listeners = new Set<() => void>();
/** The runs going on now (a paused one is not). */
const active = new Set<string>();

function subscribe(fn: () => void) {
  listeners.add(fn);
  return () => listeners.delete(fn);
}

function update(caseId: string, change: Partial<RunState>) {
  const prev = runs.get(caseId);
  if (!prev) return;
  runs.set(caseId, { ...prev, ...change });
  for (const fn of listeners) fn();
}

export function useRun(caseId: string): RunState | undefined {
  return useSyncExternalStore(subscribe, () => runs.get(caseId));
}

export function runOf(caseId: string): RunState | undefined {
  return runs.get(caseId);
}

export function isRunning(caseId: string): boolean {
  return active.has(caseId);
}

export function stopRun(caseId: string) {
  update(caseId, { stopping: true });
}

/** The line is closed: a finished or stopped run is forgotten. */
export function forgetRun(caseId: string) {
  if (active.has(caseId)) return;
  runs.delete(caseId);
  for (const fn of listeners) fn();
}

/** Tests: start clean. */
export function resetRuns() {
  runs.clear();
  active.clear();
  for (const fn of listeners) fn();
}

const lockedMeanwhile = (e: unknown) => (e as UiError | undefined)?.action?.kind === "unlock";

/**
 * The whole report, in the background. `api` is the case screen's (its review sends at once
 * when the gate cleared the request, and puts a refusal in the strip). `verify`: the check
 * against the sources is on.
 */
export async function runWholeReport(api: CaseApi, opts: { verify: boolean; ai: string }): Promise<void> {
  const { caseId } = api;
  if (active.has(caseId)) return;
  active.add(caseId);
  runs.set(caseId, { phase: "sort", done: 0, total: 0, checked: 0, toCheck: 0, failed: 0, stopping: false });
  for (const fn of listeners) fn();
  let paused = false;
  const title = (key: string) => api.detail.sections.find((s) => s.key === key)?.title ?? key;
  const stopping = () => runs.get(caseId)?.stopping === true;
  try {
    // 1. Sorting first, so each section gets only its passages.
    if (api.detail.routing.some((r) => r.needs_sorting)) {
      const prepared = await ipc.prepareSort(caseId);
      await api.review({
        title: "מיון החומרים לסעיפים",
        prepared,
        reprepare: () => ipc.prepareSort(caseId),
        onSend: async (id) => {
          await api.track({ label: `${opts.ai} קורא את החומרים ומשייך קטעים לסעיפים`, task: "sort", approval: id }, () => ipc.sendSort(id), () => api.reload());
        },
      });
    }
    if (stopping()) return;

    // 2. Every section with materials and no text yet, three at a time.
    const list: [string, Prepared][] = await ipc.prepareFullDraft(caseId);
    const busy = new Set(api.jobs.map((j) => j.section));
    const queue = list.filter(([k]) => !busy.has(k));
    update(caseId, { phase: "write", total: queue.length });
    const written: string[] = [];
    const writeOne = async ([key, first]: [string, Prepared]) => {
      try {
        const sent = await api.review({
          title: `טיוטה לסעיף ${title(key)}`,
          prepared: first,
          reprepare: () => ipc.prepareSection(caseId, key, DRAFT_INSTRUCTION),
          onSend: async (id) => {
            await api.track({ label: `${opts.ai} כותב את "${title(key)}"`, task: "draft", section: key, approval: id }, () => ipc.sendSection(id), () => api.reload());
          },
        });
        if (sent) written.push(key);
        else update(caseId, { failed: (runs.get(caseId)?.failed ?? 0) + 1 });
      } catch (e) {
        if (lockedMeanwhile(e)) paused = true;
        else update(caseId, { failed: (runs.get(caseId)?.failed ?? 0) + 1 });
      } finally {
        update(caseId, { done: written.length });
      }
    };
    const worker = async () => {
      for (let next = queue.shift(); next && !stopping() && !paused; next = queue.shift()) await writeOne(next);
    };
    await Promise.all(Array.from({ length: Math.min(PARALLEL, queue.length) }, worker));
    if (stopping() || paused) return;

    // 3. The new drafts against their sources, once each (when the check is on). A section
    //    the check is not for (one written from the approved sections) is skipped.
    if (opts.verify && written.length > 0) {
      update(caseId, { phase: "check", toCheck: written.length });
      let checked = 0;
      for (const key of written) {
        if (stopping() || paused) return;
        try {
          const prepared = await ipc.prepareVerify(caseId, key);
          await api.review({
            title: `בדיקת הטיוטה של ${title(key)} מול המקורות`,
            prepared,
            reprepare: () => ipc.prepareVerify(caseId, key),
            onSend: async (id) => {
              await api.track({ label: `${opts.ai} בודק את "${title(key)}" מול המקורות`, task: "draft", approval: id }, () => ipc.sendVerify(id), (result) => {
                // The same notes the section shows after its own check.
                setKept<Map<string, ParagraphCheck>>(`case:${caseId}:section:${key}:checks`, new Map())(
                  new Map(result.draft_ids.map((d, i) => [d, result.checks[i]] as [string, ParagraphCheck])),
                );
              });
            },
          });
        } catch (e) {
          if (lockedMeanwhile(e)) {
            paused = true;
            return;
          }
        }
        checked += 1;
        update(caseId, { checked });
      }
    }
  } catch (e) {
    if (lockedMeanwhile(e)) paused = true;
    else update(caseId, { failed: (runs.get(caseId)?.failed ?? 0) + 1 });
  } finally {
    active.delete(caseId);
    update(caseId, { phase: paused ? "paused" : stopping() ? "stopped" : "done", stopping: false });
  }
}

/** The line in her words. */
export function runLine(r: RunState): string {
  switch (r.phase) {
    case "sort":
      return "התוכנה עובדת: ממיינת את החומרים לסעיפים";
    case "write":
      return r.stopping ? `עוצרת אחרי הסעיפים שכבר נשלחו (${r.done} מתוך ${r.total})` : `התוכנה עובדת: נכתבו ${r.done} מתוך ${r.total} סעיפים`;
    case "check":
      return `התוכנה עובדת: נבדקו מול המקורות ${r.checked} מתוך ${r.toCheck} סעיפים`;
    case "paused":
      return "הכתיבה נעצרה כשהכספת ננעלה, וממשיכה עכשיו";
    case "stopped":
      return `נעצר. נכתבו ${r.done} מתוך ${r.total} סעיפים`;
    case "done":
      return r.total === 0
        ? "אין סעיפים שמחכים לטיוטה"
        : r.failed > 0
          ? `נכתבו ${r.done} מתוך ${r.total} סעיפים. מה שלא הצליח מופיע למעלה, עם "לנסות שוב"`
          : r.toCheck > 0
            ? `הדוח נכתב ונבדק מול המקורות. ${r.done} סעיפים מחכים לאישורך`
            : `הדוח נכתב. ${r.done} סעיפים מחכים לאישורך`;
  }
}
