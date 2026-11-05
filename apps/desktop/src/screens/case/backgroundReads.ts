// P-09 → D-160: a scan or photo, or any file whose reading takes long, is read in the
// background. The file gets a line of its own ("נקרא ברקע (בטא)") and the rest of the case
// goes on: another file can be added, a material opened, a section written. When the reading
// ends, the line offers "לבדיקה ולשמירה", which opens the same import window as always: text
// recognized from a scan is never saved without her looking at it. When it fails, the line
// says why and offers "לנסות שוב", and blocks nothing.
//
// Scans wait for each other: one is read at a time (its pages already side by side in the
// core), so a second scan does not halve the speed of the first, or slow her other work.
// The reads outlive the materials screen (she may go to another stage and come back). A lock
// forgets them all: the page keeps no text of a vault that is closed, and a reading cut by the
// lock says nothing more.
import { useSyncExternalStore } from "react";
import { ipc, type ImportPreview, type InputKind, type UiError } from "../../ipc/client";
import { sizeBucket, timed } from "../../components/Progress";

/** A file that takes longer than this to read goes on in the background. */
export const BACKGROUND_AFTER_MS = 4000;

const IMAGE = /\.(jpe?g|png|tiff?)$/i;

/** A photo or a scanned page: read in the background from the start. */
export function isImageFile(name: string): boolean {
  return IMAGE.test(name);
}

export type ReadStatus = "waiting" | "reading" | "ready" | "failed";

export interface BackgroundRead {
  id: number;
  caseId: string;
  name: string;
  kind?: InputKind | undefined;
  status: ReadStatus;
  /** What was read, once it is ready. */
  preview?: ImportPreview;
  /** Why it failed, in her words. */
  error?: string;
}

interface Entry extends BackgroundRead {
  file: File;
  /** The reading already going on (a file that took long), or none yet (a scan waiting). */
  work?: Promise<ImportPreview> | undefined;
}

let entries: Entry[] = [];
let nextId = 1;
/** Changes with every lock: a reading cut by one never writes into the next vault. */
let generation = {};
const listeners = new Set<() => void>();
const snapshots = new Map<string, BackgroundRead[]>();

function changed() {
  snapshots.clear();
  for (const fn of listeners) fn();
}

function subscribe(fn: () => void) {
  listeners.add(fn);
  return () => listeners.delete(fn);
}

function snapshot(caseId: string): BackgroundRead[] {
  let s = snapshots.get(caseId);
  if (!s) {
    s = entries
      .filter((e) => e.caseId === caseId)
      .map((e): BackgroundRead => {
        const r: BackgroundRead = { id: e.id, caseId: e.caseId, name: e.name, kind: e.kind, status: e.status };
        if (e.preview) r.preview = e.preview;
        if (e.error !== undefined) r.error = e.error;
        return r;
      });
    snapshots.set(caseId, s);
  }
  return s;
}

/** The files of this case being read in the background, or read and waiting for her. */
export function useBackgroundReads(caseId: string): BackgroundRead[] {
  return useSyncExternalStore(subscribe, () => snapshot(caseId));
}

function set(id: number, change: Partial<Entry>, gen: object) {
  if (gen !== generation) return;
  entries = entries.map((e) => (e.id === id ? { ...e, ...change } : e));
  changed();
}

const lockedMeanwhile = (e: unknown) => (e as UiError | undefined)?.action?.kind === "unlock";
const messageOf = (e: unknown) => (e as UiError | undefined)?.message ?? "הקריאה נכשלה.";

/** The next one: a reading already going on is followed; a waiting scan starts when no other
 *  is being read. */
function pump() {
  const gen = generation;
  for (const e of entries) {
    if (e.status === "reading" && e.work) {
      const work = e.work;
      set(e.id, { work: undefined }, gen);
      follow(e.id, work, gen);
    }
  }
  if (entries.some((e) => e.status === "reading")) return;
  const next = entries.find((e) => e.status === "waiting");
  if (!next) return;
  const work = timed("read", ipc.importDocument(next.caseId, next.file), sizeBucket(next.file.size));
  set(next.id, { status: "reading" }, gen);
  follow(next.id, work, gen);
}

function follow(id: number, work: Promise<ImportPreview>, gen: object) {
  work.then(
    (preview) => {
      set(id, { status: "ready", preview }, gen);
      if (gen === generation) pump();
    },
    (e: unknown) => {
      if (lockedMeanwhile(e) || gen !== generation) return;
      set(id, { status: "failed", error: messageOf(e) }, gen);
      pump();
    },
  );
}

/** A file to read in the background: a scan waits its turn; `work` is a reading already
 *  going on (a file that took longer than [`BACKGROUND_AFTER_MS`]). */
export function readInBackground(caseId: string, file: File, kind?: InputKind, work?: Promise<ImportPreview>) {
  entries = [...entries, { id: nextId++, caseId, name: file.name, kind, file, status: work ? "reading" : "waiting", work }];
  changed();
  pump();
}

/** "לנסות שוב" on a line that failed. */
export function retryRead(id: number) {
  const e = entries.find((x) => x.id === id);
  if (!e || e.status !== "failed") return;
  const again: Entry = { id: e.id, caseId: e.caseId, name: e.name, kind: e.kind, file: e.file, status: "waiting" };
  entries = entries.map((x) => (x.id === id ? again : x));
  changed();
  pump();
}

/** The line is done with: saved through the import window, or put away. */
export function forgetRead(id: number) {
  entries = entries.filter((e) => e.id !== id);
  changed();
}

/** On a lock (and in tests): everything is forgotten, and a reading still going says nothing. */
export function resetReads() {
  entries = [];
  generation = {};
  changed();
}

/** Read `file` in front of her for a moment; a scan, or a file still being read after
 *  [`BACKGROUND_AFTER_MS`], goes on in the background ("background"). */
export async function readOrBackground(
  caseId: string,
  file: File,
  kind?: InputKind,
): Promise<{ preview: ImportPreview } | "background"> {
  if (isImageFile(file.name)) {
    readInBackground(caseId, file, kind);
    return "background";
  }
  const work = timed("read", ipc.importDocument(caseId, file), sizeBucket(file.size));
  let timer: ReturnType<typeof setTimeout> | undefined;
  const late = new Promise<"late">((resolve) => {
    timer = setTimeout(() => resolve("late"), BACKGROUND_AFTER_MS);
  });
  try {
    const first = await Promise.race([work.then((preview) => ({ preview })), late]);
    if (first === "late") {
      readInBackground(caseId, file, kind, work);
      return "background";
    }
    return first;
  } finally {
    clearTimeout(timer);
  }
}
