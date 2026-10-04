import { useEffect, useState } from "react";
import { useApp } from "../App";
import { ipc, type ParagraphVersionView } from "../ipc/client";
import { ErrorLine, Spinner } from "./ui";

function when(seconds: number): string {
  if (!seconds) return "";
  return new Date(seconds * 1000).toLocaleString("he-IL", { day: "numeric", month: "numeric", hour: "2-digit", minute: "2-digit" });
}

/**
 * Earlier wordings of one paragraph, newest first (UX-4, D-046). Each can be brought back:
 * the current wording is kept as a version in turn, so nothing she wrote is lost.
 * The text is shown as plain text, never as HTML.
 */
export function ParagraphVersions({ caseId, draftId, onRestored }: { caseId: string; draftId: string; onRestored: () => Promise<void> | void }) {
  const { fail } = useApp();
  const [versions, setVersions] = useState<ParagraphVersionView[] | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);

  useEffect(() => {
    let alive = true;
    ipc
      .paragraphVersions(caseId, draftId)
      .then((v) => alive && setVersions(v))
      .catch((e: unknown) => alive && setError(fail(e as never)));
    return () => {
      alive = false;
    };
  }, [caseId, draftId, fail]);

  async function restore(id: string) {
    setError(null);
    setBusy(true);
    try {
      await ipc.restoreParagraphVersion(caseId, draftId, id);
      await onRestored();
    } catch (e) {
      setError(fail(e as never));
    } finally {
      setBusy(false);
    }
  }

  return (
    <div className="versions">
      <b>גרסאות קודמות של הפסקה</b>
      <span className="small muted">החזרה של גרסה מאשרת אותה, והניסוח הנוכחי נשמר כאן כגרסה.</span>
      {versions === null && !error && <span className="small muted"><Spinner /> טוענת…</span>}
      {versions?.length === 0 && <span className="small muted">אין גרסאות קודמות.</span>}
      {versions && versions.length > 0 && (
        <ol className="versions-list">
          {versions.map((v) => (
            <li key={v.id} className="version">
              <span className="small muted">{v.by_ai ? "✦ ניסוח של AI" : "הניסוח שלך"}{v.saved_at ? ` · ${when(v.saved_at)}` : ""}</span>
              <p className="version-text">{v.text}</p>
              <button type="button" className="btn btn-small" disabled={busy} onClick={() => void restore(v.id)}>להחזיר את הניסוח הזה</button>
            </li>
          ))}
        </ol>
      )}
      <ErrorLine error={error} />
    </div>
  );
}
