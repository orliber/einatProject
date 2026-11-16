// The waiting list: families that asked for an assessment and have nothing scheduled yet. The
// urgent ones first, then whoever waits longest. Scheduling a first meeting brings a family in
// (the core makes the card active), and the list updates by itself.
import { useMemo, useState } from "react";
import { useApp } from "../../App";
import { ageOf } from "../../components/AgeField";
import { PlusIcon, Skeleton } from "../../components/ui";
import { clinicApi } from "../../ipc/clinic";
import type { Patient } from "../../ipc/generated/Patient";
import type { PatientRow } from "../../ipc/generated/PatientRow";
import { clinicKeys, useClinicSettings, useOptimistic, usePatients, useReloadClinic, useSavePatient, useToday } from "./clinicData";
import { CONTACT_ROLE_LABEL, daysBetween, formatDate, isIsoDate } from "./format";
import { ConfirmDialog, EmptyState, Pager, PatientName, ReadError, useDiscreet, usePaged, useShownName } from "./parts";
import { PatientDialog } from "./PatientDialog";
import { VisitDialog } from "./VisitDialog";
import "./WaitingTab.css";

type Open =
  | { kind: "add" }
  | { kind: "edit"; patient: Patient }
  | { kind: "schedule"; patient: Patient }
  | { kind: "remove"; patient: Patient }
  | null;

/** The day a family joined the list; a card without one waits since the day it was opened. */
export function waitingSince(p: Patient): string {
  return p.waiting_since !== null && isIsoDate(p.waiting_since) ? p.waiting_since : p.created_on;
}

/** "מהיום", "יום אחד", "5 ימים", "שבוע", "יותר משבוע", "שבועיים", "8 שבועות", "יותר מ-8 שבועות":
 *  how long, as she would say it. Whole weeks are never rounded up. */
export function waitedWords(days: number): string {
  if (days <= 0) return "מהיום";
  if (days === 1) return "יום אחד";
  if (days === 2) return "יומיים";
  if (days < 7) return `${days} ימים`;
  const weeks = Math.floor(days / 7);
  const exact = days % 7 === 0;
  if (weeks === 1) return exact ? "שבוע" : "יותר משבוע";
  if (weeks === 2) return exact ? "שבועיים" : "יותר משבועיים";
  return exact ? `${weeks} שבועות` : `יותר מ-${weeks} שבועות`;
}

/** The urgent first; then whoever waits longest; the name keeps the order the same on every read. */
export function waitingOrder(a: PatientRow, b: PatientRow): number {
  return Number(b.patient.urgent) - Number(a.patient.urgent)
    || waitingSince(a.patient).localeCompare(waitingSince(b.patient))
    || a.patient.name.localeCompare(b.patient.name, "he")
    || a.patient.id.localeCompare(b.patient.id);
}

export function WaitingTab() {
  const now = useToday();
  const rows = usePatients(now.date);
  const settings = useClinicSettings();
  const { notify } = useApp();
  const reload = useReloadClinic();
  const shown = useShownName();
  const discreet = useDiscreet();
  const save = useSavePatient();
  const [open, setOpen] = useState<Open>(null);

  const waiting = useMemo(() => (rows.data ?? []).filter((r) => r.patient.status === "waiting").sort(waitingOrder), [rows.data]);
  const paged = usePaged(waiting, 20);

  // "דחוף" shows at once (and the family moves up); the vault's refusal puts it back.
  const mark = useOptimistic<PatientRow[], { patient: Patient; urgent: boolean }>(
    clinicKeys.patients(now.date),
    (list, a) => list.map((r) => (r.patient.id === a.patient.id ? { ...r, patient: { ...r.patient, urgent: a.urgent } } : r)),
    (a) => clinicApi.savePatient({ ...a.patient, urgent: a.urgent }),
  );

  if (rows.error && !rows.data) return <ReadError error={rows.error.message} retry={reload} />;
  if (!rows.data) {
    return (
      <div className="stack waiting" aria-busy="true">
        <Skeleton lines={1} label="טוען את רשימת ההמתנה" />
        <Skeleton card lines={4} label="טוען את המשפחות שממתינות" />
      </div>
    );
  }

  /** From how many weeks a wait is a long one (the clinic's setting; the core's alert uses it too). */
  const longFrom = settings.data ? settings.data.waiting_weeks * 7 : null;
  const urgent = waiting.filter((r) => r.patient.urgent).length;

  return (
    <div className="stack waiting">
      <header className="waiting-head">
        <div className="stack waiting-title">
          <h2>רשימת ההמתנה</h2>
          <span className="muted" role="status">
            {waiting.length === 0 ? "אין כרגע משפחות שממתינות"
              : `${waiting.length === 1 ? "משפחה אחת ממתינה" : `${waiting.length} משפחות ממתינות`}${urgent > 0 ? `, ${urgent === 1 ? "אחת מהן דחופה" : `${urgent} מהן דחופות`}` : ""}`}
          </span>
        </div>
        <span className="grow" />
        <button type="button" className="btn btn-primary" onClick={() => setOpen({ kind: "add" })}><PlusIcon /> להוסיף לרשימת ההמתנה</button>
      </header>

      {waiting.length === 0 ? (
        <EmptyState title="אף משפחה לא מחכה כרגע"
          actions={<button type="button" className="btn" onClick={() => setOpen({ kind: "add" })}>להוסיף משפחה לרשימה</button>}>
          <p>כשמשפחה פונה ועוד אין לה מועד, אפשר להוסיף אותה לכאן. הרשימה תזכור כמה זמן היא מחכה, ותזכיר כשההמתנה מתארכת.</p>
        </EmptyState>
      ) : (
        <ul className="clinic-list waiting-list" aria-label="המשפחות שממתינות, מי שמחכה הכי הרבה זמן קודם">
          {paged.shown.map((row) => {
            const p = row.patient;
            const since = waitingSince(p);
            const days = Math.max(0, daysBetween(since, now.date));
            const long = longFrom !== null && longFrom > 0 && days >= longFrom;
            const contact = p.contacts.find((c) => c.name !== "" || c.phone !== "");
            const who = shown(p.name);
            return (
              <li key={p.id} className={p.urgent ? "card waiting-family waiting-urgent" : "card waiting-family"}>
                <div className="grow stack waiting-body">
                  <span className="row waiting-name">
                    <PatientName name={p.name} id={p.id} strong />
                    {p.urgent && <span className="chip chip-warn">דחוף</span>}
                    {row.age && <span className="muted">{ageOf(row.age, p.gender)}</span>}
                  </span>
                  <span className={long ? "waiting-time waiting-long" : "waiting-time"}>
                    {days === 0 ? "ממתינים מהיום" : `ממתינים ${waitedWords(days)}, מאז ${formatDate(since)}`}
                    {long && <span className="chip chip-warn">המתנה ארוכה</span>}
                  </span>
                  {p.reason && !discreet && <span className="waiting-reason">{p.reason}</span>}
                  {(p.referred_by || contact) && (
                    <span className="muted small waiting-more">
                      {[
                        p.referred_by && !discreet ? `הפנה: ${p.referred_by}` : "",
                        contact ? `${CONTACT_ROLE_LABEL[contact.role]}${contact.name ? `: ${shown(contact.name)}` : ""}` : "",
                      ].filter(Boolean).join(" · ")}
                    </span>
                  )}
                </div>
                <div className="waiting-actions" role="group" aria-label={`פעולות עבור ${who}`}>
                  <button type="button" className="btn btn-small btn-primary" onClick={() => setOpen({ kind: "schedule", patient: p })}>לקבוע פגישת היכרות</button>
                  <button type="button" className="btn btn-small" aria-pressed={p.urgent} onClick={() => void mark({ patient: p, urgent: !p.urgent })}>
                    {p.urgent ? "להסיר את הסימון דחוף" : "לסמן כדחוף"}
                  </button>
                  <button type="button" className="btn btn-small" onClick={() => setOpen({ kind: "edit", patient: p })}>עריכת הפרטים</button>
                  <button type="button" className="btn btn-small" onClick={() => setOpen({ kind: "remove", patient: p })}>להסיר מהרשימה</button>
                </div>
              </li>
            );
          })}
        </ul>
      )}
      <Pager page={paged.page} pages={paged.pages} onPage={paged.setPage} label="עמודי רשימת ההמתנה" />

      {open?.kind === "add" && <PatientDialog waiting onClose={() => setOpen(null)} />}
      {open?.kind === "edit" && <PatientDialog patient={open.patient} onClose={() => setOpen(null)} />}
      {open?.kind === "schedule" && <VisitDialog start={{ patientId: open.patient.id, kind: "intake" }} onClose={() => setOpen(null)} />}
      {open?.kind === "remove" && (
        <ConfirmDialog title="להסיר מרשימת ההמתנה" action="להסיר מהרשימה" onClose={() => setOpen(null)}
          onConfirm={async () => {
            await save.mutateAsync({ ...open.patient, status: "closed", urgent: false });
            notify("המשפחה הוסרה מרשימת ההמתנה. הכרטיס נשמר בין הכרטיסים שהסתיימו.");
            setOpen(null);
          }}>
          <p>
            המשפחה של <PatientName name={open.patient.name} strong /> תרד מרשימת ההמתנה, והכרטיס ייסגר.
          </p>
          <p className="muted">שום דבר לא נמחק: הכרטיס נשאר ברשימת המטופלים תחת "הסתיים", ואפשר לפתוח אותו מחדש.</p>
        </ConfirmDialog>
      )}
    </div>
  );
}
