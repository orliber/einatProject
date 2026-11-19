// The help of each screen and window (D-206): every place has its words, every picture is in
// the program, and "?" opens what is on the screen.
import { render } from "@testing-library/react";
import { ALL_TOPICS, searchTopics, topicOf } from "./topics";
import { shotNames, shotUrl } from "./shots";
import { HELP_IDS } from "./types";
import { currentHelp, helpOfRoute } from "./where";
import { SETTINGS_PANES } from "../screens/settingsPanes";
import { CLINIC_TABS } from "../screens/clinic/tabs";

const words = (t: (typeof ALL_TOPICS)[number]) =>
  [t.title, t.intro, t.shotAlt ?? "", ...t.can.flatMap((c) => [c.title, c.text]), ...(t.tips ?? []), ...(t.faq ?? []).flatMap((f) => [f.q, f.a])];

describe("the help's topics", () => {
  it("every screen and window has exactly one topic, with something to do", () => {
    expect(ALL_TOPICS.map((t) => t.id).sort()).toEqual([...HELP_IDS].sort());
    for (const t of ALL_TOPICS) {
      expect(t.title.trim()).not.toBe("");
      expect(t.can.length).toBeGreaterThan(0);
      for (const r of t.related ?? []) expect(topicOf(r), `${t.id} → ${r}`).toBeDefined();
    }
  });

  it("is plain text: no markup, and no em dash", () => {
    for (const t of ALL_TOPICS) {
      for (const w of words(t)) {
        expect(w, t.id).not.toMatch(/<\/?[a-z]|&[a-z]+;|\*\*/i);
        expect(w, t.id).not.toContain("—");
      }
    }
  });

  it("every picture named is in the program, and every picture in the program is used", () => {
    const named = ALL_TOPICS.flatMap((t) => (t.shot ? [t.shot] : []));
    for (const n of named) expect(shotUrl(n), n).toBeDefined();
    expect(shotNames().sort()).toEqual([...new Set(named)].sort());
    for (const t of ALL_TOPICS) if (t.shot) expect(t.shotAlt, t.id).toBeTruthy();
  });

  it("finds a topic by any of its words", () => {
    expect(searchTopics("גיבוי").map((t) => t.id)).toContain("settings-backup");
    expect(searchTopics("   ")).toEqual([]);
    expect(searchTopics("מילה-שאין-בשום-מקום")).toEqual([]);
  });
});

describe("which help is open", () => {
  it("each page has its own", () => {
    expect(helpOfRoute({ name: "cases" })).toBe("cases");
    expect(helpOfRoute({ name: "case", id: "c", view: "materials" })).toBe("case-materials");
    expect(helpOfRoute({ name: "case", id: "c", view: "report" })).toBe("case-report");
    expect(helpOfRoute({ name: "clinic", tab: "calendar" })).toBe("clinic-calendar");
    expect(helpOfRoute({ name: "clinic", patientId: "p" })).toBe("clinic-patient");
    expect(helpOfRoute({ name: "settings" })).toBe("settings-general");
    for (const tab of CLINIC_TABS) expect(topicOf(helpOfRoute({ name: "clinic", tab: tab.id })), tab.id).toBeDefined();
    for (const pane of SETTINGS_PANES) expect(topicOf(helpOfRoute({ name: "settings", pane: pane.id })), pane.id).toBeDefined();
  });

  it("a window on top with help of its own wins; a small question shows the page's", () => {
    const { rerender, unmount } = render(<section role="dialog" aria-modal="true" data-help="new-case" />);
    expect(currentHelp({ name: "cases" })).toBe("new-case");
    rerender(<section role="dialog" aria-modal="true" />);
    expect(currentHelp({ name: "cases" })).toBe("cases");
    unmount();
    expect(currentHelp({ name: "consult" })).toBe("consult");
  });
});
