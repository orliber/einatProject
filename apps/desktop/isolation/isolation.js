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
  "approve_section",
  "approve_style_draft",
  "backup_status",
  "case_detail",
  "change_password",
  "chat",
  "check_backup",
  "check_export",
  "check_update",
  "choose_backup",
  "confirm_readiness",
  "confirm_recovery_key",
  "consultation",
  "consultations",
  "copy_secret",
  "create_case",
  "create_folder",
  "create_follow_up",
  "create_vault",
  "decide_suspect",
  "delete_case",
  "delete_consultation",
  "delete_folder",
  "delete_input",
  "delete_style_source",
  "discard_style_draft",
  "discard_style_upload",
  "dismiss_style_suggestion",
  "edit_paragraph",
  "export_report",
  "find_name_matches",
  "folders",
  "follow_up",
  "forget_backup",
  "hold_unsaved",
  "import_document",
  "import_style_source",
  "install_update",
  "keep_case_longer",
  "list_cases",
  "list_trash",
  "lock",
  "mark_activity_reviewed",
  "move_case",
  "move_folder",
  "new_recovery_kit",
  "ping",
  "prepare_consult",
  "prepare_full_draft",
  "prepare_section",
  "prepare_sort",
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
  "reset_style",
  "restore_backup",
  "restore_case",
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
  "set_api_key",
  "set_auto_backup",
  "set_identities",
  "set_input_sections",
  "set_lock_minutes",
  "set_model",
  "set_monthly_cap",
  "set_practitioner",
  "set_report_settings",
  "set_review_only_suspect",
  "set_screen_protection",
  "set_speed",
  "set_style_enabled",
  "set_windows_hello",
  "style_overview",
  "take_unsaved",
  "touch",
  "unlock",
  "unlock_with_hello",
  "unlock_with_recovery",
  "update_case",
  "update_input",
  "usage_summary",
  "write_backup",
]);

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
  const ok =
    payload !== null &&
    typeof payload === "object" &&
    typeof payload.cmd === "string" &&
    ALLOWED.has(payload.cmd) &&
    (isBytes(payload.payload) || plain(payload.payload === undefined ? null : payload.payload, 0));
  if (!ok) {
    payload.cmd = REFUSED;
    payload.payload = {};
  }
  return payload;
};
