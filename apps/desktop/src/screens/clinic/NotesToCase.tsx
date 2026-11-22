// The bridge from the clinic to the report: when the card is linked to a diagnostic case and
// some session records were not copied to its materials yet, the notes box says how many, and
// one window copies the ones she chooses. Each copy is the same step as "להעביר לחומרי התיק"
// in the note's own window (D-171): only the record, never the personal note or a recording,
// from the vault to the vault. Nothing leaves the computer here; what later goes to the AI
// from the case goes through the case's own filter, as every material does.
import { useState } from "react";
import { useApp } from "../../App";
import { Dialog, ErrorLine } from "../../components/ui";
import type { PatientCard } from "../../ipc/generated/PatientCard";
import type { SessionNote } from "../../ipc/generated/SessionNote";
import { useNoteToCase } from "./clinicData";
import { formatDate, VISIT_KIND_LABEL } from "./format";

/** The records that can still go to the case: written, and not copied yet. Oldest first, the
 *  order the report tells them in. */
export function notesForCase(card: PatientCard): SessionNote[] {
  if (card.case === null || card.case.in_trash) return [];
  return card.notes
    .filter((n) => n.case_input_id === null && n.record.trim() !== "")
    .sort((a, b) => (a.date === b.date ? 0 : a.date < b.date ? -1 : 1));
}

/** The line under the notes' title, and the window it opens. Nothing when there is nothing to copy. */
export function NotesToCase({ card }: { card: PatientCard }) {
  const [open, setOpen] = useState(false);
  const waiting = notesForCase(card);
  if (waiting.length === 0 || card.case === null) return null;
  return (
    <div className="row note-sand pcard-to-case">
      <span className="grow">
        {waiting.length === 1 ? "רשומה אחת עוד לא הועברה" : `${waiting.length} רשומות עוד לא הועברו`} לחומרי תיק האבחון {card.case.code}.
      </span>
      <button type="button" className="btn btn-small" onClick={() => setOpen(true)}>להעביר לתיק</button>
      {open && <ToCaseDialog card={card} notes={waiting} onClose={() => setOpen(false)} />}
    </div>
  );
}

function ToCaseDialog({ card, notes, onClose }: { card: PatientCard; notes: SessionNote[]; onClose: () => void }) {
  const { notify } = useApp();
  const toCase = useNoteToCase();
  const [chosen, setChosen] = useState(() => new Set(notes.map((n) => n.id)));
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const kindOf = (n: SessionNote) => {
    const visit = card.visits.find((v) => v.visit.id === n.visit_id)?.visit;
    return visit ? VISIT_KIND_LABEL[visit.kind] : "שיחה";
  };
  const toggle = (id: string) => setChosen((old) => {
    const next = new Set(old);
    if (!next.delete(id)) next.add(id);
    return next;
  });

  async function send() {
    setBusy(true);
    setError(null);
    let done = 0;
    try {
      // One after the other: a refusal names where it stopped, and what was copied stays copied.
      for (const n of notes) {
        if (!chosen.has(n.id)) continue;
        await toCase.mutateAsync(n.id);
        done += 1;
      }
      notify(done === 1 ? "רשומה אחת הועתקה לחומרי התיק" : `${done} רשומות הועתקו לחומרי התיק`);
      onClose();
    } catch (e) {
      const why = (e as { message?: string }).message ?? "הרשומה לא הועברה. אפשר לנסות שוב.";
      setError(done > 0 ? `${done === 1 ? "רשומה אחת הועתקה" : `${done} רשומות הועתקו`}, ואז: ${why}` : why);
      setBusy(false);
    }
  }

  return (
    <Dialog narrow title="להעביר רשומות לתיק האבחון" subtitle={`תיק ${card.case?.code ?? ""}`} {...(busy ? {} : { onClose })}
      footer={<>
        <span className="grow" />
        <button type="button" className="btn" disabled={busy} onClick={onClose}>ביטול</button>
        <button type="button" className="btn btn-primary" disabled={busy || chosen.size === 0} onClick={() => void send()}>
          {chosen.size === 1 ? "להעביר רשומה אחת" : `להעביר ${chosen.size} רשומות`}
        </button>
      </>}>
      <div className="stack">
        <p>הרשומות שנבחרו יועתקו לחומרי התיק, וישמשו לכתיבת הדוח. רק הרשומה מועתקת: התרשומת האישית וההקלטות נשארות בכרטיס.</p>
        <ul className="clinic-list pcard-to-case-list" aria-label="הרשומות שעוד לא הועברו">
          {notes.map((n) => (
            <li key={n.id}>
              <label className="row clinic-check">
                <input type="checkbox" checked={chosen.has(n.id)} disabled={busy} onChange={() => toggle(n.id)} />
                <span className="num" dir="ltr">{formatDate(n.date)}</span>
                <span>{kindOf(n)}</span>
              </label>
            </li>
          ))}
        </ul>
        <ErrorLine error={error} />
      </div>
    </Dialog>
  );
}
