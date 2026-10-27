// The only module that talks to the Rust core. Types come from Rust via ts-rs.
import { invoke, type InvokeOptions } from "@tauri-apps/api/core";
import type { Commands } from "./generated/commands";
import { rememberReplay } from "../errors";
import { listen } from "@tauri-apps/api/event";
import type { ActivityPage } from "./generated/ActivityPage";
import type { SentCursor } from "./generated/SentCursor";
import type { AppStatus } from "./generated/AppStatus";
import type { RetentionItem } from "./generated/RetentionItem";
import type { BackupCheckView } from "./generated/BackupCheckView";
import type { BackupDone } from "./generated/BackupDone";
import type { BackupStatus } from "./generated/BackupStatus";
import type { StagedBackup } from "./generated/StagedBackup";
import type { CaseDetail } from "./generated/CaseDetail";
import type { CaseInput } from "./generated/CaseInput";
import type { CaseMeta } from "./generated/CaseMeta";
import type { DsmSeverity } from "./generated/DsmSeverity";
import type { SectionMode } from "./generated/SectionMode";
import type { CaseSummary } from "./generated/CaseSummary";
import type { Folder } from "./generated/Folder";
import type { GoogleStatus } from "./generated/GoogleStatus";
import type { DriveBackupStatus } from "./generated/DriveBackupStatus";
import type { DriveProblem } from "./generated/DriveProblem";
import type { DriveCleanupResult } from "./generated/DriveCleanupResult";
import type { NameMatch } from "./generated/NameMatch";
import type { ChatView } from "./generated/ChatView";
import type { ConsultationSummary } from "./generated/ConsultationSummary";
import type { ConsultationView } from "./generated/ConsultationView";
import type { ExportCheck } from "./generated/ExportCheck";
import type { Identity } from "./generated/Identity";
import type { IdentityInput } from "./generated/IdentityInput";
import type { ImportPreview } from "./generated/ImportPreview";
import type { InputKind } from "./generated/InputKind";
import type { Instrument } from "./generated/Instrument";
import type { InstrumentApproval } from "./generated/InstrumentApproval";
import type { InstrumentStatus } from "./generated/InstrumentStatus";
import type { ParagraphVersionView } from "./generated/ParagraphVersionView";
import type { PingResponse } from "./generated/PingResponse";
import type { Prepared } from "./generated/Prepared";
import type { ReportSettings } from "./generated/ReportSettings";
import type { TemplateView } from "./generated/TemplateView";
import type { LogoView } from "./generated/LogoView";
import type { ParentsOptions } from "./generated/ParentsOptions";
import type { ParagraphView } from "./generated/ParagraphView";
import type { SimilarParagraph } from "./generated/SimilarParagraph";
import type { LibraryLine } from "./generated/LibraryLine";
import type { SectionLibrary } from "./generated/SectionLibrary";
import type { SimilarReport } from "./generated/SimilarReport";
import type { FitMonth } from "./generated/FitMonth";
import type { Role } from "./generated/Role";
import type { ScoreSheet } from "./generated/ScoreSheet";
import type { UsageSummary } from "./generated/UsageSummary";
import type { Readiness } from "./generated/Readiness";
import type { SecurityItem } from "./generated/SecurityItem";
import type { SecurityLayer } from "./generated/SecurityLayer";
import type { SecurityStatus } from "./generated/SecurityStatus";
import type { UnsavedEdit } from "./generated/UnsavedEdit";
import type { SectionResult } from "./generated/SectionResult";
import type { ParagraphCheck } from "./generated/ParagraphCheck";
import type { Verdict } from "./generated/Verdict";
import type { VerifyResult } from "./generated/VerifyResult";
import type { ReportCheckNote } from "./generated/ReportCheckNote";
import type { ReportCheckResult } from "./generated/ReportCheckResult";
import type { ReportNoteKind } from "./generated/ReportNoteKind";
import type { SortResult } from "./generated/SortResult";
import type { SourceExcerpt } from "./generated/SourceExcerpt";
import type { MaterialRouting } from "./generated/MaterialRouting";
import type { PriorRecordView } from "./generated/PriorRecordView";
import type { Setting } from "./generated/Setting";
import type { SuspectDecision } from "./generated/SuspectDecision";
import type { FollowUpView } from "./generated/FollowUpView";
import type { UiError } from "./generated/UiError";
import type { UpdateView } from "./generated/UpdateView";
import type { StyleAnalysisResult } from "./generated/StyleAnalysisResult";
import type { StyleImportPreview } from "./generated/StyleImportPreview";
import type { StyleItem } from "./generated/StyleItem";
import type { StyleKind } from "./generated/StyleKind";
import type { StyleLearned } from "./generated/StyleLearned";
import type { StyleOverview } from "./generated/StyleOverview";
import type { Interpretation } from "./generated/Interpretation";
import type { InterpretationView } from "./generated/InterpretationView";
import type { StyleProfile } from "./generated/StyleProfile";
import type { StyleProfileView } from "./generated/StyleProfileView";
import type { StyleSourceView } from "./generated/StyleSourceView";
import type { StyleSuggestion } from "./generated/StyleSuggestion";

/** The largest logo the core takes (`dv_export::MAX_LOGO_BYTES`). */
export const LOGO_MAX_BYTES = 1024 * 1024;

/** Every failure reaches the UI as a UiError with a Hebrew message and its next step (F6). */
export function asUiError(e: unknown): UiError {
  if (e && typeof e === "object" && "message" in e && "code" in e) {
    const u = e as Partial<UiError> & { code: string; message: string };
    if (u.action && u.details) return u as UiError;
    return { ...u, details: u.details ?? [], action: u.action ?? { kind: "report" } };
  }
  // Not the core's answer (a refused grant, a fault in the window): trying the same again
  // rarely helps, so it goes to Or with what was thrown.
  return { code: "internal", message: "משהו השתבש. אפשר לשלוח לאור דוח תקלה.", details: [String(e)], action: { kind: "report" } };
}

/** An argument that is a password, a key or a typed kit: such a call is never kept in memory
 *  for "לנסות שוב" (F6). Matched by name, so a new command is covered as well. */
const SECRET_ARG = /password|secret|key|current|typed/i;

function keepsNoSecret(args: unknown): boolean {
  if (args === undefined || args === null) return true;
  if (args instanceof Uint8Array || args instanceof ArrayBuffer) return false;
  return typeof args === "object" && !Object.keys(args).some((k) => SECRET_ARG.test(k));
}

type Command = keyof Commands;
/** A command that takes nothing is called with no arguments; any other with exactly its own. */
type CallArgs<K extends Command> = Commands[K]["args"] extends undefined
  ? [args?: undefined, options?: InvokeOptions]
  : [args: Commands[K]["args"], options?: InvokeOptions];

function send<K extends Command>(cmd: K, args: unknown, options: InvokeOptions | undefined): Promise<Commands[K]["returns"]> {
  return options === undefined ? invoke<Commands[K]["returns"]>(cmd, args as never) : invoke<Commands[K]["returns"]>(cmd, args as never, options);
}

/** One command, checked by `tsc` against the Rust signature it was generated from (F5). Its
 *  answer is for the screen that asked, so a failure keeps nothing to run again: running it
 *  from the error line would drop the answer. */
async function call<K extends Command>(cmd: K, ...[args, options]: CallArgs<K>): Promise<Commands[K]["returns"]> {
  try {
    return await send(cmd, args, options);
  } catch (e) {
    throw asUiError(e);
  }
}

/** A command that answers nothing. "לנסות שוב" runs this same call again (F6), unless it
 *  carries a password, a key or a file. */
async function run<K extends Command>(cmd: K, ...[args, options]: CallArgs<K>): Promise<undefined> {
  const again = () => send(cmd, args, options);
  try {
    await again();
  } catch (e) {
    const err = asUiError(e);
    if (keepsNoSecret(args)) rememberReplay(err, again);
    throw err;
  }
  return undefined;
}

/** What the core tells the window on its own (F3): no case data, only that something changed. */
export type CoreEvent =
  | { kind: "locked" }
  | { kind: "idle_soon"; seconds: number }
  | { kind: "backed_up" };

/** Listen to the core's events. Resolves to the function that stops listening, or to null
 *  when events cannot reach the window (the browser preview, tests, or a refused grant): the
 *  caller then polls often, so a lock never shows late (F2). */
export async function onCoreEvent(handler: (e: CoreEvent) => void): Promise<(() => void) | null> {
  try {
    return await listen<CoreEvent>("core", (e) => handler(e.payload));
  } catch {
    return null;
  }
}

export const ipc = {
  ping: () => call("ping"),
  /** The last crashes, in the crash log's own fixed shape, for the bug report (U6). */
  recentCrashes: () => call("recent_crashes"),
  status: () => call("app_status"),
  createVault: (password: string) => call("create_vault", { password }),
  confirmRecoveryKey: (typed: string) => call("confirm_recovery_key", { typed }),
  unlock: (password: string) => call("unlock", { password }),
  unlockWithRecovery: (key: string) => call("unlock_with_recovery", { key }),
  lock: () => run("lock"),
  /** Typing or scrolling in the window counts as activity for the idle lock. */
  touch: () => run("touch"),
  holdUnsaved: (edits: UnsavedEdit[]) => run("hold_unsaved", { edits }),
  takeUnsaved: () => call("take_unsaved"),
  readiness: () => call("readiness"),
  securityStatus: () => call("security_status"),
  /** The protection layers as a PDF (L6), into the same folder as the reports. */
  exportSecurityPdf: () => call("export_security_pdf"),
  setSendStopped: (stopped: boolean) => call("set_send_stopped", { stopped }),
  confirmReadiness: (key: string, done: boolean) => call("confirm_readiness", { key, done }),
  /** Settings → המבחנים: every instrument with her answer (D-098). */
  instrumentApprovals: () => call("instrument_approvals"),
  setInstrumentStatus: (key: string, status: InstrumentStatus) => call("set_instrument_status", { key, status }),
  usageSummary: () => call("usage_summary"),
  /** Dollars; `null` removes the ceiling. */
  setMonthlyCap: (capUsd: number | null) => run("set_monthly_cap", { capUsd }),

  /** `retentionAck`: she read what ChatGPT or Gemini keeps (required for their keys; D-040). */
  setApiKey: (provider: string, key: string, retentionAck = false) => run("set_api_key", { provider, key, retentionAck }),
  setSpeed: (speed: "fast" | "balanced" | "thorough") => run("set_speed", { speed }),
  setModel: (model: string) => run("set_model", { model }),
  setLockMinutes: (minutes: number) => run("set_lock_minutes", { minutes }),
  setPractitioner: (names: string[]) => run("set_practitioner", { names }),
  setSectionGuide: (sectionKey: string, text: string) => run("set_section_guide", { sectionKey, text }),
  setReviewAlways: (on: boolean) => run("set_review_always", { on }),
  setScreenProtection: (on: boolean) => run("set_screen_protection", { on }),
  reportSettings: () => call("report_settings"),
  setReportSettings: (settings: ReportSettings) => run("set_report_settings", { settings }),
  /** Her Word template (EX-1): the file's bytes go as the raw body. */
  setReportTemplate: async (file: File): Promise<TemplateView> => {
    const bytes = new Uint8Array(await file.arrayBuffer());
    return await call("set_report_template", bytes);
  },
  clearReportTemplate: () => run("clear_report_template"),
  reportTemplate: () => call("report_template"),
  /** Her logo (round 55): a PNG or JPEG of at most 1 MB, the file's bytes as the raw body. */
  setReportLogo: async (file: File): Promise<LogoView> => {
    // Refused before the file is read, the way the core refuses it (1 MB, `MAX_LOGO_BYTES`).
    if (file.size > LOGO_MAX_BYTES) {
      throw { code: "refused", message: "התמונה גדולה מ-1MB. שומרים אותה בגודל קטן יותר ובוחרים שוב.", details: [], action: { kind: "dismiss" } } satisfies UiError;
    }
    const bytes = new Uint8Array(await file.arrayBuffer());
    return await call("set_report_logo", bytes);
  },
  clearReportLogo: () => run("clear_report_logo"),
  reportLogo: () => call("report_logo"),

  listCases: () => call("list_cases"),
  createCase: (meta: CaseMeta, identities: IdentityInput[]) => call("create_case", { meta, identities }),
  createSampleCase: () => call("create_sample_case"),
  updateCase: (caseId: string, meta: CaseMeta) => run("update_case", { caseId, meta }),
  setCaseGuide: (caseId: string, sectionKey: string, text: string) => run("set_case_guide", { caseId, sectionKey, text }),
  /** The approved recommendations she left out of the letters (round 44); `null`: none. */
  setLetterLeftOut: (caseId: string, ids: string[] | null) => run("set_letter_left_out", { caseId, ids }),
  /** The DSM-5 severity she set (round 43): a level 1–3 for each domain, `null` for none. */
  setDsmSeverity: (caseId: string, social: number | null, restricted: number | null) => run("set_dsm_severity", { caseId, social, restricted }),
  /** Into the recycle bin (D-023); restorable for 30 days. */
  deleteCase: (caseId: string) => run("delete_case", { caseId }),
  listTrash: () => call("list_trash"),
  restoreCase: (caseId: string) => run("restore_case", { caseId }),
  /** Erase from the bin now; the password is asked again. */
  purgeCase: (caseId: string, password: string) => run("purge_case", { caseId, password }),
  folders: () => call("folders"),
  createFolder: (parentId: string | null, name: string) => call("create_folder", { parentId, name }),
  renameFolder: (id: string, name: string) => run("rename_folder", { id, name }),
  moveFolder: (id: string, parentId: string | null) => run("move_folder", { id, parentId }),
  deleteFolder: (id: string) => run("delete_folder", { id }),
  moveCase: (caseId: string, folderId: string | null) => run("move_case", { caseId, folderId }),
  findNameMatches: (caseId: string | null, names: string[]) => call("find_name_matches", { caseId, names }),
  setIdentities: (caseId: string, identities: IdentityInput[]) =>
    call("set_identities", { caseId, identities }),
  caseDetail: (caseId: string) => call("case_detail", { caseId }),

  addInput: (caseId: string, kind: InputKind, title: string, content: string) =>
    call("add_input", { caseId, kind, title, content }),
  updateInput: (caseId: string, inputId: string, title: string, content: string) =>
    run("update_input", { caseId, inputId, title, content }),
  deleteInput: (caseId: string, inputId: string) => run("delete_input", { caseId, inputId }),
  /** Undoes the last deletion of this material (until another is deleted or the vault locks). */
  restoreInput: (caseId: string, inputId: string) => run("restore_input", { caseId, inputId }),
  scoreInstruments: () => call("score_instruments"),
  previewScores: (sheet: ScoreSheet) => call("preview_scores", { sheet }),
  /** Saves (or, with `inputId`, replaces) a score table as a "test scores" material. */
  saveScores: (caseId: string, inputId: string | null, sheet: ScoreSheet) =>
    call("save_scores", { caseId, inputId, sheet }),
  scoreSheet: (caseId: string, inputId: string) => call("score_sheet", { caseId, inputId }),
  /** "למה כתבת את זה?": the passages a paragraph leans on, best match first (stays local). */
  paragraphSources: (caseId: string, draftId: string) =>
    call("paragraph_sources", { caseId, draftId }),
  /** The file's bytes go as the raw body; nothing else of the file system is exposed. */
  importDocument: async (caseId: string, file: File): Promise<ImportPreview> => {
    const bytes = new Uint8Array(await file.arrayBuffer());
    return await call("import_document", bytes, {
      headers: { "x-case-id": encodeURIComponent(caseId), "x-file-name": encodeURIComponent(file.name) },
    });
  },
  previewFilter: (caseId: string, text: string) => call("preview_filter", { caseId, text }),
  decideSuspect: (caseId: string, token: string, decision: SuspectDecision) =>
    run("decide_suspect", { caseId, token, decision }),
  settleForeignTags: (caseId: string) => call("settle_foreign_tags", { caseId }),
  /** "להחזיר" on the summary card: keep what the filter hid as written, for this case. */
  restoreAutoHidden: (caseId: string, token: string, tag: string) =>
    run("restore_auto_hidden", { caseId, token, tag }),
  /** "נכון, תמיד להסתיר": a name the filter kept is confirmed for the whole case. */
  confirmAutoHidden: (caseId: string, token: string, tag: string) =>
    run("confirm_auto_hidden", { caseId, token, tag }),
  /** Who a name the filter kept is; it gets a tag for that role. */
  changeRole: (caseId: string, tag: string, role: Role) => call("change_role", { caseId, tag, role }),

  /** `replaces`: the one proposed paragraph the answer rewrites; otherwise the answer is the
   *  section's new draft, in place of the paragraphs not approved yet. */
  prepareSection: (caseId: string, sectionKey: string, instruction: string, replaces?: string) =>
    call("prepare_section", { caseId, sectionKey, instruction, replaces: replaces ?? null }),
  prepareFullDraft: (caseId: string) => call("prepare_full_draft", { caseId }),
  /** A section she writes herself, worded from her points only (round 30). */
  prepareFromPoints: (caseId: string, sectionKey: string, points: string) =>
    call("prepare_from_points", { caseId, sectionKey, points }),
  /** Beside her own section: how she wrote it in her reports. Local, nothing is sent. */
  ownSectionHelp: (sectionKey: string) => call("own_section_help", { sectionKey }),
  /** Who writes each section of the institute's report (round 30). */
  sectionModes: () => call("section_modes"),
  setSectionMode: (sectionKey: string, mode: SectionMode) => run("set_section_mode", { sectionKey, mode }),
  /** Words Claude has written so far for a request on its way (streamed answers). */
  sendProgress: (approvalId: string) => call("send_progress", { approvalId }),
  sendSection: (approvalId: string) => call("send_section", { approvalId }),
  /** The check of a draft against its sources: on unless switched off in settings (round 41). */
  verifyStageOn: () => call("verify_stage_on"),
  setVerifyStage: (on: boolean) => run("set_verify_stage", { on }),
  /** The optional name model (D-053): off by default, downloaded only when turned on. */
  nameModelStatus: () => call("name_model_status"),
  /** Download it once (pinned files, checked), then turn it on. */
  downloadNameModel: () => call("download_name_model"),
  /** Bytes done and total while it downloads. */
  nameModelProgress: () => call("name_model_progress"),
  /** Off deletes its files. */
  setNameModel: (on: boolean) => call("set_name_model", { on }),
  prepareVerify: (caseId: string, sectionKey: string) => call("prepare_verify", { caseId, sectionKey }),
  sendVerify: (approvalId: string) => call("send_verify", { approvalId }),
  /** The check of the whole approved report (D-097): off until she turns it on, its cost shown. */
  reportCheckSetting: () => call("report_check_setting"),
  setReportCheck: (on: boolean) => run("set_report_check", { on }),
  prepareReportCheck: (caseId: string) => call("prepare_report_check", { caseId }),
  sendReportCheck: (approvalId: string) => call("send_report_check", { approvalId }),
  /** D-022: sort every material not sorted yet into sections (one review screen). */
  prepareSort: (caseId: string) => call("prepare_sort", { caseId }),
  sendSort: (approvalId: string) => call("send_sort", { approvalId }),
  /** Einat's choice of sections for one material; it always wins. */
  setInputSections: (caseId: string, inputId: string, sections: string[]) =>
    run("set_input_sections", { caseId, inputId, sections }),
  chat: (caseId: string, sectionKey: string) => call("chat", { caseId, sectionKey }),
  approveParagraph: (caseId: string, draftId: string) => run("approve_paragraph", { caseId, draftId }),
  /** D-029: a follow-up assessment of a case (same names and folder, a new consent). */
  createFollowUp: (caseId: string) => call("create_follow_up", { caseId }),
  followUp: (caseId: string) => call("follow_up", { caseId }),
  addComparisonMaterial: (caseId: string) => call("add_comparison_material", { caseId }),
  /** Approve every paragraph waiting in the section; returns how many. */
  approveSection: (caseId: string, sectionKey: string) => call("approve_section", { caseId, sectionKey }),
  rejectParagraph: (caseId: string, draftId: string) => run("reject_paragraph", { caseId, draftId }),
  /** `openedWith`: the text the editor showed when it opened. `true` back = the paragraph
   *  changed meanwhile; her text is saved and the other wording is kept among the versions. */
  editParagraph: (caseId: string, draftId: string, text: string, openedWith: string | null = null) =>
    call("edit_paragraph", { caseId, draftId, text, openedWith }),
  /** Earlier wordings of a paragraph, newest first; bring one back (D-046). */
  paragraphVersions: (caseId: string, draftId: string) =>
    call("paragraph_versions", { caseId, draftId }),
  /** The paragraph of her past reports closest to this one (ST2); worked out locally. */
  similarParagraph: (caseId: string, draftId: string) =>
    call("similar_paragraph", { caseId, draftId }),
  /** Her diagnoses, recommendations and the reports most like the case (round 36); local. */
  sectionLibrary: (caseId: string, sectionKey: string) =>
    call("section_library", { caseId, sectionKey }),
  restoreParagraphVersion: (caseId: string, draftId: string, versionId: string) =>
    run("restore_paragraph_version", { caseId, draftId, versionId }),
  /** Her own paragraph. `at`: "end" (default), "first", or the id of the paragraph it follows. */
  addOwnParagraph: (caseId: string, sectionKey: string, text: string, at: string = "end") =>
    run("add_own_paragraph", { caseId, sectionKey, text, first: at === "first", after: at === "end" || at === "first" ? null : at }),

  prepareConsult: (caseId: string | null, conversationId: string | null, message: string) =>
    call("prepare_consult", { caseId, conversationId, message }),
  /** Saved conversations, newest first; one conversation; delete one. */
  consultations: () => call("consultations"),
  consultation: (id: string) => call("consultation", { id }),
  deleteConsultation: (id: string) => run("delete_consultation", { id }),
  sendConsult: (approvalId: string) => call("send_consult", { approvalId }),

  checkExport: (caseId: string) => call("check_export", { caseId }),
  /** With `parents`, the parents' version (round 55, D-095): built here, nothing is sent. */
  exportReport: (caseId: string, password: string | null, parents: ParentsOptions | null = null) =>
    call("export_report", { caseId, password, parents }),
  /** EX-3: a print-only PDF (cannot be changed; opens without a password, D-081). */
  exportPdf: (caseId: string, password: string | null, parents: ParentsOptions | null = null) =>
    call("export_pdf", { caseId, password, parents }),
  /** When this vault exported exactly this PDF, or `null` (changed, or not made here). */
  checkOriginal: async (file: File): Promise<number | null> => {
    const bytes = new Uint8Array(await file.arrayBuffer());
    return await call("check_original", bytes);
  },
  /** EX-4: a short letter from the approved report, through the same review and gate. */
  prepareLetter: (caseId: string, audience: "parents" | "school", note: string) =>
    call("prepare_letter", { caseId, audience, note }),
  letter: (caseId: string, audience: "parents" | "school") => call("letter", { caseId, audience }),
  exportLetter: (caseId: string, audience: "parents" | "school", password: string | null) =>
    call("export_letter", { caseId, audience, password }),
  /** The file password to the clipboard: out of history and cloud sync, cleared after N seconds (returned). */
  copySecret: (text: string) => call("copy_secret", { text }),

  /** The system's print window (the macOS webview ignores `window.print()`). */
  printPage: () => run("print_page"),

  // Activity log (metadata only; cases named on this computer) and retention reminders.
  activity: (before: number | null) => call("activity", { before }),
  markActivityReviewed: () => run("mark_activity_reviewed"),
  /** "מה יצא מהמחשב": one page of the requests kept with a case (or every case), newest first, from `before` on. */
  sentHistory: (caseId: string | null, place: string | null, query: string, before: SentCursor | null) =>
    call("sent_history", { caseId, place, query, before }),
  /** How many requests left since `since` (seconds), of every case: counted, not read. */
  sentCount: (since: number) => call("sent_count", { since }),
  sentText: (caseId: string, id: string) => call("sent_text", { caseId, id }),
  retentionDue: () => call("retention_due"),
  keepCaseLonger: (caseId: string, years: number) => run("keep_case_longer", { caseId, years }),

  /** `current` is the password, or the recovery kit when the password was forgotten. */
  changePassword: (current: string, withRecovery: boolean, newPassword: string) =>
    run("change_password", { current, withRecovery, newPassword }),
  newRecoveryKit: (current: string, withRecovery: boolean) =>
    call("new_recovery_kit", { current, withRecovery }),

  /** Forgot the password → sign in with Google (D-041). Readable while locked. */
  googleStatus: () => call("google_status"),
  /** Opens Google in her browser; resolves when she is back and the slot is written. */
  googleTurnOn: (password: string) => run("google_turn_on", { password }),
  googleTurnOff: (password: string) => run("google_turn_off", { password }),
  /** Stop a sign-in that waits for the browser. */
  googleCancel: () => run("google_cancel"),
  /** Locked, password forgotten: sign in with Google, open, and set `newPassword` at once. */
  googleRecover: (newPassword: string) => call("google_recover", { newPassword }),

  // Encrypted backup (D-024). The system's own window picks the file; `null` = cancelled.
  backupStatus: () => call("backup_status"),
  writeBackup: () => call("write_backup"),
  chooseBackup: () => call("choose_backup"),
  checkBackup: (password: string) => call("check_backup", { password }),
  restoreBackup: (password: string | null, recoveryKey: string | null) =>
    call("restore_backup", { password, recoveryKey }),
  forgetBackup: () => run("forget_backup"),
  setAutoBackup: (on: boolean) => call("set_auto_backup", { on }),
  /** The encrypted copy of each backup in her Google Drive (D-099). Off by default. */
  driveBackupStatus: () => call("drive_backup_status"),
  /** Opens Google in her browser; resolves when she is back and the copy is on. */
  driveBackupTurnOn: () => call("drive_backup_turn_on"),
  /** How far deleting the copies and withdrawing the permission at Google got. */
  driveBackupTurnOff: () => call("drive_backup_turn_off"),

  // Writing style (D-043): past reports kept neutralized, the profile, learning from edits.
  styleOverview: () => call("style_overview"),
  /** A past report: the bytes go as the raw body. Nothing is kept until `saveStyleSource`. */
  importStyleSource: async (file: File): Promise<StyleImportPreview> => {
    const bytes = new Uint8Array(await file.arrayBuffer());
    return await call("import_style_source", bytes, {
      headers: { "x-file-name": encodeURIComponent(file.name) },
    });
  },
  saveStyleSource: (token: string, included: number[], title: string | null) =>
    call("save_style_source", { token, included, title }),
  discardStyleUpload: () => run("discard_style_upload"),
  deleteStyleSource: (id: string) => run("delete_style_source", { id }),
  prepareStyleAnalysis: (sourceId: string) => call("prepare_style_analysis", { sourceId }),
  sendStyleAnalysis: (approvalId: string) => call("send_style_analysis", { approvalId }),
  prepareStyleProfile: () => call("prepare_style_profile"),
  sendStyleProfile: (approvalId: string) => call("send_style_profile", { approvalId }),
  prepareInterpretations: () => call("prepare_interpretations"),
  sendInterpretations: (approvalId: string) => call("send_interpretations", { approvalId }),
  updateInterpretation: (id: string, text: string | null, approved: boolean | null, enabled: boolean | null) =>
    call("update_interpretation", { id, text, approved, enabled }),
  deleteInterpretation: (id: string) => run("delete_interpretation", { id }),
  saveStyleDraft: (profile: StyleProfile) => call("save_style_draft", { profile }),
  approveStyleDraft: () => call("approve_style_draft"),
  discardStyleDraft: () => run("discard_style_draft"),
  restoreStyleVersion: (id: string) => call("restore_style_version", { id }),
  setStyleEnabled: (on: boolean) => run("set_style_enabled", { on }),
  resetStyle: () => run("reset_style"),
  forgetStyleLearning: () => run("forget_style_learning"),
  acceptStyleSuggestion: (id: string) => call("accept_style_suggestion", { id }),
  dismissStyleSuggestion: (id: string) => run("dismiss_style_suggestion", { id }),

  /** A newer version, signed by the developer (D-033); `null` when this is the newest. */
  checkUpdate: () => call("check_update"),
  /** In the background: fetch and verify the newest version. `true` when it waits for her click. */
  prepareUpdate: () => call("prepare_update"),
  /** Lock, run the verified installer (fetched now if it is not waiting); closes and opens again. */
  installUpdate: () => run("install_update"),
  /** R7: the page of released versions, in her browser, to go back one version. */
  openReleasesPage: () => run("open_releases_page"),
};

export type {
  FitMonth,
  Interpretation,
  InterpretationView,
  ParagraphView,
  SimilarParagraph,
  LibraryLine,
  SectionLibrary,
  SimilarReport,
  StyleAnalysisResult,
  StyleImportPreview,
  StyleItem,
  StyleKind,
  StyleLearned,
  StyleOverview,
  StyleProfile,
  StyleProfileView,
  StyleSourceView,
  StyleSuggestion,
  UpdateView,
  UsageSummary,
  Readiness,
  SecurityItem,
  SecurityLayer,
  SecurityStatus,
  UnsavedEdit,
  ActivityPage,
  ConsultationSummary,
  ConsultationView,
  AppStatus,
  RetentionItem,
  BackupCheckView,
  BackupDone,
  BackupStatus,
  StagedBackup,
  Folder,
  GoogleStatus,
  DriveBackupStatus,
  DriveCleanupResult,
  DriveProblem,
  NameMatch,
  MaterialRouting,
  PriorRecordView,
  Setting,
  SortResult,
  SourceExcerpt,
  CaseDetail,
  CaseInput,
  CaseMeta,
  DsmSeverity,
  CaseSummary,
  ChatView,
  ExportCheck,
  Identity,
  IdentityInput,
  ImportPreview,
  InputKind,
  Instrument,
  InstrumentApproval,
  InstrumentStatus,
  ParagraphVersionView,
  PingResponse,
  Prepared,
  ReportSettings,
  TemplateView,
  LogoView,
  ParentsOptions,
  FollowUpView,
  ScoreSheet,
  SectionResult,
  SuspectDecision,
  UiError,
  ParagraphCheck,
  Verdict,
  VerifyResult,
  ReportCheckNote,
  ReportCheckResult,
  ReportNoteKind,
};
