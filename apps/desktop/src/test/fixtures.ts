// Fabricated data for UI tests.
import type { AppStatus, Prepared } from "../ipc/client";

export const status = (over: Partial<AppStatus> = {}): AppStatus => ({
  vault_exists: true,
  unlocked: true,
  disk_encryption: "on",
  cloud_synced_folder: null,
  screen_protection: true,
  fips_active: false,
  demo_mode: true,
  model: "claude-opus-5",
  provider: "anthropic",
  ai_name: "Claude",
  keys: [],
  speed: "balanced",
  integrity_warning: null,
  lock_minutes: 10,
  practitioner: ["רותם בדויה"],
  review_only_suspect: false,
  review_choice_available: false,
  ...over,
});

export const prepared = (over: Partial<Prepared> = {}): Prepared => ({
  approval_id: "abc",
  parts: [
    {
      label: "S1 · שיחה עם הגננת",
      original: [
        { text: "נועם", mark: "replaced", label: "שם הילד/ה" },
        { text: " משחק עם ", mark: null, label: null },
        { text: "יובל", mark: "suspect", label: "שם לא מוכר" },
      ],
      outgoing: [
        { text: "[ילד]", mark: "tag", label: "שם הילד/ה" },
        { text: " משחק עם ", mark: null, label: null },
        { text: "יובל", mark: "suspect", label: "שם לא מוכר" },
      ],
    },
  ],
  suspects: [],
  hidden: ["שם הילד/ה"],
  checks: { declared_names: 1, patterns: 0, name_suspects: 0, indirect_suspects: 0 },
  blocked: [],
  demo_mode: true,
  ...over,
});
