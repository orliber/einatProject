// The calendar: day, week and month (docs/CLINIC.md, "The window").
// Props: none. The day shown is the tab's own state; today comes from `useToday()`.
// Data: `useCalendar(from, to)` with `weekRange` / `monthRange` of format.ts (at most 100 days
//   in one answer); `useClinicSettings()` for `day_start`, `day_end` and `work_days`.
// Writes: `VisitDialog` (new: `start={{ date, start }}` from the hour she pressed; existing:
//   `view`); a quick "התקיימה" as in TodayTab (`useOptimistic` over `clinicKeys.calendar`).
// Parts: `VisitRow` (the day and agenda lists), `KindBadge`, `StatusBadge`, `PatientName`
//   (every name, so discreet mode holds), `EmptyState`, `Skeleton` while loading.
// A block ("זמן שמור") shows her own title; an overlapping visit carries `overlaps`.
import { EmptyState } from "./parts";

export function CalendarTab() {
  return (
    <EmptyState title="היומן בבנייה">
      <p>כאן יופיעו הפגישות לפי יום, שבוע וחודש. בינתיים אפשר לקבוע פגישה ולראות את הפגישות של היום בלשונית "היום".</p>
    </EmptyState>
  );
}
