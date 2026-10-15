// Round 36, beside a section: what her past reports offer, worked out on her computer and
// never sent. Her diagnoses and recommendations to choose from (K9, K10), each with how often
// she gave it in reports like this case, and the reports most like this case (K14). Plain text.
import { useQuery } from "@tanstack/react-query";
import { useState } from "react";
import { useApp } from "../App";
import { ipc, type LibraryLine } from "../ipc/client";
import { rolesAsWords } from "./Similar";
import { ErrorLine } from "./ui";
import "./SectionLibrary.css";

const LIST_TITLE: Record<string, string> = {
  diagnoses: "האבחנות שאת נותנת",
  recommendations: "ההמלצות שלך",
};

export function lineDetail(l: LibraryLine): string {
  if (l.similar > 0 && l.in_similar > 0) return `ב-${l.in_similar} מ-${l.similar} דוחות דומים`;
  return `ב-${l.reports} דוחות`;
}

export function SectionLibraryPanel(props: { caseId: string; sectionKey: string; onAdded: () => Promise<void> | void }) {
  const { fail, notify } = useApp();
  const [error, setError] = useState<string | null>(null);
  const [added, setAdded] = useState<string[]>([]);
  const lib = useQuery({
    queryKey: ["section-library", props.caseId, props.sectionKey],
    queryFn: () => ipc.sectionLibrary(props.caseId, props.sectionKey),
  });
  const data = lib.data;
  if (!data || (data.lines.length === 0 && data.similar.length === 0)) return null;

  async function add(text: string) {
    setError(null);
    try {
      await ipc.addOwnParagraph(props.caseId, props.sectionKey, text);
      setAdded((a) => [...a, text]);
      notify("נוסף לסעיף.");
      await props.onAdded();
    } catch (e) {
      setError(fail(e as never));
    }
  }

  return (
    <details className="section-library">
      <summary>מהדוחות הקודמים שלך</summary>
      <div className="stack library-body">
        <p className="muted small">נמצא במחשב שלך מהדוחות שהעלית ב"הסגנון שלי". לא נשלח.</p>
        {data.lines.length > 0 && (
          <div className="stack">
            <b className="small">{LIST_TITLE[props.sectionKey] ?? "השורות שחוזרות אצלך"}</b>
            <ul className="library-lines">
              {data.lines.map((l) => (
                <li key={l.text} className="row">
                  <span className="grow">{l.text} <span className="muted small">({lineDetail(l)})</span></span>
                  <button type="button" className="btn btn-small" disabled={added.includes(l.text)}
                    onClick={() => void add(l.text)}>{added.includes(l.text) ? "נוסף" : "להוסיף"}</button>
                </li>
              ))}
            </ul>
          </div>
        )}
        {data.similar.length > 0 && (
          <div className="stack">
            <b className="small">מה כתבת בסעיף הזה בדוחות הכי דומים</b>
            {data.similar.map((s) => (
              <blockquote key={s.report} className="library-similar">
                <span className="small muted">{s.report}{s.reasons.length > 0 && ` · ${s.reasons.join(" · ")}`}</span>
                <p className="serif">{rolesAsWords(s.text)}</p>
              </blockquote>
            ))}
          </div>
        )}
        <ErrorLine error={error} />
      </div>
    </details>
  );
}
