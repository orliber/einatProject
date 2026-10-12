import { useEffect, useState } from "react";
import { useApp } from "../App";
import { ipc, type SimilarParagraph } from "../ipc/client";
import { ErrorLine, Spinner } from "./ui";
import "./Why.css";

/** "[ילד_גן_2]" → "ילד גן 2": her past reports are kept with roles in place of names. */
export function rolesAsWords(text: string): string {
  return text.replace(/\[([^[\]\n]{1,40})\]/g, (_, t: string) => t.replace(/_/g, " "));
}

/**
 * ST2: the paragraph of her past reports closest to this one, to read beside it while she
 * edits. Worked out on her computer; it never goes into a request.
 */
export function Similar({ caseId, draftId }: { caseId: string; draftId: string }) {
  const { fail } = useApp();
  const [found, setFound] = useState<SimilarParagraph | null | undefined>(undefined);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    let alive = true;
    ipc
      .similarParagraph(caseId, draftId)
      .then((x) => alive && setFound(x))
      .catch((e: unknown) => alive && setError(fail(e as never)));
    return () => {
      alive = false;
    };
  }, [caseId, draftId, fail]);

  if (error) return <ErrorLine error={error} />;
  if (found === undefined) return <p className="small muted"><Spinner /> מחפשת בדוחות שלך…</p>;
  if (found === null) {
    return <p className="small muted">לא נמצאה פסקה דומה בדוחות שהעלית. אפשר להעלות דוחות קודמים במסך "הסגנון שלי".</p>;
  }
  return (
    <div className="why">
      <span className="small muted">הפסקה הכי דומה מהדוחות שלך (רק לעיון, לא נשלחת):</span>
      <blockquote className="why-item">
        <span className="why-label">{found.report}{found.same_section ? "" : " · מסעיף אחר"}</span>
        <p className="serif why-text">{rolesAsWords(found.text)}</p>
      </blockquote>
    </div>
  );
}
