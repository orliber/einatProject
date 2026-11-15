// Money: the income diary of a month, the year at a glance, and who owes what.
// Props: none. The month and the year shown are the tab's own state.
// Data: `useDiary(month)` (`DiaryMonth`: days, lines, totals, by method, missing receipts),
//   `useIncome(year)` (`IncomeYear`), `usePatients(date)` for the open balances.
// Writes: `useSavePayment`, `useVoidPayment`, `useSaveCharge`, `useVoidCharge` (a voided line
//   cannot be edited: offer only its history); the payment message through
//   `clinicApi.message("payment", patientId)` and `ipc.copySecret`, as the reminder in TodayTab.
// Parts: `Money`, `MoneyField` + `parseMoney`, `PatientPicker`, `PatientName`, `RecordHistory`
//   (every line has `record_id`), `ConfirmDialog` (a void asks for the reason), `Pager`.
// The tab must say that the program is not registered with the tax authority, and that
//   receipts are issued in the invoicing program (D-172). No export to a file (not in the beta).
import { EmptyState } from "./parts";

export function MoneyTab() {
  return (
    <EmptyState title="הכספים בבנייה">
      <p>כאן יופיעו יומן ההכנסות, התקבולים והיתרות. בינתיים הסכומים העיקריים מופיעים בלשונית "היום".</p>
    </EmptyState>
  );
}
