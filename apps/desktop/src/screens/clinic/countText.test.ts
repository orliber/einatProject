// How a day's visits are counted in words: her own time apart from the visits.
import { describe, expect, it } from "vitest";
import type { VisitView } from "../../ipc/generated/VisitView";
import { countText } from "./CalendarTab";

const view = (kind: string, patient: string | null) => ({ visit: { kind, patient_id: patient } }) as unknown as VisitView;
const visit = view("assessment", "p1");
const own = view("block", null);

describe("a day's count in words", () => {
  it("counts her own time apart, joined as Hebrew joins a word and a number", () => {
    expect(countText([])).toBe("אין פגישות");
    expect(countText([visit])).toBe("פגישה אחת");
    expect(countText([visit, visit, visit, own])).toBe("3 פגישות וזמן שמור אחד");
    expect(countText([visit, own, own])).toBe("פגישה אחת ו-2 זמנים שמורים");
    expect(countText([own])).toBe("זמן שמור אחד");
  });
});
