// The patient list: search, filter by status, a row for each family.
// Props: none. A row opens the card through `useClinicNav().openPatient(id)` (the route).
// Data: `usePatients(useToday().date)` gives `PatientRow[]` (age, next visit, last held visit,
//   what is owed, open tasks). Search with `useDebounced`; long lists with `usePaged` + `Pager`.
// Writes: `PatientDialog` for a new family.
// Parts: `PatientName` (every name, so discreet mode holds), `Money` (tone "owed" when
//   `due > 0`), `EmptyState`, `Skeleton`; labels from format.ts (`PATIENT_STATUS_LABEL`,
//   `VISIT_KIND_LABEL`, `formatDateShort`, `formatTime`).
import { EmptyState } from "./parts";

export function PatientsTab() {
  return (
    <EmptyState title="רשימת המטופלים בבנייה">
      <p>כאן תופיע רשימת המטופלים, עם חיפוש לפי שם. בינתיים אפשר להוסיף מטופל חדש מהלשונית "היום".</p>
    </EmptyState>
  );
}
