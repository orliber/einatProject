// Free times and holidays beside a date (D-196, D-197): the line under a visit's date that says
// when a day is a holiday, and, on request, the first free times of her calendar for the
// visit's length. A click on one puts its date and hour in the form. Everything is worked out
// by the core from her own calendar; nothing leaves the computer.
import { useEffect, useRef, useState } from "react";
import type { Slot } from "../../ipc/generated/Slot";
import { useFreeSlots, useHolidays } from "./clinicData";
import { dayName, formatDate, formatTime, isIsoDate } from "./format";
import "./FreeTimes.css";

/** The holiday of a date, said in one quiet line; a day the clinic is closed says so. */
export function HolidayLine({ date, enabled }: { date: string; enabled: boolean }) {
  const ok = enabled && isIsoDate(date);
  const read = useHolidays(date, date, ok);
  const days = ok ? (read.data ?? []) : [];
  if (days.length === 0) return null;
  const names = days.map((h) => h.name).join(", ");
  const closed = days.some((h) => h.closed);
  return (
    <p className={closed ? "note-warn clinic-holiday-line" : "hint clinic-holiday-line"} role="status">
      <span>{closed ? `ביום הזה חל ${names}, וזה יום חג. הפגישה תישמר אם תבחרי, אבל כדאי לבדוק.` : `ביום הזה: ${names}.`}</span>
    </p>
  );
}

/** "להציע שעות פנויות": a quiet button that opens the first free times from `from`. */
export function FreeTimes({ from, minutes, skip, today, onPick }: {
  from: string; minutes: number; skip: string | null; today: string; onPick: (slot: Slot) => void;
}) {
  const [open, setOpen] = useState(false);
  const usable = isIsoDate(from) && Number.isInteger(minutes) && minutes >= 5 && minutes <= 720;
  const read = useFreeSlots(open && usable ? from : "", minutes, skip, today);
  // The button she pressed goes away (opening, hiding, or a pick that changes the list):
  // focus moves to what took its place, not back to the top of the dialog.
  const group = useRef<HTMLDivElement>(null);
  const opener = useRef<HTMLButtonElement>(null);
  const focusNext = useRef<"group" | "opener" | null>(null);
  useEffect(() => {
    if (focusNext.current === "group") group.current?.focus();
    else if (focusNext.current === "opener") opener.current?.focus();
    focusNext.current = null;
  });
  if (!open) {
    return (
      <button ref={opener} type="button" className="link-btn clinic-free-open" disabled={!usable}
        onClick={() => { focusNext.current = "group"; setOpen(true); }}>
        להציע שעות פנויות
      </button>
    );
  }
  const slots = read.data ?? [];
  return (
    <div ref={group} tabIndex={-1} className="clinic-free" role="group" aria-label="שעות פנויות ביומן">
      <div className="row clinic-free-head">
        <span className="small muted">שעות פנויות של {minutes} דקות, מ-{formatDate(from)}:</span>
        <span className="grow" />
        <button type="button" className="link-btn small" onClick={() => { focusNext.current = "opener"; setOpen(false); }}>להסתיר</button>
      </div>
      {read.isLoading && <span className="small muted" role="status">מחפשת שעות פנויות…</span>}
      {read.error && <span className="small clinic-free-error" role="alert">{read.error.message}</span>}
      {read.data && slots.length === 0 && (
        <span className="small muted" role="status">לא נמצאה שעה פנויה בארבעת החודשים הקרובים. אפשר לקצר את הפגישה או לבחור שעה ביד.</span>
      )}
      {slots.length > 0 && (
        <ul className="clinic-free-list">
          {slots.map((s) => (
            <li key={`${s.date}-${s.start}`}>
              <button type="button" className="chip clinic-free-slot" onClick={() => { if (s.date !== from) focusNext.current = "group"; onPick(s); }}
                aria-label={`יום ${dayName(s.date)}, ${formatDate(s.date)}, בשעה ${formatTime(s.start)}`}>
                <span>{dayName(s.date)} {formatDate(s.date).slice(0, 5)}</span>
                <span className="num" dir="ltr">{formatTime(s.start)}</span>
              </button>
            </li>
          ))}
        </ul>
      )}
    </div>
  );
}
