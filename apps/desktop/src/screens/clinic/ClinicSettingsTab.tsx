// The clinic's settings: calendar hours and working days, the usual length and fee of each
// kind of visit, places, the two message texts, note templates, and the three thresholds.
// Props: none.
// Data: `useClinicSettings()`; writes: `useSaveSettings` (the whole `ClinicSettings` at once;
//   the core tidies and checks it, and its refusal is a sentence to show in `ErrorLine`).
// Parts: `TextField`, `TextAreaField`, `MoneyField` + `parseMoney`, `TimeField` + `parseTime`,
//   `CheckField`, `Segmented` (components/Switch). Labels: `VISIT_KINDS`, `VISIT_KIND_LABEL`.
// The message texts take the placeholders {הורה} {ילד} {יום} {תאריך} {שעה} {מקום} {סכום} {שמי};
//   say beside them that the default leaves the child's name out on purpose (D-173).
import { EmptyState } from "./parts";

export function ClinicSettingsTab() {
  return (
    <EmptyState title="הגדרות הקליניקה בבנייה">
      <p>כאן יופיעו שעות היומן, המחירים, המקומות ונוסחי ההודעות. בינתיים הקליניקה עובדת עם ברירות המחדל.</p>
    </EmptyState>
  );
}
