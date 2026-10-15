// Round 41: "write the whole report" as one piece of work in the background (master plan
// 7.2). One click sorts what is not sorted yet, writes every section that has materials and no
// text, three at a time, each showing up in the report as soon as it is in, and then checks
// the new drafts against their sources. The screen shows one line: "the program is working,
// 3 of 9", with "stop". Nothing enters the report without her approval.
//
// The run outlives the case screen (she may go elsewhere and come back: the screen showing
// the case then reloads as the run moves on). A lock ends it: the page forgets everything it
// showed, the run included, and what was not written yet is offered again by the next step
// ("✦ לכתוב"), with a fresh request for each section, since an approval does not outlive the
// lock. Only the case id, counts and an error line are kept here, in memory, never text.
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

export type RunPhase = "sort" | "write" | "check" | "done" | "stopped";

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
  /** Sections whose check against the sources did not come back. */
  uncheckedFailed: number;
  /** What stopped the run before it wrote anything (no consent, the sorting failed): her words. */
  error: string | null;
  /** "לעצור" was pressed: what is being written finishes, nothing new is sent. */
  stopping: boolean;
}

const runs = new Map<string, RunState>();
const listeners = new Set<() => void>();
/** The runs going on now, each with its own token: a run cut by a lock never writes into the
 *  next one. */
const active = new Map<string, object>();

function subscribe(fn: () => void) {
  listeners.add(fn);
  return () => listeners.delete(fn);
}

function update(caseId: string, change: Partial<RunState>, token?: object) {
  const prev = runs.get(caseId);
  if (!prev || (token && active.get(caseId) !== token)) return;
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

/** On a lock (and in tests): every run is forgotten, and one still going says nothing more. */
export function resetRuns() {
  runs.clear();
  active.clear();
  for (const fn of listeners) fn();
}

/** The case screen showing each case now, so a run started from an earlier one sees what she
 *  did since (a section she wrote meanwhile is not written again). */
const screens = new Map<string, CaseApi>();

/** The case screen shows this case (and `null`: it closed). */
export function showingCase(caseId: string, api: CaseApi | null) {
  if (api) screens.set(caseId, api);
  else screens.delete(caseId);
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
  const token = {};
  active.set(caseId, token);
  runs.set(caseId, { phase: "sort", done: 0, total: 0, checked: 0, toCheck: 0, failed: 0, uncheckedFailed: 0, error: null, stopping: false });
  for (const fn of listeners) fn();
  const set = (change: Partial<RunState>) => update(caseId, change, token);
  const failedOne = () => set({ failed: (runs.get(caseId)?.failed ?? 0) + 1 });
  let locked = false;
  const title = (key: string) => api.detail.sections.find((s) => s.key === key)?.title ?? key;
  const stopping = () => runs.get(caseId)?.stopping === true || active.get(caseId) !== token;
  try {
    // 1. Sorting first, so each section gets only its passages.
    if (api.detail.routing.some((r) => r.needs_sorting)) {
      const prepared = await ipc.prepareSort(caseId);
      await api.review({
        title: "מיון החומרים לסעיפים",
        prepared,
        reprepare: () => ipc.prepareSort(caseId),
        onSend: async (id) => {
          await api.track({ label: `${opts.ai} קורא את החומרים ומשייך קטעים לסעיפים`, task: "sort", approval: id }, () => ipc.sendSort(id));
        },
      });
    }
    if (stopping()) return;

    // 2. Every section with materials and no text yet, three at a time.
    const list: [string, Prepared][] = await ipc.prepareFullDraft(caseId);
    const busy = new Set(api.jobs.map((j) => j.section));
    const queue = list.filter(([k]) => !busy.has(k));
    set({ phase: "write", total: queue.length });
    const written: string[] = [];
    const writeOne = async ([key, first]: [string, Prepared]) => {
      // Written or being written meanwhile (by hand, or one draft at a time): not sent again.
      const now = screens.get(caseId) ?? api;
      const there = now.detail.sections.find((s) => s.key === key)?.paragraphs?.length ?? 0;
      if (there > 0 || now.jobs.some((j) => j.section === key)) {
        set({ total: (runs.get(caseId)?.total ?? 1) - 1 });
        return;
      }
      try {
        const sent = await api.review({
          title: `טיוטה לסעיף ${title(key)}`,
          prepared: first,
          reprepare: () => ipc.prepareSection(caseId, key, DRAFT_INSTRUCTION),
          onSend: async (id) => {
            await api.track({ label: `${opts.ai} כותב את "${title(key)}"`, task: "draft", section: key, approval: id }, () => ipc.sendSection(id));
          },
        });
        if (sent) written.push(key);
        else failedOne();
      } catch (e) {
        if (lockedMeanwhile(e)) locked = true;
        else failedOne();
      } finally {
        set({ done: written.length });
      }
    };
    const worker = async () => {
      for (let next = queue.shift(); next && !stopping() && !locked; next = queue.shift()) await writeOne(next);
    };
    await Promise.all(Array.from({ length: Math.min(PARALLEL, queue.length) }, worker));
    if (stopping() || locked) return;

    // 3. The new drafts against their sources, once each (when the check is on). A section
    //    the check is not for (one written from the approved sections) is skipped. A check
    //    that did not come back is counted as such, never as checked.
    if (opts.verify && written.length > 0) {
      set({ phase: "check", toCheck: written.length });
      let checked = 0;
      let unchecked = 0;
      for (const key of written) {
        if (stopping()) return;
        let ok = false;
        try {
          const prepared = await ipc.prepareVerify(caseId, key);
          ok = await api.review({
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
            locked = true;
            return;
          }
        }
        if (ok) checked += 1;
        else unchecked += 1;
        set({ checked, uncheckedFailed: unchecked });
      }
    }
  } catch (e) {
    if (lockedMeanwhile(e)) locked = true;
    else set({ error: (e as UiError | undefined)?.message || "משהו השתבש." });
  } finally {
    if (active.get(caseId) === token) {
      set({ phase: stopping() ? "stopped" : "done", stopping: false });
      active.delete(caseId);
      // A lock: the page forgets the run (and in the built app, reloads).
      if (locked) forgetRun(caseId);
    }
  }
}

/** The line in her words. */
export function runLine(r: RunState): string {
  switch (r.phase) {
    case "sort":
      return r.stopping ? "עוצרת אחרי המיון" : "התוכנה עובדת: ממיינת את החומרים לסעיפים";
    case "write":
      return r.stopping ? `עוצרת אחרי הסעיפים שכבר נשלחו (${r.done} מתוך ${r.total})` : `התוכנה עובדת: נכתבו ${r.done} מתוך ${r.total} סעיפים`;
    case "check":
      return r.stopping ? `עוצרת אחרי הבדיקה שכבר נשלחה (${r.checked} מתוך ${r.toCheck})` : `התוכנה עובדת: נבדקו מול המקורות ${r.checked} מתוך ${r.toCheck} סעיפים`;
    case "stopped":
      return `נעצר. נכתבו ${r.done} מתוך ${r.total} סעיפים`;
    case "done": {
      if (r.error) return `כתיבת הדוח לא התחילה: ${r.error}`;
      if (r.failed > 0) return `נכתבו ${r.done} מתוך ${r.total} סעיפים. מה שלא הצליח מופיע מתחת, עם "לנסות שוב"`;
      if (r.total === 0) return "אין סעיפים שמחכים לטיוטה";
      const waiting = r.done === 1 ? "סעיף אחד מחכה לאישורך" : `${r.done} סעיפים מחכים לאישורך`;
      if (r.uncheckedFailed > 0) {
        const n = r.uncheckedFailed === 1 ? "סעיף אחד לא נבדק" : `${r.uncheckedFailed} סעיפים לא נבדקו`;
        return `הדוח נכתב. ${waiting}. ${n} מול המקורות, ואפשר לבדוק בסעיף עצמו`;
      }
      return r.toCheck > 0 ? `הדוח נכתב ונבדק מול המקורות. ${waiting}` : `הדוח נכתב. ${waiting}`;
    }
  }
}
