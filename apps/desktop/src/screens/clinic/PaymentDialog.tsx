// "תקבול חדש": recording money that was received. Opened from the patient's card, and from a
// visit's "⋯" on the front page and in the calendar's day list ("לרשום תקבול"): the family pays
// at the end of the session, and she records it in the same place she marked it "התקיימה",
// without opening the card. A payment is never deleted, only voided on the card (D-172).
import { useState } from "react";
import { useApp } from "../../App";
import { DateField } from "../../components/DateField";
import { Dialog, ErrorLine } from "../../components/ui";
import type { Payment } from "../../ipc/generated/Payment";
import type { PayMethod } from "../../ipc/generated/PayMethod";
import type { Visit } from "../../ipc/generated/Visit";
import { usePatientCard, useSavePayment } from "./clinicData";
import { formatDate, formatMoney, parseMoney, PAY_METHOD_LABEL, PAY_METHODS, VISIT_KIND_LABEL } from "./format";
import { ConfirmDialog, Field, MoneyField, moneyText, SelectField, TextAreaField, TextField, useShownName } from "./parts";

const messageOf = (e: unknown, fallback: string) => (e as { message?: string }).message ?? fallback;

/** What a visit charges (the core's rule): its fee when held, or when missed and charged anyway. */
export function visitCharge(v: Visit): number {
  if (v.status === "held") return v.fee;
  return (v.status === "cancelled" || v.status === "no_show") && v.charge_missed ? v.fee : 0;
}

/** How this family paid last time (the newest payment that was not voided), or a bank transfer
 *  when nothing was paid yet: a family that pays in Bit pays in Bit next time too, and she is
 *  free to change it. */
export function lastMethod(payments: readonly Payment[]): PayMethod {
  const live = payments.filter((p) => !p.voided);
  const newest = live.reduce<Payment | undefined>((best, p) => (best === undefined || p.date >= best.date ? p : best), undefined);
  return newest === undefined || newest.method === "unknown" ? "transfer" : newest.method;
}

/** "תקבול חדש": money that was received. The amount starts from what is still owed, or from
 *  `amount` (the fee of the visit it was opened from); then the box says what the family owes. */
export function PaymentDialog({ patientId, today, due, amount, about, method, dueKnown = true, onClose }: {
  patientId: string; today: string; due: number; amount?: number | undefined;
  /** `due` is the family's balance as read (not a stand-in): an amount above it asks first. */
  dueKnown?: boolean;
  /** The method the box opens on: the family's last one. */
  method?: PayMethod | undefined;
  /** Away from the card: whose payment this is, said first in the dialog. */
  about?: string | undefined; onClose: () => void;
}) {
  const { notify } = useApp();
  const save = useSavePayment();
  const [first] = useState({ amount: moneyText(amount !== undefined && amount > 0 ? amount : due > 0 ? due : null), method: method ?? ("transfer" as PayMethod), receipt: "", payer: "", date: today, note: "" });
  const [form, setForm] = useState(first);
  const [error, setError] = useState<string | null>(null);
  /** An amount above what the family owes, waiting for her word (a zero too many is a typo). */
  const [over, setOver] = useState<number | null>(null);
  const set = (patch: Partial<typeof first>) => setForm((f) => ({ ...f, ...patch }));
  const owed = Math.max(due, 0);

  async function submit() {
    const agorot = parseMoney(form.amount);
    if (agorot === null || agorot === 0) return setError("צריך לכתוב סכום גדול מאפס, בשקלים. למשל 450.");
    if (form.date === "") return setError("צריך לכתוב את תאריך התקבול: יום.חודש.שנה.");
    setError(null);
    if (dueKnown && agorot > owed) {
      setOver(agorot);
      return;
    }
    await record(agorot);
  }

  async function record(agorot: number) {
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
  /** What the box says about the family's balance, when it is not the sum in the box. */
  const balanceHint = amount === undefined || due === amount ? undefined
    : due > 0 ? `היתרה של המשפחה: ${formatMoney(due)}.`
    : !dueKnown ? undefined
    : due === 0 ? "למשפחה אין יתרה פתוחה: הכול כבר שולם."
    : `למשפחה כבר יש זכות של ${formatMoney(-due)}.`;
  return (
    <Dialog help="payment" narrow title="תקבול חדש" dirty={JSON.stringify(form) !== JSON.stringify(first)} onClose={onClose}
      footer={(close) => <>
        <span className="grow" />
        <button type="button" className="btn" onClick={close}>ביטול</button>
        <button type="button" className="btn btn-primary" disabled={save.isPending} onClick={() => void submit()}>רישום התקבול</button>
      </>}>
      <div className="stack clinic-form">
        {about !== undefined && <p className="clinic-quiet">{about}</p>}
        <div className="clinic-form-row">
          <MoneyField id="payment-amount" label="סכום" value={form.amount} onChange={(text) => set({ amount: text })}
            {...(balanceHint !== undefined ? { hint: balanceHint } : {})} />
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
      {over !== null && (
        <ConfirmDialog title="הסכום גבוה מהיתרה" action={`לרשום ${formatMoney(over)}`} onClose={() => setOver(null)}
          onConfirm={async () => {
            setOver(null);
            await record(over);
          }}>
          <p>
            {due < 0 ? `למשפחה כבר יש זכות של ${formatMoney(-due)}.` : due === 0 ? "למשפחה אין יתרה פתוחה." : `היתרה של המשפחה היא ${formatMoney(due)}.`}
            {" "}אחרי הרישום תהיה למשפחה זכות של {formatMoney(over - due)}. לרשום את הסכום כמו שהוא?
          </p>
        </ConfirmDialog>
      )}
    </Dialog>
  );
}

/** The payment for one visit, from its row: the amount starts from the visit's fee, and the
 *  box says what the family owes when that is a different sum. The balance is read from the
 *  card; if it cannot be read the dialog opens all the same, with the fee. */
export function VisitPaymentDialog({ patientId, visit, today, onClose }: { patientId: string; visit: Visit; today: string; onClose: () => void }) {
  const card = usePatientCard(patientId, today);
  const shown = useShownName();
  if (!card.data && !card.error) return null;
  // The name as the screen may show it now (initials in discreet mode).
  const who = card.data ? shown(card.data.patient.name) : "";
  const about = `${who === "" ? "תקבול" : `תקבול מהמשפחה של ${who}`}, על ${VISIT_KIND_LABEL[visit.kind]} מיום ${formatDate(visit.date)}.`;
  return <PaymentDialog patientId={patientId} today={today} due={card.data?.balance.due ?? 0} dueKnown={card.data !== undefined} amount={visitCharge(visit)} about={about} method={lastMethod(card.data?.payments ?? [])} onClose={onClose} />;
}
