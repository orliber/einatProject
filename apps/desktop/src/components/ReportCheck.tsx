// The check of the whole report (G2 stage two, D-097): once every draft is approved, one
// request with the approved sections only asks where two sections contradict each other,
// where something is said twice, and where a finding is in the summary but not in the body;
// and, as a supervisor would read it (D-169), whether the summary answers the referral, a
// difficulty has no recommendation, a recommendation has no finding, the diagnosis differs
// between sections, or a professional term is left unexplained.
// Off until she turns it on in settings, where its cost is shown first. The notes are plain
// text; they only point at a section, and nothing in the report changes.
import { useState } from "react";
import { useQuery, useQueryClient } from "@tanstack/react-query";
import { useApp } from "../App";
import { ipc, type ReportCheckResult, type ReportNoteKind } from "../ipc/client";
import { useKept, useKeptValue } from "../kept";
import type { CaseApi } from "../screens/CaseScreen";
import { Switch } from "./Switch";
import { ErrorLine, Spinner } from "./ui";

/** A note of the check, by kind: what it is called. */
export const REPORT_NOTE_TITLE: Record<ReportNoteKind, string> = {
  contradiction: "סתירה בין סעיפים",
  repetition: "חזרה",
  summary_only: "מופיע רק בסיכום",
  referral_unanswered: "סיבת ההפניה בלי מענה בסיכום",
  no_recommendation: "קושי בלי המלצה",
  unfounded_recommendation: "המלצה בלי ממצא",
  diagnosis_mismatch: "האבחנה לא זהה בין הסעיפים",
  unexplained_term: "מונח מקצועי בלי הסבר",
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
        hint={`אחרי שכל הסעיפים אושרו, ה-AI קורא את הסעיפים המאושרים בלבד ומצביע על סתירות בין סעיפים, חזרות, וממצא שמופיע בסיכום ולא בגוף הדוח; וגם על סיבת הפניה שהסיכום לא עונה עליה, קושי בלי המלצה, המלצה בלי ממצא, אבחנה שלא זהה בין הסעיפים, ומונח מקצועי בלי הסבר. בקשה אחת לדוח, באותו סינון, בלי לשנות שום מילה. ${cost}.`}
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

const resultKey = (caseId: string) => `case:${caseId}:finish:report-check`;

/** What the last check of this case found (kept while the vault is open), for the finish
 *  page's list; `null` before a check. */
export function useReportCheckResult(caseId: string): ReportCheckResult | null {
  return useKeptValue<ReportCheckResult | null>(resultKey(caseId)) ?? null;
}

/** The quiet row in the finish screen: the button, and what the check said in one line. Its
 *  notes are cards in the page's list, with the other things worth a look. */
export function ReportCheckItem({ api }: { api: CaseApi }) {
  const { fail } = useApp();
  const { caseId, detail } = api;
  const setting = useQuery({ queryKey: ["report-check"], queryFn: ipc.reportCheckSetting });
  // Kept while the vault is open: leaving the finish screen and coming back keeps the notes.
  const [result, setResult] = useKept<ReportCheckResult | null>(resultKey(caseId), null);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  if (!setting.data?.on) return null;

  const content = detail.sections.filter((s) => s.key !== "signature");
  const waiting = content.filter((s) => s.paragraphs.some((p) => p.status === "proposed" && !p.replaces));
  const approved = content.filter((s) => s.paragraphs.some((p) => p.status === "approved")).length;
  const ready = waiting.length === 0 && approved >= 2;

  async function run() {
    setError(null);
    // The notes of an earlier check are not left under a new one that failed.
    setResult(null);
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
    <section className="fin-report-check" aria-label="בדיקת הדוח כולו">
      <div className="row">
        <b className="grow">בדיקת הדוח כולו</b>
        <button type="button" className="btn btn-small" disabled={!ready || busy} onClick={() => void run()}>
          {result ? "לבדוק שוב" : "לבדוק את הדוח כולו"}
        </button>
      </div>
      {!ready && (
        <span className="small muted">
          {waiting.length > 0 ? `אחרי שכל הטיוטות אושרו. עוד מחכות: ${waiting.map((s) => s.title).join(", ")}.` : "צריך לפחות שני סעיפים מאושרים."}
        </span>
      )}
      {busy && <span className="muted"><Spinner /> קוראת את הדוח כולו…</span>}
      {result && !busy && (
        <span className="small muted">
          {result.notes.length === 0
            ? "לא נמצאו הערות: אין סתירות בין סעיפים, חזרות, ממצא שמופיע רק בסיכום, או פער בין סיבת ההפניה, הממצאים, האבחנה וההמלצות."
            : result.notes.length === 1
              ? "נמצאה הערה אחת. היא ברשימה \"כדאי לבדוק\"."
              : `נמצאו ${result.notes.length} הערות. הן ברשימה "כדאי לבדוק".`}
          {result.demo ? " מצב הדגמה: נמצאו רק משפטים שחוזרים מילה במילה ומספרים שמופיעים רק בסיכום, בלי שליחה." : ""}
          {" "}שום דבר בדוח לא השתנה, וההחלטה שלך.
        </span>
      )}
      <ErrorLine error={error} />
    </section>
  );
}
