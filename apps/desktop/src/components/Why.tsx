import { useEffect, useState } from "react";
import { useApp } from "../App";
import { ipc, type SourceExcerpt } from "../ipc/client";
import { ErrorLine, Spinner } from "./ui";
import "./Why.css";

/**
 * "למה כתבת את זה?" (AI-7): the passages a paragraph leans on, best match first, so she can
 * check a sentence against where it came from before approving it. Shown on this computer only.
 */
export function Why({ caseId, draftId }: { caseId: string; draftId: string }) {
  const { fail } = useApp();
  const [items, setItems] = useState<SourceExcerpt[] | null>(null);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    let alive = true;
    ipc
      .paragraphSources(caseId, draftId)
      .then((x) => alive && setItems(x))
      .catch((e: unknown) => alive && setError(fail(e as never)));
    return () => {
      alive = false;
    };
  }, [caseId, draftId, fail]);

  if (error) return <ErrorLine error={error} />;
  if (!items) return <p className="small muted"><Spinner /> מחפשת את המקורות…</p>;
  if (items.length === 0) return <p className="small muted">לפסקה הזאת אין מקור מסומן: היא נכתבה בלי חומרים, או שכתבת אותה בעצמך.</p>;
  return (
    <div className="why">
      <span className="small muted">הקטעים שהפסקה נשענת עליהם, הקרוב ביותר קודם:</span>
      {items.map((x, i) => (
        <blockquote key={i} className="why-item">
          <span className="why-label">{x.label}</span>
          <p className="serif why-text">{x.text}</p>
        </blockquote>
      ))}
    </div>
  );
}
