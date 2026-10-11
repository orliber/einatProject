// What the check against the sources said about one proposed paragraph. Text only; it never
// changes the paragraph or approves it.
import type { ParagraphCheck, Verdict } from "../ipc/client";
import { WarnIcon } from "./ui";

const LABEL: Record<Verdict, string> = {
  supported: "נבדק מול המקורות: הטענות נתמכות",
  partial: "נבדק מול המקורות: נתמך חלקית",
  unsupported: "נבדק מול המקורות: יש טענה שאין לה תמיכה",
  contradicted: "נבדק מול המקורות: סותר מקור",
  unchecked: "הפסקה הזאת לא נבדקה",
};

export function VerdictNote({ check }: { check: ParagraphCheck }) {
  const fine = check.verdict === "supported";
  return (
    <div className={fine ? "verdict verdict-ok" : "note-warn"}>
      {fine ? <span aria-hidden="true">✓</span> : <WarnIcon />}
      <span>
        <b>{LABEL[check.verdict]}</b>
        {check.notes.map((n, i) => (
          <span key={i} className="verdict-note">
            {" "}· {n.note}{n.source_refs.length > 0 && ` (${n.source_refs.join(", ")})`}
          </span>
        ))}
      </span>
    </div>
  );
}
