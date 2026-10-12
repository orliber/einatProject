import { describe, expect, it } from "vitest";
import type { AutoHidden } from "../ipc/generated/AutoHidden";
import { cardRows } from "./AutoHiddenCard";

const item = (token: string, tag: string, extra: Partial<AutoHidden> = {}): AutoHidden => ({
  token, tag, role: "teacher", reason: "אחרי 'הגננת'", uncertain: false, kind: "name", ...extra,
});

describe("the card's rows", () => {
  it("puts every form of one name in one row", () => {
    const rows = cardRows([item("רוני", "[גננת_1]"), item("ורוני", "[גננת_1]"), item("לרוני", "[גננת_1]"), item("דנה", "[אמא_1]")]);
    expect(rows.map((r) => r.tokens)).toEqual([["רוני", "ורוני", "לרוני"], ["דנה"]]);
  });

  it("keeps every item: nothing is dropped, only grouped", () => {
    const items = [item("רוני", "[גננת_1]"), item("ורוני", "[גננת_1]"), item("גננת", "עובדת בתחום החינוך", { kind: "indirect" }), item("מורה", "עובדת בתחום החינוך", { kind: "indirect" })];
    const tokens = cardRows(items).flatMap((r) => r.tokens).sort();
    expect(tokens).toEqual(items.map((i) => i.token).sort());
  });

  it("shows what is worth a look first", () => {
    const rows = cardRows([item("דנה", "[אמא_1]"), item("שירה", "[ילדה_2]", { uncertain: true }), item("שאלון", "שא[ילד_1]", { kind: "declared_in_word" })]);
    expect(rows.map((r) => r.tokens[0])).toEqual(["שאלון", "שירה", "דנה"]);
  });

  it("marks a row unsure when any of its forms was", () => {
    const rows = cardRows([item("שירה", "[ילדה_2]"), item("ושירה", "[ילדה_2]", { uncertain: true })]);
    expect(rows).toHaveLength(1);
    expect(rows[0]?.uncertain).toBe(true);
  });
});
