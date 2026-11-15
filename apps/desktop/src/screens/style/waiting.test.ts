import { describe, expect, it } from "vitest";
import type { StyleItem, StyleOverview } from "../../ipc/client";
import { waitingFor } from "./waiting";

const item = (id: string, text: string, enabled: boolean): StyleItem =>
  ({ id, section: null, kind: "rule", text, enabled, origin: "reports", support: 1 });

describe("waitingFor", () => {
  it("does not ask again about an item a rebuild switched off because she did (D-166)", () => {
    const o = {
      draft: { id: "d", profile: { reports: 2, items: [item("a", "כלל חדש", true), item("b", "כלל שכיבתה", false)] } },
      active: null,
      suggestions: [],
      interpretations: [],
    } as unknown as StyleOverview;
    const asks = waitingFor(o, []);
    expect(asks.map((a) => a.key)).toEqual(["item:a"]);
  });
});
