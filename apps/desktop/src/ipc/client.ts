// The only module that talks to the Rust core. Types come from Rust via ts-rs.
import { invoke } from "@tauri-apps/api/core";
import type { AppStatus } from "./generated/AppStatus";
import type { CaseDetail } from "./generated/CaseDetail";
import type { CaseInput } from "./generated/CaseInput";
import type { CaseMeta } from "./generated/CaseMeta";
import type { CaseSummary } from "./generated/CaseSummary";
import type { ChatView } from "./generated/ChatView";
import type { ConsultResult } from "./generated/ConsultResult";
import type { CreatedVault } from "./generated/CreatedVault";
import type { ExportCheck } from "./generated/ExportCheck";
import type { FilterOutcome } from "./generated/FilterOutcome";
import type { Identity } from "./generated/Identity";
import type { IdentityInput } from "./generated/IdentityInput";
import type { ImportPreview } from "./generated/ImportPreview";
import type { InputKind } from "./generated/InputKind";
import type { Instrument } from "./generated/Instrument";
import type { PingResponse } from "./generated/PingResponse";
import type { Prepared } from "./generated/Prepared";
import type { ReportSettings } from "./generated/ReportSettings";
import type { ScoreSheet } from "./generated/ScoreSheet";
import type { SectionResult } from "./generated/SectionResult";
import type { SortResult } from "./generated/SortResult";
import type { MaterialRouting } from "./generated/MaterialRouting";
import type { SuspectDecision } from "./generated/SuspectDecision";
import type { UiError } from "./generated/UiError";

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

  setApiKey: (key: string) => run("set_api_key", { key }),
  setModel: (model: string) => run("set_model", { model }),
  setLockMinutes: (minutes: number) => run("set_lock_minutes", { minutes }),
  setPractitioner: (names: string[]) => run("set_practitioner", { names }),
  setReviewOnlySuspect: (on: boolean) => run("set_review_only_suspect", { on }),
  reportSettings: () => call<ReportSettings>("report_settings"),
  setReportSettings: (settings: ReportSettings) => run("set_report_settings", { settings }),

  listCases: () => call<CaseSummary[]>("list_cases"),
  createCase: (meta: CaseMeta, identities: IdentityInput[]) => call<string>("create_case", { meta, identities }),
  updateCase: (caseId: string, meta: CaseMeta) => run("update_case", { caseId, meta }),
  deleteCase: (caseId: string) => run("delete_case", { caseId }),
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

  prepareSection: (caseId: string, sectionKey: string, instruction: string) =>
    call<Prepared>("prepare_section", { caseId, sectionKey, instruction }),
  prepareFullDraft: (caseId: string) => call<[string, Prepared][]>("prepare_full_draft", { caseId }),
  sendSection: (approvalId: string) => call<SectionResult>("send_section", { approvalId }),
  /** D-022: sort every material not sorted yet into sections (one review screen). */
  prepareSort: (caseId: string) => call<Prepared>("prepare_sort", { caseId }),
  sendSort: (approvalId: string) => call<SortResult>("send_sort", { approvalId }),
  /** Einat's choice of sections for one material; it always wins. */
  setInputSections: (caseId: string, inputId: string, sections: string[]) =>
    run("set_input_sections", { caseId, inputId, sections }),
  chat: (caseId: string, sectionKey: string) => call<ChatView[]>("chat", { caseId, sectionKey }),
  approveParagraph: (caseId: string, draftId: string) => run("approve_paragraph", { caseId, draftId }),
  rejectParagraph: (caseId: string, draftId: string) => run("reject_paragraph", { caseId, draftId }),
  editParagraph: (caseId: string, draftId: string, text: string) =>
    run("edit_paragraph", { caseId, draftId, text }),
  addOwnParagraph: (caseId: string, sectionKey: string, text: string) =>
    run("add_own_paragraph", { caseId, sectionKey, text }),

  prepareConsult: (caseId: string | null, message: string) => call<Prepared>("prepare_consult", { caseId, message }),
  sendConsult: (approvalId: string) => call<ConsultResult>("send_consult", { approvalId }),

  checkExport: (caseId: string) => call<ExportCheck>("check_export", { caseId }),
  exportReport: (caseId: string, password: string | null) => call<string>("export_report", { caseId, password }),
};

export type {
  AppStatus,
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
  PingResponse,
  Prepared,
  ReportSettings,
  ScoreSheet,
  SectionResult,
  SuspectDecision,
  UiError,
};
