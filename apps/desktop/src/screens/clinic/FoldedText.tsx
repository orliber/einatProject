// Plain text that folds to a few lines when it is long: a session's record on the patient's
// card, and what the last session left when a visit is opened ("לקראת הפגישה").
import { useId, useState } from "react";
import "./PatientCardView.css";

/** From how much text a record is folded to a few lines. */
const LONG_RECORD = 260;

/** Plain text that folds to four lines when it is long, with the link that opens it whole. */
export function FoldedText({ text, more, less }: { text: string; more: string; less: string }) {
  const [whole, setWhole] = useState(false);
  const id = useId();
  const long = text.length > LONG_RECORD || text.split("\n").length > 4;
  return (
    <>
      <p id={id} className={long && !whole ? "pcard-note-text pcard-note-folded" : "pcard-note-text"}>{text}</p>
      {long && (
        <button type="button" className="link-btn" aria-expanded={whole} aria-controls={id} onClick={() => setWhole((w) => !w)}>
          {whole ? less : more}
        </button>
      )}
    </>
  );
}
