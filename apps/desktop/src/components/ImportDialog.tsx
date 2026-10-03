import { useState } from "react";
import { useApp } from "../App";
import { kindLabel, kindOrder, roleLabel } from "../i18n/he";
import { ipc, type ImportPreview, type InputKind } from "../ipc/client";
import type { NameSuggestion } from "../ipc/generated/NameSuggestion";
import { Dialog, ErrorLine, Segments } from "./ui";
import "./ImportDialog.css";
import { useAi } from "../ai";

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
  const ai = useAi();
  const p = props.preview;
  const [kind, setKind] = useState<InputKind>(props.kind ?? p.suggested_kind);
  const [title, setTitle] = useState(p.title);
  const [body, setBody] = useState(p.body);
  const [editing, setEditing] = useState(false);
  const [hidden, setHidden] = useState<string[]>([]);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);

  async function hide(s: NameSuggestion) {
    setError(null);
    try {
      const detail = await ipc.caseDetail(props.caseId);
      const current = detail.identities.map((i) => ({ id: i.id, role: i.role, value: i.value, aliases: i.aliases }));
      await ipc.setIdentities(props.caseId, [...current, { id: null, role: s.role, value: s.value, aliases: [] }]);
      setHidden([...hidden, s.value]);
    } catch (e) {
      setError(fail(e as never));
    }
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

          {p.name_suggestions.length > 0 && (
            <div className="stack" style={{ gap: 8 }}>
              <span className="label">שמות שנמצאו בשולי המסמך</span>
              {p.name_suggestions.map((s) => (
                <div key={s.value} className="card suggestion">
                  <div className="grow stack" style={{ gap: 0 }}>
                    <b>{s.value}</b>
                    <span className="small muted">{s.source} · יוסתר כ{roleLabel[s.role]}</span>
                  </div>
                  {hidden.includes(s.value) ? (
                    <span className="chip chip-ok">יוסתר</span>
                  ) : (
                    <button type="button" className="btn btn-primary btn-small" onClick={() => void hide(s)}>להסתיר</button>
                  )}
                </div>
              ))}
            </div>
          )}

          {p.suspects.length > 0 && (
            <p className="note-sand">בטקסט יש {p.suspects.length} מילים שנראות כמו שמות שלא הוגדרו בתיק. לפני שליחה ל-{ai} תתבקשי להחליט לגביהן.</p>
          )}

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
              {body === p.body ? <Segments segments={p.preview} side="original" /> : body}
            </div>
          )}
        </section>
        <ErrorLine error={error} />
      </div>
    </Dialog>
  );
}
