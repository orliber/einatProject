// What a visit on a list offers (review item 3 of 18.11.2026, D-204): one step, the one that
// fits the time, and everything else under "⋯". A visit that has begun and is not marked
// offers "התקיימה" (the main button once it is over); one that was held without a note,
// "לכתוב סיכום"; one still ahead, "תזכורת". The same piece on the clinic's front page, in the
// calendar's day list and on the patient's card, so a visit offers the same steps everywhere.
import { useEffect, useRef } from "react";
import { ActionsMenu, type MenuItem } from "../../components/Menu";
import { dialogOpen } from "../../components/ui";
import type { Today } from "../../ipc/generated/Today";
import type { Visit } from "../../ipc/generated/Visit";
import type { VisitView } from "../../ipc/generated/VisitView";
import { formatDate, formatTime, visitEnded, visitStep } from "./format";
import { useShownName } from "./parts";
import { visitCharge } from "./PaymentDialog";
import { SendMenu } from "./SendMenu";

/** The focus is nowhere: on the page itself, or on an element that has left it. */
export function focusLost(): boolean {
  const at = document.activeElement;
  return at === null || at === document.body || !at.isConnected;
}

/**
 * The step a visit offers now, and "⋯" with the rest, always ending with "לפתוח את הפגישה".
 * Only for a visit with a patient (not time she kept for herself): `patientId` is its patient.
 * On the card (`onCard`) the "⋯" is named by the visit's date, there is no reminder
 * (`reminders={false}`), and "לא הגיעו" is offered (`onNoShow`).
 */
export function VisitActions({ view, patientId, now, onHold, onNote, onOpen, onNoShow, onPay, reminders = true, onCard = false, follow = false }: {
  view: VisitView;
  patientId: string;
  now: Today;
  onHold: (v: Visit) => void;
  /** With `record`, from "להקליט לעצמך": the note opens and a recording starts (D-208). */
  onNote: (v: Visit, record?: boolean) => void;
  onOpen: () => void;
  /** The lists (not the card, which has "תקבול חדש"): "לרשום תקבול" in "⋯" of a visit that charges. */
  onPay?: (v: Visit) => void;
  /** The card only: "לא הגיעו" in "⋯". */
  onNoShow?: (v: Visit) => void;
  /** False on the card, which has no reminder. */
  reminders?: boolean;
  /** The "⋯" is named by date, not by patient. */
  onCard?: boolean;
  /** The card only: this visit was just marked there and its row moved to the other list,
   *  where it is a new row. It takes the focus that fell to the page. */
  follow?: boolean;
}) {
  // Hooks first, whatever the visit offers.
  const shown = useShownName();
  const v = view.visit;
  const raw = visitStep(v, view.has_note, now);
  const step = raw === "remind" && !reminders ? null : raw;
  const hasStep = step === "hold" || step === "note";

  // The step's button leaves the row once its step is done (a note saved from "לכתוב סיכום", or
  // "התקיימה" for a visit whose note was written ahead). The focus would fall to the page, so it
  // goes to this row's "⋯" (WCAG 2.4.3). `owns`: her focus is on that button, or in a dialog it
  // opened; such a dialog cannot give the focus back to it, so this waits for it to close.
  const stepRef = useRef<HTMLButtonElement>(null);
  const moreRef = useRef<HTMLButtonElement | null>(null);
  const owns = useRef(false);
  useEffect(() => {
    const moved = (e: FocusEvent) => {
      const at = e.target;
      owns.current = at === stepRef.current || (owns.current && at instanceof Element && at.closest('[role="dialog"]') !== null);
    };
    document.addEventListener("focusin", moved);
    return () => document.removeEventListener("focusin", moved);
  }, []);
  useEffect(() => {
    if (!owns.current || hasStep || dialogOpen()) return;
    owns.current = false;
    const at = document.activeElement;
    if (at === null || at === document.body) moreRef.current?.focus();
  });
  // A row that moved to the other list after a mark (`follow`) is built anew there, and the
  // focus on the button she pressed fell to the page (WCAG 2.4.3): it comes to this row's step,
  // which is now "לכתוב סיכום", or to its "⋯" when there is none. Only if it is nowhere else.
  useEffect(() => {
    if (follow && focusLost()) (stepRef.current ?? moreRef.current)?.focus();
  }, [follow]);

  /** Its day has come and it is still not marked. */
  const due = v.status === "planned" && v.date <= now.date;
  // No reminder in "⋯": while the visit is ahead, "תזכורת" is its step, and once it has begun a
  // reminder no longer makes sense (D-204 §4).
  const items: MenuItem[] = [
    ...(due && step !== "hold" ? [{ label: "התקיימה", run: () => onHold(v) }] : []),
    ...(due && onNoShow ? [{ label: "לא הגיעו", run: () => onNoShow(v) }] : []),
    ...(v.status !== "cancelled" && step !== "note" ? [{ label: view.has_note ? "לפתוח את הסיכום" : "לכתוב סיכום", run: () => onNote(v) }] : []),
    // Right after a session, a few words aloud for herself (D-208): once it has begun.
    ...(v.status !== "cancelled" && v.status !== "no_show" && (v.date < now.date || (v.date === now.date && v.start <= now.minute)) ? [{ label: "להקליט לעצמך", run: () => onNote(v, true) }] : []),
    // The family pays at the end of the session: recorded from the same row, for a visit that
    // charges (held with a fee, or missed and charged anyway).
    ...(onPay && visitCharge(v) > 0 ? [{ label: "לרשום תקבול", run: () => onPay(v) }] : []),
    // Always last, so the menu is never empty.
    { label: "לפתוח את הפגישה", run: onOpen },
  ];
  // The name as the screen may show it now (initials in discreet mode); never in a title.
  const who = onCard ? `מיום ${formatDate(v.date)}` : `של ${view.patient_name ? shown(view.patient_name) : "ללא שם"}`;
  // "התקיימה" and "לכתוב סיכום" are one button in one place, without a key: after the hold
  // React keeps the element, so the focus stays on it while it turns into the next step.
  const primary = step === "remind"
    ? <SendMenu kind="reminder" patientId={patientId} visitId={v.id} label="תזכורת" />
    : hasStep
      ? (
        <button ref={stepRef} type="button" className={step === "hold" && visitEnded(v, now) ? "btn btn-small btn-primary" : "btn btn-small"}
          onClick={() => (step === "hold" ? onHold(v) : onNote(v))}>
          {step === "hold" ? "התקיימה" : "לכתוב סיכום"}
        </button>
      )
      : null;
  return (
    <>
      {primary}
      <ActionsMenu items={items} label={`עוד פעולות לפגישה ${who}, ${formatTime(v.start)}`} buttonRef={moreRef} />
    </>
  );
}
