// "תקבול חדש": recording money that was received. Opened from the patient's card, and from a
// visit's "⋯" on the front page and in the calendar's day list ("לרשום תקבול"): the family pays
// at the end of the session, and she records it in the same place she marked it "התקיימה",
// without opening the card. A payment is never deleted, only voided on the card (D-172).
import { useState } from "react";
import { useApp } from "../../App";
import { DateField } from "../../components/DateField";
import { Dialog, ErrorLine } from "../../components/ui";
import type { PayMethod } from "../../ipc/generated/PayMethod";
import type { Visit } from "../../ipc/generated/Visit";
import { usePatientCard, useSavePayment } from "./clinicData";
import { formatMoney, parseMoney, PAY_METHOD_LABEL, PAY_METHODS } from "./format";
import { Field, MoneyField, moneyText, SelectField, TextAreaField, TextField } from "./parts";

const messageOf = (e: unknown, fallback: string) => (e as { message?: string }).message ?? fallback;

/** What a visit charges (the core's rule): its fee when held, or when missed and charged anyway. */
export function visitCharge(v: Visit): number {
  if (v.status === "held") return v.fee;
  return (v.status === "cancelled" || v.status === "no_show") && v.charge_missed ? v.fee : 0;
}

/** "תקבול חדש": money that was received. The amount starts from what is still owed, or from
 *  `amount` (the fee of the visit it was opened from); then the box says what the family owes. */
export function PaymentDialog({ patientId, today, due, amount, onClose }: { patientId: string; today: string; due: number; amount?: number | undefined; onClose: () => void }) {
  const { notify } = useApp();
  const save = useSavePayment();
  const [first] = useState({ amount: moneyText(amount !== undefined && amount > 0 ? amount : due > 0 ? due : null), method: "transfer" as PayMethod, receipt: "", payer: "", date: today, note: "" });
  const [form, setForm] = useState(first);
  const [error, setError] = useState<string | null>(null);
  const set = (patch: Partial<typeof first>) => setForm((f) => ({ ...f, ...patch }));

  async function submit() {
    const agorot = parseMoney(form.amount);
    if (agorot === null || agorot === 0) return setError("צריך לכתוב סכום גדול מאפס, בשקלים. למשל 450.");
    if (form.date === "") return setError("צריך לכתוב את תאריך התקבול: יום.חודש.שנה.");
    setError(null);
    try {
      await save.mutateAsync({
        id: "", patient_id: patientId, date: form.date, amount: agorot, method: form.method, receipt: form.receipt, payer: form.payer, note: form.note,
        voided: false, void_reason: "",
      });
      notify("התקבול נרשם");
      onClose();
    } catch (e) {
      setError(messageOf(e, "התקבול לא נרשם. אפשר לנסות שוב."));
    }
  }
  return (
    <Dialog help="payment" narrow title="תקבול חדש" dirty={JSON.stringify(form) !== JSON.stringify(first)} onClose={onClose}
      footer={(close) => <>
        <span className="grow" />
        <button type="button" className="btn" onClick={close}>ביטול</button>
        <button type="button" className="btn btn-primary" disabled={save.isPending} onClick={() => void submit()}>רישום התקבול</button>
      </>}>
      <div className="stack clinic-form">
        <div className="clinic-form-row">
          <MoneyField id="payment-amount" label="סכום" value={form.amount} onChange={(text) => set({ amount: text })}
            {...(amount !== undefined && due > 0 && due !== amount ? { hint: `היתרה של המשפחה: ${formatMoney(due)}.` } : {})} />
          <SelectField<PayMethod> id="payment-method" label="אמצעי תשלום" value={form.method} onChange={(method) => set({ method })}
            options={PAY_METHODS.map((m) => ({ value: m, label: PAY_METHOD_LABEL[m] }))} />
          <Field id="payment-date" label="תאריך"><DateField id="payment-date" value={form.date} onChange={(date) => set({ date })} /></Field>
        </div>
        <div className="clinic-form-row">
          <TextField id="payment-receipt" label="מספר קבלה" value={form.receipt} maxLength={200} dir="ltr" className="grow fit-s"
            hint="המספר של הקבלה שהופקה בתוכנת החשבוניות. אפשר להשלים אחר כך." onChange={(receipt) => set({ receipt })} />
          <TextField id="payment-payer" label="מי שילם (לא חובה)" value={form.payer} maxLength={200} className="grow"
            hint="כשלא ההורים שילמו: קופה, מוסד." onChange={(payer) => set({ payer })} />
        </div>
        <TextAreaField id="payment-note" label="הערה (לא חובה)" value={form.note} rows={2} maxLength={4000} onChange={(note) => set({ note })} />
        <ErrorLine error={error} />
      </div>
    </Dialog>
  );
}

/** The payment for one visit, from its row: the amount starts from the visit's fee, and the
 *  box says what the family owes when that is a different sum. The balance is read from the
 *  card; if it cannot be read the dialog opens all the same, with the fee. */
export function VisitPaymentDialog({ patientId, visit, today, onClose }: { patientId: string; visit: Visit; today: string; onClose: () => void }) {
  const card = usePatientCard(patientId, today);
  if (!card.data && !card.error) return null;
  return <PaymentDialog patientId={patientId} today={today} due={card.data?.balance.due ?? 0} amount={visitCharge(visit)} onClose={onClose} />;
}
