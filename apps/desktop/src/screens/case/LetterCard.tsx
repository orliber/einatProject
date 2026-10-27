import { useCallback, useEffect, useRef, useState } from "react";
import { useApp } from "../../App";
import { STALE_EDIT_KEPT } from "../../i18n/he";
import { ipc, type ParagraphView, type Prepared } from "../../ipc/client";
import { ReviewDialog } from "../../components/ReviewDialog";
import { ExportDialog } from "../../components/ExportDialog";
import { ErrorLine, Spinner } from "../../components/ui";
import { recordDuration, timed, WaitLine } from "../../components/Progress";
import { useAi } from "../../ai";
import { LocalText, type LocalTextHandle } from "../../components/LocalText";
import { useHoldUnsaved } from "../../components/Unsaved";
import { useKept } from "../../kept";
import { notSentMessage } from "../../sending";
import type { CaseApi } from "../CaseScreen";

type Audience = "parents" | "school";

/**
 * EX-4: a short letter to the parents or the school, drafted from the approved report through
 * the same review screen and gate. The school letter is written from the recommendations only.
 * Round 44: she marks which approved recommendations go into the letters (all, until she
 * unmarks one); the mark is kept with the case.
 */
export function LetterCard({ api }: { api: CaseApi }) {
  const { fail, status, notify } = useApp();
  const ai = useAi();
  const [audience, setAudience] = useState<Audience>("parents");
  const [note, setNote] = useState("");
  const [paragraphs, setParagraphs] = useState<ParagraphView[]>([]);
  const [review, setReview] = useState<Prepared | null>(null);
  const [busy, setBusy] = useState(false);
  /** Where a new wording is: being prepared here, or being written by the AI (for the wait line). */
  const [stage, setStage] = useState<"prepare" | "write" | null>(null);
  /** The paragraph being edited, with the text it started from; what is typed stays in `editBox`. */
  // Kept while the vault is open, and held for a lock, like every paragraph editor (F1a).
  const [editing, setEditing] = useKept<{ id: string; text: string; original?: string } | null>(`case:${api.caseId}:letter:${audience}:editing`, null);
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

  /** Sends the letter's request and waits for its paragraphs, keeping how long it took. */
  async function write(approvalId: string) {
    setStage("write");
    const started = Date.now();
    await ipc.sendSection(approvalId);
    if (!status.demo_mode) recordDuration("draft", `letter:${status.speed}`, Date.now() - started);
    await load();
  }

  async function act(fn: () => Promise<unknown>) {
    setError(null);
    setBusy(true);
    try {
      await fn();
    } catch (e) {
      setError(fail(e as never));
    } finally {
      setBusy(false);
      setStage(null);
    }
  }

  const approved = paragraphs.filter((p) => p.status === "approved").length;
  const recommendations = api.detail.sections.find((s) => s.key === "recommendations")?.paragraphs.filter((p) => p.status === "approved") ?? [];
  // What she left out is kept, so a recommendation approved or rewritten later is in until
  // she unmarks it (D-083; review of stage 6, R2).
  const leftOut = api.detail.meta.letter_left_out ?? [];
  const isPicked = (id: string) => !leftOut.includes(id);
  const pick = (id: string, on: boolean) => void act(async () => {
    const next = recommendations.map((r) => r.id).filter((r) => (r === id ? !on : !isPicked(r)));
    await ipc.setLetterLeftOut(api.caseId, next.length ? next : null);
    await api.reload();
  });

  return (
    <section className="card finish-file letter-card">
      <b>מכתב קצר</b>
      <span className="muted small">
        גרסה קצרה מהדוח המאושר. למסגרת החינוכית: רק ההמלצות.
      </span>
      <div className="row" role="radiogroup" aria-label="למי המכתב">
        <label className="row small"><input type="radio" name="letter-audience" checked={audience === "parents"} onChange={() => setAudience("parents")} /> להורים</label>
        <label className="row small"><input type="radio" name="letter-audience" checked={audience === "school"} onChange={() => setAudience("school")} /> לצוות החינוכי</label>
      </div>
      {/* One recommendation she unmarked shows too, so it can be marked again. */}
      {(recommendations.length > 1 || recommendations.some((r) => !isPicked(r.id))) && (
        <fieldset className="stack letter-picks">
          <legend className="small">ההמלצות שנכנסות למכתב</legend>
          {recommendations.map((r) => (
            <label key={r.id} className="row small letter-pick">
              <input type="checkbox" checked={isPicked(r.id)} disabled={busy} onChange={(e) => pick(r.id, e.target.checked)} />
              <span>{r.text}</span>
            </label>
          ))}
          {recommendations.every((r) => !isPicked(r.id)) && <span className="small muted">לא סימנת אף המלצה, אז המכתב ייכתב בלי המלצות.</span>}
        </fieldset>
      )}
      {/* A label that stays while she types (a placeholder alone goes as the first letter does). */}
      <div className="field">
        <label htmlFor="letter-note" className="small">מה עוד חשוב שיהיה במכתב? (לא חובה)</label>
        <input id="letter-note" className="input" value={note} onChange={(e) => setNote(e.target.value)} />
      </div>
      <button type="button" className="btn" disabled={busy}
        onClick={() => void act(async () => {
          setStage("prepare");
          const prepared = await timed("prepare", ipc.prepareLetter(api.caseId, audience, note), "letter");
          // Round 26: what the gate cleared goes at once; the review only when she asked for it.
          if (status.review_always) setReview(prepared);
          else if (prepared.approval_id) {
            await write(prepared.approval_id);
          } else setError(notSentMessage(prepared));
        })}>
        {busy ? <><Spinner /> {stage === "write" ? "כותב…" : "מכינה…"}</> : paragraphs.length ? "ניסוח מחדש" : "ניסוח המכתב"}
      </button>
      {stage === "prepare" && <WaitLine task="prepare" speed="letter" label="מכינה את הבקשה: מסתירה שמות ובודקת מה יוצא" />}
      {stage === "write" && <WaitLine task="draft" speed={`letter:${status.speed}`} demo={status.demo_mode} delay={0} label={`${ai} כותב את המכתב`} />}
      {paragraphs.map((p) =>
        editing?.id === p.id ? (
          <div key={p.id} className="stack" style={{ gap: 6 }}>
            <LocalText key={p.id} ref={editBox} className="textarea serif" rows={4} autoFocus initial={editing.text} onLive={holdLive} aria-label="עריכת הפסקה"
              onText={(text) => setEditing((e) => (e?.id === p.id ? { ...e, text } : e))} />
            <div className="row">
              <button type="button" className="btn btn-primary btn-small"
                onClick={() => {
                  const text = editBox.current?.value() ?? editing.text;
                  void act(async () => { if (await ipc.editParagraph(api.caseId, editing.id, text, editing.original ?? null)) notify(STALE_EDIT_KEPT); setEditing(null); await load(); });
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
              <button type="button" className="link-small" onClick={() => setEditing({ id: p.id, text: p.text, original: p.text })}>עריכה</button>
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
            await act(() => write(id));
          }} />
      )}
      {exporting && <ExportDialog api={api} letter={audience} onClose={() => setExporting(false)} />}
    </section>
  );
}
