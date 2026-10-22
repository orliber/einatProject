// The check of the whole report (G2 stage two, D-097): once every draft is approved, one
// request with the approved sections only asks where two sections contradict each other,
// where something is said twice, and where a finding is in the summary but not in the body.
// Off until she turns it on in settings, where its cost is shown first. The notes are plain
// text; they only point at a section, and nothing in the report changes.
import { useState } from "react";
import { useQuery, useQueryClient } from "@tanstack/react-query";
import { useApp } from "../App";
import { ipc, type ReportCheckResult, type ReportNoteKind } from "../ipc/client";
import { useKept } from "../kept";
import type { CaseApi } from "../screens/CaseScreen";
import { Switch } from "./Switch";
import { ErrorLine, Spinner } from "./ui";

const KIND: Record<ReportNoteKind, string> = {
  contradiction: "סתירה בין סעיפים",
  repetition: "חזרה",
  summary_only: "מופיע רק בסיכום",
  other: "הערה",
};

/** "כ-15 סנט" / "כ-$1.20": what one check costs, rounded up. */
export function costWords(cents: number): string {
  return cents < 100 ? `כ-${cents} סנט` : `כ-$${(cents / 100).toFixed(2)}`;
}

/** The switch in settings. Turning it on asks first, with the cost of one check. */
export function ReportCheckSettings() {
  const { notify, fail } = useApp();
  const qc = useQueryClient();
  const setting = useQuery({ queryKey: ["report-check"], queryFn: ipc.reportCheckSetting });
  const [asking, setAsking] = useState(false);
  const cents = setting.data?.cents_per_check ?? null;
  const cost = cents === null ? "העלות לא ידועה לדגם הזה" : `${costWords(cents)} לבדיקה של דוח באורך רגיל (הערכה)`;

  async function change(value: boolean) {
    setAsking(false);
    try {
      await ipc.setReportCheck(value);
      await qc.invalidateQueries({ queryKey: ["report-check"] });
      notify(value ? "בדיקת הדוח כולו פעילה. היא מופיעה במסך הסיום." : "בדיקת הדוח כולו כבויה.");
    } catch (e) {
      notify(fail(e as never));
    }
  }

  return (
    <section className="card setting" aria-labelledby="s-report-check">
      <h2 id="s-report-check">בדיקת הדוח כולו</h2>
      <Switch id="report-check" label="לאפשר בדיקה של הדוח כולו לפני ההפקה" checked={setting.data?.on ?? false} disabled={setting.data === undefined}
        hint={`אחרי שכל הסעיפים אושרו, ה-AI קורא את הסעיפים המאושרים בלבד ומצביע על סתירות בין סעיפים, חזרות, וממצא שמופיע בסיכום ולא בגוף הדוח. בקשה אחת לדוח, באותו סינון, בלי לשנות שום מילה. ${cost}.`}
        onChange={(v) => (v ? setAsking(true) : void change(false))} />
      {asking && (
        <div className="note-warn stack" role="group" aria-label="לפני ההפעלה" style={{ gap: 8 }}>
          <span>
            כל בדיקה היא בקשה נוספת, ועולה {cents === null ? "סכום שלא ידוע לדגם הזה" : costWords(cents)} (לפי מחירון, לדוח באורך רגיל).
            היא רצה רק כשלוחצים עליה במסך הסיום, ונרשמת ב"מה יצא מהמחשב".
          </span>
          <div className="row">
            <button type="button" className="btn btn-primary btn-small" onClick={() => void change(true)}>להפעיל</button>
            <button type="button" className="btn btn-small" onClick={() => setAsking(false)}>ביטול</button>
          </div>
        </div>
      )}
    </section>
  );
}

/** The line in the finish screen: the button, and then the notes, each opening its section. */
export function ReportCheckItem({ api, onOpen }: { api: CaseApi; onOpen: (key: string) => void }) {
  const { fail } = useApp();
  const { caseId, detail } = api;
  const setting = useQuery({ queryKey: ["report-check"], queryFn: ipc.reportCheckSetting });
  // Kept while the vault is open: leaving the finish screen and coming back keeps the notes.
  const [result, setResult] = useKept<ReportCheckResult | null>(`case:${caseId}:finish:report-check`, null);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  if (!setting.data?.on) return null;

  const content = detail.sections.filter((s) => s.key !== "signature");
  const title = (key: string) => content.find((s) => s.key === key)?.title ?? key;
  const waiting = content.filter((s) => s.paragraphs.some((p) => p.status === "proposed" && !p.replaces));
  const approved = content.filter((s) => s.paragraphs.some((p) => p.status === "approved")).length;
  const ready = waiting.length === 0 && approved >= 2;

  async function run() {
    setError(null);
    setBusy(true);
    try {
      const prepare = () => ipc.prepareReportCheck(caseId);
      const prepared = await prepare();
      await api.review({
        title: "בדיקת הדוח כולו",
        prepared,
        reprepare: prepare,
        onSend: async (id) => {
          setResult(await ipc.sendReportCheck(id));
        },
      });
    } catch (e) {
      setError(fail(e as never));
    } finally {
      setBusy(false);
    }
  }

  return (
    <li className="info">
      <span className="finish-mark" aria-hidden="true">i</span>
      <div className="grow stack" style={{ gap: 6 }}>
        <b>בדיקת הדוח כולו</b>
        {!ready && (
          <span className="small muted">
            {waiting.length > 0 ? `אחרי שכל הטיוטות אושרו. עוד מחכות: ${waiting.map((s) => s.title).join(", ")}.` : "צריך לפחות שני סעיפים מאושרים."}
          </span>
        )}
        {busy && <span className="muted"><Spinner /> קוראת את הדוח כולו…</span>}
        {result && !busy && (
          <>
            {result.notes.length === 0 ? (
              <span>לא נמצאו סתירות בין סעיפים, חזרות או ממצא שמופיע רק בסיכום.</span>
            ) : (
              <span>{result.notes.length === 1 ? "הערה אחת לבדיקה:" : `${result.notes.length} הערות לבדיקה:`}</span>
            )}
            {result.notes.map((n, i) => {
              const where = n.sections.map(title).join(" · ");
              const body = (
                <>
                  <span className="small muted">{KIND[n.kind]}{where && ` · ${where}`}</span>
                  <span>{n.note}</span>
                </>
              );
              const first = n.sections[0];
              return first !== undefined ? (
                <button key={i} type="button" className="complete-item" onClick={() => onOpen(first)}>{body}</button>
              ) : (
                <div key={i} className="complete-item">{body}</div>
              );
            })}
            <span className="small muted">
              {result.demo ? "מצב הדגמה: נמצאו רק משפטים שחוזרים מילה במילה ומספרים שמופיעים רק בסיכום, בלי שליחה. " : ""}
              ההערות רק מצביעות; שום דבר בדוח לא השתנה, וההחלטה שלך.
            </span>
          </>
        )}
        <ErrorLine error={error} />
        <div className="row">
          <button type="button" className="btn btn-small" disabled={!ready || busy} onClick={() => void run()}>
            {result ? "לבדוק שוב" : "לבדוק את הדוח כולו"}
          </button>
        </div>
      </div>
    </li>
  );
}
