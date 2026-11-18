// A message to the family (D-201): the toast after the chat opened names the contact as the
// screen may show it now, initials in discreet mode (D-192, D-204), and never carries the
// message. The names are invented; the chat is never really opened (mocked).
import { configure, render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { forgetDiscreet, setDiscreet } from "../../discreet";
import { appApi, Host, serve, type Override } from "../../test/host";
import { SendMenu } from "./SendMenu";

configure({ asyncUtilTimeout: 5000 });

afterEach(() => {
  forgetDiscreet();
});

/** The chat opens with an invented parent; the window never sees the text. */
const opened: Override = (cmd) => (cmd === "send_message" ? { to: "יעל לדוגמה", clears_in: 20 } : undefined);

async function show() {
  const served = await serve(opened);
  const notes: string[] = [];
  const app = appApi({ notify: (text: string) => void notes.push(text) });
  const user = userEvent.setup();
  render(<Host app={app}><SendMenu kind="reminder" patientId="p-invented" visitId="v-invented" label="תזכורת" /></Host>);
  return { ...served, notes, user };
}

describe("a message to the family", () => {
  it("in discreet mode the WhatsApp toast names the contact by initials, and never carries the text", async () => {
    setDiscreet(true);
    const { user, notes, calls } = await show();
    await user.click(screen.getByRole("button", { name: "תזכורת" }));
    await user.click(await screen.findByRole("menuitem", { name: "לשלוח בוואטסאפ" }));
    await waitFor(() => expect(notes).toHaveLength(1));
    expect(notes[0]).toBe("וואטסאפ נפתח מול י. ל. התזכורת בלוח המוגן: מדביקים (Ctrl+V) ושולחים. יימחק מהלוח בעוד 20 שניות.");
    expect(notes[0]).not.toContain("יעל");
    expect(notes[0]).not.toContain("לדוגמה");
    // The window asked for the chat only: it never read the message, nor copied it itself.
    expect(calls.filter((c) => c.cmd === "send_message")).toHaveLength(1);
    expect(calls.some((c) => c.cmd === "copy_secret" || (c.cmd === "clinic" && (c.args.request as { op?: string }).op === "message"))).toBe(false);
  });

  it("outside discreet mode the toast says the contact's name, as before", async () => {
    const { user, notes } = await show();
    await user.click(screen.getByRole("button", { name: "תזכורת" }));
    await user.click(await screen.findByRole("menuitem", { name: "לשלוח בוואטסאפ" }));
    await waitFor(() => expect(notes).toHaveLength(1));
    expect(notes[0]).toBe("וואטסאפ נפתח מול יעל לדוגמה. התזכורת בלוח המוגן: מדביקים (Ctrl+V) ושולחים. יימחק מהלוח בעוד 20 שניות.");
  });
});
