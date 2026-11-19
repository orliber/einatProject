// The assessment pathway in one go (D-198): from the day she picks, each step of the pathway in
// her settings at the first free time at least its gap after the step before. She sees every
// visit before anything is saved, can move any of them, and saves them all at once; with a
// package price in the settings she can charge the whole assessment as one package, and then
// the visits themselves carry no fee. Nothing here leaves the computer.
import { useState } from "react";
import { useApp } from "../../App";
import { DateField } from "../../components/DateField";
import { Dialog, ErrorLine, Skeleton } from "../../components/ui";
import type { Visit } from "../../ipc/generated/Visit";
import { useClinicSettings, useProposedPathway, useSavePathway, useToday } from "./clinicData";
import { HolidayLine } from "./FreeTimes";
import { addDays, formatDateLong, formatTime, parseTime, VISIT_KIND_LABEL } from "./format";
import { CheckField, Field, Money, PatientName, TimeField } from "./parts";

interface Row {
  visit: Visit;
  start: string;
}

export function PathwayDialog({ patientId, patientName, onClose }: { patientId: string; patientName: string; onClose: () => void }) {
  const now = useToday();
  const { notify } = useApp();
  const settings = useClinicSettings();
  const save = useSavePathway();
  const [from, setFrom] = useState(() => addDays(now.date, 1));
  const [rows, setRows] = useState<Row[] | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [packaged, setPackaged] = useState(false);
  const [busy, setBusy] = useState(false);
  // A new start day proposes the whole pathway again; what she changed in the rows is hers
  // until then.
  const proposal = useProposedPathway(patientId, from, now.date);
  const [shownFor, setShownFor] = useState<Visit[] | null>(null);
  // Each proposal shown starts the rows' date boxes afresh, even when it came from the cache.
  const [rev, setRev] = useState(0);
  if (proposal.data && proposal.data !== shownFor) {
    setShownFor(proposal.data);
    setRows(proposal.data.map((visit) => ({ visit, start: formatTime(visit.start) })));
    setRev((n) => n + 1);
  }

  const fee = settings.data?.package_fee ?? 0;
  const set = (i: number, patch: Partial<Row> & { date?: string }) =>
    setRows((list) => list?.map((r, j) => (j === i ? { start: patch.start ?? r.start, visit: { ...r.visit, ...(patch.date === undefined ? {} : { date: patch.date }) } } : r)) ?? list);

  async function submit() {
    if (!rows || rows.length === 0) return;
    const visits: Visit[] = [];
    for (const [i, r] of rows.entries()) {
      const start = parseTime(r.start);
      if (r.visit.date === "") { setError(`צריך לבחור תאריך לפגישה ${i + 1}.`); return; }
      if (start === null) { setError(`צריך לכתוב שעה לפגישה ${i + 1}, למשל 9:30.`); return; }
      if (start + r.visit.minutes > 1440) { setError(`פגישה ${i + 1} צריכה להסתיים עד חצות.`); return; }
      visits.push({ ...r.visit, start });
    }
    setBusy(true);
    setError(null);
    try {
      const saved = await save.mutateAsync({ patientId, visits, packaged: packaged && fee > 0 });
      const clash = saved.overlapping;
      notify(`נקבעו ${saved.visits.length} פגישות של מסלול האבחון${clash > 0 ? `. ${clash === 1 ? "אחת מהן חופפת" : `${clash} מהן חופפות`} לפגישה אחרת ביומן` : ""}`);
      onClose();
    } catch (e) {
      setError((e as { message?: string }).message ?? "המסלול לא נשמר. אפשר לנסות שוב.");
    } finally {
      setBusy(false);
    }
  }

  // No start day (empty, or not a whole date yet): nothing is asked, and nothing can be saved.
  const noStart = from === "";
  const loading = !noStart && (proposal.isFetching || rows === null || proposal.data !== shownFor);
  const refused = !noStart && proposal.error && !proposal.isFetching ? proposal.error.message : null;
  const shown = !noStart && !loading && !refused && rows !== null && rows.length > 0;
  return (
    <Dialog help="pathway" wide title="לקבוע את מסלול האבחון" subtitle={<PatientName name={patientName} />} onClose={onClose} dirty={rows !== null && rows.length > 0}
      footer={(close) => <>
        <span className="grow" />
        <button type="button" className="btn" onClick={close}>ביטול</button>
        <button type="button" className="btn btn-primary" disabled={busy || !shown} onClick={() => void submit()}>
          {shown ? `לקבוע ${rows.length} פגישות` : "לקבוע"}
        </button>
      </>}>
      <div className="stack clinic-form">
        <p className="hint clinic-quiet">
          כל שלב נקבע בשעה הפנויה הראשונה, לפי השלבים שבהגדרות הקליניקה. אפשר לשנות כל תאריך ושעה לפני השמירה. שום דבר לא נשמר עד שלוחצים "לקבוע".
        </p>
        <Field id="pathway-from" label="מתחילים מיום">
          <DateField id="pathway-from" value={from} onChange={setFrom} />
        </Field>
        {noStart && <p className="hint" role="status">צריך לכתוב את היום שמתחילים ממנו: יום.חודש.שנה.</p>}
        {loading && !refused && <Skeleton lines={4} label="מחפשת שעות פנויות למסלול" />}
        {refused && <ErrorLine error={refused} />}
        {shown && (
          <ol className="clinic-list pathway-rows" aria-label="פגישות המסלול">
            {rows.map((r, i) => (
              <li key={`${rev}-${i}`}><div className="clinic-step" role="group" aria-label={`פגישה ${i + 1}: ${VISIT_KIND_LABEL[r.visit.kind]}`}>
                <span className="clinic-step-num num" aria-hidden="true">{i + 1}</span>
                <div className="stack pathway-what">
                  <strong>{VISIT_KIND_LABEL[r.visit.kind]}</strong>
                  <span className="small muted">{formatDateLong(r.visit.date)} · {r.visit.minutes} דקות</span>
                </div>
                <Field id={`pathway-${i}-date`} label="תאריך">
                  <DateField id={`pathway-${i}-date`} value={r.visit.date} onChange={(date) => set(i, { date })} />
                </Field>
                <TimeField id={`pathway-${i}-start`} label="שעה" value={r.start} onChange={(start) => set(i, { start })} />
                <div className="pathway-holiday"><HolidayLine date={r.visit.date} enabled={settings.data?.holidays === true} /></div>
              </div></li>
            ))}
          </ol>
        )}
        {fee > 0 && shown && (
          <CheckField id="pathway-package" label={`לחייב כחבילה: ${settings.data?.package_name ?? "חבילת אבחון"}`} checked={packaged} onChange={setPackaged}
            hint={<>נרשם חיוב אחד של <Money agorot={fee} />, ביום הפגישה הראשונה, והפגישות של המסלול עצמן בלי מחיר. בלי החבילה כל פגישה מחויבת לפי המחיר שלה.</>} />
        )}
        <ErrorLine error={error} />
      </div>
    </Dialog>
  );
}
