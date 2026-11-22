// The typed client of the clinic's one command (D-170). Every request names the kind of answer
// it expects; an answer of another kind is a fault of the program, shown as one, never cast.
//
// Which answer each request gets:
//   overview → overview · calendar → visits · save_visit → saved · delete_visit → done
//   patients → patients · patient_card → card · save_patient → patient · erase_patient → done
//   save_note → note · delete_note → done · note_to_case → note (with its `case_input_id`)
//   save_payment, void_payment → payment · save_charge, void_charge → charge
//   tasks → tasks · save_task → task · delete_task → done
//   diary → diary · income → income · history → history
//   settings, save_settings → settings · message → text
//   case_choices → cases · link_case → patient · create_case → case
//   holidays → holidays · free_slots → slots · propose_pathway → proposed · save_pathway → saved
//   starter_tasks → tasks · insights → insights
//   save_recording, delete_recording → note · recording → sound (D-208)
import { ipc, type UiError } from "./client";
import type { CaseChoice } from "./generated/CaseChoice";
import type { CaseLink } from "./generated/CaseLink";
import type { Charge } from "./generated/Charge";
import type { ClinicReply } from "./generated/ClinicReply";
import type { ClinicRequest } from "./generated/ClinicRequest";
import type { ClinicSettings } from "./generated/ClinicSettings";
import type { ClinicTask } from "./generated/ClinicTask";
import type { DiaryMonth } from "./generated/DiaryMonth";
import type { HistoryEntry } from "./generated/HistoryEntry";
import type { Holiday } from "./generated/Holiday";
import type { Insights } from "./generated/Insights";
import type { IncomeYear } from "./generated/IncomeYear";
import type { MessageKind } from "./generated/MessageKind";
import type { Overview } from "./generated/Overview";
import type { Patient } from "./generated/Patient";
import type { PatientCard } from "./generated/PatientCard";
import type { PatientRow } from "./generated/PatientRow";
import type { Payment } from "./generated/Payment";
import type { RecordingSound } from "./generated/RecordingSound";
import type { Repeat } from "./generated/Repeat";
import type { SavedVisits } from "./generated/SavedVisits";
import type { Scope } from "./generated/Scope";
import type { SessionNote } from "./generated/SessionNote";
import type { Slot } from "./generated/Slot";
import type { TaskView } from "./generated/TaskView";
import type { Today } from "./generated/Today";
import type { Visit } from "./generated/Visit";
import type { VisitView } from "./generated/VisitView";

export type ReplyKind = ClinicReply["kind"];
/** What an answer of kind `K` carries; `null` for the one that carries nothing (`done`). */
export type ReplyData<K extends ReplyKind> = Extract<ClinicReply, { kind: K }> extends { data: infer D } ? D : null;

/** An answer of the wrong kind: only Or can do something about it, and no clinic text goes
 *  into the report (the two kinds are fixed words of the program). */
function wrongKind(op: ClinicRequest["op"], wanted: ReplyKind, got: string): UiError {
  return {
    code: "internal",
    message: "משהו השתבש בקליניקה. אפשר לשלוח לאור דוח תקלה.",
    details: [`clinic ${op}: expected ${wanted}, got ${got}`],
    action: { kind: "report" },
  };
}

/** The data of a reply, once it is known to be of the kind asked for. */
export function dataOf<K extends ReplyKind>(op: ClinicRequest["op"], reply: ClinicReply, kind: K): ReplyData<K> {
  const got: unknown = (reply as { kind?: unknown } | null)?.kind;
  if (got !== kind) throw wrongKind(op, kind, typeof got === "string" ? got : "nothing");
  // Checked just above: the reply is the member of the union that `kind` names.
  return ("data" in reply ? reply.data : null) as ReplyData<K>;
}

/** Sends one request and gives back the data of its answer. Throws a `UiError`. */
export async function ask<K extends ReplyKind>(request: ClinicRequest, kind: K): Promise<ReplyData<K>> {
  return dataOf(request.op, await ipc.clinic(request), kind);
}

const done = (request: ClinicRequest): Promise<void> => ask(request, "done").then(() => undefined);

export const clinicApi = {
  overview: (today: Today): Promise<Overview> => ask({ op: "overview", today }, "overview"),
  /** Visits from `from` to `to` (ISO dates), both included. */
  calendar: (from: string, to: string): Promise<VisitView[]> => ask({ op: "calendar", from, to }, "visits"),
  /** `repeat` only with a new visit; `scope` matters only for a visit of a series. */
  saveVisit: (visit: Visit, repeat: Repeat | null = null, scope: Scope = "one"): Promise<SavedVisits> =>
    ask({ op: "save_visit", visit, repeat, scope }, "saved"),
  deleteVisit: (id: string, scope: Scope = "one"): Promise<void> => done({ op: "delete_visit", id, scope }),
  patients: (today: Today): Promise<PatientRow[]> => ask({ op: "patients", today }, "patients"),
  patientCard: (id: string, today: Today): Promise<PatientCard> => ask({ op: "patient_card", id, today }, "card"),
  savePatient: (patient: Patient): Promise<Patient> => ask({ op: "save_patient", patient }, "patient"),
  /** `name`: the patient's name as she typed it again. */
  erasePatient: (id: string, name: string): Promise<void> => done({ op: "erase_patient", id, name }),
  saveNote: (note: SessionNote): Promise<SessionNote> => ask({ op: "save_note", note }, "note"),
  deleteNote: (id: string): Promise<void> => done({ op: "delete_note", id }),
  /** Copies the formal record (never the personal note) into the patient's case. */
  noteToCase: (id: string): Promise<SessionNote> => ask({ op: "note_to_case", id }, "note"),
  /** Keeps a recording with a note (D-208); `noteId` empty: the visit's note, or a new one. `data` is base64. */
  saveRecording: (a: { patientId: string; noteId: string; visitId: string | null; date: string; mime: string; seconds: number; name: string; data: string }): Promise<SessionNote> =>
    ask({ op: "save_recording", patient_id: a.patientId, note_id: a.noteId, visit_id: a.visitId, date: a.date, mime: a.mime, seconds: a.seconds, name: a.name, data: a.data }, "note"),
  /** The sound of a recording, to play; it never goes anywhere else. */
  recording: (id: string): Promise<RecordingSound> => ask({ op: "recording", id }, "sound"),
  deleteRecording: (noteId: string, id: string): Promise<SessionNote> => ask({ op: "delete_recording", note_id: noteId, id }, "note"),
  savePayment: (payment: Payment): Promise<Payment> => ask({ op: "save_payment", payment }, "payment"),
  voidPayment: (id: string, reason: string): Promise<Payment> => ask({ op: "void_payment", id, reason }, "payment"),
  saveCharge: (charge: Charge): Promise<Charge> => ask({ op: "save_charge", charge }, "charge"),
  voidCharge: (id: string, reason: string): Promise<Charge> => ask({ op: "void_charge", id, reason }, "charge"),
  tasks: (today: Today): Promise<TaskView[]> => ask({ op: "tasks", today }, "tasks"),
  saveTask: (task: ClinicTask): Promise<ClinicTask> => ask({ op: "save_task", task }, "task"),
  deleteTask: (id: string): Promise<void> => done({ op: "delete_task", id }),
  /** `month`: `YYYY-MM`. */
  diary: (month: string): Promise<DiaryMonth> => ask({ op: "diary", month }, "diary"),
  income: (year: number): Promise<IncomeYear> => ask({ op: "income", year }, "income"),
  history: (recordId: string): Promise<HistoryEntry[]> => ask({ op: "history", record_id: recordId }, "history"),
  settings: (): Promise<ClinicSettings> => ask({ op: "settings" }, "settings"),
  saveSettings: (settings: ClinicSettings): Promise<ClinicSettings> => ask({ op: "save_settings", settings }, "settings"),
  /** The filled text of a message to the parents, for her to copy. */
  message: (kind: MessageKind, patientId: string, visitId: string | null = null): Promise<string> =>
    ask({ op: "message", kind, patient_id: patientId, visit_id: visitId }, "text"),
  caseChoices: (): Promise<CaseChoice[]> => ask({ op: "case_choices" }, "cases"),
  /** `caseId` = `null` unlinks. */
  linkCase: (patientId: string, caseId: string | null): Promise<Patient> =>
    ask({ op: "link_case", patient_id: patientId, case_id: caseId }, "patient"),
  createCase: (patientId: string, today: Today): Promise<CaseLink> =>
    ask({ op: "create_case", patient_id: patientId, today }, "case"),
  /** Israel's holidays from `from` to `to` (ISO dates, both included; D-196). */
  holidays: (from: string, to: string): Promise<Holiday[]> => ask({ op: "holidays", from, to }, "holidays"),
  /** Free times of `minutes` from `from` on (D-197). `skipVisit`: the visit being moved. */
  freeSlots: (today: Today, from: string, minutes: number, count: number, skipVisit: string | null = null): Promise<Slot[]> =>
    ask({ op: "free_slots", today, from, minutes, count, skip_visit: skipVisit }, "slots"),
  /** The assessment pathway's visits, proposed and not saved (D-198). */
  proposePathway: (patientId: string, today: Today, from: string): Promise<Visit[]> =>
    ask({ op: "propose_pathway", patient_id: patientId, today, from }, "proposed"),
  /** Saves the pathway's visits; with `package`, one charge of the package and free visits. */
  savePathway: (patientId: string, visits: Visit[], packaged: boolean, today: Today): Promise<SavedVisits> =>
    ask({ op: "save_pathway", patient_id: patientId, visits, package: packaged, today }, "saved"),
  /** Opens the starter follow-ups of a family (D-199). */
  starterTasks: (patientId: string, today: Today): Promise<TaskView[]> =>
    ask({ op: "starter_tasks", patient_id: patientId, today }, "tasks"),
  /** The picture of the clinic over whole months (D-200). */
  insights: (today: Today, fromMonth: string, toMonth: string): Promise<Insights> =>
    ask({ op: "insights", today, from_month: fromMonth, to_month: toMonth }, "insights"),
};
