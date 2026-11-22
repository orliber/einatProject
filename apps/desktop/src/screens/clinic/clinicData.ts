// The clinic's data for its screens: one hook for each read and one for each write. Reads are
// cached under ["clinic", …]; every write empties that cache, so each tab shows what the vault
// holds now. A failure goes through the app's `fail`, like on every other screen, so the
// error line can offer its next step and a locked vault returns to the lock screen.
import { useCallback, useEffect, useRef, useState } from "react";
import { useMutation, useQuery, useQueryClient, type QueryKey, type UseMutationResult, type UseQueryResult } from "@tanstack/react-query";
import { useApp } from "../../App";
import { asUiError, type UiError } from "../../ipc/client";
import { clinicApi } from "../../ipc/clinic";
import type { Charge } from "../../ipc/generated/Charge";
import type { ClinicSettings } from "../../ipc/generated/ClinicSettings";
import type { ClinicTask } from "../../ipc/generated/ClinicTask";
import type { DiaryErased } from "../../ipc/generated/DiaryErased";
import type { Patient } from "../../ipc/generated/Patient";
import type { Payment } from "../../ipc/generated/Payment";
import type { Repeat } from "../../ipc/generated/Repeat";
import type { Scope } from "../../ipc/generated/Scope";
import type { SessionNote } from "../../ipc/generated/SessionNote";
import type { Today } from "../../ipc/generated/Today";
import type { Visit } from "../../ipc/generated/Visit";
import { today } from "./format";

/** Every clinic query lives under this key; a write invalidates all of them at once. */
export const CLINIC_KEY = ["clinic"] as const;

export const clinicKeys = {
  overview: (date: string) => ["clinic", "overview", date] as const,
  calendar: (from: string, to: string) => ["clinic", "calendar", from, to] as const,
  patients: (date: string) => ["clinic", "patients", date] as const,
  card: (id: string, date: string) => ["clinic", "card", id, date] as const,
  tasks: (date: string) => ["clinic", "tasks", date] as const,
  diary: (month: string) => ["clinic", "diary", month] as const,
  income: (year: number) => ["clinic", "income", year] as const,
  yearErased: (year: number) => ["clinic", "year-erased", year] as const,
  history: (recordId: string) => ["clinic", "history", recordId] as const,
  settings: () => ["clinic", "settings"] as const,
  caseChoices: () => ["clinic", "case-choices"] as const,
  holidays: (from: string, to: string) => ["clinic", "holidays", from, to] as const,
  slots: (from: string, minutes: number, skip: string | null, date: string) => ["clinic", "slots", from, minutes, skip, date] as const,
  insights: (from: string, to: string, date: string) => ["clinic", "insights", from, to, date] as const,
};

/** Her local date and minute, kept current while a clinic screen is open: at midnight "today"
 *  moves on, and a visit that just ended becomes one to mark. */
export function useToday(): Today {
  const [now, setNow] = useState(() => today());
  useEffect(() => {
    const t = window.setInterval(() => {
      const next = today();
      setNow((prev) => (prev.date === next.date && prev.minute === next.minute ? prev : next));
    }, 20_000);
    return () => window.clearInterval(t);
  }, []);
  return now;
}

/** A read whose failure is remembered by the app before the screen shows it. */
function useRead<T>(key: QueryKey, read: () => Promise<T>, options: { enabled?: boolean; everyMinute?: boolean } = {}): UseQueryResult<T, UiError> {
  const { fail } = useApp();
  return useQuery<T, UiError>({
    queryKey: key,
    queryFn: async () => {
      try {
        return await read();
      } catch (e) {
        const err = asUiError(e);
        fail(err);
        throw err;
      }
    },
    enabled: options.enabled ?? true,
    // What depends on the minute (a visit that just ended) is read again as time passes.
    ...(options.everyMinute ? { refetchInterval: 60_000 } : {}),
  });
}

// The date is in the key (a new day is a new answer); the minute is read when the request goes.
export const useOverview = (date: string) => useRead(clinicKeys.overview(date), () => clinicApi.overview(today()), { everyMinute: true });
export const useCalendar = (from: string, to: string) => useRead(clinicKeys.calendar(from, to), () => clinicApi.calendar(from, to));
export const usePatients = (date: string) => useRead(clinicKeys.patients(date), () => clinicApi.patients(today()));
/** `id` = `null`: nothing is asked (a dialog that has no patient yet). */
export const usePatientCard = (id: string | null, date: string) =>
  useRead(clinicKeys.card(id ?? "", date), () => clinicApi.patientCard(id ?? "", today()), { enabled: id !== null });
export const useTasks = (date: string) => useRead(clinicKeys.tasks(date), () => clinicApi.tasks(today()));
export const useDiary = (month: string) => useRead(clinicKeys.diary(month), () => clinicApi.diary(month));
export const useIncome = (year: number) => useRead(clinicKeys.income(year), () => clinicApi.income(year));

/** What the cards that were erased counted for in a year (D-179): the anonymous lines of its
 *  twelve months, added up; `null` when the year has none. The yearly summary already holds
 *  these figures inside its own (it does not say which part of them they are), so this is
 *  read from the months' diaries, where each month names its line. */
export async function readYearErased(year: number): Promise<DiaryErased | null> {
  const months = await Promise.all(Array.from({ length: 12 }, (_, i) => clinicApi.diary(`${year}-${String(i + 1).padStart(2, "0")}`)));
  let sum: DiaryErased | null = null;
  for (const line of months.map((m) => m.erased)) {
    if (line === null) continue;
    sum = sum === null ? { ...line } : {
      label: sum.label, held: sum.held + line.held, cancelled: sum.cancelled + line.cancelled, no_show: sum.no_show + line.no_show,
      charged: sum.charged + line.charged, received: sum.received + line.received,
    };
  }
  return sum;
}
export const useYearErased = (year: number) => useRead(clinicKeys.yearErased(year), () => readYearErased(year));
export const useHistory = (recordId: string | null) =>
  useRead(clinicKeys.history(recordId ?? ""), () => clinicApi.history(recordId ?? ""), { enabled: recordId !== null });
export const useClinicSettings = () => useRead(clinicKeys.settings(), clinicApi.settings);
export const useCaseChoices = (enabled = true) => useRead(clinicKeys.caseChoices(), clinicApi.caseChoices, { enabled });
/** Israel's holidays of a stretch (D-196); nothing is asked while `enabled` is false. */
export const useHolidays = (from: string, to: string, enabled = true) =>
  useRead(clinicKeys.holidays(from, to), () => clinicApi.holidays(from, to), { enabled });
/** Free times of `minutes` from `from` (D-197); `from` = "" asks nothing. */
export const useFreeSlots = (from: string, minutes: number, skip: string | null, date: string, count = 6) =>
  useRead(clinicKeys.slots(from, minutes, skip, date), () => clinicApi.freeSlots(today(), from, minutes, count, skip), { enabled: from !== "" && minutes >= 5 && minutes <= 720 });
/** The assessment pathway proposed for a patient from `from` (D-198); nothing is saved. */
export const useProposedPathway = (patientId: string, from: string, date: string) =>
  useRead(["clinic", "pathway", patientId, from, date], () => clinicApi.proposePathway(patientId, today(), from), { enabled: from !== "" });
/** The picture of the clinic over the months `from` to `to` (D-200). */
export const useInsights = (from: string, to: string, date: string) =>
  useRead(clinicKeys.insights(from, to, date), () => clinicApi.insights(today(), from, to));

/** Reads the clinic's data again: what "לנסות שוב" does after a read that failed. */
export function useReloadClinic(): () => void {
  const qc = useQueryClient();
  return useCallback(() => void qc.invalidateQueries({ queryKey: CLINIC_KEY }), [qc]);
}

/** A write: on success every clinic query is read again; a failure is remembered by the app and
 *  thrown to the caller (`mutateAsync`), whose error line shows `message`. `alsoCases`: the
 *  write changed the cases too (a case opened, a material added). */
function useWrite<A, R>(write: (args: A) => Promise<R>, alsoCases = false): UseMutationResult<R, UiError, A> {
  const { fail } = useApp();
  const qc = useQueryClient();
  return useMutation<R, UiError, A>({
    mutationFn: async (args) => {
      try {
        return await write(args);
      } catch (e) {
        const err = asUiError(e);
        fail(err);
        throw err;
      }
    },
    onSuccess: async () => {
      await qc.invalidateQueries({ queryKey: CLINIC_KEY });
      if (alsoCases) await qc.invalidateQueries({ queryKey: ["cases"] });
    },
  });
}

export const useSaveVisit = () => useWrite((a: { visit: Visit; repeat?: Repeat | null; scope?: Scope }) => clinicApi.saveVisit(a.visit, a.repeat ?? null, a.scope ?? "one"));
export const useDeleteVisit = () => useWrite((a: { id: string; scope?: Scope }) => clinicApi.deleteVisit(a.id, a.scope ?? "one"));
export const useSavePatient = () => useWrite((patient: Patient) => clinicApi.savePatient(patient));
export const useErasePatient = () => useWrite((a: { id: string; name: string }) => clinicApi.erasePatient(a.id, a.name));
export const useSaveNote = () => useWrite((note: SessionNote) => clinicApi.saveNote(note));
export const useDeleteNote = () => useWrite((id: string) => clinicApi.deleteNote(id));
export const useNoteToCase = () => useWrite((id: string) => clinicApi.noteToCase(id), true);
export const useSaveRecording = () => useWrite((a: Parameters<typeof clinicApi.saveRecording>[0]) => clinicApi.saveRecording(a));
export const useDeleteRecording = () => useWrite((a: { noteId: string; id: string }) => clinicApi.deleteRecording(a.noteId, a.id));
export const useSavePayment = () => useWrite((payment: Payment) => clinicApi.savePayment(payment));
export const useVoidPayment = () => useWrite((a: { id: string; reason: string }) => clinicApi.voidPayment(a.id, a.reason));
export const useSaveCharge = () => useWrite((charge: Charge) => clinicApi.saveCharge(charge));
export const useVoidCharge = () => useWrite((a: { id: string; reason: string }) => clinicApi.voidCharge(a.id, a.reason));
export const useSaveTask = () => useWrite((task: ClinicTask) => clinicApi.saveTask(task));
export const useDeleteTask = () => useWrite((id: string) => clinicApi.deleteTask(id));
export const useSaveSettings = () => useWrite((settings: ClinicSettings) => clinicApi.saveSettings(settings));
export const useLinkCase = () => useWrite((a: { patientId: string; caseId: string | null }) => clinicApi.linkCase(a.patientId, a.caseId), true);
export const useCreateCase = () => useWrite((patientId: string) => clinicApi.createCase(patientId, today()), true);
export const useSavePathway = () =>
  useWrite((a: { patientId: string; visits: Visit[]; packaged: boolean }) => clinicApi.savePathway(a.patientId, a.visits, a.packaged, today()));
export const useStarterTasks = () => useWrite((patientId: string) => clinicApi.starterTasks(patientId, today()));

/**
 * A quick action that shows on screen before the vault answers (marking a visit, a task done).
 * `apply` changes the cached answer of every query under `prefix`; if the write fails, what was
 * cached comes back and the failure is said in a toast (there is no form to show it in).
 * Either way the clinic's queries are read again afterwards.
 */
export function useOptimistic<T, A>(prefix: QueryKey, apply: (cached: T, args: A) => T, write: (args: A) => Promise<unknown>): (args: A) => Promise<boolean> {
  const { fail, notify } = useApp();
  const qc = useQueryClient();
  // The latest functions, without making the returned action new on every render.
  const live = useRef({ apply, write });
  useEffect(() => {
    live.current = { apply, write };
  });
  const key = JSON.stringify(prefix);
  return useCallback(async (args: A) => {
    const queryKey = JSON.parse(key) as QueryKey;
    // An answer already on its way would overwrite the change a moment later.
    await qc.cancelQueries({ queryKey });
    const before = qc.getQueriesData<T>({ queryKey });
    qc.setQueriesData<T>({ queryKey }, (cached) => (cached === undefined ? cached : live.current.apply(cached, args)));
    try {
      await live.current.write(args);
      return true;
    } catch (e) {
      for (const [k, data] of before) qc.setQueryData(k, data);
      notify(fail(asUiError(e)));
      return false;
    } finally {
      await qc.invalidateQueries({ queryKey: CLINIC_KEY });
    }
  }, [qc, key, fail, notify]);
}

/** A value that follows what she types after a short pause (a search box). */
export function useDebounced<T>(value: T, ms = 250): T {
  const [settled, setSettled] = useState(value);
  useEffect(() => {
    const t = window.setTimeout(() => setSettled(value), ms);
    return () => window.clearTimeout(t);
  }, [value, ms]);
  return settled;
}
