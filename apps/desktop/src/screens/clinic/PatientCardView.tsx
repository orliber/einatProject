// A patient's card: the details, the way of the assessment, visits, notes, money, tasks and
// the link to the diagnostic case.
// Props: `patientId` (from the route) and `onBack` (back to the patient list).
// Data: `usePatientCard(patientId, useToday().date)` gives `PatientCard` (patient, age, visits
//   newest first, notes, payments, charges, tasks, balance, process, case, alerts).
// Writes: `PatientDialog` (edit), `VisitDialog` (`start={{ patientId }}` or `view`),
//   `NoteDialog` (`visit`, `noteId`, or neither for a note on its own), `useSavePayment`,
//   `useSaveCharge`, the two voids, `useSaveTask`, `useCreateCase`, `useLinkCase` with
//   `useCaseChoices`, `useErasePatient`.
// Erasing (D-174): only a card where nothing happened, or after the retention period. The
//   core refuses otherwise with a sentence to show; offer "לסגור את הכרטיס" (`useSavePatient`
//   with `status: "closed"`) beside it. A note that has text is never deleted.
// Parts: `PatientName`, `Money`, `VisitRow`, `AlertList`, `StatusBadge`, `KindBadge`,
//   `RecordHistory`, `ConfirmDialog`, `Section`, `EmptyState`, `Pager` + `usePaged`.
// A `case` that is `null` (none, or one that was purged) offers to open or link one.
import { EmptyState } from "./parts";

export function PatientCardView({ onBack }: { patientId: string; onBack: () => void }) {
  return (
    <EmptyState title="כרטיס המטופל בבנייה"
      actions={<button type="button" className="btn" onClick={onBack}>חזרה לרשימת המטופלים</button>}>
      <p>כאן יופיעו הפרטים, מסלול האבחון, הפגישות, הסיכומים והתשלומים של המטופל.</p>
    </EmptyState>
  );
}
