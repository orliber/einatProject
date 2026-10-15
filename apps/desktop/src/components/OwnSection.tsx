// A section she writes herself (round 30): in place of the conversation, how she wrote it in
// her reports (from this computer, never sent), and her points with "לנסח מהנקודות שלי",
// which sends only the points, the section's guide and her style. Everything is plain text.
import { useRef, useState } from "react";
import { useQuery } from "@tanstack/react-query";
import { useKept } from "../kept";
import { useApp } from "../App";
import { ipc } from "../ipc/client";
import { LocalText, type LocalTextHandle } from "./LocalText";
import { useHoldUnsaved } from "./Unsaved";
import { rolesAsWords } from "./Similar";
import { ErrorLine, Spinner } from "./ui";
import "./Why.css";
import "./OwnSection.css";

function List({ title, lines }: { title: string; lines: string[] }) {
  if (lines.length === 0) return null;
  return (
    <div className="stack own-list">
      <b className="small">{title}</b>
      <ul>
        {lines.map((l, i) => <li key={i} className="serif">{rolesAsWords(l)}</li>)}
      </ul>
    </div>
  );
}

export function OwnSectionPanel(props: {
  caseId: string;
  sectionKey: string;
  ai: string;
  working: boolean;
  /** Prepare, review and send the points; resolves once they went (or were not sent). */
  onWord: (points: string) => Promise<boolean>;
}) {
  const { fail } = useApp();
  const help = useQuery({ queryKey: ["own-help", props.sectionKey], queryFn: () => ipc.ownSectionHelp(props.sectionKey) });
  const [points, setPoints] = useKept(`case:${props.caseId}:section:${props.sectionKey}:points`, "");
  const [empty, setEmpty] = useState(() => !points.trim());
  const box = useRef<LocalTextHandle>(null);
  // Her points are held for the automatic lock like a paragraph (the review of stage 3).
  const holdLive = useHoldUnsaved(props.caseId, "הנקודות שלך לסעיף", points.trim() ? points : null);
  const h = help.data;
  const nothing = h && h.examples.length === 0 && h.openings.length === 0 && h.phrases.length === 0;
  return (
    <section className="chat own-panel" aria-label="הסעיף שלך">
      <div className="chat-head">הסעיף שלך</div>
      <div className="chat-body stack">
        <p className="small muted">את כותבת את הסעיף הזה. שום חומר מהתיק לא נשלח בשבילו.</p>
        <div className="stack own-help">
          <b className="small">מהדוחות שלך (רק לעיון, לא נשלח)</b>
          {help.isLoading && <Spinner />}
          <ErrorLine error={help.error ? fail(help.error as never) : null} />
          {nothing && <p className="small muted">עוד אין בדוחות שהעלית את הסעיף הזה. אפשר להעלות דוחות קודמים במסך "הסגנון שלי".</p>}
          {h && h.examples.map((e, i) => (
            <blockquote key={i} className="why-item">
              <p className="serif">{rolesAsWords(e.text)}</p>
              <span className="small muted">{e.report}</span>
            </blockquote>
          ))}
          {h && <List title="איך את פותחת את הסעיף" lines={h.openings} />}
          {h && <List title="איך את סוגרת אותו" lines={h.closings} />}
          {h && <List title="הביטויים שלך לסעיף" lines={h.phrases} />}
        </div>
      </div>
      <form className="chat-foot" onSubmit={(e) => {
        e.preventDefault();
        const text = box.current?.value() ?? points;
        if (!text.trim() || props.working) return;
        void props.onWord(text).then((sent) => {
          if (sent) { setPoints(""); box.current?.set(""); setEmpty(true); }
        });
      }}>
        <label htmlFor="own-points" className="small">הנקודות שלך</label>
        <LocalText id="own-points" ref={box} rows={3} className="textarea" initial={points} onLive={holdLive}
          placeholder="נקודה בכל שורה: מה ראית, מה עלה במפגש. שמות מוסתרים לפני השליחה."
          onText={setPoints} onEmptyChange={setEmpty} />
        <div className="row">
          <span className="small muted grow">יוצאות רק הנקודות, ההנחיה לסעיף והסגנון שלך.</span>
          <button type="submit" className="btn btn-primary" disabled={props.working || empty}>לנסח מהנקודות שלי</button>
        </div>
      </form>
    </section>
  );
}
