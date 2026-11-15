// Round 36, beside a section: what her past reports offer, worked out on her computer and
// never sent. Her diagnoses and recommendations to choose from (K9, K10), each with how often
// she gave it in reports like this case, and the reports most like this case (K14). Plain text.
//
// D-168: she ticks the lines she wants and one button adds them, in the list's order, as her
// own approved paragraphs. A line the section already holds is shown as added and cannot be
// added twice: that is read from the section's paragraphs, not remembered here. Folded into
// one quiet line until opened; on the diagnoses section it is open and leads (`lead`), since
// the psychologist diagnoses, not the AI.
import { useQuery } from "@tanstack/react-query";
import { useRef, useState } from "react";
import { useApp } from "../App";
import { ipc, type LibraryLine } from "../ipc/client";
import { rolesAsWords } from "./Similar";
import { ErrorLine } from "./ui";
import "./SectionLibrary.css";

const LIST_TITLE: Record<string, string> = {
  diagnoses: "האבחנות שאת נותנת",
  recommendations: "ההמלצות שלך",
};

/** One and many, for the folded line. */
const KIND: Record<string, [string, string]> = {
  diagnoses: ["אבחנה אחת שחוזרת", "אבחנות שחוזרות"],
  recommendations: ["המלצה אחת שחוזרת", "המלצות שחוזרות"],
};

export function lineDetail(l: LibraryLine): string {
  if (l.similar > 0 && l.in_similar > 0) return `ב-${l.in_similar} מ-${l.similar} דוחות דומים`;
  return `ב-${l.reports} דוחות`;
}

/** A line and a paragraph that say the same thing: spaces and the closing mark apart. */
export function sameLine(text: string): string {
  return text.normalize("NFC").replace(/\s+/g, " ").trim().replace(/[.,;:׃]+$/u, "").trim();
}

/** The folded line: how many lines her reports offer for the section. */
export function librarySummary(sectionKey: string, lines: number): string {
  if (lines === 0) return "מהדוחות שלך: מה כתבת בסעיף הזה בדוחות דומים";
  const [one, many] = KIND[sectionKey] ?? ["שורה אחת שחוזרת", "שורות שחוזרות"];
  return `מהדוחות שלך: ${lines === 1 ? one : `${lines} ${many}`}`;
}

export function SectionLibraryPanel(props: {
  caseId: string;
  sectionKey: string;
  /** The section's paragraphs as she sees them: a line among them is already added. */
  present: readonly string[];
  /** Open from the start (the diagnoses section, where her choice leads). */
  lead?: boolean;
  onAdded: () => Promise<void> | void;
}) {
  const { fail, notify } = useApp();
  const [error, setError] = useState<string | null>(null);
  const [picked, setPicked] = useState<string[]>([]);
  const [busy, setBusy] = useState(false);
  const adding = useRef(false);
  const lib = useQuery({
    queryKey: ["section-library", props.caseId, props.sectionKey],
    queryFn: () => ipc.sectionLibrary(props.caseId, props.sectionKey),
  });
  const data = lib.data;
  if (!data || (data.lines.length === 0 && data.similar.length === 0)) return null;

  const present = new Set(props.present.map(sameLine));
  const added = (text: string) => present.has(sameLine(text));
  // In the list's order, whatever order she ticked them in; never one the section holds.
  const chosen = data.lines.map((l) => l.text).filter((t) => picked.includes(t) && !added(t));

  // One at a time, in order, so they land in the section as the list shows them. A second
  // press while they are being added does nothing.
  async function addChosen() {
    if (adding.current || chosen.length === 0) return;
    adding.current = true;
    setBusy(true);
    setError(null);
    let count = 0;
    try {
      for (const text of chosen) {
        await ipc.addOwnParagraph(props.caseId, props.sectionKey, text);
        count += 1;
        setPicked((p) => p.filter((t) => t !== text));
      }
    } catch (e) {
      setError(fail(e as never));
    } finally {
      if (count > 0) {
        notify(count === 1 ? "נוסף לסעיף." : `${count} נוספו לסעיף.`);
        await props.onAdded();
      }
      adding.current = false;
      setBusy(false);
    }
  }

  return (
    <details className="section-library" open={props.lead ? true : undefined}>
      <summary>{librarySummary(props.sectionKey, data.lines.length)}</summary>
      <div className="stack library-body">
        <p className="muted small">נמצא במחשב שלך מהדוחות שהעלית ב"הסגנון שלי". לא נשלח.</p>
        {data.lines.length > 0 && (
          <div className="stack">
            <b className="small">{LIST_TITLE[props.sectionKey] ?? "השורות שחוזרות אצלך"}</b>
            <ul className="library-lines">
              {data.lines.map((l) => {
                const there = added(l.text);
                return (
                  <li key={l.text}>
                    <label className={there ? "library-line is-added" : "library-line"}>
                      <input type="checkbox" checked={there || picked.includes(l.text)} disabled={there || busy}
                        onChange={(e) => setPicked((p) => (e.target.checked ? [...p, l.text] : p.filter((t) => t !== l.text)))} />
                      <span className="grow">{l.text} <span className="muted small">({lineDetail(l)})</span></span>
                      {there && <span className="muted small">נוסף</span>}
                    </label>
                  </li>
                );
              })}
            </ul>
            <div className="row">
              <button type="button" className="btn btn-small btn-primary" disabled={busy || chosen.length === 0} onClick={() => void addChosen()}>
                {chosen.length > 0 ? `להוסיף ${chosen.length}` : "להוסיף"}
              </button>
              <span className="muted small">{chosen.length > 0 ? "ייכנסו לסעיף כפסקאות שלך, לפי הסדר שברשימה." : "מסמנים שורות, ומוסיפים את כולן יחד."}</span>
            </div>
          </div>
        )}
        {data.similar.length > 0 && (
          <div className="stack">
            <b className="small">מה כתבת בסעיף הזה בדוחות הכי דומים</b>
            {data.similar.map((s) => (
              <blockquote key={`${s.report}:${s.text}`} className="library-similar">
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
