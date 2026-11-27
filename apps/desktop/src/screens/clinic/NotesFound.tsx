// What the patients' search finds inside the session notes (D-216): "which family was it,
// with the speech therapist, three months ago?". She types a word in the same search box
// that finds a child by name, and under the list come the notes that hold it: whose, when,
// and a few words around it. A press opens that family's card. The search runs inside the
// vault; nothing is written and nothing leaves. In discreet mode the personal notes are not
// read and the words of the record are not shown, only that a note was found.
import { useDiscreet } from "../../discreet";
import type { NoteHit } from "../../ipc/generated/NoteHit";
import { useNoteSearch } from "./clinicData";
import { formatDate, VISIT_KIND_LABEL } from "./format";
import { PatientName, Section, useClinicNav } from "./parts";

const countWords = (n: number, more: boolean) => (more ? `${n} הראשונים, ויש עוד` : n === 1 ? "סיכום אחד" : `${n} סיכומים`);

export function NotesFound({ query }: { query: string }) {
  const discreet = useDiscreet();
  const found = useNoteSearch(query, !discreet);
  const nav = useClinicNav();
  const hits = found.data?.hits ?? [];
  // Nothing to say while it reads, under two letters, or when no note holds the words: the
  // list of patients above already answers the search.
  if (hits.length === 0) return null;
  return (
    <Section title="נמצא בסיכומי הפגישות" aside={<span className="muted small" role="status">{countWords(hits.length, found.data?.more === true)}</span>}>
      <ul className="clinic-list notes-found">
        {hits.map((hit) => <Hit key={hit.note_id} hit={hit} discreet={discreet} onOpen={() => nav.openPatient(hit.patient_id)} />)}
      </ul>
      {found.data?.more === true && <p className="hint">מוצגים הסיכומים האחרונים. מילה נוספת בחיפוש תצמצם את הרשימה.</p>}
      {discreet && <p className="hint">במצב דיסקרטי לא מחפשים בתרשומות האישיות.</p>}
    </Section>
  );
}

function Hit({ hit, discreet, onOpen }: { hit: NoteHit; discreet: boolean; onOpen: () => void }) {
  return (
    // The whole row opens the card for the mouse; the name is the button for the keyboard.
    <li className="notes-found-row" onClick={(e) => { if (!(e.target as HTMLElement).closest("button")) onOpen(); }}>
      <div className="row notes-found-head">
        <PatientName name={hit.patient_name} id={hit.patient_id} strong />
        <span className="num muted" dir="ltr">{formatDate(hit.date)}</span>
        <span className="chip chip-sand">{hit.visit_kind ? VISIT_KIND_LABEL[hit.visit_kind] : "שיחה"}</span>
        {hit.in_personal && <span className="chip">בתרשומת האישית</span>}
      </div>
      {discreet
        ? <p className="muted small clinic-quiet">הרשומה מוסתרת במצב דיסקרטי.</p>
        : hit.snippet !== "" && <p className="clinic-quiet notes-found-text">{hit.snippet}</p>}
    </li>
  );
}
