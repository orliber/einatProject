// The waiting list: families that asked for an assessment and have nothing scheduled yet.
// Props: none.
// Data: `usePatients(useToday().date)`, the rows with `patient.status === "waiting"`: the
//   urgent first, then by `waiting_since` (oldest first); weeks waiting with `daysBetween`.
// Writes: `PatientDialog` with `waiting` (a new family onto the list) or `patient` (edit);
//   `VisitDialog` with `start={{ patientId, kind: "intake" }}` (scheduling brings the family
//   in: the core makes them active); `useSavePatient` for "דחוף" (optimistic, `useOptimistic`
//   over `clinicKeys.patients`).
// Parts: `PatientName`, `EmptyState`, `Pager` + `usePaged`, `Skeleton`.
import { EmptyState } from "./parts";

export function WaitingTab() {
  return (
    <EmptyState title="רשימת ההמתנה בבנייה">
      <p>כאן יופיעו המשפחות שממתינות לאבחון, לפי הזמן שהן מחכות.</p>
    </EmptyState>
  );
}
