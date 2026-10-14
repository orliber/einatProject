import { act, fireEvent, render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { Profiler, useRef, useState } from "react";
import { LocalText, TEXT_DEBOUNCE_MS, type LocalTextHandle } from "./LocalText";

// Fabricated text only (tests/fixtures rule): a Hebrew sentence repeated to 500 letters.
const HEBREW = "הילד בונה מגדל מקוביות וצוחק ";
const LONG = HEBREW.repeat(Math.ceil(500 / HEBREW.length)).slice(0, 500);

/** A screen around the box that keeps what the box tells it. */
function Screen(props: { texts: string[]; flips: boolean[]; initial?: string | undefined }) {
  const [empty, setEmpty] = useState(!(props.initial ?? "").trim());
  const box = useRef<LocalTextHandle>(null);
  const [sent, setSent] = useState<string | null>(null);
  return (
    <div>
      <LocalText ref={box} aria-label="טקסט" rows={4} placeholder="כותבים כאן" initial={props.initial}
        onText={(t) => props.texts.push(t)}
        onEmptyChange={(e) => { props.flips.push(e); setEmpty(e); }} />
      <button type="button" disabled={empty} onClick={() => { setSent(box.current?.value() ?? null); box.current?.set(""); }}>שליחה</button>
      <button type="button" onClick={() => box.current?.set("הצעה מוכנה")}>הצעה</button>
      {sent !== null && <output>{sent}</output>}
    </div>
  );
}

function setup(initial?: string) {
  const renders = { n: 0 };
  const texts: string[] = [];
  const flips: boolean[] = [];
  // Every render of the screen (or of anything in it) is one commit of this profiler.
  const screenWith = (start?: string) => (
    <Profiler id="screen" onRender={() => { renders.n += 1; }}>
      <Screen texts={texts} flips={flips} initial={start} />
    </Profiler>
  );
  const view = render(screenWith(initial));
  return { renders, texts, flips, view, screenWith, box: screen.getByRole("textbox", { name: "טקסט" }) as HTMLTextAreaElement };
}

describe("LocalText", () => {
  afterEach(() => vi.useRealTimers());

  it("typing 500 Hebrew letters gives exactly that text, and the screen around it renders a constant number of times", async () => {
    const user = userEvent.setup({ delay: null });
    const { renders, flips, box } = setup();
    const before = renders.n;
    await user.type(box, LONG);
    expect(box).toHaveValue(LONG);
    expect(box.value).toHaveLength(500);
    // One render when the box stopped being empty; none per key.
    expect(flips).toEqual([false]);
    expect(renders.n - before).toBeLessThanOrEqual(2);
    // Send reads the box itself, not a debounced copy.
    await user.click(screen.getByRole("button", { name: "שליחה" }));
    expect(screen.getByRole("status")).toHaveTextContent(LONG.trim());
    expect(box).toHaveValue("");
  }, 30_000);

  it("its own re-renders never reset what was typed, and a changed `initial` waits for a new key", async () => {
    const user = userEvent.setup({ delay: null });
    const { box, view, screenWith } = setup("התחלה");
    expect(box).toHaveValue("התחלה");
    await user.type(box, " ועוד");
    view.rerender(screenWith("אחר"));
    expect(screen.getByRole("textbox", { name: "טקסט" })).toHaveValue("התחלה ועוד");
  });

  it("passes the textarea attributes through", () => {
    const { box } = setup();
    expect(box).toHaveAttribute("rows", "4");
    expect(box).toHaveAttribute("placeholder", "כותבים כאן");
  });

  it("reports the text debounced, and at once on blur", () => {
    vi.useFakeTimers();
    const { texts, box } = setup();
    // Key by key: each change restarts the wait.
    for (const typed of ["ש", "של", "שלו", "שלום"]) fireEvent.change(box, { target: { value: typed } });
    expect(texts).toEqual([]);
    act(() => vi.advanceTimersByTime(TEXT_DEBOUNCE_MS - 1));
    expect(texts).toEqual([]);
    act(() => vi.advanceTimersByTime(1));
    expect(texts).toEqual(["שלום"]);

    fireEvent.change(box, { target: { value: "שלום לכולם" } });
    expect(texts).toEqual(["שלום"]);
    fireEvent.blur(box);
    expect(texts).toEqual(["שלום", "שלום לכולם"]);
    // Nothing is left waiting after the blur.
    act(() => vi.advanceTimersByTime(TEXT_DEBOUNCE_MS * 2));
    expect(texts).toEqual(["שלום", "שלום לכולם"]);
  });

  it("reports what is still unreported when the box goes away", async () => {
    const texts: string[] = [];
    const { unmount } = render(<LocalText aria-label="טקסט" onText={(t) => texts.push(t)} />);
    fireEvent.change(screen.getByRole("textbox", { name: "טקסט" }), { target: { value: "טיוטה שלא נשמרה" } });
    expect(texts).toEqual([]);
    unmount();
    expect(texts).toEqual(["טיוטה שלא נשמרה"]);
  });

  it("says when the box turns empty and back only on the flips: twice for type-then-clear", async () => {
    const user = userEvent.setup({ delay: null });
    const { flips, box } = setup();
    const send = screen.getByRole("button", { name: "שליחה" });
    expect(send).toBeDisabled();
    await user.type(box, "מילים רבות מאוד");
    expect(send).toBeEnabled();
    await user.clear(box);
    expect(flips).toEqual([false, true]);
    expect(send).toBeDisabled();
    // Spaces only count as empty.
    await user.type(box, "   ");
    expect(flips).toEqual([false, true]);
  });

  it("text set from code is in the box, flips emptiness and is reported at once", async () => {
    const user = userEvent.setup({ delay: null });
    const { flips, texts, box } = setup();
    await user.click(screen.getByRole("button", { name: "הצעה" }));
    expect(box).toHaveValue("הצעה מוכנה");
    expect(flips).toEqual([false]);
    expect(texts).toEqual(["הצעה מוכנה"]);
  });

  it("says when the text starts and stops differing from where it began", () => {
    const edits: boolean[] = [];
    render(<LocalText aria-label="טקסט" initial="מקור" onEditedChange={(e) => edits.push(e)} />);
    const box = screen.getByRole("textbox", { name: "טקסט" });
    fireEvent.change(box, { target: { value: "מקורי" } });
    fireEvent.change(box, { target: { value: "מקורית" } });
    fireEvent.change(box, { target: { value: "מקור" } });
    expect(edits).toEqual([true, false]);
  });
});
