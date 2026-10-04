import { useState } from "react";
import { useApp } from "../App";
import { kindLabel, kindOrder } from "../i18n/he";
import { ipc, type ImportPreview, type InputKind } from "../ipc/client";
import type { AutoHidden } from "../ipc/generated/AutoHidden";
import { AutoHiddenCard } from "./AutoHiddenCard";
import { Dialog, ErrorLine, Segments } from "./ui";
import "./ImportDialog.css";

const FORMAT: Record<string, string> = { docx: "Word", odt: "ODT", pdf: "PDF", text: "טקסט" };

/** What was read from a document, before anything is stored. Nothing is sent from here. */
export function ImportDialog(props: {
  caseId: string;
  preview: ImportPreview;
  /** Uploaded from a slot: the kind of that slot, not a guess. */
  kind?: InputKind | undefined;
  onClose: () => void;
  onSaved: (inputId: string) => Promise<void>;
}) {
  const { fail } = useApp();
  const p = props.preview;
  const [kind, setKind] = useState<InputKind>(props.kind ?? p.suggested_kind);
  const [title, setTitle] = useState(p.title);
  const [body, setBody] = useState(p.body);
  const [editing, setEditing] = useState(false);
  const [found, setFound] = useState<AutoHidden[]>(p.auto_hidden);
  const [segments, setSegments] = useState(p.preview);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);

  /** After "להחזיר" or a role change on the card: the text and the card as they are now. */
  async function refilter() {
    const [fresh, detail] = await Promise.all([ipc.previewFilter(props.caseId, p.body), ipc.caseDetail(props.caseId)]);
    const kept = (a: AutoHidden) => detail.identities.find((i) => i.source !== "manual" && i.value === a.token);
    const next = found.flatMap((a) => {
      if (a.kind === "name" || a.kind === "other_case") {
        const i = kept(a);
        return i ? [{ ...a, tag: i.tag, role: i.role }] : [];
      }
      return fresh.auto_hidden.filter((x) => x.token === a.token);
    });
    for (const a of fresh.auto_hidden) if (!next.some((x) => x.token === a.token)) next.push(a);
    setFound(next);
    setSegments(fresh.original_segments);
  }

  async function save() {
    if (!body.trim()) return setError("אין טקסט לשמור.");
    setBusy(true);
    try {
      const saved = await ipc.addInput(props.caseId, kind, title.trim(), body);
      await props.onSaved(saved.id);
    } catch (e) {
      setError(fail(e as never));
      setBusy(false);
    }
  }

  const pages = p.pages > 0 ? ` · ${p.pages} עמודים` : "";
  return (
    <Dialog title="ייבוא מסמך" onClose={props.onClose}
      subtitle={`${p.file_name} · ${FORMAT[p.format] ?? p.format}${pages} · שום דבר לא נשלח בשלב הזה`}
      footer={
        <>
          <span className="small muted grow">רק הטקסט שבצד שמאל נשמר בתיק, מוצפן.</span>
          <button type="button" className="btn" onClick={props.onClose}>ביטול</button>
          <button type="button" className="btn btn-primary" disabled={busy} onClick={() => void save()}>שמירה בתיק</button>
        </>
      }>
      <div className="import">
        <div className="import-side stack">
          <fieldset className="plain stack" style={{ gap: 8 }}>
            <legend className="label">סוג החומר <span className="small muted">{props.kind ? "(לפי המשבצת שממנה הועלה)" : "(הוצע לפי התוכן)"}</span></legend>
            <div className="kind-chips">
              {kindOrder.map((k) => (
                <label key={k} className={k === kind ? "kind-chip on" : "kind-chip"}>
                  <input type="radio" name="kind" checked={k === kind} onChange={() => setKind(k)} />
                  {kindLabel[k]}
                </label>
              ))}
            </div>
          </fieldset>
          <div className="field">
            <label htmlFor="imp-title">כותרת</label>
            <input id="imp-title" className="input" value={title} onChange={(e) => setTitle(e.target.value)} />
          </div>

          <AutoHiddenCard items={found} caseId={props.caseId} onChanged={refilter} disabled={busy} />

          {(p.left_out.length > 0 || p.warnings.length > 0) && (
            <div className="note-sand stack" style={{ gap: 4 }}>
              <b>לא נקלט מהקובץ</b>
              {p.left_out.slice(0, 6).map((l, i) => <span key={i} className="left-out-line">{l}</span>)}
              {p.warnings.map((w, i) => <span key={`w${i}`}>{w}</span>)}
            </div>
          )}
        </div>

        <section className="card import-text" aria-label="הטקסט שיישמר בתיק">
          <div className="import-text-head">
            <b>הטקסט שיישמר בתיק</b>
            <button type="button" className="btn btn-small" onClick={() => setEditing(!editing)}>{editing ? "תצוגה" : "עריכה"}</button>
          </div>
          {editing ? (
            <label className="import-edit">
              <span className="visually-hidden">עריכת הטקסט</span>
              <textarea className="textarea serif" value={body} onChange={(e) => setBody(e.target.value)} />
            </label>
          ) : (
            <div className="import-body serif">
              {body === p.body ? <Segments segments={segments} side="original" /> : body}
            </div>
          )}
        </section>
        <ErrorLine error={error} />
      </div>
    </Dialog>
  );
}
