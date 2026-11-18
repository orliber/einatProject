// The small "?" beside a fixed explanation (D-204): a toggletip the keyboard, a touch and a screen
// reader all reach. The copy here is invented.
import { render, screen, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { a11yViolations } from "../test/a11y";
import { HelpTip } from "./HelpTip";

function renderTip() {
  return render(
    <div>
      <HelpTip label="איך מזיזים פגישה"><p>שורה ראשונה</p></HelpTip>
      <button type="button">אחר</button>
    </div>,
  );
}

const helpButton = () => screen.getByRole("button", { name: "איך מזיזים פגישה" });

afterEach(() => {
  vi.restoreAllMocks();
});

describe("the “?” beside a fixed explanation", () => {
  it("starts closed: aria-expanded is false, the text is not on the page, and aria-controls names the live region", () => {
    renderTip();
    const button = helpButton();
    expect(button).toHaveAttribute("aria-expanded", "false");
    expect(button).toHaveAttribute("title", "איך מזיזים פגישה");
    expect(screen.queryByText("שורה ראשונה")).toBeNull();
    const region = document.getElementById(button.getAttribute("aria-controls") ?? "");
    expect(region).not.toBeNull();
    expect(region).toBe(screen.getByRole("status"));
    expect(region).toBeEmptyDOMElement();
  });

  it("a click opens it, the text appears inside the role=status region, and a second click closes it", async () => {
    const user = userEvent.setup();
    renderTip();
    const button = helpButton();
    await user.click(button);
    expect(button).toHaveAttribute("aria-expanded", "true");
    const region = screen.getByRole("status");
    expect(region.id).toBe(button.getAttribute("aria-controls"));
    expect(within(region).getByText("שורה ראשונה")).toBeInTheDocument();
    await user.click(button);
    expect(button).toHaveAttribute("aria-expanded", "false");
    expect(screen.queryByText("שורה ראשונה")).toBeNull();
    expect(screen.getByRole("status")).toBeEmptyDOMElement();
  });

  it("Escape closes it and the focus stays on the button", async () => {
    const user = userEvent.setup();
    renderTip();
    // A dialog around it listens on the window: the Escape that closes the "?" is not also its.
    const heard: string[] = [];
    const listen = (e: KeyboardEvent) => heard.push(e.key);
    window.addEventListener("keydown", listen);
    try {
      const button = helpButton();
      await user.click(button);
      await user.keyboard("{Escape}");
      expect(button).toHaveAttribute("aria-expanded", "false");
      expect(screen.queryByText("שורה ראשונה")).toBeNull();
      expect(button).toHaveFocus();
      expect(heard).toEqual([]);
      // Closed, Escape is not its own any more and goes on as usual.
      await user.keyboard("{Escape}");
      expect(heard).toEqual(["Escape"]);
    } finally {
      window.removeEventListener("keydown", listen);
    }
  });

  it("a press outside closes it, and so does Tab to the next button", async () => {
    const user = userEvent.setup();
    renderTip();
    const button = helpButton();
    const other = screen.getByRole("button", { name: "אחר" });
    await user.click(button);
    await user.click(other);
    expect(button).toHaveAttribute("aria-expanded", "false");
    expect(screen.queryByText("שורה ראשונה")).toBeNull();

    await user.click(button);
    expect(screen.getByText("שורה ראשונה")).toBeInTheDocument();
    await user.tab();
    expect(other).toHaveFocus();
    expect(button).toHaveAttribute("aria-expanded", "false");
    expect(screen.queryByText("שורה ראשונה")).toBeNull();
  });

  it("a press on the card's own text keeps it open", async () => {
    const user = userEvent.setup();
    renderTip();
    await user.click(helpButton());
    await user.click(screen.getByText("שורה ראשונה"));
    expect(helpButton()).toHaveAttribute("aria-expanded", "true");
    expect(screen.getByText("שורה ראשונה")).toBeInTheDocument();
  });

  it("opens toward the page, and the other way near the window's left edge", async () => {
    const user = userEvent.setup();
    const { container } = renderTip();
    const root = container.querySelector(".help-tip");
    const at = (right: number) =>
      vi.spyOn(HTMLElement.prototype, "getBoundingClientRect").mockReturnValue(
        { right, left: right - 44, top: 0, bottom: 44, width: 44, height: 44, x: right - 44, y: 0, toJSON: () => ({}) },
      );
    at(900);
    await user.click(helpButton());
    expect(root).not.toHaveClass("help-tip-flip");
    await user.click(helpButton());
    vi.restoreAllMocks();
    at(120);
    await user.click(helpButton());
    expect(root).toHaveClass("help-tip-flip");
  });

  it("passes the accessibility check open and closed", async () => {
    const user = userEvent.setup();
    const { container } = renderTip();
    expect(await a11yViolations(container)).toEqual([]);
    await user.click(helpButton());
    expect(screen.getByText("שורה ראשונה")).toBeInTheDocument();
    expect(await a11yViolations(container)).toEqual([]);
  });
});
