// The consultation screen on its own, over the demo core: when "שליחה" can be pressed, what
// she sees while a question is on its way, a question the check did not let go, the line about
// a part left out, the saved conversations (open, delete) and the scope of a new question.
// Typing in Hebrew with an input method is covered in its own test. Every name is invented.
import { configure, render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { clearKept } from "../kept";
import { a11yViolations } from "../test/a11y";
import { prepared } from "../test/fixtures";
import { appApi, Host, serve, type Override } from "../test/host";
import { ConsultScreen } from "./ConsultScreen";

configure({ asyncUtilTimeout: 5000 });
vi.setConfig({ testTimeout: 30_000 });

afterEach(() => clearKept());

/** An answer that waits until the test lets it go. */
function held<T>() {
  let release: (v: T) => void = () => undefined;
  const promise = new Promise<T>((r) => (release = r));
  return { promise, release };
}

const answer = (over: Record<string, unknown> = {}) => ({ answer: "תשובה בדויה.", demo: false, left_out: false, conversation_id: "conv-1", ...over });

async function openConsult(over?: Override, caseId?: string) {
  const served = await serve(over);
  const app = appApi();
  const user = userEvent.setup();
  const view = render(<Host app={app}><ConsultScreen caseId={caseId} /></Host>);
  const box = await screen.findByLabelText("שאלה ל-Claude");
  const send = screen.getByRole("button", { name: "שליחה" });
  return { ...served, app, user, view, box, send };
}

const chat = () => screen.getByRole("region", { name: "השיחה" });

describe("the consultation screen", () => {
  it("starts empty with ideas to start from, and passes the accessibility check", async () => {
    const { view } = await openConsult();
    expect(within(chat()).getByRole("heading", { name: "שאלה מקצועית" })).toBeInTheDocument();
    expect(within(chat()).getAllByRole("button").length).toBeGreaterThanOrEqual(3);
    expect(await screen.findByText("השיחות נשמרות כאן, מוצפנות בכספת.")).toBeInTheDocument();
    expect(await a11yViolations(view.container)).toEqual([]);
  });

  it("“שליחה” waits for a question: an empty box or spaces only cannot be sent", async () => {
    const { user, box, send, calls } = await openConsult();
    expect(send).toBeDisabled();
    await user.type(box, "   ");
    expect(send).toBeDisabled();
    await user.keyboard("{Enter}");
    expect(calls.some((c) => c.cmd === "prepare_consult")).toBe(false);
    await user.clear(box);
    await user.type(box, "שאלה בדויה");
    expect(send).toBeEnabled();
  });

  it("an idea fills the box and gives it the keyboard", async () => {
    const { user, box, send } = await openConsult();
    const idea = within(chat()).getAllByRole("button")[0] as HTMLElement;
    await user.click(idea);
    expect(box).toHaveValue(idea.textContent ?? "");
    expect(box).toHaveFocus();
    expect(send).toBeEnabled();
  });

  it("Enter sends; Shift+Enter is a new line and sends nothing", async () => {
    const { user, box, calls } = await openConsult((cmd) => (cmd === "send_consult" ? answer() : undefined));
    await user.type(box, "שורה ראשונה{Shift>}{Enter}{/Shift}שורה שנייה");
    expect(box).toHaveValue("שורה ראשונה\nשורה שנייה");
    expect(calls.some((c) => c.cmd === "prepare_consult")).toBe(false);
    await user.keyboard("{Enter}");
    expect(await within(chat()).findByText("תשובה בדויה.")).toBeInTheDocument();
    expect(calls.find((c) => c.cmd === "prepare_consult")?.args).toMatchObject({ caseId: null, conversationId: null, message: "שורה ראשונה\nשורה שנייה" });
    expect(box).toHaveValue("");
    expect(box).toHaveFocus();
  });

  it("while a question is on its way nothing else can be sent or switched, and the answer comes in its place", async () => {
    const sent = held<ReturnType<typeof answer>>();
    const { user, box, send, calls } = await openConsult((cmd) => (cmd === "send_consult" ? sent.promise : undefined));
    await user.type(box, "מה בודקים בתצפית בגן?{Enter}");
    // Claude is writing: the question shows at once, marked until it is sent.
    expect(await within(chat()).findByText(/Claude כותב/)).toBeInTheDocument();
    expect(within(chat()).getByText("ממתינה לשליחה…")).toBeInTheDocument();
    await user.type(box, "שאלה נוספת");
    expect(send).toBeDisabled();
    await user.keyboard("{Enter}");
    expect(calls.filter((c) => c.cmd === "prepare_consult")).toHaveLength(1);
    expect(screen.getByRole("button", { name: "+ שיחה חדשה" })).toBeDisabled();
    for (const radio of within(screen.getByRole("radiogroup", { name: "על מה השאלה?" })).getAllByRole("radio")) expect(radio).toBeDisabled();

    sent.release(answer({ demo: true }));
    expect(await within(chat()).findByText("תשובה בדויה.")).toBeInTheDocument();
    expect(within(chat()).getByText("הדגמה")).toBeInTheDocument();
    expect(within(chat()).queryByText("ממתינה לשליחה…")).toBeNull();
    expect(within(chat()).getByText(/🔒/)).toBeInTheDocument();
    // What she typed meanwhile is still in the box, and can go now.
    expect(box).toHaveValue("שאלה נוספת");
    expect(send).toBeEnabled();
    expect(screen.getByRole("button", { name: "+ שיחה חדשה" })).toBeEnabled();
  });

  it("names the filter hid are said under the question", async () => {
    const { user, box } = await openConsult((cmd) => (cmd === "send_consult" ? answer() : undefined));
    await user.type(box, "נועם מתקשה במעברים, מה כדאי לבדוק?{Enter}");
    await within(chat()).findByText("תשובה בדויה.");
    expect(within(chat()).getByText(/^🔒 (הוסתרו לפני השליחה: .+|לא נמצאו פרטים מזהים)$/)).toBeInTheDocument();
  });

  it("a question the check did not let go comes back to the box, with why, in Hebrew", async () => {
    const refused = prepared({ approval_id: null, blocked: [{ code: "trap", message: "נמצא בטקסט פרט מזהה שלא הוסתר.", detail: null }] });
    const { user, box, calls } = await openConsult((cmd) => (cmd === "prepare_consult" ? refused : undefined));
    await user.type(box, "שאלה שלא יצאה{Enter}");
    expect(await screen.findByRole("alert")).toHaveTextContent("הבקשה לא נשלחה: נמצא בטקסט פרט מזהה שלא הוסתר. אפשר לנסות שוב.");
    expect(box).toHaveValue("שאלה שלא יצאה");
    expect(within(chat()).queryByText("ממתינה לשליחה…")).toBeNull();
    expect(calls.some((c) => c.cmd === "send_consult")).toBe(false);
  });

  it("a part of the question left out by the check is said under the answer", async () => {
    const { user, box } = await openConsult((cmd) => (cmd === "send_consult" ? answer({ left_out: true }) : undefined));
    await user.type(box, "שאלה עם קטע לא בטוח{Enter}");
    await within(chat()).findByText("תשובה בדויה.");
    expect(screen.getByText(/^קטע מהשאלה לא נשלח, כי הבדיקה לא הייתה בטוחה שאין בו שם\./)).toBeInTheDocument();
  });

  it("the new conversation is listed under “היום” and opens again from there", async () => {
    const { user, box, view } = await openConsult();
    await user.type(box, "איך לנסח המלצה בדויה?{Enter}");
    const history = screen.getByRole("navigation", { name: "שיחות קודמות" });
    const item = await within(history).findByRole("button", { name: /^איך לנסח המלצה בדויה\?/ }, { timeout: 8000 });
    expect(within(history).getByRole("list", { name: "היום" })).toContainElement(item);
    expect(item).toHaveAttribute("aria-current", "true");
    // A saved conversation stays with its scope.
    expect(screen.getByText(/שיחה שמורה נשארת עם התיק שלה/)).toBeInTheDocument();
    await user.click(screen.getByRole("button", { name: "+ שיחה חדשה" }));
    expect(within(chat()).getByRole("heading", { name: "שאלה מקצועית" })).toBeInTheDocument();
    expect(box).toHaveFocus();
    await user.click(item);
    expect(await within(chat()).findByText(/מצב הדגמה: כאן תופיע תשובה מקצועית/)).toBeInTheDocument();
    expect(await a11yViolations(view.container)).toEqual([]);
  });

  it("a saved conversation that cannot be opened says why", async () => {
    const summary = { id: "conv-x", case_id: null, case_label: null, title: "שיחה בדויה", updated_at: Math.floor(Date.now() / 1000), turns: 2 };
    const { user } = await openConsult((cmd) => {
      if (cmd === "consultations") return [summary];
      if (cmd === "consultation") throw { code: "not_found", message: "לא נמצא: השיחה", details: [] };
      return undefined;
    });
    await user.click(await screen.findByRole("button", { name: /^שיחה בדויה/ }));
    expect(await screen.findByRole("alert")).toHaveTextContent("לא נמצא: השיחה");
  });

  it("deleting a conversation asks first, and the open one is closed with it", async () => {
    const { user, box, calls } = await openConsult();
    await user.type(box, "שאלה למחיקה{Enter}");
    const history = screen.getByRole("navigation", { name: "שיחות קודמות" });
    const remove = await within(history).findByRole("button", { name: "מחיקת השיחה שאלה למחיקה" }, { timeout: 8000 });
    await user.click(remove);
    const confirm = await screen.findByRole("dialog", { name: "מחיקת שיחה" });
    expect(within(confirm).getByText('השיחה "שאלה למחיקה" תימחק מהכספת. אי אפשר לשחזר אותה.')).toBeInTheDocument();
    await user.click(within(confirm).getByRole("button", { name: "ביטול" }));
    expect(calls.some((c) => c.cmd === "delete_consultation")).toBe(false);
    await user.click(remove);
    await user.click(within(await screen.findByRole("dialog", { name: "מחיקת שיחה" })).getByRole("button", { name: "מחיקה" }));
    await waitFor(() => expect(screen.queryByRole("dialog")).toBeNull());
    expect(calls.some((c) => c.cmd === "delete_consultation")).toBe(true);
    await waitFor(() => expect(within(history).queryByRole("button", { name: /שאלה למחיקה/ })).toBeNull());
    expect(within(chat()).getByRole("heading", { name: "שאלה מקצועית" })).toBeInTheDocument();
  });

  it("a question about a case: the case is chosen, and only approved sections go, after hiding", async () => {
    const { user, box, calls, caseId } = await openConsult((cmd) => (cmd === "send_consult" ? answer() : undefined));
    const scope = screen.getByRole("radiogroup", { name: "על מה השאלה?" });
    const general = within(scope).getByRole("radio", { name: /שאלה מקצועית כללית/ });
    expect(general).toHaveAttribute("aria-checked", "true");
    const onCase = within(scope).getByRole("radio", { name: /על תיק/ });
    await waitFor(() => expect(onCase).toBeEnabled());
    expect(onCase).toHaveTextContent("נשלחים רק סעיפים שאישרת, אחרי הסתרה");
    await user.click(onCase);
    expect(onCase).toHaveAttribute("aria-checked", "true");
    expect(within(scope).getByLabelText("התיק")).toBeInTheDocument();
    await user.type(box, "מה כדאי להוסיף לסיכום?{Enter}");
    await within(chat()).findByText("תשובה בדויה.");
    expect(calls.find((c) => c.cmd === "prepare_consult")?.args.caseId).toBe(caseId);
    expect(within(chat()).getByText(/^על התיק של /)).toBeInTheDocument();
  });

  it("opened from a case, a new question is about that case", async () => {
    const { caseId } = await serve();
    render(<Host app={appApi()}><ConsultScreen caseId={caseId} /></Host>);
    expect(await screen.findByRole("heading", { name: "שאלה על נועם" })).toBeInTheDocument();
    expect(within(screen.getByRole("radiogroup", { name: "על מה השאלה?" })).getByRole("radio", { name: /על תיק/ })).toHaveAttribute("aria-checked", "true");
  });
});
