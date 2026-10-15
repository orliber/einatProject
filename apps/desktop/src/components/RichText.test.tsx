// The model's answer is laid out from its text, and never from HTML (round 45, D-086).
import { render, screen } from "@testing-library/react";
import { describe, expect, it } from "vitest";
import { inlines, parseRich, plainText, RichText } from "./RichText";

describe("RichText", () => {
  it("a heading, bold inside a sentence, and a mixed list", () => {
    const text = "## שלושה צעדים\nההורים יבינו את **המשמעות** ולא רק את המספר.\n\n1. להתחיל ממה שהילד כן עושה\n2. לתאר **מה זה אומר** ביום-יום\n- חולשה יחסית\n- קושי שדורש התערבות";
    render(<RichText text={text} />);
    expect(screen.getByRole("heading", { level: 3, name: "שלושה צעדים" })).toBeInTheDocument();
    expect(screen.getByText("המשמעות").tagName).toBe("STRONG");
    const lists = screen.getAllByRole("list");
    expect(lists).toHaveLength(2);
    expect(lists[0]?.tagName).toBe("OL");
    expect(lists[1]?.tagName).toBe("UL");
    expect(screen.getAllByRole("listitem")).toHaveLength(4);
  });

  it("a line that is only bold is a heading, as the model writes them", () => {
    const blocks = parseRich("**1. להתחיל ממה שהילד כן עושה**\nפסקה.");
    expect(blocks[0]).toEqual({ kind: "heading", level: 3, inline: [{ text: "1. להתחיל ממה שהילד כן עושה", bold: false }] });
    expect(blocks[1]?.kind).toBe("paragraph");
  });

  it("an unclosed ** stays text, and a tag is shown as text", () => {
    expect(inlines("זה **לא נסגר")).toEqual([{ text: "זה **לא נסגר", bold: false }]);
    const { container } = render(<RichText text={'שורה עם <img src=x onerror="window.__xss=1"> בפנים'} />);
    expect(container.querySelector("img")).toBeNull();
    expect(container.textContent).toContain('<img src=x onerror="window.__xss=1">');
  });

  it("an empty answer renders nothing, and single line breaks join a paragraph", () => {
    const { container } = render(<RichText text="" />);
    expect(container.querySelector(".rich")?.childElementCount).toBe(0);
    expect(parseRich("שורה אחת\nשורה שנייה\n\nפסקה חדשה")).toEqual([
      { kind: "paragraph", inline: [{ text: "שורה אחת שורה שנייה", bold: false }] },
      { kind: "paragraph", inline: [{ text: "פסקה חדשה", bold: false }] },
    ]);
  });

  it("past 200 lines the rest is one plain paragraph", () => {
    const many = Array.from({ length: 205 }, (_, i) => `- פריט ${i + 1}`).join("\n");
    const blocks = parseRich(many);
    expect(blocks[0]?.kind).toBe("list");
    const last = blocks[blocks.length - 1];
    expect(last?.kind).toBe("paragraph");
    expect(last?.kind === "paragraph" && last.inline[0]?.text).toContain("- פריט 205");
  });

  it("the clipboard text has no marks", () => {
    expect(plainText("## כותרת\nמילה **מודגשת** כאן")).toBe("כותרת\nמילה מודגשת כאן");
  });
});
