import { describe, expect, it } from "vitest";
import { choose, excerpt, feed, needsSorting, passages, sortLocally, type Routing } from "./routing";

// The same cases as crates/dv-domain/src/routing.rs, so the preview matches the app.
describe("passages", () => {
  it("joins wrapped lines and keeps a heading with what follows", () => {
    const text = "רקע התפתחותי:\nהלך בגיל שנה.\n\nבגן הוא משחק לבד\nבעיקר בחצר.\nאוהב פאזלים.\n";
    expect(passages(text)).toEqual(["רקע התפתחותי:\nהלך בגיל שנה.", "בגן הוא משחק לבד\nבעיקר בחצר.", "אוהב פאזלים."]);
  });
  it("cuts long unpunctuated text by lines", () => {
    const text = Array.from({ length: 30 }, (_, i) => `שורה ${i + 1}`).join("\n");
    expect(passages(text)).toHaveLength(3);
  });
  it("an ellipsis does not end a sentence", () => {
    expect(passages("אמר שהוא...\nלא יודע.")).toHaveLength(1);
    expect(passages("סיים (בקושי).\nהמשיך.")).toHaveLength(2);
  });
});

describe("routing", () => {
  const sorted: Routing = { suggestion: { byAi: true, passageCount: 5, sections: [{ section: "cognitive", passages: [3, 1, 3, 9] }] }, added: [], removed: [] };
  it("a fresh sorting sends only the chosen passages", () => {
    expect(feed(sorted, "cognitive", ["background"], 5)).toEqual([1, 3]);
    expect(feed(sorted, "background", ["background"], 5)).toBeNull();
  });
  it("a stale sorting falls back to the table", () => {
    expect(feed(sorted, "background", ["background"], 6)).toBe("whole");
    expect(needsSorting(sorted, 6)).toBe(true);
  });
  it("Einat always wins", () => {
    const r = choose(sorted, ["emotional"], ["background"], 5);
    expect(r.added).toEqual(["emotional"]);
    expect(r.removed).toEqual(["cognitive"]);
    expect(feed(r, "emotional", [], 5)).toBe("whole");
  });
  it("excerpts mark the gaps", () => {
    expect(excerpt(["א.", "ב.", "ג.", "ד."], [1, 2, 4])).toBe("א.\n\nב.\n\n(…)\n\nד.");
  });
  it("demo sorting places passages by their words", () => {
    expect(sortLocally(["ההריון עבר בשלום.", "בגן משחק לבד.", "תודה."], ["background", "kindergarten"])).toEqual([
      { section: "background", passages: [1] },
      { section: "kindergarten", passages: [2] },
    ]);
  });
});
