import { useEffect, useState } from "react";
import { useApp } from "../App";
import { ipc, type Prepared } from "../ipc/client";
import type { CaseApi } from "../screens/CaseScreen";
import { Dialog, ErrorLine, Segments, Spinner } from "./ui";

/**
 * "Prepare a draft of the whole report": every section with material, each request
 * prepared and gated separately. Suspects or blocks are shown before anything is sent.
 */
export function FullDraftDialog({ api, onClose }: { api: CaseApi; onClose: () => void }) {
  const { fail, notify } = useApp();
  const [items, setItems] = useState<[string, Prepared][] | null>(null);
  const [open, setOpen] = useState<string | null>(null);
  const [progress, setProgress] = useState<number | null>(null);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    ipc.prepareFullDraft(api.caseId).then(setItems).catch((e) => setError(fail(e as never)));
  }, [api.caseId, fail]);

  const title = (key: string) => api.detail.sections.find((s) => s.key === key)?.title ?? key;
  const ready = items?.filter(([, p]) => p.approval_id) ?? [];
  const stuck = items?.filter(([, p]) => !p.approval_id) ?? [];

  async function sendAll() {
    setError(null);
    setProgress(0);
    let done = 0;
    for (const [, p] of ready) {
      if (!p.approval_id) continue;
      try {
        await ipc.sendSection(p.approval_id);
      } catch (e) {
        setError(fail(e as never));
        break;
      }
      done += 1;
      setProgress(done);
    }
    await api.reload();
    if (done === ready.length) {
      notify(`טיוטות מוכנות ב-${done} סעיפים. עוברים עליהן ומאשרים.`);
      onClose();
    }
  }

  return (
    <Dialog title="טיוטה לכל הדוח" subtitle="כל סעיף שיש לו חומרים נשלח בנפרד, אחרי הסתרה. שום דבר לא נכנס לדוח בלי אישורך." onClose={onClose}
      footer={
        <>
          <span className="grow small muted">
            {progress !== null ? `נשלחו ${progress} מתוך ${ready.length}` : items ? `${ready.length} סעיפים מוכנים לשליחה` : ""}
          </span>
          <button type="button" className="btn" onClick={onClose}>ביטול</button>
          <button type="button" className="btn btn-primary" disabled={!ready.length || progress !== null} onClick={() => void sendAll()}>
            {progress !== null ? <><Spinner /> שולחת…</> : `שליחת ${ready.length} סעיפים`}
          </button>
        </>
      }>
      <div className="stack">
        {!items && !error && <p className="muted"><Spinner /> מכינה את הבקשות ובודקת מה יוצא…</p>}
        {items && items.length === 0 && <p className="muted">עוד אין חומרים שמזינים סעיפים. מוסיפים חומרים ב"חומרי התיק".</p>}
        {stuck.length > 0 && (
          <div className="note-block stack" style={{ gap: 6 }}>
            <b>{stuck.length} סעיפים מחכים להחלטה שלך לפני שליחה</b>
            <span className="small">יש בהם שם לא מוכר או פרט מזהה. פותחים את הסעיף ושולחים משם, כדי להחליט על כל מילה.</span>
            <span className="small">{stuck.map(([k]) => title(k)).join(" · ")}</span>
          </div>
        )}
        {ready.map(([key, p]) => (
          <div key={key} className="card draft-row">
            <button type="button" className="draft-row-head" aria-expanded={open === key} onClick={() => setOpen(open === key ? null : key)}>
              <b className="grow">{title(key)}</b>
              <span className="small muted">{p.hidden.length ? `יוסתרו: ${p.hidden.join(", ")}` : "אין פרטים מזהים"}</span>
              <span aria-hidden="true">{open === key ? "▴" : "▾"}</span>
            </button>
            {open === key && (
              <div className="draft-row-body serif">
                {p.parts.map((part, i) => (
                  <p key={i}><span className="small muted">{part.label}: </span><Segments segments={part.outgoing} side="outgoing" /></p>
                ))}
              </div>
            )}
          </div>
        ))}
        <ErrorLine error={error} />
      </div>
    </Dialog>
  );
}
