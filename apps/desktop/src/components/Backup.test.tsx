import { fireEvent, render, screen } from "@testing-library/react";
import { mockIPC } from "@tauri-apps/api/mocks";
import { RestoreFromBackup } from "./Backup";
import { daysAgo } from "../i18n/he";

describe("backup", () => {
  it("says how long ago in plain words, as the core says it (D-107)", () => {
    // The same table as `days_ago_reads_as_she_would_say_it` in dv-core `safety_net.rs`.
    const table: [number, string][] = [
      [0, "היום"], [-3, "היום"], [1, "אתמול"], [2, "לפני יומיים"], [3, "לפני 3 ימים"], [9, "לפני 9 ימים"], [120, "לפני 120 ימים"],
    ];
    for (const [days, words] of table) expect(daysAgo(days)).toBe(words);
  });

  it("restores on a new computer: file first (with its date), then the recovery kit", async () => {
    const calls: [string, unknown][] = [];
    mockIPC((cmd, args) => {
      calls.push([cmd, args]);
      if (cmd === "choose_backup") return { file_name: "גיבוי.vaultbak", created_at: 1_790_683_200, same_vault: null };
      if (cmd === "restore_backup") return { unlocked: true };
      return null;
    });
    const restored = vi.fn();
    render(<RestoreFromBackup onRestored={restored} onBack={() => undefined} />);
    fireEvent.click(screen.getByRole("button", { name: "בחירת קובץ הגיבוי…" }));
    expect(await screen.findByText("גיבוי.vaultbak")).toBeInTheDocument();
    expect(screen.getByText(/גיבוי מ-/)).toBeInTheDocument();

    fireEvent.click(screen.getByRole("button", { name: /ערכת השחזור/ }));
    fireEvent.change(screen.getByLabelText(/ערכת השחזור/), { target: { value: "AAAA-BBBB" } });
    fireEvent.click(screen.getByRole("button", { name: "שחזור ופתיחה" }));
    await vi.waitFor(() => expect(restored).toHaveBeenCalled());
    expect(calls.find(([c]) => c === "restore_backup")?.[1]).toMatchObject({ password: null, recoveryKey: "AAAA-BBBB" });
  });
});
