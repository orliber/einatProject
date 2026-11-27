// Round 41: "write the whole report" as one piece of work in the background (master plan
// 7.2). One click sorts what is not sorted yet, writes every section that has materials and no
// text, three at a time, each showing up in the report as soon as it is in, and checks each
// new draft against its sources as soon as it is in (D-146), three at a time too, beside the
// writing that is left. What an answer or a check said is kept by the core for the session
// (D-161), so the page only has to read the case again to show it.
// The screen shows one line: "the program is working,
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
import { sortInParts } from "./sortParts";
import type { CaseApi } from "../CaseScreen";

/** How many sections are written at the same time, and how many are checked. */
export const PARALLEL = 3;
/** The request each section is sent with (the same as the core's full draft). */
const DRAFT_INSTRUCTION = "כתוב/י טיוטה לסעיף מתוך המקורות, עם מקור לכל פסקה.";

/** The run's request for a section, with the wish she kept for it "in future reports"
 *  (D-163): the same words the core's full draft sends. */
export function runInstruction(wish: string | undefined): string {
  const w = wish?.trim();
  return w ? `${DRAFT_INSTRUCTION} מה שחשוב לי שייכתב בסעיף: ${w}` : DRAFT_INSTRUCTION;
}

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
  /** The materials were sorted first, in this run: the count of sections may change by it. */
  sorted?: boolean;
  /** Sections that showed material before the sorting and have none after it: not written,
   *  and named, so none drops out without a word. */
  noMaterial?: string[];
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
  // The sections she was offered to write before the sorting: empty, with material, and written
  // from the materials (as "לכתוב את N הריקים" counts them). The summary and the like come after
  // approval, from the approved sections: never said to have lost their material.
  const before = api.detail.sections.filter((s) => s.key !== "signature" && s.sortable && (s.paragraphs?.length ?? 0) === 0 && (s.source_count ?? 0) > 0).map((s) => s.key);
  let sorted = false;
  try {
    // 1. Sorting first, so each section gets only its passages.
    if (api.detail.routing.some((r) => r.needs_sorting)) {
      // D-134: one request per material, side by side.
      await sortInParts(api, { title: "מיון החומרים לסעיפים", label: `${opts.ai} קורא את החומרים ומשייך קטעים לסעיפים` });
      sorted = true;
    }
    if (stopping()) return;

    // 2. Every section with materials and no text yet, three at a time. Each new draft is
    //    checked against its sources (when the check is on) as soon as it is in (D-146), up
    //    to three checks at a time beside the writing, so they do not wait for the last section.
    //    A section the check is not for (one written from the approved sections) is skipped. A
    //    check that did not come back is counted as such, never as checked.
    const list: [string, Prepared][] = await ipc.prepareFullDraft(caseId);
    // Her kept wishes, for a request that has to be built again (the first one has them).
    // Keeping them is a convenience: when they cannot be read, the section is still written.
    const wishes: Record<string, string | undefined> = await ipc.draftBriefs().then((b) => b?.sections ?? {}).catch(() => ({}));
    const busy = new Set(api.jobs.map((j) => j.section));
    const queue = list.filter(([k]) => !busy.has(k));
    // After the sorting a section may turn out to have no material: it is named, not dropped.
    const noMaterial = sorted ? before.filter((k) => !busy.has(k) && !list.some(([l]) => l === k)).map(title) : [];
    set({ phase: "write", total: queue.length, sorted, noMaterial });
    const written: string[] = [];
    let checked = 0;
    let unchecked = 0;
    let toCheck = 0;
    const checkOne = async (key: string) => {
      // "לעצור" or a lock: a check not sent yet is not sent.
      if (stopping() || locked) return;
      let ok = false;
      try {
        const prepared = await ipc.prepareVerify(caseId, key);
        ok = await api.review({
          title: `בדיקת הטיוטה של ${title(key)} מול המקורות`,
          prepared,
          reprepare: () => ipc.prepareVerify(caseId, key),
          onSend: async (id) => {
            await api.track({ label: `${opts.ai} בודק את "${title(key)}" מול המקורות`, task: "draft", approval: id }, () => ipc.sendVerify(id), (result) => {
              // The core keeps the verdicts with the paragraphs (D-161); the section's own
              // page still reads them from here.
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
    };
    // The checks in the order the sections came in, at most three on their way at once.
    const waitingChecks: string[] = [];
    const sentChecks: Promise<void>[] = [];
    let checking = 0;
    const pump = () => {
      while (checking < PARALLEL) {
        const key = waitingChecks.shift();
        if (key === undefined) return;
        checking += 1;
        sentChecks.push(checkOne(key).finally(() => {
          checking -= 1;
          pump();
        }));
      }
    };
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
          reprepare: () => ipc.prepareSection(caseId, key, runInstruction(wishes[key])),
          onSend: async (id) => {
            await api.track({ label: `${opts.ai} כותב את "${title(key)}"`, task: "draft", section: key, approval: id }, () => ipc.sendSection(id));
          },
        });
        if (sent) {
          written.push(key);
          if (opts.verify && !stopping()) {
            toCheck += 1;
            set({ toCheck });
            waitingChecks.push(key);
            pump();
          }
        } else failedOne();
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
    // 3. What is left of the checks once the writing is done. A check already sent finishes
    //    even after "לעצור", as a section being written does.
    if (!stopping() && !locked && checked + unchecked < toCheck) set({ phase: "check" });
    // A check that ends starts the next one waiting, so the list grows while it is read.
    for (let i = 0; i < sentChecks.length; i += 1) await sentChecks[i];
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
  const line = phaseLine(r);
  if (r.phase === "sort" || (r.phase === "done" && r.error)) return line;
  // After the sorting: a section that has no material now is said by name.
  const none = r.noMaterial ?? [];
  const lost = none.length === 0 ? "" : none.length === 1
    ? `. אחרי המיון לא נמצא חומר לסעיף "${none[0] ?? ""}", ולכן הוא לא נכתב`
    : `. אחרי המיון לא נמצא חומר ל-${none.length} סעיפים, ולכן הם לא נכתבו: ${none.join(", ")}`;
  return r.phase === "write" && r.sorted ? `החומרים מוינו. ${line}${lost}` : `${line}${lost}`;
}

function phaseLine(r: RunState): string {
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
