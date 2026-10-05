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
import type { GoogleStatus } from "./generated/GoogleStatus";
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
import type { Role } from "./generated/Role";
import type { ScoreSheet } from "./generated/ScoreSheet";
import type { UsageSummary } from "./generated/UsageSummary";
import type { Readiness } from "./generated/Readiness";
import type { UnsavedEdit } from "./generated/UnsavedEdit";
import type { SectionResult } from "./generated/SectionResult";
import type { SortResult } from "./generated/SortResult";
import type { SourceExcerpt } from "./generated/SourceExcerpt";
import type { MaterialRouting } from "./generated/MaterialRouting";
import type { SuspectDecision } from "./generated/SuspectDecision";
import type { FollowUpView } from "./generated/FollowUpView";
import type { UiError } from "./generated/UiError";
import type { UpdateView } from "./generated/UpdateView";
import type { StyleAnalysisResult } from "./generated/StyleAnalysisResult";
import type { StyleImportPreview } from "./generated/StyleImportPreview";
import type { StyleItem } from "./generated/StyleItem";
import type { StyleKind } from "./generated/StyleKind";
import type { StyleOverview } from "./generated/StyleOverview";
import type { StyleProfile } from "./generated/StyleProfile";
import type { StyleProfileView } from "./generated/StyleProfileView";
import type { StyleSourceView } from "./generated/StyleSourceView";

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
  /** Windows Hello: her PIN, face or fingerprint (D-047). */
  unlockWithHello: () => call<AppStatus>("unlock_with_hello"),
  setWindowsHello: (on: boolean, password: string) => call<AppStatus>("set_windows_hello", { on, password }),
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

  /** `retentionAck`: she read what ChatGPT or Gemini keeps (required for their keys; D-040). */
  setApiKey: (provider: string, key: string, retentionAck = false) => run("set_api_key", { provider, key, retentionAck }),
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
  /** "למה כתבת את זה?": the passages a paragraph leans on, best match first (stays local). */
  paragraphSources: (caseId: string, draftId: string) =>
    call<SourceExcerpt[]>("paragraph_sources", { caseId, draftId }),
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
  /** "להחזיר" on the summary card: keep what the filter hid as written, for this case. */
  restoreAutoHidden: (caseId: string, token: string, tag: string) =>
    run("restore_auto_hidden", { caseId, token, tag }),
  /** Who a name the filter kept is; it gets a tag for that role. */
  changeRole: (caseId: string, tag: string, role: Role) => call<Identity[]>("change_role", { caseId, tag, role }),

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
  /** Earlier wordings of a paragraph, newest first; bring one back (D-046). */
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

  /** Forgot the password → sign in with Google (D-041). Readable while locked. */
  googleStatus: () => call<GoogleStatus>("google_status"),
  /** Opens Google in her browser; resolves when she is back and the slot is written. */
  googleTurnOn: (password: string) => run("google_turn_on", { password }),
  googleTurnOff: (password: string) => run("google_turn_off", { password }),
  /** Stop a sign-in that waits for the browser. */
  googleCancel: () => run("google_cancel"),
  /** Locked, password forgotten: sign in with Google, open, and set `newPassword` at once. */
  googleRecover: (newPassword: string) => call<AppStatus>("google_recover", { newPassword }),

  // Encrypted backup (D-024). The system's own window picks the file; `null` = cancelled.
  backupStatus: () => call<BackupStatus>("backup_status"),
  writeBackup: () => call<BackupDone | null>("write_backup"),
  chooseBackup: () => call<StagedBackup | null>("choose_backup"),
  checkBackup: (password: string) => call<BackupCheckView>("check_backup", { password }),
  restoreBackup: (password: string | null, recoveryKey: string | null) =>
    call<AppStatus>("restore_backup", { password, recoveryKey }),
  forgetBackup: () => run("forget_backup"),
  setAutoBackup: (on: boolean) => call<BackupStatus>("set_auto_backup", { on }),

  // Writing style (D-043): past reports kept neutralized, the profile, learning from edits.
  styleOverview: () => call<StyleOverview>("style_overview"),
  /** A past report: the bytes go as the raw body. Nothing is kept until `saveStyleSource`. */
  importStyleSource: async (file: File): Promise<StyleImportPreview> => {
    const bytes = new Uint8Array(await file.arrayBuffer());
    try {
      return await invoke<StyleImportPreview>("import_style_source", bytes, {
        headers: { "x-file-name": encodeURIComponent(file.name) },
      });
    } catch (e) {
      throw asUiError(e);
    }
  },
  saveStyleSource: (token: string, included: number[], title: string | null) =>
    call<StyleSourceView>("save_style_source", { token, included, title }),
  discardStyleUpload: () => run("discard_style_upload"),
  deleteStyleSource: (id: string) => run("delete_style_source", { id }),
  prepareStyleAnalysis: (sourceId: string) => call<Prepared>("prepare_style_analysis", { sourceId }),
  sendStyleAnalysis: (approvalId: string) => call<StyleAnalysisResult>("send_style_analysis", { approvalId }),
  prepareStyleProfile: () => call<Prepared>("prepare_style_profile"),
  sendStyleProfile: (approvalId: string) => call<StyleProfileView>("send_style_profile", { approvalId }),
  saveStyleDraft: (profile: StyleProfile) => call<StyleProfileView>("save_style_draft", { profile }),
  approveStyleDraft: () => call<StyleProfileView>("approve_style_draft"),
  discardStyleDraft: () => run("discard_style_draft"),
  restoreStyleVersion: (id: string) => call<StyleProfileView>("restore_style_version", { id }),
  setStyleEnabled: (on: boolean) => run("set_style_enabled", { on }),
  resetStyle: () => run("reset_style"),
  acceptStyleSuggestion: (id: string) => call<StyleProfileView>("accept_style_suggestion", { id }),
  dismissStyleSuggestion: (id: string) => run("dismiss_style_suggestion", { id }),

  /** A newer version, signed by the developer (D-033); `null` when this is the newest. */
  checkUpdate: () => call<UpdateView | null>("check_update"),
  /** In the background: fetch and verify the newest version. `true` when it waits for her click. */
  prepareUpdate: () => call<boolean>("prepare_update"),
  /** Lock, run the verified installer (fetched now if it is not waiting); closes and opens again. */
  installUpdate: () => run("install_update"),
};

export type {
  StyleAnalysisResult,
  StyleImportPreview,
  StyleItem,
  StyleKind,
  StyleOverview,
  StyleProfile,
  StyleProfileView,
  StyleSourceView,
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
  GoogleStatus,
  NameMatch,
  MaterialRouting,
  SortResult,
  SourceExcerpt,
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
