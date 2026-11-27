// The box before a draft (D-140) tells the page what was in it as it went, so it opens again
// as she left it: her words, the styles pressed and "keep it for this section" (D-204 §3).
import { StrictMode } from "react";
import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { BriefBox, STYLE_CHIPS, type BriefLeft } from "./BriefBox";

function box(over: Partial<Parameters<typeof BriefBox>[0]> = {}) {
  return (
    <BriefBox id="brief-x" title="איך לנסח את הסעיף?" hint="" placeholder="" chips={STYLE_CHIPS} chipsTitle="סגנון"
      sendLabel="✦ לנסח" busy={false} onSend={vi.fn()} onCancel={vi.fn()} {...over} />
  );
}

describe("the brief box as it goes", () => {
  for (const strict of [false, true]) {
    it(`reports her words, styles and “לזכור” once, when it really goes${strict ? " (StrictMode)" : ""}`, async () => {
      const user = userEvent.setup();
      const gone = vi.fn<(left: BriefLeft, changed: boolean) => void>();
      const ui = box({ initial: "נשמר", keep: true, onGone: gone });
      const { unmount } = render(strict ? <StrictMode>{ui}</StrictMode> : ui);
      // A trial unmount (StrictMode) is not the box going.
      expect(gone).not.toHaveBeenCalled();
      await user.click(screen.getByRole("checkbox", { name: /לזכור לסעיף הזה/ }));
      await user.click(screen.getByRole("button", { name: "מפורט" }));
      const text = screen.getByRole("textbox");
      await user.clear(text);
      await user.type(text, "רק לילד הזה");
      unmount();
      expect(gone).toHaveBeenCalledTimes(1);
      expect(gone).toHaveBeenCalledWith({ text: "רק לילד הזה", picked: ["מפורט"], keep: false }, true);
    });
  }

  it("says whether she changed anything since it opened: a box only looked at is not hers", async () => {
    const user = userEvent.setup();
    const gone = vi.fn<(left: BriefLeft, changed: boolean) => void>();
    const first = render(box({ initial: "נשמר", picked: ["קצר וממוקד", "מפורט"], keep: true, onGone: gone }));
    first.unmount();
    expect(gone).toHaveBeenLastCalledWith({ text: "נשמר", picked: ["קצר וממוקד", "מפורט"], keep: true }, false);
    // A style pressed and pressed again, in another order: still as it opened.
    const second = render(box({ initial: "נשמר", picked: ["קצר וממוקד", "מפורט"], keep: true, onGone: gone }));
    await user.click(screen.getByRole("button", { name: "קצר וממוקד" }));
    await user.click(screen.getByRole("button", { name: "קצר וממוקד" }));
    second.unmount();
    expect(gone).toHaveBeenLastCalledWith({ text: "נשמר", picked: ["מפורט", "קצר וממוקד"], keep: true }, false);
    // Only "לזכור" unticked: changed.
    const third = render(box({ initial: "נשמר", keep: true, onGone: gone }));
    await user.click(screen.getByRole("checkbox", { name: /לזכור לסעיף הזה/ }));
    third.unmount();
    expect(gone).toHaveBeenLastCalledWith({ text: "נשמר", picked: [], keep: false }, true);
  });

  it("is described by a line about it outside the box, with the sentence of what is missing", async () => {
    const user = userEvent.setup();
    render(<>{box({ describedBy: "earlier-line", needs: { what: "typed", say: "צריך לכתוב משהו" } })}<p id="earlier-line">שורה</p></>);
    expect(screen.getByRole("textbox")).toHaveAttribute("aria-describedby", "earlier-line");
    screen.getByRole("textbox").focus();
    await user.keyboard("{Control>}{Enter}{/Control}");
    expect(screen.getByRole("textbox")).toHaveAttribute("aria-describedby", "brief-x-missing earlier-line");
  });

  it("opens with the styles and the checkbox it is given", () => {
    render(box({ initial: "חדש", picked: ["קצר וממוקד"], keep: false }));
    expect(screen.getByRole("textbox")).toHaveValue("חדש");
    expect(screen.getByRole("button", { name: "קצר וממוקד" })).toHaveAttribute("aria-pressed", "true");
    expect(screen.getByRole("button", { name: "מפורט" })).toHaveAttribute("aria-pressed", "false");
    expect(screen.getByRole("checkbox", { name: /לזכור לסעיף הזה/ })).not.toBeChecked();
  });

  it("her points: Enter is the next line, and only Ctrl+Enter sends; the keys are said beside the box", async () => {
    const user = userEvent.setup();
    const onSend = vi.fn();
    render(box({ onSend, needs: { what: "typed", say: "כתבי קודם את הנקודות שלך." } }));
    const text = screen.getByRole("textbox");
    await user.click(text);
    await user.keyboard("נקודה בדויה ראשונה{Enter}נקודה בדויה שנייה");
    expect(onSend).not.toHaveBeenCalled();
    expect(text).toHaveValue("נקודה בדויה ראשונה\nנקודה בדויה שנייה");
    expect(screen.getByText(/Enter שורה חדשה · Ctrl\+Enter שולח/)).toBeInTheDocument();
    await user.keyboard("{Control>}{Enter}{/Control}");
    expect(onSend).toHaveBeenCalledTimes(1);
    expect(onSend.mock.calls[0]?.[1]).toBe("נקודה בדויה ראשונה\nנקודה בדויה שנייה");
  });

  it("a wish still goes with Enter", async () => {
    const user = userEvent.setup();
    const onSend = vi.fn();
    render(box({ onSend }));
    await user.click(screen.getByRole("textbox"));
    await user.keyboard("קצר{Enter}");
    expect(onSend).toHaveBeenCalledTimes(1);
  });
});
