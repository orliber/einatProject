// The case list, laid out like a file explorer (round 47, D-085): a folder tree on the right,
// the path and the search above, and one table of subfolders and cases. Folders nest to any
// depth (the core already keeps `parent_id`); this screen only shows them that way.
import { useMemo, useState, type DragEvent } from "react";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { useApp } from "../App";
import { BackupReminder } from "../components/Backup";
import { OldExportsNotice, RetentionNotice } from "../components/RetentionNotice";
import { TopBar } from "../components/TopBar";
import { NewCaseDialog } from "../components/NewCaseDialog";
import { ageOf } from "../components/AgeField";
import { ActionsMenu, RightClick, RightClickArea, type MenuItem } from "../components/Menu";
import { ErrorLine, Skeleton } from "../components/ui";
import { greeting } from "../i18n/he";
import { ipc, type CaseSummary, type Folder, type UiError } from "../ipc/client";
import { ConfirmDialog, FolderIcon, FolderNameDialog, MoveDialog, PurgeDialog, descendants } from "./library/LibraryDialogs";
import { FolderTree } from "./cases/FolderTree";
import { approvedCount, caseStatus, totalSections, type CaseStatus } from "./cases/caseStatus";
import "./CasesScreen.css";
import { useAi } from "../ai";
import { useShownName } from "../discreet";

const TRASH_DAYS = 30;

/** The folder Einat was in, kept while the vault is open (the page reloads on lock). */
const remembered: { folder: string | null; sort: Sort } = { folder: null, sort: "updated" };
function rememberFolder(id: string | null) {
  remembered.folder = id;
}

type Sort = "updated" | "name" | "status";
const SORTS: { id: Sort; label: string }[] = [
  { id: "updated", label: "עודכן" },
  { id: "name", label: "שם" },
  { id: "status", label: "איפה זה עומד" },
];

function updated(ts: number): string {
  const d = new Date(ts * 1000);
  const today = new Date();
  if (d.toDateString() === today.toDateString()) {
    return `היום, ${d.toLocaleTimeString("he-IL", { hour: "2-digit", minute: "2-digit" })}`;
  }
  const yesterday = new Date(today);
  yesterday.setDate(today.getDate() - 1);
  if (d.toDateString() === yesterday.toDateString()) return "אתמול";
  // Another year is said: a case from last November is not "20.11" beside this month's.
  return d.getFullYear() === today.getFullYear()
    ? d.toLocaleDateString("he-IL", { day: "numeric", month: "numeric" })
    : d.toLocaleDateString("he-IL", { day: "numeric", month: "numeric", year: "numeric" });
}

/** "3 תיקים בעבודה", in Hebrew for one and for none too. */
export function inWorkLine(n: number): string {
  return n === 0 ? "אין תיקים בעבודה." : n === 1 ? "תיק אחד בעבודה." : `${n} תיקים בעבודה.`;
}

/** When a case in the bin goes for good. At 0 it waits only for a backup, so no number is promised. */
export function goesIn(days: number): string {
  return days <= 0 ? "יימחק בקרוב" : days === 1 ? "יימחק מחר" : days === 2 ? "יימחק בעוד יומיים" : `יימחק בעוד ${days} ימים`;
}

/**
 * For every folder: how many cases are inside it at any depth, and when one of them last changed.
 * One pass over the cases (each walks up its folders), instead of a walk of the whole tree per row.
 */
export function folderStats(folders: Folder[], cases: CaseSummary[]): Map<string, { count: number; last: number }> {
  const parent = new Map(folders.map((f) => [f.id, f.parent_id ?? null]));
  const stats = new Map<string, { count: number; last: number }>();
  for (const c of cases) {
    // `hops` ends a loop in the tree, which the core refuses but a damaged vault could hold.
    for (let id = c.folder_id ?? null, hops = 0; id && parent.has(id) && hops <= folders.length; id = parent.get(id) ?? null, hops++) {
      const s = stats.get(id);
      if (s) {
        s.count += 1;
        s.last = Math.max(s.last, c.updated_at);
      } else {
        stats.set(id, { count: 1, last: c.updated_at });
      }
    }
  }
  return stats;
}

const HEBREW = new Intl.Collator("he");

/** A copy of a fetched list; `fetchedAt` only says which fetch it is. */
function fresh<T>(list: T[] | null | undefined, fetchedAt: number): T[] {
  void fetchedAt;
  return [...(list ?? [])];
}

type Modal =
  | { kind: "new-folder" }
  | { kind: "rename"; folder: Folder }
  | { kind: "move-folder"; folder: Folder }
  | { kind: "move-case"; c: CaseSummary }
  | { kind: "delete-folder"; folder: Folder }
  | { kind: "delete-case"; c: CaseSummary }
  | { kind: "purge"; c: CaseSummary };

/** A drag carries one case or one folder. */
type Drag = { case: string } | { folder: string };
const DRAG_TYPE = "application/x-dv-item";
/** The item being dragged now (one drag at a time); a drag over a place it cannot go shows "no drop" there. */
const drag: { now: Drag | null } = { now: null };
function setDragging(d: Drag | null) {
  drag.now = d;
}

/** What a drop carries, or `null` for anything that is not one of ours (a file, text from elsewhere). */
export function readDrag(raw: string): Drag | null {
  try {
    const d: unknown = JSON.parse(raw);
    if (d && typeof d === "object") {
      if (typeof (d as { case?: unknown }).case === "string") return { case: (d as { case: string }).case };
      if (typeof (d as { folder?: unknown }).folder === "string") return { folder: (d as { folder: string }).folder };
    }
  } catch {
    // Not JSON: not ours.
  }
  return null;
}

/**
 * The cases a folder view lists: in a folder, only the cases directly in it, as in a file
 * explorer (a case lives in one place); the cases of its subfolders are inside those folders.
 * A search looks everywhere.
 */
export function casesShown(all: CaseSummary[], current: string | null, matches: ((c: CaseSummary) => boolean) | null): CaseSummary[] {
  return all.filter((c) => (matches ? matches(c) : (c.folder_id ?? null) === current));
}

/**
 * One word, one line and a thin bar of the approved sections (canvas "תיקים": "שהסטטוס יהיה
 * ברור ונקי"; D-204 §2). A case ready to go out is one green chip. The bar is aria-hidden: the
 * line beside it says the same numbers, and a case still at its materials shows it empty.
 */
function StatusCell({ s }: { s: CaseStatus }) {
  if (s.steps[2] === "now") {
    return <span className="chip chip-ok status-ready" title={s.full}><span aria-hidden="true">✓</span> {s.detail} · {s.word}</span>;
  }
  return (
    <div className="status-cell" title={s.full}>
      <div className="status-line">
        <span className={s.attention ? "status-word attention" : "status-word"}>{s.word}</span>
        <span className="small muted">{s.detail}</span>
      </div>
      <progress className="meter" value={s.done} max={s.total} aria-hidden="true" />
    </div>
  );
}

/** The "since last time" line she closed this session, so it stays closed (L4). */
let sinceDismissed: string | null = null;

export function CasesScreen() {
  const { go, fail, status, notify } = useApp();
  const shown = useShownName();
  const ai = useAi();
  const qc = useQueryClient();
  const cases = useQuery({ queryKey: ["cases"], queryFn: ipc.listCases });
  const folders = useQuery({ queryKey: ["folders"], queryFn: ipc.folders });
  const trash = useQuery({ queryKey: ["trash"], queryFn: ipc.listTrash });
  const [folderId, setFolderIdState] = useState<string | null>(remembered.folder);
  const [inTrash, setInTrash] = useState(false);
  const [sinceSeen, setSinceSeenState] = useState(sinceDismissed !== null && sinceDismissed === status.since_last);
  const setSinceSeen = (seen: boolean) => {
    sinceDismissed = seen ? status.since_last : null;
    setSinceSeenState(seen);
  };
  const [query, setQuery] = useState("");
  const [sort, setSortState] = useState<Sort>(remembered.sort);
  const [treeOpen, setTreeOpen] = useState(false);
  const [creating, setCreating] = useState(false);
  const [modal, setModalState] = useState<Modal | null>(null);
  const [dropOn, setDropOn] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  // A new action (its window opens) starts clean: an older refusal does not stay beside it.
  const setModal = (m: Modal | null) => {
    if (m) setError(null);
    setModalState(m);
  };

  // A new list each time it is read again (`dataUpdatedAt`), also when the data came back equal: what is
  // computed from the lists below is redone whenever the core was asked again.
  const allFolders = useMemo(() => fresh(folders.data, folders.dataUpdatedAt), [folders.data, folders.dataUpdatedAt]);
  const allCases = useMemo(() => fresh(cases.data, cases.dataUpdatedAt), [cases.data, cases.dataUpdatedAt]);
  // A folder that no longer exists (deleted elsewhere): back to the top.
  const current = folderId && allFolders.some((f) => f.id === folderId) ? folderId : null;
  // In a narrow window the tree folds away after a pick; focus goes to the button that reopens it.
  const closeTree = () => {
    if (!treeOpen) return;
    setTreeOpen(false);
    setTimeout(() => document.querySelector<HTMLElement>(".tree-toggle")?.focus(), 0);
  };
  const setFolderId = (id: string | null) => {
    rememberFolder(id);
    setFolderIdState(id);
    setInTrash(false);
    // Going to a folder ends a search; otherwise the list would still show the search results.
    setQuery("");
    closeTree();
  };
  const setSort = (s: Sort) => {
    remembered.sort = s;
    setSortState(s);
  };

  const refreshAll = () => Promise.all(["cases", "folders", "trash"].map((k) => qc.invalidateQueries({ queryKey: [k] })));
  const sample = useMutation({
    mutationFn: ipc.createSampleCase,
    onSuccess: (id) => { void refreshAll(); go({ name: "case", id, view: "materials" }); },
    onError: (e: UiError) => setError(fail(e)),
  });
  const act = useMutation({
    mutationFn: (f: () => Promise<unknown>) => f(),
    onSuccess: () => refreshAll(),
    onError: (e: unknown) => setError(fail(e as UiError)),
  });
  /** Runs an action and refreshes the list; resolves `false` when it failed (the error is already shown). */
  const run = (f: () => Promise<unknown>, done?: string): Promise<boolean> => {
    // An older refusal does not stay beside a new try, and the same refusal is said again.
    setError(null);
    return act.mutateAsync(f).then(
      () => {
        if (done) notify(done);
        return true;
      },
      () => false,
    );
  };

  const byId = useMemo(() => new Map(allFolders.map((f) => [f.id, f])), [allFolders]);
  const stats = useMemo(() => folderStats(allFolders, allCases), [allFolders, allCases]);
  const path: Folder[] = [];
  for (let f = current ? byId.get(current) : undefined; f; f = f.parent_id ? byId.get(f.parent_id) : undefined) path.unshift(f);
  const pathOf = (id: string | null): string => {
    const names: string[] = [];
    for (let f = id ? byId.get(id) : undefined; f; f = f.parent_id ? byId.get(f.parent_id) : undefined) names.unshift(f.name);
    return names.join(" › ");
  };
  /** Cases inside a folder, at any depth. */
  const countIn = (id: string) => stats.get(id)?.count ?? 0;
  /** When a case inside a folder (at any depth) last changed: the folder's "עודכן"; `null` when empty. */
  const lastIn = (id: string): number | null => stats.get(id)?.last ?? null;
  const parentOf = current ? (byId.get(current)?.parent_id ?? null) : null;

  const q = query.trim().toLowerCase();
  // Filtered and sorted once per change of the list, the folder, the search or the sort: a drag
  // over the rows or a notice re-renders the screen without redoing this.
  const { shownFolders, shownCases, statusOf } = useMemo(() => {
    const matches = (c: CaseSummary) => (c.child_name ?? "").toLowerCase().includes(q) || c.meta.code.toLowerCase().includes(q);
    const shownFolders = q ? allFolders.filter((f) => f.name.toLowerCase().includes(q)) : allFolders.filter((f) => f.parent_id === current);
    // A new array (filter), so sorting it below never touches the query's own data.
    const shownCases = casesShown(allCases, current, q ? matches : null);
    shownFolders.sort((a, b) => HEBREW.compare(a.name, b.name));
    const statusOf = new Map(shownCases.map((c) => [c.id, caseStatus(c)]));
    const st = (c: CaseSummary) => statusOf.get(c.id) ?? caseStatus(c);
    const rank = (s: CaseStatus) => (s.attention ? 0 : s.steps[2] === "now" ? 1 : s.steps[1] === "now" ? 2 : 3);
    shownCases.sort((a, b) =>
      sort === "name" ? HEBREW.compare(a.child_name ?? a.meta.code, b.child_name ?? b.meta.code)
        : sort === "status" ? rank(st(a)) - rank(st(b)) || b.updated_at - a.updated_at
          : b.updated_at - a.updated_at);
    return { shownFolders, shownCases, statusOf };
  }, [allCases, allFolders, current, q, sort]);
  const st = (c: CaseSummary) => statusOf.get(c.id) ?? caseStatus(c);

  // "ד\"ר רותם בדויה" → "רותם": a title is not how anyone is greeted.
  const name = status.practitioner[0]?.split(" ").find((w) => w && !/^(ד["״']?ר|דר'|פרופ'|גב'|מר)$/.test(w));
  // The case she worked on last and has not finished: one click back into it.
  const { inProgress, resume } = useMemo(() => {
    const open = allCases.filter((c) => approvedCount(c) < totalSections(c));
    return { inProgress: open.length, resume: [...open].sort((a, b) => b.updated_at - a.updated_at)[0] };
  }, [allCases]);
  const resumeStatus = resume ? caseStatus(resume) : null;
  const trashCount = trash.data?.length ?? 0;

  // ---------------------------------------------------------------- moving
  // Discreet mode (D-192): initials in place of the child's name, in the list and in every notice.
  const caseName = (c: CaseSummary) => (c.child_name ? shown(c.child_name) : c.meta.code);
  const placeName = (id: string | null) => (id ? `"${byId.get(id)?.name ?? ""}"` : "הרמה העליונה");
  /** Moves a case (never copies: a case has one folder), with "ביטול" on the notice. Throws when refused. */
  const moveCase = async (c: CaseSummary, to: string | null) => {
    const from = c.folder_id ?? null;
    if (from === to) return;
    setError(null);
    await ipc.moveCase(c.id, to);
    await refreshAll();
    notify(`התיק של ${caseName(c)} הועבר ל${placeName(to)}.`, { label: "ביטול", run: () => void run(() => ipc.moveCase(c.id, from), "ההעברה בוטלה.") });
  };
  /** Whether `f` may go into `to`: not into itself, nor into a folder inside it. */
  const folderFits = (f: Folder, to: string | null) => !(to && descendants(allFolders, f.id).has(to));
  const moveFolder = async (f: Folder, to: string | null) => {
    const from = f.parent_id ?? null;
    if (from === to || !folderFits(f, to)) return;
    setError(null);
    await ipc.moveFolder(f.id, to);
    await refreshAll();
    notify(`התיקייה "${f.name}" הועברה ל${placeName(to)}.`, { label: "ביטול", run: () => void run(() => ipc.moveFolder(f.id, from), "ההעברה בוטלה.") });
  };
  const dropFailed = (e: unknown) => setError(fail(e as UiError));

  // ---------------------------------------------------------------- drag and drop
  const onDragStart = (d: Drag) => (e: DragEvent) => {
    setDragging(d);
    e.dataTransfer.setData(DRAG_TYPE, JSON.stringify(d));
    e.dataTransfer.effectAllowed = "move";
  };
  const endDrag = () => {
    setDragging(null);
    setDropOn(null);
  };
  /** Where a drop on `target` would move the item: `false` when it would change nothing or is not allowed. */
  const fits = (d: Drag, target: string | null) => {
    if ("case" in d) {
      const c = allCases.find((x) => x.id === d.case);
      return !!c && (c.folder_id ?? null) !== target;
    }
    const f = byId.get(d.folder);
    return !!f && (f.parent_id ?? null) !== target && folderFits(f, target);
  };
  const dropZone = (key: string, accepts: (d: Drag) => boolean, drop: (d: Drag) => void) => ({
    onDragOver: (e: DragEvent) => {
      if (!e.dataTransfer.types.includes(DRAG_TYPE)) return;
      const d = drag.now;
      if (d && !accepts(d)) return;
      e.preventDefault();
      e.dataTransfer.dropEffect = "move";
      if (dropOn !== key) setDropOn(key);
    },
    // Moving over a child of the row (its icon, its name) is not leaving it.
    onDragLeave: (e: DragEvent) => {
      if (!(e.currentTarget as Node).contains(e.relatedTarget as Node | null)) setDropOn((on) => (on === key ? null : on));
    },
    onDrop: (e: DragEvent) => {
      e.preventDefault();
      const d = drag.now ?? readDrag(e.dataTransfer.getData(DRAG_TYPE));
      endDrag();
      if (d && accepts(d)) drop(d);
    },
  });
  const dropProps = (target: string | null) =>
    dropZone(target ?? "top", (d) => fits(d, target), (d) => {
      if ("case" in d) {
        const c = allCases.find((x) => x.id === d.case);
        if (c) moveCase(c, target).catch(dropFailed);
      } else {
        const f = byId.get(d.folder);
        if (f) moveFolder(f, target).catch(dropFailed);
      }
    });
  // A case or a folder dropped on the recycle bin: the same question as from its menu.
  const trashDropProps = dropZone("trash", () => true, (d) => {
    if ("case" in d) {
      const c = allCases.find((x) => x.id === d.case);
      if (c) setModal({ kind: "delete-case", c });
    } else {
      const f = byId.get(d.folder);
      if (f) setModal({ kind: "delete-folder", folder: f });
    }
  });

  // ---------------------------------------------------------------- actions
  const folderItems = (f: Folder): MenuItem[] => [
    { label: "פתיחה", run: () => setFolderId(f.id) },
    { label: "תיקייה חדשה בפנים", run: () => { setFolderId(f.id); setModal({ kind: "new-folder" }); } },
    { label: "שינוי שם", run: () => setModal({ kind: "rename", folder: f }) },
    { label: "העברה לתיקייה אחרת…", run: () => setModal({ kind: "move-folder", folder: f }) },
    { label: "מחיקת התיקייה", run: () => setModal({ kind: "delete-folder", folder: f }), danger: true },
  ];
  const caseItems = (c: CaseSummary): MenuItem[] => [
    { label: "פתיחה", run: () => go({ name: "case", id: c.id, view: "materials" }) },
    { label: "העברה לתיקייה…", run: () => setModal({ kind: "move-case", c }) },
    { label: "אבחון מעקב לילד/ה הזה/ו", run: () => void followUp(c) },
    { label: "העברה לסל המחזור", run: () => setModal({ kind: "delete-case", c }), danger: true },
  ];
  /** The right-click actions of the row under the pointer: one menu for the whole table. */
  const rowMenu = (target: Element): MenuItem[] | null => {
    const row = target.closest<HTMLElement>("tr[data-folder], tr[data-case]");
    const f = row?.dataset.folder && byId.get(row.dataset.folder);
    if (f) return folderItems(f);
    const c = row?.dataset.case && allCases.find((x) => x.id === row.dataset.case);
    return c ? caseItems(c) : null;
  };
  /** D-029: a new case for the follow-up, with the same names; it opens on its details. */
  async function followUp(c: CaseSummary) {
    setError(null);
    try {
      const id = await ipc.createFollowUp(c.id);
      await refreshAll();
      notify(`נפתח תיק מעקב ל${caseName(c)}. לעדכן גיל ולרשום הסכמה חדשה.`);
      go({ name: "case", id, view: "details" });
    } catch (e) {
      setError(fail(e as never));
    }
  }

  // The first-case card stands in for the whole explorer, the only way into the recycle bin. So it
  // shows only once all three lists arrived and all are empty: a deleted last case stays reachable.
  const noCasesYet = !!cases.data && folders.isFetched && trash.isFetched
    && allCases.length === 0 && allFolders.length === 0 && trashCount === 0;
  const emptyHere = !!cases.data && !noCasesYet && shownCases.length === 0 && shownFolders.length === 0;

  return (
    <div className="page">
      <TopBar active="cases" />
      <main className="page-main cases-main">
        <div className="page-head">
          <div className="stack" style={{ gap: 4 }}>
            <h1>{greeting()}{name ? `, ${name}` : ""}</h1>
            <p className="muted">{cases.data ? (allCases.length ? inWorkLine(inProgress) : "עוד אין תיקים") : " "}</p>
          </div>
          <div className="row head-actions">
            <button type="button" className="btn btn-primary btn-big" onClick={() => setCreating(true)}>+ תיק חדש</button>
          </div>
        </div>
        {resume && resumeStatus && !q && !inTrash && (
          // D-204 §2: the one prominent button of the list. It opens the report on the section it
          // names, so "הסעיף הבא" is where she lands. A warning word stays on the card.
          <section className="paper resume" aria-labelledby="resume-title">
            <div className="resume-body">
              <h2 id="resume-title" className="resume-eyebrow">להמשיך איפה שהפסקת</h2>
              <div className="resume-who">
                <strong className="serif resume-name">{caseName(resume)}</strong>
                <span className="muted">{[ageOf(resume.meta.age, resume.meta.child_gender), resume.folder_id ? pathOf(resume.folder_id) : ""].filter(Boolean).join(" · ")}</span>
              </div>
              {(resumeStatus.attention || resumeStatus.view !== "report") && (
                <div className="status-line">
                  <span className={resumeStatus.attention ? "status-word attention" : "status-word"}>{resumeStatus.word}</span>
                  <span className="muted">{resumeStatus.detail}</span>
                </div>
              )}
              {resumeStatus.view === "report" && (
                <div className="resume-progress">
                  <progress className="meter meter-lg" value={resumeStatus.done} max={resumeStatus.total} aria-hidden="true" />
                  <span>{resumeStatus.done} מתוך {resumeStatus.total} סעיפים אושרו</span>
                </div>
              )}
              {resumeStatus.view === "report" && resume.next_section && (
                <p className="resume-next">הסעיף הבא: <strong>{resume.next_section.title}</strong></p>
              )}
            </div>
            <button type="button" className="btn btn-primary btn-big"
              onClick={() => go({
                name: "case", id: resume.id, view: resumeStatus.view,
                ...(resumeStatus.view === "report" && resume.next_section ? { section: resume.next_section.key } : {}),
              })}>
              {resumeStatus.view === "report" ? "להמשיך לכתוב ←" : "להמשיך ←"}
            </button>
          </section>
        )}
        {status.integrity_warning && (
          <ErrorLine error={`בדיקת השלמות של הכספת מצאה חריגה: ${status.integrity_warning}`} report />
        )}
        {status.since_last && !sinceSeen && (
          // What happened while she was away (L4): said once, only when there is something.
          <div className="backup-reminder" role="status">
            <span className="grow">{status.since_last}</span>
            <button type="button" className="btn btn-small" onClick={() => setSinceSeen(true)}>הבנתי</button>
          </div>
        )}
        <BackupReminder />
        <RetentionNotice />
        <OldExportsNotice />
        <ErrorLine error={error ?? (cases.error ? fail(cases.error as unknown as UiError) : null)}
          retry={!error && cases.error ? () => void qc.invalidateQueries({ queryKey: ["cases"] }) : undefined} />

        {noCasesYet ? (
          <div className="card empty">
            <h2>התיק הראשון</h2>
            <p className="muted">פותחים תיק, מוסיפים חומרים, ו-{ai} מציע טיוטה לכל סעיף.</p>
            <div className="row wrap-row">
              <button type="button" className="btn btn-primary" onClick={() => setCreating(true)}>פתיחת תיק</button>
              <button type="button" className="btn" disabled={sample.isPending} onClick={() => sample.mutate()}>לנסות קודם על תיק לדוגמה</button>
            </div>
            <p className="small muted">תיק בדוי קצר, כדי לראות איך זה עובד. אפשר למחוק אותו בכל רגע.</p>
          </div>
        ) : (
          <div className="card explorer" onDragEnd={endDrag}>
            <aside className={treeOpen ? "explorer-tree open" : "explorer-tree"}>
              <FolderTree folders={allFolders} current={current} inTrash={inTrash} trashCount={trashCount} total={allCases.length}
                countIn={countIn} onPick={setFolderId} onPickTrash={() => { setInTrash(true); setQuery(""); closeTree(); }}
                onNewFolder={() => setModal({ kind: "new-folder" })}
                dropProps={dropProps} trashDropProps={trashDropProps} dropOn={dropOn} onDragStart={(id) => onDragStart({ folder: id })}
                rightClick={(f, row) => <RightClick items={folderItems(f)}>{row}</RightClick>} />
            </aside>

            <div className="explorer-main">
              <div className="library-bar">
                <button type="button" className="btn tree-toggle" aria-expanded={treeOpen} onClick={() => setTreeOpen(!treeOpen)}>
                  <FolderIcon size={16} /> תיקיות
                </button>
                {/* D-113: at the top the tab, the tree and the path all said "תיקים"; the path
                    shows only inside a folder or the bin, where it says something. */}
                {(inTrash || current !== null) && (
                  <button type="button" className="btn btn-icon up" aria-label="לתיקייה שמעל"
                    onClick={() => (inTrash ? setFolderId(remembered.folder) : setFolderId(parentOf))}>↑</button>
                )}
                {inTrash || current !== null ? (
                  <nav className="crumbs" aria-label="מיקום">
                    <button type="button" className={dropOn === "top" ? "crumb drop" : "crumb"}
                      onClick={() => setFolderId(null)} {...dropProps(null)}>תיקים</button>
                    {!inTrash && path.map((f) => (
                      <span key={f.id} className="crumb-wrap">
                        <span className="crumb-sep" aria-hidden="true">›</span>
                        <button type="button" className={dropOn === f.id ? "crumb drop" : "crumb"} aria-current={f.id === current ? "page" : undefined}
                          onClick={() => setFolderId(f.id)} {...dropProps(f.id)}>{f.name}</button>
                      </span>
                    ))}
                    {inTrash && <span className="crumb-wrap"><span className="crumb-sep" aria-hidden="true">›</span><span className="crumb" aria-current="page">סל המחזור</span></span>}
                  </nav>
                ) : (
                  // Holds the place of the path, so the search and the buttons do not move.
                  <span className="crumbs" aria-hidden="true" />
                )}
                <input className="input search" type="search" placeholder="חיפוש בכל התיקים" aria-label="חיפוש תיקים"
                  value={query} onChange={(e) => { setQuery(e.target.value); setInTrash(false); }} />
                {!inTrash && (
                  <label className="sort">
                    <select className="input" value={sort} onChange={(e) => setSort(e.target.value as Sort)} aria-label="מיון">
                      {SORTS.map((s) => <option key={s.id} value={s.id}>מיון: {s.label}</option>)}
                    </select>
                  </label>
                )}
                {!inTrash && (
                  <button type="button" className="btn" onClick={() => setModal({ kind: "new-folder" })}><FolderIcon size={16} /> תיקייה כאן</button>
                )}
              </div>

              {inTrash ? (
                <TrashList cases={trash.data ?? []} pathOf={pathOf}
                  onRestore={(c) => {
                    const to = c.folder_id && byId.has(c.folder_id) ? placeName(c.folder_id) : "הרמה העליונה";
                    void run(() => ipc.restoreCase(c.id), `התיק שוחזר אל ${to}.`).then((ok) => {
                      // The last case restored: back to the list, where it now is.
                      if (ok && (trash.data?.length ?? 0) <= 1) setInTrash(false);
                    });
                  }}
                  onPurge={(c) => setModal({ kind: "purge", c })} />
              ) : (
                <>
                  {!cases.data && !cases.error && (
                    <div className="stack" style={{ gap: 12, padding: 16 }}>
                      <Skeleton card lines={4} label="טוען את התיקים" />
                    </div>
                  )}
                  {(shownFolders.length > 0 || shownCases.length > 0) && (
                    <table className="cases">
                      <thead>
                        <tr>
                          <th>שם</th>
                          <th>איפה זה עומד</th>
                          <th className="col-when">עודכן</th>
                          <th className="col-menu"><span className="visually-hidden">פעולות</span></th>
                        </tr>
                      </thead>
                      <RightClickArea itemsAt={rowMenu}>
                        <tbody>
                          {shownFolders.map((f) => {
                            const n = countIn(f.id);
                            const sub = allFolders.filter((x) => x.parent_id === f.id).map((x) => x.name);
                            const last = lastIn(f.id);
                            return (
                              <tr key={f.id} data-folder={f.id} className={dropOn === f.id ? "folder-row drop" : "folder-row"} onClick={() => setFolderId(f.id)} draggable
                                onDragStart={onDragStart({ folder: f.id })} {...dropProps(f.id)}>
                                <td>
                                  <div className="name-cell">
                                    <span className="row-icon folder-icon"><FolderIcon size={18} /></span>
                                    <div className="min0">
                                      {/* D-113: the row opens the folder; this is its one stop on the
                                          keyboard (Tab, then Enter), with no "פתיחה" button beside it.
                                          Draggable itself: WebKit (the Mac app) may not start a drag
                                          from inside a button; its dragstart bubbles to the row's. */}
                                      <button type="button" className="case-name folder-name" aria-label={`פתיחת התיקייה ${f.name}`} draggable
                                        onClick={(e) => { e.stopPropagation(); setFolderId(f.id); }}>{f.name}</button>
                                      <div className="small muted">
                                        {n === 0 ? "ריקה" : n === 1 ? "תיק אחד" : `${n} תיקים`}
                                        {q && f.parent_id ? ` · ${pathOf(f.parent_id)}` : ""}
                                        {last !== null && <span className="when-inline"> · {updated(last)}</span>}
                                      </div>
                                    </div>
                                  </div>
                                </td>
                                <td><span className="small muted">{sub.length ? `בפנים: ${sub.join(", ")}` : ""}</span></td>
                                {/* The last change to a case inside it; nothing for an empty folder. */}
                                <td className="small muted col-when">{last !== null ? updated(last) : ""}</td>
                                <td>
                                  <div className="row-actions">
                                    <ActionsMenu items={folderItems(f)} label={`פעולות על התיקייה ${f.name}`} />
                                  </div>
                                </td>
                              </tr>
                            );
                          })}
                          {shownCases.map((c) => {
                            const s = st(c);
                            const open = () => go({ name: "case", id: c.id, view: s.view });
                            const where = c.folder_id && c.folder_id !== current ? pathOf(c.folder_id) : "";
                            return (
                              <tr key={c.id} data-case={c.id} onClick={open} className="case-row" draggable onDragStart={onDragStart({ case: c.id })}>
                                <td>
                                  <div className="name-cell">
                                    <span className="row-icon">
                                      <svg width="18" height="18" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.8" strokeLinejoin="round" aria-hidden="true">
                                        <path d="M6 3h8l4 4v14H6z" /><path d="M14 3v4h4M9 12h6M9 16h6" />
                                      </svg>
                                    </span>
                                    <div className="min0">
                                      {/* D-204 §2, as D-113 did for a folder: the name is the row's one stop
                                          on the keyboard, and its name for a screen reader keeps the verb.
                                          "(ללא שם)" keeps the words she sees inside that name (WCAG 2.5.3). */}
                                      <div className="case-title">
                                        <button type="button" className="case-name case-open serif" draggable
                                          aria-label={`${s.button} בתיק של ${caseName(c)}${c.child_name ? "" : " (ללא שם)"}`}
                                          onClick={(e) => { e.stopPropagation(); open(); }}>
                                          {c.child_name ? shown(c.child_name) : "ללא שם"}
                                        </button>
                                        {c.meta.follows && <span className="chip chip-sand follow-chip">מעקב</span>}
                                      </div>
                                      <div className="small muted ellipsis">
                                        {[ageOf(c.meta.age, c.meta.child_gender), c.meta.code || "ללא קוד", where].filter(Boolean).join(" · ")}
                                        <span className="when-inline"> · {updated(c.updated_at)}</span>
                                      </div>
                                    </div>
                                  </div>
                                </td>
                                <td><StatusCell s={s} /></td>
                                <td className="small muted col-when">{updated(c.updated_at)}</td>
                                <td>
                                  <div className="row-actions">
                                    <ActionsMenu items={caseItems(c)} label={`פעולות על התיק של ${caseName(c)}`} />
                                  </div>
                                </td>
                              </tr>
                            );
                          })}
                        </tbody>
                      </RightClickArea>
                    </table>
                  )}
                  {emptyHere && (
                    <p className="muted empty-folder">{q ? "לא נמצא תיק או תיקייה בשם הזה." : "התיקייה ריקה. גוררים לכאן תיקים, או פותחים תיק חדש."}</p>
                  )}
                  {cases.data && !emptyHere && (
                    <div className="explorer-foot small muted">
                      <span>{shownCases.length === 1 ? "תיק אחד" : `${shownCases.length} תיקים`}{q ? " בכל התיקיות" : ""}</span>
                    </div>
                  )}
                </>
              )}
            </div>
          </div>
        )}
        <p className="small muted">השמות נשארים במחשב. {ai} מקבל תפקידים ("הילד", "הגננת") במקום שמות.</p>
      </main>

      {creating && (
        <NewCaseDialog folderId={current} onClose={() => setCreating(false)}
          onCreated={(id) => { void refreshAll(); go({ name: "case", id, view: "materials" }); }} />
      )}
      {modal?.kind === "new-folder" && (
        <FolderNameDialog title={current ? `תיקייה חדשה בתוך "${byId.get(current)?.name ?? ""}"` : "תיקייה חדשה"} onClose={() => setModal(null)}
          onSave={async (n) => { await ipc.createFolder(current, n); await refreshAll(); setModal(null); setInTrash(false); }} />
      )}
      {modal?.kind === "rename" && (
        <FolderNameDialog title="שינוי שם התיקייה" initial={modal.folder.name} onClose={() => setModal(null)}
          onSave={async (n) => { await ipc.renameFolder(modal.folder.id, n); await refreshAll(); setModal(null); }} />
      )}
      {modal?.kind === "move-folder" && (
        <MoveDialog what={`התיקייה "${modal.folder.name}"`} folders={allFolders} current={modal.folder.parent_id} exclude={descendants(allFolders, modal.folder.id)}
          onClose={() => setModal(null)}
          onMove={async (t) => { await moveFolder(modal.folder, t); setModal(null); }} />
      )}
      {modal?.kind === "move-case" && (
        <MoveDialog what={`התיק של ${caseName(modal.c)}`} folders={allFolders} current={modal.c.folder_id}
          onClose={() => setModal(null)}
          onMove={async (t) => { await moveCase(modal.c, t); setModal(null); }} />
      )}
      {modal?.kind === "delete-folder" && (
        <ConfirmDialog title={`מחיקת התיקייה "${modal.folder.name}"`} action="מחיקת התיקייה"
          text={countIn(modal.folder.id) > 0 || allFolders.some((f) => f.parent_id === modal.folder.id)
            ? "רק התיקייה נמחקת. מה שבתוכה עובר לתיקייה שמעליה."
            : "התיקייה ריקה ותימחק."}
          onClose={() => setModal(null)}
          onConfirm={async () => {
            await ipc.deleteFolder(modal.folder.id);
            // She was inside it: she stays where its contents went, one level up.
            if (current === modal.folder.id) setFolderId(modal.folder.parent_id);
            await refreshAll();
            setModal(null);
          }} />
      )}
      {modal?.kind === "delete-case" && (
        <ConfirmDialog title={`העברת התיק של ${caseName(modal.c)} לסל המחזור`} action="העברה לסל המחזור" danger
          text={`התיק נשמר בסל ${TRASH_DAYS} יום, מוצפן, ואפשר לשחזר אותו. אחר כך נמחק לצמיתות.`}
          onClose={() => setModal(null)}
          onConfirm={async () => {
            const id = modal.c.id;
            await ipc.deleteCase(id);
            await refreshAll();
            setModal(null);
            notify("התיק הועבר לסל המחזור.", { label: "ביטול", run: () => void run(() => ipc.restoreCase(id), "התיק שוחזר.") });
          }} />
      )}
      {modal?.kind === "purge" && (
        <PurgeDialog name={`התיק של ${caseName(modal.c)}`} onClose={() => setModal(null)}
          onPurge={async (pw) => { await ipc.purgeCase(modal.c.id, pw); await refreshAll(); setModal(null); notify("התיק נמחק לצמיתות."); }} />
      )}
    </div>
  );
}

function TrashList(props: { cases: CaseSummary[]; pathOf: (id: string | null) => string; onRestore: (c: CaseSummary) => void; onPurge: (c: CaseSummary) => void }) {
  const shown = useShownName();
  const [now] = useState(() => Date.now() / 1000);
  const daysLeft = (c: CaseSummary) => Math.max(0, TRASH_DAYS - Math.floor((now - (c.deleted_at ?? 0)) / 86_400));
  if (!props.cases.length) {
    return <p className="muted empty-folder">סל המחזור ריק. תיק שנמחק נשמר כאן {TRASH_DAYS} יום.</p>;
  }
  return (
    <>
      <p className="small muted trash-note">נמחקים לצמיתות אחרי {TRASH_DAYS} יום, ורק אחרי שנשמר גיבוי. מחיקה מוקדמת דורשת סיסמה.</p>
      <table className="cases">
        <thead><tr><th>שם</th><th>נמחק</th><th className="col-go"><span className="visually-hidden">פעולות</span></th></tr></thead>
        <tbody>
          {props.cases.map((c) => (
            <tr key={c.id}>
              <td>
                <div className="case-name serif">{c.child_name ? shown(c.child_name) : "ללא שם"}</div>
                <div className="small muted">{[c.meta.code, c.folder_id && props.pathOf(c.folder_id) ? `היה ב: ${props.pathOf(c.folder_id)}` : ""].filter(Boolean).join(" · ")}</div>
              </td>
              <td className="small">{c.deleted_at ? updated(c.deleted_at) : ""} · {goesIn(daysLeft(c))}</td>
              <td>
                <div className="row-actions">
                  <button type="button" className="btn btn-primary" onClick={() => props.onRestore(c)}>שחזור</button>
                  <button type="button" className="btn btn-danger-quiet" onClick={() => props.onPurge(c)}>מחיקה לצמיתות</button>
                </div>
              </td>
            </tr>
          ))}
        </tbody>
      </table>
    </>
  );
}
