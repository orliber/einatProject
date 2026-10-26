// Typing Hebrew into the real screens (L6, D-117), in jsdom: key events of the Hebrew layout (the
// letter on its physical key, as Windows sends it) and IME composition, into the editors where
// typing once stopped on Windows (F1a). The end-to-end twin, in a real Chromium, is
// e2e/hebrew-typing.spec.ts. Every name and text is invented.
import { act, configure, fireEvent, render, screen, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { mockIPC } from "@tauri-apps/api/mocks";
import { App } from "../App";
import { FakeCore } from "../web/fakeCore";
import { TEXT_DEBOUNCE_MS } from "../components/LocalText";
import { HEBREW_LAYOUT } from "../test/hebrewKeyboard";

configure({ asyncUtilTimeout: 5000 });
vi.setConfig({ testTimeout: 30_000 });

/** user-event typing on the Hebrew layout: `[KeyA]` is ש. The keys every editor also needs stay. */
const keyboardMap = [
  ...Object.entries(HEBREW_LAYOUT).map(([key, [code, keyCode]]) => ({ key, code, keyCode })),
  { key: "Enter", code: "Enter", keyCode: 13 },
  { key: "Escape", code: "Escape", keyCode: 27 },
  { key: "Backspace", code: "Backspace", keyCode: 8 },
  { key: "Control", code: "ControlLeft", keyCode: 17 },
  { key: "Shift", code: "ShiftLeft", keyCode: 16 },
];

/** The keys of the Hebrew layout that type `text`, in user-event's syntax: "שלום" → "[KeyA][KeyK][KeyU][KeyO]". */
function keys(text: string): string {
  return [...text].map((ch) => {
    const key = HEBREW_LAYOUT[ch];
    if (!key) throw new Error(`no key for ${JSON.stringify(ch)}`);
    return `[${key[0]}]`;
  }).join("");
}

async function openApp() {
  const core = new FakeCore();
  await core.handle("unlock", { password: "x" });
  await core.handle("set_review_always", { on: false });
  mockIPC((cmd, args) => core.handle(cmd, args));
  const user = userEvent.setup({ keyboardMap, delay: null });
  render(<App />);
  return { user };
}

async function openReport(user: ReturnType<typeof userEvent.setup>) {
  await user.click(await screen.findByRole("button", { name: /בתיק של נועם$/ }));
  return screen.findByRole("article", { name: "הדוח, כמו בקובץ" });
}

/** A pause longer than the box's report: the screen hears the text and re-renders. */
async function pause() {
  await act(() => new Promise((done) => setTimeout(done, TEXT_DEBOUNCE_MS + 100)));
}

/** A word through an input method, the way the browser delivers it: composition start, an
 *  update and a composing input per letter, then the end and the committed input. */
function compose(box: HTMLTextAreaElement, word: string) {
  const at = box.selectionStart;
  const before = box.value.slice(0, at);
  const after = box.value.slice(box.selectionEnd);
  fireEvent.compositionStart(box, { data: "" });
  let shown = "";
  for (const ch of word) {
    shown += ch;
    fireEvent.compositionUpdate(box, { data: shown });
    fireEvent.input(box, { target: { value: before + shown + after }, isComposing: true, data: shown, inputType: "insertCompositionText" });
    box.setSelectionRange(at + shown.length, at + shown.length);
  }
  fireEvent.compositionEnd(box, { data: word });
  fireEvent.input(box, { target: { value: before + word + after }, data: word, inputType: "insertCompositionText" });
  // user-event keeps its own copy of what it typed: a plain write of the same value tells it the
  // box changed under it (React has already heard the input; the same value is no new change).
  box.value = box.value; // eslint-disable-line no-self-assign
  box.setSelectionRange(at + word.length, at + word.length);
}

/** Every key event the box heard, as `code:key`. */
function recordKeys(box: HTMLElement): string[] {
  const heard: string[] = [];
  box.addEventListener("keydown", (e) => heard.push(`${e.code}:${e.key}`));
  return heard;
}

describe("Hebrew typing in the editors (F1a, L6)", () => {
  it("the report's paragraph editor: every key of the Hebrew layout lands, in order, through a re-render", async () => {
    const { user } = await openApp();
    const page = await openReport(user);
    await user.click(within(page).getByText(/הופנה לאבחון פסיכולוגי-התפתחותי על ידי הוריו/));
    const box = await screen.findByRole("textbox", { name: /עריכת פסקה בסעיף/ }) as HTMLTextAreaElement;
    const start = box.value;
    const heard = recordKeys(box);
    box.setSelectionRange(start.length, start.length);

    await user.keyboard(keys(" הילד בנה מגדל"));
    expect(heard.slice(0, 3)).toEqual(["Space: ", "KeyV:ה", "KeyH:י"]);
    // The screen hears the text, and re-renders; the box keeps it and the caret.
    await pause();
    await user.keyboard(keys(" גבוה."));
    let expected = `${start} הילד בנה מגדל גבוה.`;
    expect(box).toHaveValue(expected);
    expect(box.selectionStart).toBe(expected.length);
    expect(heard).toHaveLength([..." הילד בנה מגדל גבוה."].length);

    // Into the middle, after another re-render: inserted there, the caret right after it.
    await pause();
    const at = start.length + " הילד".length;
    box.setSelectionRange(at, at);
    await user.keyboard(keys(" הקטן"));
    expected = expected.slice(0, at) + " הקטן" + expected.slice(at);
    expect(box).toHaveValue(expected);
    expect(box.selectionStart).toBe(at + " הקטן".length);
    await pause();
    expect(box).toHaveValue(expected);
    expect(document.activeElement).toBe(box);

    // Ctrl+Enter saves it, with the Hebrew layout on.
    await user.keyboard("{Control>}{Enter}{/Control}");
    expect(await within(page).findByText(new RegExp("הילד הקטן בנה מגדל גבוה"))).toBeInTheDocument();
  });

  it("the report's paragraph editor: a word through IME composition, at the end and in the middle", async () => {
    const { user } = await openApp();
    const page = await openReport(user);
    await user.click(within(page).getByText(/הופנה לאבחון פסיכולוגי-התפתחותי על ידי הוריו/));
    const box = await screen.findByRole("textbox", { name: /עריכת פסקה בסעיף/ }) as HTMLTextAreaElement;
    const start = box.value;
    box.setSelectionRange(start.length, start.length);

    compose(box, " שוב");
    expect(box).toHaveValue(`${start} שוב`);
    await pause();
    box.setSelectionRange(0, 0);
    compose(box, "כך ");
    expect(box).toHaveValue(`כך ${start} שוב`);
    expect(box.selectionStart).toBe(3);
    await pause();
    expect(box).toHaveValue(`כך ${start} שוב`);
    expect(document.activeElement).toBe(box);
  });

  it("a new paragraph of her own takes Hebrew keys and composition alike", async () => {
    const { user } = await openApp();
    const page = await openReport(user);
    await user.click(within(page).getAllByRole("button", { name: "לכתוב בעצמי" })[0] as HTMLElement);
    const box = await screen.findByRole("textbox", { name: /פסקה חדשה בסעיף/ }) as HTMLTextAreaElement;
    await user.keyboard(keys("הילדה משחקת"));
    compose(box, " בחצר");
    await pause();
    await user.keyboard(keys(" עם חברות."));
    expect(box).toHaveValue("הילדה משחקת בחצר עם חברות.");
    expect(box.selectionStart).toBe(box.value.length);
  });

  it("the material's text box keeps every letter while the dialog re-renders around it", async () => {
    const { user } = await openApp();
    await user.click(await screen.findByRole("button", { name: /בתיק של נועם$/ }));
    await user.click((await screen.findAllByRole("button", { name: /חומרים/ }))[0] as HTMLElement);
    await user.click(await screen.findByRole("button", { name: "הוספה לאבחונים קודמים" }));
    await user.click(await screen.findByRole("menuitem", { name: "הדבקת טקסט" }));
    const dialog = await screen.findByRole("dialog", { name: "הוספת טקסט" });
    const box = within(dialog).getByLabelText("הטקסט") as HTMLTextAreaElement;
    expect(document.activeElement).toBe(box);
    const text = "האם מספרת שהילד ישן היטב, אוכל מגוון ומשחק בפאזלים.";
    await user.keyboard(keys(text.slice(0, 20)));
    await pause();
    compose(box, text.slice(20, 26));
    await user.keyboard(keys(text.slice(26)));
    expect(box).toHaveValue(text);
    expect(box.selectionStart).toBe(text.length);
  });

  it("the consultation: Enter while composing commits the word and does not send; Enter after it sends", async () => {
    const { user } = await openApp();
    await user.click((await screen.findAllByRole("button", { name: "התייעצות" }))[0] as HTMLElement);
    const box = await screen.findByLabelText(/^שאלה ל-/) as HTMLTextAreaElement;
    await user.click(box);
    await user.keyboard(keys("איך לנסח בעדינות"));
    compose(box, " להורים");
    // The Enter an IME takes for itself arrives with isComposing (and key code 229 on Windows).
    fireEvent.compositionStart(box, { data: "" });
    fireEvent.keyDown(box, { key: "Enter", code: "Enter", keyCode: 229, isComposing: true });
    fireEvent.compositionEnd(box, { data: "" });
    expect(box).toHaveValue("איך לנסח בעדינות להורים");
    expect(within(screen.getByRole("region", { name: "השיחה" })).queryByText("איך לנסח בעדינות להורים", { selector: ".chat-bubble-me" })).toBeNull();
    await user.keyboard("{Enter}");
    expect(await within(screen.getByRole("region", { name: "השיחה" })).findByText("איך לנסח בעדינות להורים", { selector: ".chat-bubble-me" })).toBeInTheDocument();
    expect(box).toHaveValue("");
  });
});
