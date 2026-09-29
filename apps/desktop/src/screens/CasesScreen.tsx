import { useMemo, useState, type DragEvent } from "react";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { useApp } from "../App";
import { BackupReminder } from "../components/Backup";
import { TopBar } from "../components/TopBar";
import { NewCaseDialog } from "../components/NewCaseDialog";
import { ageWords } from "../components/AgeField";
import { ActionsMenu, RightClick, type MenuItem } from "../components/Menu";
import { ErrorLine } from "../components/ui";
import { greeting } from "../i18n/he";
import { ipc, type CaseSummary, type Folder, type UiError } from "../ipc/client";
import { ConfirmDialog, FolderIcon, FolderNameDialog, MoveDialog, PurgeDialog, descendants } from "./library/LibraryDialogs";
import "./CasesScreen.css";

const TOTAL_SECTIONS = 16;
const TRASH_DAYS = 30;

/** The folder Einat was in, kept while the vault is open (the page reloads on lock). */
const remembered: { folder: string | null } = { folder: null };
function rememberFolder(id: string | null) {
  remembered.folder = id;
}

/** What to do next in a case, in one chip and one line (canvas "תיקים"). */
function nextStep(c: CaseSummary): { chip: string; cls: string; text: string; start: boolean } {
  const n = c.approved_sections.length;
  if (!c.meta.consent) return { chip: "הסכמה", cls: "chip chip-warn", text: "לרשום את הסכמת ההורים", start: false };
  if (n >= TOTAL_SECTIONS) return { chip: "✓ מוכן", cls: "chip chip-ok", text: "להפיק דוח Word", start: false };
  if (n > 0) return { chip: "בכתיבה", cls: "chip", text: `לאשר עוד ${TOTAL_SECTIONS - n} סעיפים`, start: false };
  return { chip: "חומרים", cls: "chip chip-empty", text: "להוסיף חומרים ולהכין טיוטה", start: true };
}

function updated(ts: number): string {
  const d = new Date(ts * 1000);
  const today = new Date();
  if (d.toDateString() === today.toDateString()) {
    return `היום, ${d.toLocaleTimeString("he-IL", { hour: "2-digit", minute: "2-digit" })}`;
  }
  return d.toLocaleDateString("he-IL", { day: "numeric", month: "numeric" });
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

export function CasesScreen() {
  const { go, fail, status, notify } = useApp();
  const qc = useQueryClient();
  const cases = useQuery({ queryKey: ["cases"], queryFn: ipc.listCases });
  const folders = useQuery({ queryKey: ["folders"], queryFn: ipc.folders });
  const trash = useQuery({ queryKey: ["trash"], queryFn: ipc.listTrash });
  const [folderId, setFolderIdState] = useState<string | null>(remembered.folder);
  const [inTrash, setInTrash] = useState(false);
  const [query, setQuery] = useState("");
  const [creating, setCreating] = useState(false);
  const [modal, setModal] = useState<Modal | null>(null);
  const [dropOn, setDropOn] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);

  const allFolders = useMemo(() => folders.data ?? [], [folders.data]);
  const allCases = useMemo(() => cases.data ?? [], [cases.data]);
  // A folder that no longer exists (deleted elsewhere): back to the top.
  const current = folderId && allFolders.some((f) => f.id === folderId) ? folderId : null;
  const setFolderId = (id: string | null) => {
    rememberFolder(id);
    setFolderIdState(id);
    setInTrash(false);
  };

  const refreshAll = () => Promise.all(["cases", "folders", "trash"].map((k) => qc.invalidateQueries({ queryKey: [k] })));
  const act = useMutation({
    mutationFn: (f: () => Promise<unknown>) => f(),
    onSuccess: () => refreshAll(),
    onError: (e: unknown) => setError(fail(e as UiError)),
  });
  const run = (f: () => Promise<unknown>, done?: string) =>
    act.mutateAsync(f).then(() => {
      if (done) notify(done);
    });

  const byId = new Map(allFolders.map((f) => [f.id, f]));
  const path: Folder[] = [];
  for (let f = current ? byId.get(current) : undefined; f; f = f.parent_id ? byId.get(f.parent_id) : undefined) path.unshift(f);
  const pathOf = (id: string | null): string => {
    const names: string[] = [];
    for (let f = id ? byId.get(id) : undefined; f; f = f.parent_id ? byId.get(f.parent_id) : undefined) names.unshift(f.name);
    return names.join(" › ");
  };
  /** Cases inside a folder, at any depth. */
  const countIn = (id: string) => {
    const ids = descendants(allFolders, id);
    return allCases.filter((c) => c.folder_id && ids.has(c.folder_id)).length;
  };

  const q = query.trim().toLowerCase();
  const matches = (c: CaseSummary) => !q || (c.child_name ?? "").toLowerCase().includes(q) || c.meta.code.toLowerCase().includes(q);
  const shownFolders = q ? allFolders.filter((f) => f.name.toLowerCase().includes(q)) : allFolders.filter((f) => f.parent_id === current);
  const shownCases = q ? allCases.filter(matches) : allCases.filter((c) => (c.folder_id ?? null) === current);
  shownFolders.sort((a, b) => a.name.localeCompare(b.name, "he"));

  // "ד\"ר רותם בדויה" → "רותם": a title is not how anyone is greeted.
  const name = status.practitioner[0]?.split(" ").find((w) => w && !/^(ד["״']?ר|דר'|פרופ'|גב'|מר)$/.test(w));
  const inProgress = allCases.filter((c) => c.approved_sections.length < TOTAL_SECTIONS).length;
  const trashCount = trash.data?.length ?? 0;

  // ---------------------------------------------------------------- drag and drop
  const onDragStart = (d: Drag) => (e: DragEvent) => {
    e.dataTransfer.setData("application/x-dv-item", JSON.stringify(d));
    e.dataTransfer.effectAllowed = "move";
  };
  const dropProps = (target: string | null) => ({
    onDragOver: (e: DragEvent) => {
      if (e.dataTransfer.types.includes("application/x-dv-item")) {
        e.preventDefault();
        setDropOn(target ?? "top");
      }
    },
    onDragLeave: () => setDropOn(null),
    onDrop: (e: DragEvent) => {
      e.preventDefault();
      setDropOn(null);
      const raw = e.dataTransfer.getData("application/x-dv-item");
      if (!raw) return;
      const d = JSON.parse(raw) as Drag;
      if ("case" in d) void run(() => ipc.moveCase(d.case, target), "התיק הועבר.");
      else if (d.folder !== target && !(target && descendants(allFolders, d.folder).has(target))) void run(() => ipc.moveFolder(d.folder, target), "התיקייה הועברה.");
    },
  });

  // ---------------------------------------------------------------- actions
  const folderItems = (f: Folder): MenuItem[] => [
    { label: "פתיחה", run: () => setFolderId(f.id) },
    { label: "שינוי שם", run: () => setModal({ kind: "rename", folder: f }) },
    { label: "העברה לתיקייה אחרת…", run: () => setModal({ kind: "move-folder", folder: f }) },
    { label: "מחיקת התיקייה", run: () => setModal({ kind: "delete-folder", folder: f }), danger: true },
  ];
  const caseItems = (c: CaseSummary): MenuItem[] => [
    { label: "פתיחה", run: () => go({ name: "case", id: c.id, view: "materials" }) },
    { label: "העברה לתיקייה…", run: () => setModal({ kind: "move-case", c }) },
    { label: "העברה לסל המחזור", run: () => setModal({ kind: "delete-case", c }), danger: true },
  ];
  const caseName = (c: CaseSummary) => c.child_name ?? c.meta.code;

  return (
    <div className="page">
      <TopBar active="cases" />
      <main className="page-main">
        <div className="page-head">
          <div className="stack" style={{ gap: 4 }}>
            <h1>{greeting()}{name ? `, ${name}` : ""}</h1>
            <p className="muted">{cases.data ? (allCases.length ? `${inProgress} תיקים בעבודה.` : "עוד אין תיקים") : " "}</p>
          </div>
          <div className="row head-actions">
            <button type="button" className="btn btn-big" onClick={() => setModal({ kind: "new-folder" })}><FolderIcon size={17} /> תיקייה חדשה</button>
            <button type="button" className="btn btn-primary btn-big" onClick={() => setCreating(true)}>+ תיק חדש</button>
          </div>
        </div>
        {status.integrity_warning && (
          <p className="error" role="alert">בדיקת השלמות של הכספת מצאה חריגה: {status.integrity_warning}</p>
        )}
        <BackupReminder />
        <ErrorLine error={error ?? (cases.error ? fail(cases.error as unknown as UiError) : null)} />

        <div className="library-bar">
          <nav className="crumbs" aria-label="מיקום">
            <button type="button" className={dropOn === "top" ? "crumb drop" : "crumb"} aria-current={!inTrash && !current ? "page" : undefined}
              onClick={() => setFolderId(null)} {...dropProps(null)}>כל התיקים</button>
            {!inTrash && path.map((f) => (
              <span key={f.id} className="crumb-wrap">
                <span className="crumb-sep" aria-hidden="true">›</span>
                <button type="button" className={dropOn === f.id ? "crumb drop" : "crumb"} aria-current={f.id === current ? "page" : undefined}
                  onClick={() => setFolderId(f.id)} {...dropProps(f.id)}>{f.name}</button>
              </span>
            ))}
            {inTrash && <span className="crumb-wrap"><span className="crumb-sep" aria-hidden="true">›</span><span className="crumb" aria-current="page">סל המחזור</span></span>}
          </nav>
          <input className="input search" type="search" placeholder="חיפוש לפי שם או קוד, בכל התיקיות" aria-label="חיפוש תיקים"
            value={query} onChange={(e) => { setQuery(e.target.value); setInTrash(false); }} />
          <button type="button" className={inTrash ? "btn trash-btn on" : "btn trash-btn"} onClick={() => { setInTrash(!inTrash); setQuery(""); }}>
            סל המחזור{trashCount > 0 ? ` (${trashCount})` : ""}
          </button>
        </div>

        {inTrash ? (
          <TrashList cases={trash.data ?? []} pathOf={pathOf}
            onRestore={(c) => void run(() => ipc.restoreCase(c.id), "התיק שוחזר.")}
            onPurge={(c) => setModal({ kind: "purge", c })} />
        ) : (
          <>
            {allCases.length === 0 && allFolders.length === 0 && cases.data && (
              <div className="card empty">
                <h2>התיק הראשון</h2>
                <p className="muted">פותחים תיק, מוסיפים את החומרים שכבר יש (דוחות, אינטייק, סיכומי מפגשים וציונים), ו-Claude מציע טיוטה לכל סעיף בדוח. אפשר לסדר את התיקים בתיקיות, כמו במחשב.</p>
                <button type="button" className="btn btn-primary" onClick={() => setCreating(true)}>פתיחת תיק</button>
              </div>
            )}

            {shownFolders.length > 0 && (
              <ul className="folders" aria-label="תיקיות">
                {shownFolders.map((f) => (
                  <li key={f.id}>
                    <RightClick items={folderItems(f)}>
                      <div className={dropOn === f.id ? "folder drop" : "folder"} draggable onDragStart={onDragStart({ folder: f.id })} {...dropProps(f.id)}>
                        <button type="button" className="folder-open" onClick={() => setFolderId(f.id)}>
                          <span className="folder-icon"><FolderIcon size={26} /></span>
                          <span className="folder-text">
                            <span className="folder-name">{f.name}</span>
                            <span className="small muted">{q && f.parent_id ? `${pathOf(f.parent_id)} · ` : ""}{countIn(f.id) === 1 ? "תיק אחד" : `${countIn(f.id)} תיקים`}</span>
                          </span>
                        </button>
                        <ActionsMenu items={folderItems(f)} label={`פעולות על התיקייה ${f.name}`} />
                      </div>
                    </RightClick>
                  </li>
                ))}
              </ul>
            )}

            {shownCases.length > 0 && (
              <div className="card table-card">
                <table className="cases">
                  <thead>
                    <tr>
                      <th>ילד/ה</th>
                      <th>הצעד הבא</th>
                      <th className="col-progress">הדוח</th>
                      <th className="col-go"><span className="visually-hidden">פעולות</span></th>
                    </tr>
                  </thead>
                  <tbody>
                    {shownCases.map((c) => {
                      const s = nextStep(c);
                      const pct = Math.round((c.approved_sections.length / TOTAL_SECTIONS) * 100);
                      const open = () => go({ name: "case", id: c.id, view: "materials" });
                      return (
                        <RightClick key={c.id} items={caseItems(c)}>
                          <tr onClick={open} className="case-row" draggable onDragStart={onDragStart({ case: c.id })}>
                            <td>
                              <div className="case-name serif">{c.child_name ?? "ללא שם"}</div>
                              <div className="small muted">
                                {c.meta.age ? `${ageWords(c.meta.age)} · ` : ""}{c.meta.code || "ללא קוד"} · עודכן {updated(c.updated_at)}
                                {q && c.folder_id ? ` · ${pathOf(c.folder_id)}` : ""}
                              </div>
                            </td>
                            <td>
                              <span className="next-cell"><span className={s.cls}>{s.chip}</span><span>{s.text}</span></span>
                            </td>
                            <td>
                              <div className="progress-row">
                                <div className="bar" role="progressbar" aria-label="סעיפים שאושרו" aria-valuemin={0} aria-valuemax={TOTAL_SECTIONS} aria-valuenow={c.approved_sections.length}>
                                  <div className={pct >= 100 ? "bar-fill bar-done" : "bar-fill"} style={{ width: `${pct}%` }} />
                                </div>
                                <span className="small muted num">{c.approved_sections.length} מתוך {TOTAL_SECTIONS}</span>
                              </div>
                            </td>
                            <td>
                              <div className="row-actions">
                                <button type="button" className={s.start ? "btn" : "btn btn-primary"}
                                  onClick={(e) => { e.stopPropagation(); open(); }}>
                                  {s.start ? "פתיחה" : "להמשיך"}<span className="visually-hidden"> בתיק של {caseName(c)}</span>
                                </button>
                                <ActionsMenu items={caseItems(c)} label={`פעולות על התיק של ${caseName(c)}`} />
                              </div>
                            </td>
                          </tr>
                        </RightClick>
                      );
                    })}
                  </tbody>
                </table>
              </div>
            )}

            {cases.data && (allCases.length > 0 || allFolders.length > 0) && shownCases.length === 0 && shownFolders.length === 0 && (
              <p className="muted empty-folder">{q ? "לא נמצאו תיקים או תיקיות בשם הזה." : "התיקייה ריקה. אפשר לגרור לכאן תיקים, או לפתוח תיק חדש כשהתיקייה פתוחה."}</p>
            )}
          </>
        )}
        <p className="small muted">השמות, גם של התיקיות, מוצגים רק כאן, במחשב שלך. Claude מקבל תמיד תפקידים ("הילד", "הגננת") במקום שמות.</p>
      </main>

      {creating && (
        <NewCaseDialog folderId={current} onClose={() => setCreating(false)}
          onCreated={(id) => { void refreshAll(); go({ name: "case", id, view: "materials" }); }} />
      )}
      {modal?.kind === "new-folder" && (
        <FolderNameDialog title={current ? `תיקייה חדשה בתוך "${byId.get(current)?.name ?? ""}"` : "תיקייה חדשה"} onClose={() => setModal(null)}
          onSave={async (n) => { await ipc.createFolder(current, n); await refreshAll(); setModal(null); }} />
      )}
      {modal?.kind === "rename" && (
        <FolderNameDialog title="שינוי שם התיקייה" initial={modal.folder.name} onClose={() => setModal(null)}
          onSave={async (n) => { await ipc.renameFolder(modal.folder.id, n); await refreshAll(); setModal(null); }} />
      )}
      {modal?.kind === "move-folder" && (
        <MoveDialog what={`התיקייה "${modal.folder.name}"`} folders={allFolders} current={modal.folder.parent_id} exclude={descendants(allFolders, modal.folder.id)}
          onClose={() => setModal(null)}
          onMove={async (t) => { await ipc.moveFolder(modal.folder.id, t); await refreshAll(); setModal(null); notify("התיקייה הועברה."); }} />
      )}
      {modal?.kind === "move-case" && (
        <MoveDialog what={`התיק של ${caseName(modal.c)}`} folders={allFolders} current={modal.c.folder_id}
          onClose={() => setModal(null)}
          onMove={async (t) => { await ipc.moveCase(modal.c.id, t); await refreshAll(); setModal(null); notify("התיק הועבר."); }} />
      )}
      {modal?.kind === "delete-folder" && (
        <ConfirmDialog title={`מחיקת התיקייה "${modal.folder.name}"`} action="מחיקת התיקייה"
          text={countIn(modal.folder.id) > 0 || allFolders.some((f) => f.parent_id === modal.folder.id)
            ? "רק התיקייה נמחקת. התיקים והתיקיות שבתוכה יעברו לתיקייה שמעליה, ושום תיק לא יימחק."
            : "התיקייה ריקה ותימחק."}
          onClose={() => setModal(null)}
          onConfirm={async () => { await ipc.deleteFolder(modal.folder.id); await refreshAll(); setModal(null); }} />
      )}
      {modal?.kind === "delete-case" && (
        <ConfirmDialog title={`העברת התיק של ${caseName(modal.c)} לסל המחזור`} action="העברה לסל המחזור" danger
          text={`התיק יישמר בסל המחזור ${TRASH_DAYS} יום, מוצפן כמו כל תיק, ואפשר לשחזר אותו. אחרי ${TRASH_DAYS} יום הוא יימחק לצמיתות.`}
          onClose={() => setModal(null)}
          onConfirm={async () => { await ipc.deleteCase(modal.c.id); await refreshAll(); setModal(null); notify("התיק הועבר לסל המחזור."); }} />
      )}
      {modal?.kind === "purge" && (
        <PurgeDialog name={`התיק של ${caseName(modal.c)}`} onClose={() => setModal(null)}
          onPurge={async (pw) => { await ipc.purgeCase(modal.c.id, pw); await refreshAll(); setModal(null); notify("התיק נמחק לצמיתות."); }} />
      )}
    </div>
  );
}

function TrashList(props: { cases: CaseSummary[]; pathOf: (id: string | null) => string; onRestore: (c: CaseSummary) => void; onPurge: (c: CaseSummary) => void }) {
  const [now] = useState(() => Date.now() / 1000);
  const daysLeft = (c: CaseSummary) => Math.max(0, TRASH_DAYS - Math.floor((now - (c.deleted_at ?? 0)) / 86_400));
  if (!props.cases.length) {
    return <p className="muted empty-folder">סל המחזור ריק. תיק שנמחק נשמר כאן {TRASH_DAYS} יום, ואפשר לשחזר אותו.</p>;
  }
  return (
    <div className="card table-card">
      <p className="small muted trash-note">תיקים בסל נשמרים מוצפנים {TRASH_DAYS} יום ואז נמחקים לצמיתות. מחיקה לצמיתות לפני כן דורשת את הסיסמה.</p>
      <table className="cases">
        <thead><tr><th>ילד/ה</th><th>נמחק</th><th className="col-go"><span className="visually-hidden">פעולות</span></th></tr></thead>
        <tbody>
          {props.cases.map((c) => (
            <tr key={c.id}>
              <td>
                <div className="case-name serif">{c.child_name ?? "ללא שם"}</div>
                <div className="small muted">{c.meta.code}{c.folder_id ? ` · היה ב: ${props.pathOf(c.folder_id)}` : ""}</div>
              </td>
              <td className="small">{c.deleted_at ? updated(c.deleted_at) : ""} · יימחק בעוד {daysLeft(c)} ימים</td>
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
    </div>
  );
}
