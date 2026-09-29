import { fireEvent, render, screen } from "@testing-library/react";
import { mockIPC } from "@tauri-apps/api/mocks";
import { RestoreFromBackup, daysAgo } from "./Backup";

describe("backup", () => {
  it("says how long ago in plain words", () => {
    expect(daysAgo(0)).toBe("היום");
    expect(daysAgo(1)).toBe("אתמול");
    expect(daysAgo(9)).toBe("לפני 9 ימים");
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
