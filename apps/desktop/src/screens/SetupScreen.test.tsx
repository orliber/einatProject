import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { mockIPC } from "@tauri-apps/api/mocks";
import { SetupScreen } from "./SetupScreen";
import { status } from "../test/fixtures";
import { a11yViolations } from "../test/a11y";

async function toAfterPassword(googleAvailable: boolean, backup: unknown = { path: "E:\\גיבוי בדוי.vaultbak", created_at: 1_790_000_000 }) {
  const calls: string[] = [];
  mockIPC((cmd) => {
    calls.push(cmd);
    if (cmd === "google_status") return { available: googleAvailable, on: false, needs_setup_here: false };
    if (cmd === "create_vault") return null;
    if (cmd === "write_backup") return backup;
    return null;
  });
  const user = userEvent.setup();
  render(<SetupScreen status={status({ vault_exists: false, unlocked: false })} onDone={async () => undefined} />);
  await user.type(screen.getByLabelText("סיסמה"), "כלב ירוק רץ מהר");
  await user.type(screen.getByLabelText("שוב, לאימות"), "כלב ירוק רץ מהר");
  await user.click(screen.getByRole("button", { name: "יצירת הכספת" }));
  return { user, calls };
}

describe("first run: Google for a forgotten password (D-096)", () => {
  it("comes right after the password (no printed kit, D-103), and 'אחר כך' skips it", async () => {
    const { user, calls } = await toAfterPassword(true);
    expect(await screen.findByRole("heading", { name: "להפעיל כניסה עם גוגל כששוכחים סיסמה" })).toBeInTheDocument();
    expect(screen.getByRole("list", { name: "שלבי ההגדרה" })).toHaveTextContent("כניסה עם גוגל");
    expect(screen.getByRole("list", { name: "שלבי ההגדרה" })).not.toHaveTextContent("ערכת שחזור");
    expect(calls).not.toContain("confirm_recovery_key");
    expect(await a11yViolations(document.body)).toEqual([]);
    await user.click(screen.getByRole("button", { name: "אחר כך" }));
    expect(await screen.findByRole("heading", { name: "גיבוי לדיסק נייד" })).toBeInTheDocument();
    expect(calls).not.toContain("google_turn_on");
  });

  it("turns it on with the same dialog as the settings, then goes on", async () => {
    const { user, calls } = await toAfterPassword(true);
    await user.click(await screen.findByRole("button", { name: "הפעלה…" }));
    await user.type(screen.getByLabelText("הסיסמה הנוכחית של הכספת"), "כלב ירוק רץ מהר");
    await user.click(screen.getByRole("button", { name: "המשך לגוגל" }));
    expect(await screen.findByRole("heading", { name: "גיבוי לדיסק נייד" })).toBeInTheDocument();
    expect(screen.getByText(/הכניסה עם גוגל הופעלה/)).toBeInTheDocument();
    expect(calls).toContain("google_turn_on");
  });

  it("is not there where this version has no Google sign-in", async () => {
    await toAfterPassword(false);
    expect(await screen.findByRole("heading", { name: "גיבוי לדיסק נייד" })).toBeInTheDocument();
    expect(screen.getByRole("list", { name: "שלבי ההגדרה" })).not.toHaveTextContent("גוגל");
  });
});

describe("first run: a first backup to a removable drive (D-103)", () => {
  it("says what it is for and what skipping costs; a saved backup goes on to her details", async () => {
    const { user, calls } = await toAfterPassword(false);
    expect(await screen.findByRole("heading", { name: "גיבוי לדיסק נייד" })).toBeInTheDocument();
    expect(screen.getByText(/בלי גיבוי, מחשב שהתקלקל = אין דרך לפתוח את התיקים/)).toBeInTheDocument();
    expect(screen.getByRole("list", { name: "שלבי ההגדרה" })).toHaveTextContent("גיבוי");
    expect(await a11yViolations(document.body)).toEqual([]);
    await user.click(screen.getByRole("button", { name: "בחירת מקום וגיבוי ראשון…" }));
    expect(await screen.findByRole("heading", { name: "הפרטים שלך" })).toBeInTheDocument();
    expect(screen.getByText(/הגיבוי הראשון נשמר/)).toBeInTheDocument();
    expect(calls).toContain("write_backup");
  });

  it("a cancelled \"Save as\" stays on the step", async () => {
    const { user } = await toAfterPassword(false, null);
    await user.click(await screen.findByRole("button", { name: "בחירת מקום וגיבוי ראשון…" }));
    expect(await screen.findByRole("heading", { name: "גיבוי לדיסק נייד" })).toBeInTheDocument();
  });
});
