import { Fragment, useEffect, useRef, useState } from "react";
import { useApp } from "../../App";
import { STALE_EDIT_KEPT } from "../../i18n/he";
import { ipc, type ExportCheck, type ReportSettings } from "../../ipc/client";
import type { ParagraphCheck } from "../../ipc/generated/ParagraphCheck";
import { getKept, useKept } from "../../kept";
import { ageWords } from "../../components/AgeField";
import { ErrorLine } from "../../components/ui";
import { timed, WaitLine } from "../../components/Progress";
import { useHoldUnsaved } from "../../components/Unsaved";
import { LocalText, type LocalTextHandle } from "../../components/LocalText";
import { ReportCheckItem } from "../../components/ReportCheck";
import type { CaseApi } from "../CaseScreen";
import { LetterCard } from "./LetterCard";
import "./FinishView.css";

const MARK = /\[חסר[^\]]*\]/;

/** A paragraph with its "[חסר: …]" markers highlighted. */
function Marked({ text }: { text: string }) {
  const parts = text.split(/(\[חסר[^\]]*\])/g);
  return <>{parts.map((p, i) => (p.startsWith("[חסר") ? <mark key={i} className="a4-mark">{p}</mark> : <Fragment key={i}>{p}</Fragment>))}</>;
}

/**
 * The last stage: the report as the file will be, always in view (almost empty, full, or with
 * things to complete), beside what is checked before the file is made. A missing-information
 * marker is completed right here; it never stops the file, it only asks her to confirm.
 */
export function FinishView({ api, onExport, onOpen }: { api: CaseApi; onExport: () => void; onOpen: (key: string) => void }) {
  const { fail, notify } = useApp();
  const { detail, caseId, reload } = api;
  const [check, setCheck] = useState<ExportCheck | null>(null);
  const [settings, setSettings] = useState<ReportSettings | null>(null);
  /** A marked paragraph being completed: the text is in `editBox`; `text` is its debounced copy, for the lock. */
  // Kept while the vault is open: leaving and coming back finds the editor with her text.
  const [editing, setEditing] = useKept<{ id: string; text: string; original?: string } | null>(`case:${caseId}:finish:editing`, null);
  const editBox = useRef<LocalTextHandle>(null);
  const [error, setError] = useState<string | null>(null);
  const holdLive = useHoldUnsaved(caseId, "פסקה בשלב הסיום", editing?.text ?? null);

  useEffect(() => {
    let alive = true;
    timed("check", ipc.checkExport(caseId)).then((c) => alive && setCheck(c)).catch((e: unknown) => alive && setError(fail(e as never)));
    return () => {
      alive = false;
    };
  }, [caseId, detail, fail]);
  useEffect(() => {
    ipc.reportSettings().then(setSettings).catch(() => setSettings(null));
  }, []);

  const content = detail.sections.filter((s) => s.key !== "signature");
  const parts = Array.from(new Set(content.map((s) => s.part)));
  const waiting = content.filter((s) => s.paragraphs.some((p) => p.status === "proposed" && !p.replaces));
  const toComplete = content.flatMap((s) =>
    s.paragraphs.filter((p) => p.status === "approved" && MARK.test(p.text)).map((p) => ({ section: s, p })),
  );
  // Approved paragraphs with something worth a second look: a number unlike the score table,
  // a word the answer scan flagged, a sentence from an old report, or what the check against
  // the sources said in this session. None of these stops the file; she decides.
  const flagged = content.flatMap((s) => {
    const checks = getKept<Map<string, ParagraphCheck>>(`case:${caseId}:section:${s.key}:checks`, new Map());
    return s.paragraphs
      .filter((p) => p.status === "approved")
      .flatMap((p) => {
        const verdict = checks.get(p.id)?.verdict;
        const notes = [
          ...p.warnings,
          ...(verdict === "contradicted" ? ["בבדיקה מול המקורות: סותרת את החומרים"] : []),
          ...(verdict === "unsupported" ? ["בבדיקה מול המקורות: לא נמצא לה בסיס בחומרים"] : []),
        ];
        return notes.length ? [{ section: s, p, notes }] : [];
      });
  });
  const child = detail.identities.find((i) => i.role === "child")?.value ?? "";
  const blocked = (check?.blocking.length ?? 0) > 0;
  const nothing = check !== null && check.included_sections === 0;
  // Round 26: what is left to complete is listed, and never stops the file. The core also marks
  // a tag it could not put words back into; those sections are listed by name.
  const ready = check !== null && !blocked && !nothing;
  const markedElsewhere = (check?.to_complete ?? []).filter((t) => !toComplete.some(({ section }) => t.startsWith(`${section.title}:`)));

  async function save() {
    if (!editing) return;
    const text = editBox.current?.value() ?? editing.text;
    setError(null);
    try {
      if (await ipc.editParagraph(caseId, editing.id, text, editing.original ?? null)) notify(STALE_EDIT_KEPT);
      setEditing(null);
      await reload();
    } catch (e) {
      setError(fail(e as never));
    }
  }

  return (
    <div className="view finish">
      <div className="finish-body">
        <section className="finish-side" aria-label="לפני שהקובץ יוצא">
          <div className="stack" style={{ gap: 4 }}>
            <h1>{ready ? "הדוח מוכן להוצאה" : nothing ? "עוד אין מה להוציא" : "כמעט מוכן"}</h1>
            <span className="muted">משמאל: הדוח כמו שייצא. כאן: מה עוד לעשות.</span>
          </div>
          {!check && !error && <WaitLine task="check" label="בודקת שהדוח מוכן להוצאה" />}
          {check && (
            <ul className="finish-checks">
              <li className={check.included_sections > 0 ? "ok" : "warn"}>
                <span className="finish-mark" aria-hidden="true">{check.included_sections > 0 ? "✓" : "!"}</span>
                <span className="grow">
                  {check.included_sections > 0 ? `${check.included_sections} סעיפים מאושרים ייכנסו לקובץ.` : "עוד אין סעיף מאושר."}
                  {check.empty_sections.length > 0 && <span className="small muted"> {check.empty_sections.length} ריקים לא ייכנסו.</span>}
                </span>
                <button type="button" className="link-small" onClick={() => onOpen("")}>לדוח</button>
              </li>
              {waiting.length > 0 && (
                <li className="info">
                  <span className="finish-mark" aria-hidden="true">i</span>
                  <span className="grow">טיוטות שעוד לא אושרו לא ייכנסו: {waiting.map((s) => s.title).join(", ")}.</span>
                  <button type="button" className="link-small" onClick={() => onOpen(waiting[0]?.key ?? "")}>לאישור</button>
                </li>
              )}
              {flagged.length > 0 && (
                <li className="info">
                  <span className="finish-mark" aria-hidden="true">i</span>
                  <div className="grow stack" style={{ gap: 6 }}>
                    <span>{flagged.length === 1 ? "פסקה מאושרת אחת שווה מבט נוסף:" : `${flagged.length} פסקאות מאושרות שוות מבט נוסף:`}</span>
                    {flagged.map(({ section, p, notes }) => (
                      <button key={p.id} type="button" className="complete-item" onClick={() => onOpen(section.key)}>
                        <span className="small muted">{section.title} · {notes.join(" · ")}</span>
                        <span className="serif">{p.text.length > 140 ? `${p.text.slice(0, 140)}…` : p.text}</span>
                      </button>
                    ))}
                  </div>
                </li>
              )}
              <ReportCheckItem api={api} onOpen={onOpen} />
              {check.blocking.map((b) => (
                <li key={b} className="warn">
                  <span className="finish-mark" aria-hidden="true">!</span>
                  <span className="grow">{b}</span>
                  <button type="button" className="link-small" onClick={() => onOpen(content.find((s) => b.startsWith(`${s.title}:`))?.key ?? "")}>לתיקון</button>
                </li>
              ))}
              {toComplete.length > 0 && (
                <li className="warn complete">
                  <span className="finish-mark" aria-hidden="true">!</span>
                  <div className="grow stack" style={{ gap: 8 }}>
                    <b>{toComplete.length === 1 ? "מקום אחד מסומן להשלמה" : `${toComplete.length} מקומות מסומנים להשלמה`}</b>
                    {toComplete.map(({ section, p }) =>
                      editing?.id === p.id ? (
                        <div key={p.id} className="stack" style={{ gap: 6 }}>
                          <span className="small muted">{section.title}</span>
                          <LocalText key={p.id} ref={editBox} className="textarea serif" rows={4} autoFocus initial={editing.text} onLive={holdLive}
                            onText={(text) => setEditing((e) => (e?.id === p.id ? { ...e, text } : e))} />
                          <div className="row">
                            <button type="button" className="btn btn-primary btn-small" onClick={() => void save()}>שמירה</button>
                            <button type="button" className="btn btn-small" onClick={() => setEditing(null)}>ביטול</button>
                          </div>
                        </div>
                      ) : (
                        <button key={p.id} type="button" className="complete-item" onClick={() => setEditing({ id: p.id, text: p.text, original: p.text })}>
                          <span className="small muted">{section.title} · להשלמה כאן</span>
                          <span className="serif"><Marked text={p.text} /></span>
                        </button>
                      ),
                    )}
                    <span className="small muted">הקובץ יוצא גם כך: הסימונים נשארים בו, ואפשר להשלים אותם בוורד.</span>
                  </div>
                </li>
              )}
              {markedElsewhere.length > 0 && (
                <li className="warn">
                  <span className="finish-mark" aria-hidden="true">!</span>
                  <span className="grow">יסומנו בקובץ להשלמה: {markedElsewhere.join(" · ")}</span>
                </li>
              )}
              {!blocked && check.included_sections > 0 && (
                <li className="ok">
                  <span className="finish-mark" aria-hidden="true">✓</span>
                  <span className="grow">השמות חוזרים למקומם בקובץ.</span>
                </li>
              )}
            </ul>
          )}
          <ErrorLine error={error} />
          <section className="card finish-file">
            <b>קובץ Word או PDF</b>
            <span className="muted small">נשמר בתיקיית ההורדות, ונפתח בלי סיסמה.</span>
            <button type="button" className="btn btn-primary btn-big" disabled={!ready} onClick={onExport}>הפקת הקובץ</button>
            {!ready && check && (
              <span className="small muted">
                {nothing ? "צריך לפחות סעיף מאושר אחד." : "קודם מתקנים את מה שמסומן למעלה."}
              </span>
            )}
          </section>
          <LetterCard api={api} />
        </section>

        <div className="finish-preview" role="region" tabIndex={0} aria-label="תצוגה מקדימה של הקובץ">
          <article className="a4 a4-small" style={{ fontFamily: settings?.font ? `"${settings.font}", var(--font-display)` : undefined }}>
            <header className="a4-header">{settings?.confidentiality}</header>
            <h1 className="a4-title">{settings?.title ?? "דוח אבחון פסיכולוגי"}</h1>
            <dl className="a4-info">
              {child && <><dt>שם הילד/ה</dt><dd>{child}</dd></>}
              {detail.meta.age && <><dt>גיל בזמן האבחון</dt><dd>{ageWords(detail.meta.age)}</dd></>}
              <dt>תאריך הדוח</dt><dd>{new Date().toLocaleDateString("he-IL")}</dd>
            </dl>
            {parts.map((part) => {
              const sections = content.filter((s) => s.part === part);
              return (
                <section key={part}>
                  <h2 className="a4-part">{part}</h2>
                  {sections.map((s) => {
                    const ok = s.paragraphs.filter((p) => p.status === "approved");
                    return (
                      <div key={s.key} className={ok.length ? "a4-section" : "a4-section a4-out"}>
                        <h3 className="a4-heading">{s.title}</h3>
                        {ok.length ? ok.map((p) => <p key={p.id} className="a4-p"><Marked text={p.text} /></p>)
                          : <p className="a4-out-note">לא ייכנס לקובץ (אין פסקה מאושרת)</p>}
                      </div>
                    );
                  })}
                </section>
              );
            })}
          </article>
        </div>
      </div>
    </div>
  );
}
