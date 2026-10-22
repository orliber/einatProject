import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { mockIPC } from "@tauri-apps/api/mocks";
import { SetupScreen } from "./SetupScreen";
import { status } from "../test/fixtures";
import { a11yViolations } from "../test/a11y";

// An invented kit, never a real one.
const KEY = "AAAA-BBBB-CCCC-DDDD";

async function toAfterKit(googleAvailable: boolean) {
  const calls: string[] = [];
  mockIPC((cmd) => {
    calls.push(cmd);
    if (cmd === "google_status") return { available: googleAvailable, on: false, needs_setup_here: false };
    if (cmd === "create_vault") return { recovery_key: KEY };
    if (cmd === "confirm_recovery_key") return true;
    return null;
  });
  const user = userEvent.setup();
  render(<SetupScreen status={status({ vault_exists: false, unlocked: false })} onDone={async () => undefined} />);
  await user.type(screen.getByLabelText("סיסמה"), "כלב ירוק רץ מהר");
  await user.type(screen.getByLabelText("שוב, לאימות"), "כלב ירוק רץ מהר");
  await user.click(screen.getByRole("button", { name: "יצירת הכספת" }));
  await user.type(await screen.findByLabelText(/הקלידי אותה מהדף/), KEY);
  await user.click(screen.getByRole("button", { name: "המשך" }));
  return { user, calls };
}

describe("first run: Google for a forgotten password (D-096)", () => {
  it("comes right after the kit, and 'אחר כך' skips it", async () => {
    const { user, calls } = await toAfterKit(true);
    expect(await screen.findByRole("heading", { name: "להפעיל כניסה עם גוגל כששוכחים סיסמה" })).toBeInTheDocument();
    expect(screen.getByRole("list", { name: "שלבי ההגדרה" })).toHaveTextContent("כניסה עם גוגל");
    expect(await a11yViolations(document.body)).toEqual([]);
    await user.click(screen.getByRole("button", { name: "אחר כך" }));
    expect(await screen.findByRole("heading", { name: "הפרטים שלך" })).toBeInTheDocument();
    expect(calls).not.toContain("google_turn_on");
  });

  it("turns it on with the same dialog as the settings, then goes on", async () => {
    const { user, calls } = await toAfterKit(true);
    await user.click(await screen.findByRole("button", { name: "הפעלה…" }));
    await user.type(screen.getByLabelText("הסיסמה הנוכחית של הכספת"), "כלב ירוק רץ מהר");
    await user.click(screen.getByRole("button", { name: "המשך לגוגל" }));
    expect(await screen.findByRole("heading", { name: "הפרטים שלך" })).toBeInTheDocument();
    expect(screen.getByText(/הכניסה עם גוגל הופעלה/)).toBeInTheDocument();
    expect(calls).toContain("google_turn_on");
  });

  it("is not there where this version has no Google sign-in", async () => {
    await toAfterKit(false);
    expect(await screen.findByRole("heading", { name: "הפרטים שלך" })).toBeInTheDocument();
    expect(screen.getByRole("list", { name: "שלבי ההגדרה" })).not.toHaveTextContent("גוגל");
  });
});
