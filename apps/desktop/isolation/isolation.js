// Tauri isolation app (D-047, S-4). Every message from the window to the core passes through
// here, in a separate sandboxed frame the window's own scripts cannot reach, and is then
// encrypted for the core. A command that is not one of the app's own, or arguments that are
// not plain data or a document's bytes, are turned into a command that does not exist, so the core refuses it and
// the call fails instead of hanging.
//
// Must list exactly the commands in `src-tauri/build.rs` (`cargo xtask check-invariants`).
"use strict";

const ALLOWED = new Set([
  "accept_style_suggestion",
  "activity",
  "add_comparison_material",
  "add_input",
  "add_own_paragraph",
  "app_status",
  "approve_paragraph",
  "approve_clean_paragraphs",
  "approve_paragraphs",
  "approve_section",
  "approve_style_draft",
  "backup_status",
  "case_detail",
  "change_password",
  "change_role",
  "chat",
  "check_backup",
  "check_export",
  "check_original",
  "check_update",
  "choose_backup",
  "clear_report_template",
  "clear_report_logo",
  "confirm_readiness",
  "safety_net",
  "instrument_approvals",
  "set_instrument_status",
  "prepare_verify",
  "security_status",
  "export_security_pdf",
  "send_verify",
  "report_check_setting",
  "set_report_check",
  "prepare_report_check",
  "send_report_check",
  "set_send_stopped",
  "confirm_recovery_key",
  "consultation",
  "consultations",
  "copy_secret",
  "create_case",
  "create_sample_case",
  "create_folder",
  "create_follow_up",
  "create_vault",
  "decide_suspect",
  "settle_foreign_tags",
  "delete_case",
  "delete_consultation",
  "delete_folder",
  "delete_input",
  "restore_input",
  "delete_style_source",
  "discard_style_draft",
  "discard_style_upload",
  "download_name_model",
  "dismiss_style_suggestion",
  "drive_backup_status",
  "drive_backup_turn_off",
  "drive_backup_turn_on",
  "edit_paragraph",
  "export_letter",
  "export_pdf",
  "export_report",
  "find_name_matches",
  "folders",
  "follow_up",
  "forget_backup",
  "google_cancel",
  "google_recover",
  "google_status",
  "google_turn_off",
  "google_turn_on",
  "hold_unsaved",
  "import_document",
  "import_style_source",
  "install_update",
  "old_exports",
  "open_old_exports",
  "open_releases_page",
  "system_check",
  "keep_case_longer",
  "letter",
  "list_cases",
  "list_trash",
  "lock",
  "mark_activity_reviewed",
  "move_case",
  "move_folder",
  "name_model_progress",
  "name_model_status",
  "new_recovery_kit",
  "retire_recovery_kit",
  "paragraph_sources",
  "paragraph_versions",
  "similar_paragraph",
  "section_library",
  "own_section_help",
  "ping",
  "recent_crashes",
  "prepare_consult",
  "prepare_from_points",
  "prepare_full_draft",
  "prepare_letter",
  "prepare_section",
  "prepare_sort",
  "prepare_sort_one",
  "prepare_sort_parts",
  "prepare_style_analysis",
  "prepare_style_profile",
  "prepare_update",
  "preview_filter",
  "preview_scores",
  "print_page",
  "purge_case",
  "readiness",
  "reject_paragraph",
  "rename_folder",
  "report_settings",
  "report_template",
  "report_logo",
  "reset_style",
  "forget_style_learning",
  "restore_auto_hidden",
  "confirm_auto_hidden",
  "restore_backup",
  "restore_case",
  "restore_paragraph_version",
  "restore_style_version",
  "retention_due",
  "save_scores",
  "save_style_draft",
  "save_style_source",
  "score_instruments",
  "score_sheet",
  "send_consult",
  "send_progress",
  "send_section",
  "send_sort",
  "send_style_analysis",
  "send_style_profile",
  "prepare_interpretations",
  "send_interpretations",
  "update_interpretation",
  "delete_interpretation",
  "sent_history",
  "sent_count",
  "sent_text",
  "set_api_key",
  "set_auto_backup",
  "set_case_guide",
  "set_dsm_severity",
  "set_letter_left_out",
  "set_identities",
  "set_input_sections",
  "set_lock_minutes",
  "set_model",
  "set_monthly_cap",
  "set_name_model",
  "set_practitioner",
  "set_report_settings",
  "set_report_template",
  "set_report_logo",
  "set_review_always",
  "set_screen_protection",
  "section_modes",
  "set_section_guide",
  "set_section_mode",
  "set_speed",
  "set_style_enabled",
  "set_verify_stage",
  "style_overview",
  "take_unsaved",
  "touch",
  "unlock",
  "unlock_with_recovery",
  "update_case",
  "update_input",
  "usage_summary",
  "verify_stage_on",
  "write_backup",
]);

// The window may also listen to the core's one event, and to no other (D-060). Must match the
// two `core:event:` grants in `src-tauri/capabilities/main.json`.
const EVENT_COMMANDS = new Set(["plugin:event|listen", "plugin:event|unlisten"]);
const CORE_EVENT = "core";

const REFUSED = "refused_by_isolation";

/** Plain data only: no functions, no prototypes other than Object/Array, bounded depth. */
function plain(value, depth) {
  if (depth > 32) return false;
  if (value === null) return true;
  const t = typeof value;
  if (t === "string" || t === "number" || t === "boolean") return true;
  if (t !== "object") return false;
  if (Array.isArray(value)) return value.every((v) => plain(v, depth + 1));
  const proto = Object.getPrototypeOf(value);
  if (proto !== Object.prototype && proto !== null) return false;
  return Object.keys(value).every((k) => plain(value[k], depth + 1));
}

/** A document's bytes, sent as the raw body (`import_document`, `import_style_source`). */
function isBytes(value) {
  return value instanceof Uint8Array || value instanceof ArrayBuffer;
}

window.__TAURI_ISOLATION_HOOK__ = (payload) => {
  const command =
    payload !== null &&
    typeof payload === "object" &&
    typeof payload.cmd === "string" &&
    ALLOWED.has(payload.cmd) &&
    (isBytes(payload.payload) || plain(payload.payload === undefined ? null : payload.payload, 0));
  const event =
    payload !== null &&
    typeof payload === "object" &&
    typeof payload.cmd === "string" &&
    EVENT_COMMANDS.has(payload.cmd) &&
    payload.payload !== null &&
    typeof payload.payload === "object" &&
    payload.payload.event === CORE_EVENT &&
    plain(payload.payload, 0);
  const ok = command || event;
  if (!ok) {
    payload.cmd = REFUSED;
    payload.payload = {};
  }
  return payload;
};
