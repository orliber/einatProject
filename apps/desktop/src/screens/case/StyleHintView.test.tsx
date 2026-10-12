// Her style next to a proposed paragraph: the figure, the reasons, the request and her usual
// changes. Invented text only.
import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { MarkedText, StyleHintView, applyMark, markPieces, wholeWordSpans } from "./StyleHintView";

describe("her usual changes in the text", () => {
  it("are found as whole words only", () => {
    expect(wholeWordSpans("הילד מפגין עניין, ומפגין שוב.", "מפגין")).toEqual([[5, 10]]);
    expect(wholeWordSpans("הילדה מפגינה עניין.", "מפגין")).toEqual([]);
  });

  it("are replaced with her wording, or deleted without leaving gaps", () => {
    expect(applyMark("הילד מפגין עניין. הוא מפגין סבלנות.", { from: "מפגין", to: "מראה" }))
      .toBe("הילד מראה עניין. הוא מראה סבלנות.");
    expect(applyMark("חשוב לציין כי הילד שיתף פעולה , ושמח.", { from: "חשוב לציין כי", to: "" }))
      .toBe("הילד שיתף פעולה, ושמח.");
  });

  it("are marked as text, never as markup", () => {
    const text = '<img src=x onerror="1"> הילד מפגין עניין';
    const { container } = render(<p><MarkedText text={text} marks={[{ from: "מפגין", to: "מראה" }]} /></p>);
    expect(container.querySelector("img")).toBeNull();
    expect(container.querySelector("mark.habit")).toHaveTextContent("מפגין");
    expect(container.textContent).toBe(text);
    expect(markPieces("א ב", [])).toEqual([{ text: "א ב" }]);
  });
});

describe("StyleHintView", () => {
  it("shows the figure, opens the reasons, asks to bring it closer and applies a change", async () => {
    const user = userEvent.setup();
    const onCloser = vi.fn();
    const onApply = vi.fn();
    render(<StyleHintView busy={false} onCloser={onCloser} onApply={onApply} hint={{
      fit: 58,
      details: ["משפטים ארוכים מהרגיל אצלך"],
      ask: "משפטים קצרים יותר, כ-14 מילים במשפט",
      marks: [{ from: "מפגין", to: "מראה" }, { from: "חשוב לציין כי", to: "" }],
    }} />);
    const chip = screen.getByRole("button", { name: "קרוב לסגנון שלך: 58" });
    expect(chip).toHaveAttribute("aria-expanded", "false");
    await user.click(chip);
    expect(screen.getByText("משפטים ארוכים מהרגיל אצלך")).toBeInTheDocument();
    await user.click(screen.getByRole("button", { name: "לקרב לסגנון שלי" }));
    expect(onCloser).toHaveBeenCalledWith("משפטים קצרים יותר, כ-14 מילים במשפט");
    await user.click(screen.getByRole("button", { name: "למחוק «חשוב לציין כי»" }));
    expect(onApply).toHaveBeenCalledWith({ from: "חשוב לציין כי", to: "" });
  });

  it("says nothing until there is something to say", () => {
    const { container } = render(<StyleHintView busy={false} onCloser={vi.fn()} onApply={vi.fn()} hint={{ fit: null, details: [], ask: null, marks: [] }} />);
    expect(container).toBeEmptyDOMElement();
  });

  it("a paragraph that reads like her says so, and offers no rewrite", async () => {
    render(<StyleHintView busy={false} onCloser={vi.fn()} onApply={vi.fn()} hint={{ fit: 91, details: [], ask: null, marks: [] }} />);
    await userEvent.setup().click(screen.getByRole("button", { name: "קרוב לסגנון שלך: 91" }));
    expect(screen.getByText("הפסקה נשמעת כמו הכתיבה שלך.")).toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "לקרב לסגנון שלי" })).toBeNull();
  });
});
