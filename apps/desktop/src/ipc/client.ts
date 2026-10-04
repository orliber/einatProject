// The only module that talks to the Rust core. Types come from Rust via ts-rs.
import { invoke } from "@tauri-apps/api/core";
import type { ActivityPage } from "./generated/ActivityPage";
import type { AppStatus } from "./generated/AppStatus";
import type { RetentionItem } from "./generated/RetentionItem";
import type { BackupCheckView } from "./generated/BackupCheckView";
import type { BackupDone } from "./generated/BackupDone";
import type { BackupStatus } from "./generated/BackupStatus";
import type { StagedBackup } from "./generated/StagedBackup";
import type { CaseDetail } from "./generated/CaseDetail";
import type { CaseInput } from "./generated/CaseInput";
import type { CaseMeta } from "./generated/CaseMeta";
import type { CaseSummary } from "./generated/CaseSummary";
import type { Folder } from "./generated/Folder";
import type { NameMatch } from "./generated/NameMatch";
import type { ChatView } from "./generated/ChatView";
import type { ConsultationSummary } from "./generated/ConsultationSummary";
import type { ConsultationView } from "./generated/ConsultationView";
import type { ConsultResult } from "./generated/ConsultResult";
import type { CreatedVault } from "./generated/CreatedVault";
import type { ExportCheck } from "./generated/ExportCheck";
import type { FilterOutcome } from "./generated/FilterOutcome";
import type { Identity } from "./generated/Identity";
import type { IdentityInput } from "./generated/IdentityInput";
import type { ImportPreview } from "./generated/ImportPreview";
import type { InputKind } from "./generated/InputKind";
import type { Instrument } from "./generated/Instrument";
import type { ParagraphVersionView } from "./generated/ParagraphVersionView";
import type { PingResponse } from "./generated/PingResponse";
import type { Prepared } from "./generated/Prepared";
import type { ReportSettings } from "./generated/ReportSettings";
import type { ScoreSheet } from "./generated/ScoreSheet";
import type { UsageSummary } from "./generated/UsageSummary";
import type { Readiness } from "./generated/Readiness";
import type { UnsavedEdit } from "./generated/UnsavedEdit";
import type { SectionResult } from "./generated/SectionResult";
import type { SortResult } from "./generated/SortResult";
import type { MaterialRouting } from "./generated/MaterialRouting";
import type { SuspectDecision } from "./generated/SuspectDecision";
import type { FollowUpView } from "./generated/FollowUpView";
import type { UiError } from "./generated/UiError";
import type { UpdateView } from "./generated/UpdateView";

/** Every failure reaches the UI as a UiError with a Hebrew message. */
export function asUiError(e: unknown): UiError {
  if (e && typeof e === "object" && "message" in e && "code" in e) {
    return e as UiError;
  }
  return { code: "internal", message: "משהו השתבש. אפשר לנסות שוב.", details: [String(e)] };
}

async function call<T>(cmd: string, args?: Record<string, unknown>): Promise<T> {
  try {
    return await invoke<T>(cmd, args);
  } catch (e) {
    throw asUiError(e);
  }
}

/** A command that answers nothing. */
async function run(cmd: string, args?: Record<string, unknown>): Promise<undefined> {
  await call<unknown>(cmd, args);
  return undefined;
}

export const ipc = {
  ping: () => call<PingResponse>("ping"),
  status: () => call<AppStatus>("app_status"),
  createVault: (password: string) => call<CreatedVault>("create_vault", { password }),
  confirmRecoveryKey: (typed: string) => call<boolean>("confirm_recovery_key", { typed }),
  unlock: (password: string) => call<AppStatus>("unlock", { password }),
  unlockWithRecovery: (key: string) => call<AppStatus>("unlock_with_recovery", { key }),
  lock: () => run("lock"),
  /** Typing or scrolling in the window counts as activity for the idle lock. */
  touch: () => run("touch"),
  holdUnsaved: (edit: UnsavedEdit | null) => run("hold_unsaved", { edit }),
  takeUnsaved: () => call<UnsavedEdit | null>("take_unsaved"),
  readiness: () => call<Readiness>("readiness"),
  confirmReadiness: (key: string, done: boolean) => call<Readiness>("confirm_readiness", { key, done }),
  usageSummary: () => call<UsageSummary>("usage_summary"),
  /** Dollars; `null` removes the ceiling. */
  setMonthlyCap: (capUsd: number | null) => run("set_monthly_cap", { capUsd }),

  setApiKey: (key: string) => run("set_api_key", { key }),
  setSpeed: (speed: "fast" | "balanced" | "thorough") => run("set_speed", { speed }),
  setModel: (model: string) => run("set_model", { model }),
  setLockMinutes: (minutes: number) => run("set_lock_minutes", { minutes }),
  setPractitioner: (names: string[]) => run("set_practitioner", { names }),
  setReviewOnlySuspect: (on: boolean) => run("set_review_only_suspect", { on }),
  setScreenProtection: (on: boolean) => run("set_screen_protection", { on }),
  reportSettings: () => call<ReportSettings>("report_settings"),
  setReportSettings: (settings: ReportSettings) => run("set_report_settings", { settings }),

  listCases: () => call<CaseSummary[]>("list_cases"),
  createCase: (meta: CaseMeta, identities: IdentityInput[]) => call<string>("create_case", { meta, identities }),
  updateCase: (caseId: string, meta: CaseMeta) => run("update_case", { caseId, meta }),
  /** Into the recycle bin (D-023); restorable for 30 days. */
  deleteCase: (caseId: string) => run("delete_case", { caseId }),
  listTrash: () => call<CaseSummary[]>("list_trash"),
  restoreCase: (caseId: string) => run("restore_case", { caseId }),
  /** Erase from the bin now; the password is asked again. */
  purgeCase: (caseId: string, password: string) => run("purge_case", { caseId, password }),
  folders: () => call<Folder[]>("folders"),
  createFolder: (parentId: string | null, name: string) => call<Folder>("create_folder", { parentId, name }),
  renameFolder: (id: string, name: string) => run("rename_folder", { id, name }),
  moveFolder: (id: string, parentId: string | null) => run("move_folder", { id, parentId }),
  deleteFolder: (id: string) => run("delete_folder", { id }),
  moveCase: (caseId: string, folderId: string | null) => run("move_case", { caseId, folderId }),
  findNameMatches: (caseId: string | null, names: string[]) => call<NameMatch[]>("find_name_matches", { caseId, names }),
  setIdentities: (caseId: string, identities: IdentityInput[]) =>
    call<Identity[]>("set_identities", { caseId, identities }),
  caseDetail: (caseId: string) => call<CaseDetail>("case_detail", { caseId }),

  addInput: (caseId: string, kind: InputKind, title: string, content: string) =>
    call<CaseInput>("add_input", { caseId, kind, title, content }),
  updateInput: (caseId: string, inputId: string, title: string, content: string) =>
    run("update_input", { caseId, inputId, title, content }),
  deleteInput: (caseId: string, inputId: string) => run("delete_input", { caseId, inputId }),
  scoreInstruments: () => call<Instrument[]>("score_instruments"),
  previewScores: (sheet: ScoreSheet) => call<string>("preview_scores", { sheet }),
  /** Saves (or, with `inputId`, replaces) a score table as a "test scores" material. */
  saveScores: (caseId: string, inputId: string | null, sheet: ScoreSheet) =>
    call<CaseInput>("save_scores", { caseId, inputId, sheet }),
  scoreSheet: (caseId: string, inputId: string) => call<ScoreSheet | null>("score_sheet", { caseId, inputId }),
  /** The file's bytes go as the raw body; nothing else of the file system is exposed. */
  importDocument: async (caseId: string, file: File): Promise<ImportPreview> => {
    const bytes = new Uint8Array(await file.arrayBuffer());
    try {
      return await invoke<ImportPreview>("import_document", bytes, {
        headers: { "x-case-id": encodeURIComponent(caseId), "x-file-name": encodeURIComponent(file.name) },
      });
    } catch (e) {
      throw asUiError(e);
    }
  },
  previewFilter: (caseId: string, text: string) => call<FilterOutcome>("preview_filter", { caseId, text }),
  decideSuspect: (caseId: string, token: string, decision: SuspectDecision) =>
    run("decide_suspect", { caseId, token, decision }),

  /** `replaces`: the one proposed paragraph the answer rewrites; otherwise the answer is the
   *  section's new draft, in place of the paragraphs not approved yet. */
  prepareSection: (caseId: string, sectionKey: string, instruction: string, replaces?: string) =>
    call<Prepared>("prepare_section", { caseId, sectionKey, instruction, replaces: replaces ?? null }),
  prepareFullDraft: (caseId: string) => call<[string, Prepared][]>("prepare_full_draft", { caseId }),
  /** Words Claude has written so far for a request on its way (streamed answers). */
  sendProgress: (approvalId: string) => call<number>("send_progress", { approvalId }),
  sendSection: (approvalId: string) => call<SectionResult>("send_section", { approvalId }),
  /** D-022: sort every material not sorted yet into sections (one review screen). */
  prepareSort: (caseId: string) => call<Prepared>("prepare_sort", { caseId }),
  sendSort: (approvalId: string) => call<SortResult>("send_sort", { approvalId }),
  /** Einat's choice of sections for one material; it always wins. */
  setInputSections: (caseId: string, inputId: string, sections: string[]) =>
    run("set_input_sections", { caseId, inputId, sections }),
  chat: (caseId: string, sectionKey: string) => call<ChatView[]>("chat", { caseId, sectionKey }),
  approveParagraph: (caseId: string, draftId: string) => run("approve_paragraph", { caseId, draftId }),
  /** D-029: a follow-up assessment of a case (same names and folder, a new consent). */
  createFollowUp: (caseId: string) => call<string>("create_follow_up", { caseId }),
  followUp: (caseId: string) => call<FollowUpView | null>("follow_up", { caseId }),
  addComparisonMaterial: (caseId: string) => call<CaseInput>("add_comparison_material", { caseId }),
  /** Approve every paragraph waiting in the section; returns how many. */
  approveSection: (caseId: string, sectionKey: string) => call<number>("approve_section", { caseId, sectionKey }),
  rejectParagraph: (caseId: string, draftId: string) => run("reject_paragraph", { caseId, draftId }),
  editParagraph: (caseId: string, draftId: string, text: string) =>
    run("edit_paragraph", { caseId, draftId, text }),
  /** Earlier wordings of a paragraph, newest first; bring one back (D-043). */
  paragraphVersions: (caseId: string, draftId: string) =>
    call<ParagraphVersionView[]>("paragraph_versions", { caseId, draftId }),
  restoreParagraphVersion: (caseId: string, draftId: string, versionId: string) =>
    run("restore_paragraph_version", { caseId, draftId, versionId }),
  /** Her own paragraph. `at`: "end" (default), "first", or the id of the paragraph it follows. */
  addOwnParagraph: (caseId: string, sectionKey: string, text: string, at: string = "end") =>
    run("add_own_paragraph", { caseId, sectionKey, text, first: at === "first", after: at === "end" || at === "first" ? null : at }),

  prepareConsult: (caseId: string | null, conversationId: string | null, message: string) =>
    call<Prepared>("prepare_consult", { caseId, conversationId, message }),
  /** Saved conversations, newest first; one conversation; delete one. */
  consultations: () => call<ConsultationSummary[]>("consultations"),
  consultation: (id: string) => call<ConsultationView>("consultation", { id }),
  deleteConsultation: (id: string) => run("delete_consultation", { id }),
  sendConsult: (approvalId: string) => call<ConsultResult>("send_consult", { approvalId }),

  checkExport: (caseId: string) => call<ExportCheck>("check_export", { caseId }),
  exportReport: (caseId: string, password: string | null) => call<string>("export_report", { caseId, password }),
  /** The file password to the clipboard: out of history and cloud sync, cleared after N seconds (returned). */
  copySecret: (text: string) => call<number>("copy_secret", { text }),

  /** The system's print window (the macOS webview ignores `window.print()`). */
  printPage: () => run("print_page"),

  // Activity log (metadata only; cases named on this computer) and retention reminders.
  activity: (before: number | null) => call<ActivityPage>("activity", { before }),
  markActivityReviewed: () => run("mark_activity_reviewed"),
  retentionDue: () => call<RetentionItem[]>("retention_due"),
  keepCaseLonger: (caseId: string, years: number) => run("keep_case_longer", { caseId, years }),

  /** `current` is the password, or the recovery kit when the password was forgotten. */
  changePassword: (current: string, withRecovery: boolean, newPassword: string) =>
    run("change_password", { current, withRecovery, newPassword }),
  newRecoveryKit: (current: string, withRecovery: boolean) =>
    call<CreatedVault>("new_recovery_kit", { current, withRecovery }),

  // Encrypted backup (D-024). The system's own window picks the file; `null` = cancelled.
  backupStatus: () => call<BackupStatus>("backup_status"),
  writeBackup: () => call<BackupDone | null>("write_backup"),
  chooseBackup: () => call<StagedBackup | null>("choose_backup"),
  checkBackup: (password: string) => call<BackupCheckView>("check_backup", { password }),
  restoreBackup: (password: string | null, recoveryKey: string | null) =>
    call<AppStatus>("restore_backup", { password, recoveryKey }),
  forgetBackup: () => run("forget_backup"),
  setAutoBackup: (on: boolean) => call<BackupStatus>("set_auto_backup", { on }),

  /** A newer version, signed by the developer (D-033); `null` when this is the newest. */
  checkUpdate: () => call<UpdateView | null>("check_update"),
  /** Download, verify, lock, run the installer; the program closes and opens again. */
  installUpdate: () => run("install_update"),
};

export type {
  UpdateView,
  UsageSummary,
  Readiness,
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
  NameMatch,
  MaterialRouting,
  SortResult,
  CaseDetail,
  CaseInput,
  CaseMeta,
  CaseSummary,
  ChatView,
  ExportCheck,
  Identity,
  IdentityInput,
  ImportPreview,
  InputKind,
  Instrument,
  ParagraphVersionView,
  PingResponse,
  Prepared,
  ReportSettings,
  FollowUpView,
  ScoreSheet,
  SectionResult,
  SuspectDecision,
  UiError,
};
