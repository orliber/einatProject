import { act, fireEvent, render, screen } from "@testing-library/react";
import { mockIPC } from "@tauri-apps/api/mocks";
import { LockScreen } from "./LockScreen";
import type { AppStatus } from "../ipc/client";
import { a11yViolations } from "../test/a11y";

/**
 * D-104: a newer version opened the vault. The lock screen says so before anything is typed,
 * offers the new version, and never asks for the password or the recovery kit.
 */

const PING = { ipc_version: 1, core_version: "0.9.0", build_commit: "test", fips_active: false, platform: "macos" };
const NEWS = "הכספת נפתחה בגרסה חדשה יותר (0.9.2). צריך להתקין את הגרסה החדשה.";

function locked(extra: Partial<AppStatus> = {}): AppStatus {
  return {
    vault_exists: true, unlocked: false, disk_encryption: "on", cloud_synced_folder: null, fips_active: false,
    demo_mode: true, model: "", provider: "anthropic", ai_name: "Claude", keys: [], speed: "balanced",
    integrity_warning: null, lock_minutes: 10, idle_lock_in: null, practitioner: [], review_always: true,
    screen_protection: true, since_last: null, ...extra,
  };
}

// The update found is kept for the whole program (components/Update.tsx), so the order of the
// tests below matters: none found, one that does not install here, then one that does.
const UPDATE = {
  version: "0.9.2", current: "0.9.0", published: "2026-10-27", notes: ["עדכון בדוי"], size_mb: 14, can_install: true,
};

describe("a vault a newer version opened", () => {
  it("says to install the new version, instead of the password and the kit", async () => {
    mockIPC((cmd) => {
      if (cmd === "ping") return PING;
      if (cmd === "google_status") return { on: true, available: true };
      return null;
    });
    const { container } = render(<LockScreen status={locked({ newer_version: "0.9.2" })} />);
    expect(screen.getByText(NEWS)).toBeInTheDocument();
    expect(screen.getByText(/לא צריך את ערכת השחזור/)).toBeInTheDocument();
    expect(screen.queryByLabelText("סיסמה")).toBeNull();
    expect(screen.queryByLabelText("ערכת השחזור")).toBeNull();
    expect(screen.queryByRole("button", { name: "שכחתי את הסיסמה" })).toBeNull();
    expect(screen.queryByRole("button", { name: /ערכת השחזור/ })).toBeNull();
    expect(screen.getByRole("button", { name: "להתקנת הגרסה החדשה" })).toBeInTheDocument();
    await act(async () => {});
    expect(await a11yViolations(container)).toEqual([]);
  });

  it("where it does not install by itself, points to the releases page", async () => {
    const asked: string[] = [];
    mockIPC((cmd) => {
      asked.push(cmd);
      if (cmd === "ping") return PING;
      if (cmd === "google_status") return { on: false };
      if (cmd === "check_update") return { ...UPDATE, version: "0.9.3", can_install: false };
      return null;
    });
    render(<LockScreen status={locked({ newer_version: "" })} />);
    expect(screen.getByText("הכספת נפתחה בגרסה חדשה יותר. צריך להתקין את הגרסה החדשה.")).toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "להתקנת הגרסה החדשה" }));
    expect(await screen.findByText(/מורידים אותה מדף הגרסאות/)).toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "לדף הגרסאות (נפתח בדפדפן)" }));
    await act(async () => {});
    expect(asked).toContain("open_releases_page");
  });

  it("finds the new version and opens the same signed update as everywhere", async () => {
    const asked: string[] = [];
    mockIPC((cmd) => {
      asked.push(cmd);
      if (cmd === "ping") return PING;
      if (cmd === "google_status") return { on: false };
      if (cmd === "check_update") return UPDATE;
      if (cmd === "prepare_update") return true;
      return null;
    });
    render(<LockScreen status={locked({ newer_version: "0.9.2" })} />);
    fireEvent.click(screen.getByRole("button", { name: "להתקנת הגרסה החדשה" }));
    expect(await screen.findByRole("dialog", { name: /גרסה 0\.9\.2 מוכנה/ })).toBeInTheDocument();
    expect(asked).toContain("check_update");
    expect(asked).not.toContain("unlock");
    expect(asked).not.toContain("unlock_with_recovery");
  });

  it("when the opening itself says so, the form turns into the same message, not the kit", async () => {
    mockIPC((cmd) => {
      if (cmd === "ping") return PING;
      if (cmd === "google_status") return { on: false };
      if (cmd === "unlock") throw { code: "newer_version", message: NEWS, details: [] };
      return null;
    });
    render(<LockScreen status={locked()} />);
    fireEvent.change(screen.getByLabelText("סיסמה"), { target: { value: "סיסמה בדויה לגמרי" } });
    fireEvent.click(screen.getByRole("button", { name: "פתיחה" }));
    expect(await screen.findByText("הכספת נפתחה בגרסה חדשה יותר. צריך להתקין את הגרסה החדשה.")).toBeInTheDocument();
    expect(screen.queryByLabelText("סיסמה")).toBeNull();
    expect(screen.queryByLabelText("ערכת השחזור")).toBeNull();
    expect(screen.queryByText(/הכספת לא נפתחה בגרסה הזו/)).toBeNull();
  });
});
