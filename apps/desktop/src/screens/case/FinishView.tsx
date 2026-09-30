import { useEffect, useState } from "react";
import { useApp } from "../../App";
import { ipc, type ExportCheck } from "../../ipc/client";
import { ErrorLine, Spinner } from "../../components/ui";
import type { CaseApi } from "../CaseScreen";
import "./FinishView.css";

/**
 * The last stage: what is checked before the Word file is made, each problem one click from
 * where it is fixed, then the file itself (password-protected, in the export dialog).
 */
export function FinishView({ api, onExport, onOpen }: { api: CaseApi; onExport: () => void; onOpen: (key: string) => void }) {
  const { fail } = useApp();
  const { detail, caseId } = api;
  const [check, setCheck] = useState<ExportCheck | null>(null);
  const [error, setError] = useState<string | null>(null);
  const waiting = detail.sections.filter((s) => s.paragraphs.some((p) => p.status === "proposed"));

  useEffect(() => {
    let alive = true;
    ipc.checkExport(caseId).then((c) => alive && setCheck(c)).catch((e: unknown) => alive && setError(fail(e as never)));
    return () => {
      alive = false;
    };
  }, [caseId, detail, fail]);

  /** The section a problem line is about ("המלצות: יש מידע חסר…"). */
  const keyOf = (line: string) => detail.sections.find((s) => line.startsWith(`${s.title}:`))?.key;
  const blocked = (check?.blocking.length ?? 0) > 0;
  const nothing = check !== null && check.included_sections === 0;

  return (
    <div className="view finish">
      <div className="view-head">
        <div className="stack" style={{ gap: 4 }}>
          <h1>{blocked || nothing ? "כמעט מוכן להוצאה" : "הדוח מוכן להוצאה"}</h1>
          <span className="muted">לפני שהקובץ יוצא בודקים כמה דברים. כל בעיה מקושרת למקום שבו מתקנים אותה.</span>
        </div>
      </div>
      <div className="finish-body">
        {!check && !error && <p className="muted"><Spinner /> בודקת…</p>}
        {check && (
          <ul className="finish-checks">
            <li className={check.included_sections > 0 ? "ok" : "warn"}>
              <span className="finish-mark" aria-hidden="true">{check.included_sections > 0 ? "✓" : "!"}</span>
              <span className="grow">
                {check.included_sections > 0 ? `${check.included_sections} סעיפים מאושרים ייכנסו לקובץ.` : "עוד אין סעיף מאושר. רק פסקאות שאישרת נכנסות לקובץ."}
                {check.empty_sections.length > 0 && <span className="small muted"> {check.empty_sections.length} ריקים לא ייכנסו.</span>}
              </span>
              <button type="button" className="link-small" onClick={() => onOpen("")}>לדוח</button>
            </li>
            {waiting.length > 0 && (
              <li className="warn">
                <span className="finish-mark" aria-hidden="true">!</span>
                <span className="grow">טיוטות שעוד לא אושרו לא ייכנסו: {waiting.map((s) => s.title).join(", ")}.</span>
                <button type="button" className="link-small" onClick={() => onOpen(waiting[0]?.key ?? "")}>לאישור</button>
              </li>
            )}
            {check.blocking.map((b) => (
              <li key={b} className="warn">
                <span className="finish-mark" aria-hidden="true">!</span>
                <span className="grow">{b}</span>
                <button type="button" className="link-small" onClick={() => onOpen(keyOf(b) ?? "")}>לתיקון</button>
              </li>
            ))}
            {!blocked && (
              <li className="ok">
                <span className="finish-mark" aria-hidden="true">✓</span>
                <span className="grow">השמות חוזרים למקומם בקובץ, ולא נשארה אף תגית.</span>
              </li>
            )}
            {check.score_tables > 0 && (
              <li className="ok">
                <span className="finish-mark" aria-hidden="true">✓</span>
                <span className="grow">{check.score_tables === 1 ? "טבלת ציונים אחת תצורף כנספח." : `${check.score_tables} טבלאות ציונים יצורפו כנספח.`}</span>
              </li>
            )}
          </ul>
        )}
        <ErrorLine error={error} />
        <section className="card finish-file">
          <b>קובץ Word מוגן בסיסמה</b>
          <span className="muted">הסיסמה נוצרת לבד. שולחים אותה להורים בנפרד מהקובץ, למשל בהודעה.</span>
          <div className="row">
            <button type="button" className="btn btn-primary btn-big" disabled={!check || blocked || nothing} onClick={onExport}>הפקת הקובץ</button>
            {(blocked || nothing) && <span className="small muted">ייפתח אחרי התיקונים למעלה</span>}
          </div>
        </section>
      </div>
    </div>
  );
}
