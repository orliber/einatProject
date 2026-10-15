import { useCallback, useEffect, useRef, useState } from "react";
import { useApp } from "../../App";
import { ipc, type ParagraphView, type Prepared } from "../../ipc/client";
import { ReviewDialog } from "../../components/ReviewDialog";
import { ExportDialog } from "../../components/ExportDialog";
import { ErrorLine, Spinner } from "../../components/ui";
import { LocalText, type LocalTextHandle } from "../../components/LocalText";
import { useHoldUnsaved } from "../../components/Unsaved";
import { useKept } from "../../kept";
import { notSentMessage } from "../../sending";
import type { CaseApi } from "../CaseScreen";

type Audience = "parents" | "school";

/**
 * EX-4: a short letter to the parents or the school, drafted from the approved report through
 * the same review screen and gate. The school letter is written from the recommendations only.
 */
export function LetterCard({ api }: { api: CaseApi }) {
  const { fail, status } = useApp();
  const [audience, setAudience] = useState<Audience>("parents");
  const [note, setNote] = useState("");
  const [paragraphs, setParagraphs] = useState<ParagraphView[]>([]);
  const [review, setReview] = useState<Prepared | null>(null);
  const [busy, setBusy] = useState(false);
  /** The paragraph being edited, with the text it started from; what is typed stays in `editBox`. */
  // Kept while the vault is open, and held for a lock, like every paragraph editor (F1a).
  const [editing, setEditing] = useKept<{ id: string; text: string } | null>(`case:${api.caseId}:letter:${audience}:editing`, null);
  const holdLive = useHoldUnsaved(api.caseId, audience === "parents" ? "פסקה במכתב להורים" : "פסקה במכתב לצוות", editing?.text ?? null, "finish");
  const editBox = useRef<LocalTextHandle>(null);
  const [exporting, setExporting] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const load = useCallback(async () => {
    try {
      setParagraphs(await ipc.letter(api.caseId, audience));
    } catch (e) {
      setError(fail(e as never));
    }
  }, [api.caseId, audience, fail]);
  useEffect(() => {
    let alive = true;
    ipc.letter(api.caseId, audience).then((p) => alive && setParagraphs(p)).catch((e: unknown) => alive && setError(fail(e as never)));
    return () => {
      alive = false;
    };
  }, [api.caseId, audience, fail]);

  async function act(fn: () => Promise<unknown>) {
    setError(null);
    setBusy(true);
    try {
      await fn();
    } catch (e) {
      setError(fail(e as never));
    } finally {
      setBusy(false);
    }
  }

  const approved = paragraphs.filter((p) => p.status === "approved").length;

  return (
    <section className="card finish-file letter-card">
      <b>מכתב קצר</b>
      <span className="muted small">
        גרסה קצרה מהדוח המאושר. למסגרת החינוכית: רק ההמלצות.
      </span>
      <div className="row" role="radiogroup" aria-label="למי המכתב">
        <label className="row small"><input type="radio" checked={audience === "parents"} onChange={() => setAudience("parents")} /> להורים</label>
        <label className="row small"><input type="radio" checked={audience === "school"} onChange={() => setAudience("school")} /> לצוות החינוכי</label>
      </div>
      <input className="input" placeholder="מה עוד חשוב שיהיה במכתב? (לא חובה)" value={note} onChange={(e) => setNote(e.target.value)} />
      <button type="button" className="btn" disabled={busy}
        onClick={() => void act(async () => {
          const prepared = await ipc.prepareLetter(api.caseId, audience, note);
          // Round 26: what the gate cleared goes at once; the review only when she asked for it.
          if (status.review_always) setReview(prepared);
          else if (prepared.approval_id) {
            await ipc.sendSection(prepared.approval_id);
            await load();
          } else setError(notSentMessage(prepared));
        })}>
        {busy ? <><Spinner /> מכינה…</> : paragraphs.length ? "ניסוח מחדש" : "ניסוח המכתב"}
      </button>
      {paragraphs.map((p) =>
        editing?.id === p.id ? (
          <div key={p.id} className="stack" style={{ gap: 6 }}>
            <LocalText key={p.id} ref={editBox} className="textarea serif" rows={4} autoFocus initial={editing.text} onLive={holdLive}
              onText={(text) => setEditing((e) => (e?.id === p.id ? { id: p.id, text } : e))} />
            <div className="row">
              <button type="button" className="btn btn-primary btn-small"
                onClick={() => {
                  const text = editBox.current?.value() ?? editing.text;
                  void act(async () => { await ipc.editParagraph(api.caseId, editing.id, text); setEditing(null); await load(); });
                }}>שמירה</button>
              <button type="button" className="btn btn-small" onClick={() => setEditing(null)}>ביטול</button>
            </div>
          </div>
        ) : (
          <div key={p.id} className="stack" style={{ gap: 4 }}>
            <p className="serif">{p.text}</p>
            <div className="row">
              {p.status !== "approved" && (
                <button type="button" className="link-small" onClick={() => void act(async () => { await ipc.approveParagraph(api.caseId, p.id); await load(); })}>אישור</button>
              )}
              {p.status === "approved" && <span className="small muted">✓ מאושר</span>}
              <button type="button" className="link-small" onClick={() => setEditing({ id: p.id, text: p.text })}>עריכה</button>
            </div>
          </div>
        ),
      )}
      {paragraphs.length > 0 && (
        <button type="button" className="btn btn-primary" disabled={approved === 0} onClick={() => setExporting(true)}>הפקת המכתב</button>
      )}
      <ErrorLine error={error} />
      {review && (
        <ReviewDialog title={audience === "parents" ? "מכתב להורים" : "מכתב לצוות החינוכי"} caseId={api.caseId} prepared={review}
          reprepare={() => ipc.prepareLetter(api.caseId, audience, note)}
          onClose={() => setReview(null)}
          onSend={async (id) => {
            setReview(null);
            await act(async () => { await ipc.sendSection(id); await load(); });
          }} />
      )}
      {exporting && <ExportDialog api={api} letter={audience} onClose={() => setExporting(false)} />}
    </section>
  );
}
