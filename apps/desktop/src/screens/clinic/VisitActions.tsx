// What a visit on a list offers (review item 3 of 18.11.2026, D-203): one step, the one that
// fits the time, and everything else under "⋯". A visit that has begun and is not marked
// offers "התקיימה" (the main button once it is over); one that was held without a note,
// "לכתוב סיכום"; one still ahead, "תזכורת". The same piece on the clinic's front page, in the
// calendar's day list and on the patient's card, so a visit offers the same steps everywhere.
import { ActionsMenu, type MenuItem } from "../../components/Menu";
import type { Today } from "../../ipc/generated/Today";
import type { Visit } from "../../ipc/generated/Visit";
import type { VisitView } from "../../ipc/generated/VisitView";
import { formatDate, formatTime, visitEnded, visitStep } from "./format";
import { useShownName } from "./parts";
import { SendMenu, useSend } from "./SendMenu";

/**
 * The step a visit offers now, and "⋯" with the rest, always ending with "לפתוח את הפגישה".
 * Only for a visit with a patient (not time she kept for herself): `patientId` is its patient.
 * On the card (`onCard`) the "⋯" is named by the visit's date, there is no reminder
 * (`reminders={false}`), and "לא הגיעו" is offered (`onNoShow`).
 */
export function VisitActions({ view, patientId, now, onHold, onNote, onOpen, onNoShow, reminders = true, onCard = false }: {
  view: VisitView;
  patientId: string;
  now: Today;
  onHold: (v: Visit) => void;
  onNote: (v: Visit) => void;
  onOpen: () => void;
  /** The card only: "לא הגיעו" in "⋯". */
  onNoShow?: (v: Visit) => void;
  /** False on the card, which has no reminder. */
  reminders?: boolean;
  /** The "⋯" is named by date, not by patient. */
  onCard?: boolean;
}) {
  // Hooks first, whatever the visit offers.
  const shown = useShownName();
  const { send, copy } = useSend("reminder", patientId, view.visit.id);
  const v = view.visit;
  const raw = visitStep(v, view.has_note, now);
  const step = raw === "remind" && !reminders ? null : raw;
  /** Its day has come and it is still not marked. */
  const due = v.status === "planned" && v.date <= now.date;
  const items: MenuItem[] = [
    ...(due && step !== "hold" ? [{ label: "התקיימה", run: () => onHold(v) }] : []),
    ...(due && onNoShow ? [{ label: "לא הגיעו", run: () => onNoShow(v) }] : []),
    ...(v.status !== "cancelled" && step !== "note" ? [{ label: view.has_note ? "לפתוח את הסיכום" : "לכתוב סיכום", run: () => onNote(v) }] : []),
    ...(reminders && v.status === "planned" && step !== "remind"
      ? [
        { label: "לשלוח תזכורת בוואטסאפ", run: () => void send("whatsapp") },
        { label: "לשלוח תזכורת ב-SMS", run: () => void send("sms") },
        { label: "לשלוח תזכורת במייל", run: () => void send("email") },
        { label: "להעתיק את התזכורת", run: () => void copy() },
      ]
      : []),
    // Always last, so the menu is never empty.
    { label: "לפתוח את הפגישה", run: onOpen },
  ];
  // The name as the screen may show it now (initials in discreet mode); never in a title.
  const who = onCard ? `מיום ${formatDate(v.date)}` : `של ${view.patient_name ? shown(view.patient_name) : "ללא שם"}`;
  // "התקיימה" and "לכתוב סיכום" are one button in one place, without a key: after the hold
  // React keeps the element, so the focus stays on it while it turns into the next step.
  const primary = step === "remind"
    ? <SendMenu kind="reminder" patientId={patientId} visitId={v.id} label="תזכורת" />
    : step === "hold" || step === "note"
      ? (
        <button type="button" className={step === "hold" && visitEnded(v, now) ? "btn btn-small btn-primary" : "btn btn-small"}
          onClick={() => (step === "hold" ? onHold(v) : onNote(v))}>
          {step === "hold" ? "התקיימה" : "לכתוב סיכום"}
        </button>
      )
      : null;
  return (
    <>
      {primary}
      <ActionsMenu items={items} label={`עוד פעולות לפגישה ${who}, ${formatTime(v.start)}`} />
    </>
  );
}
