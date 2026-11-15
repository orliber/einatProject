// Follow-ups and tasks: what is open (overdue first), what was done, and what the clinic
// noticed by itself.
// Props: none.
// Data: `useTasks(useToday().date)` gives `TaskView[]` in order (open, then done);
//   `useOverview(date).data.alerts` for "מה דורש אותי".
// Writes: `useSaveTask` (new, edit, done: `done_on` = today; reopening: `done_on` = null),
//   `useDeleteTask` behind `ConfirmDialog`; ticking a task is optimistic, as in TodayTab
//   (`useOptimistic` over `clinicKeys.tasks`).
// Parts: `AlertList`, `PatientPicker` (a task may belong to a patient), `PatientName`,
//   `TextField`, `SelectField` (`TASK_KINDS`, `TASK_KIND_LABEL`), `EmptyState`, `Pager`.
import { EmptyState } from "./parts";

export function TasksTab() {
  return (
    <EmptyState title="המעקבים בבנייה">
      <p>כאן יופיעו כל המשימות והמעקבים. בינתיים המשימות הפתוחות מופיעות בלשונית "היום".</p>
    </EmptyState>
  );
}
