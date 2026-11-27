// The search over a card's session notes ("finding something from three months ago"): shown
// from six notes on, reads the record, the date and the kind of visit, and the personal note
// only outside discreet mode. Over the demo core; every name and text is invented.
import { configure, screen, waitFor, within } from "@testing-library/react";
import type { SessionNote } from "../../ipc/generated/SessionNote";
import type { Visit } from "../../ipc/generated/Visit";
import { setDiscreet } from "../../discreet";
import { forgetDiscreet } from "./ClinicScreen";
import { noteMatches } from "./PatientCardView";
import { ask, openCard } from "./patientsTestKit";

configure({ asyncUtilTimeout: 5000 });
vi.setConfig({ testTimeout: 30_000 });

beforeEach(() => {
  vi.useFakeTimers({ toFake: ["Date"] });
  vi.setSystemTime(new Date(2026, 10, 15, 8, 0));
});

afterEach(() => {
  vi.useRealTimers();
  forgetDiscreet();
  window.history.replaceState({}, "", "/");
});

const note = (over: Partial<SessionNote>): SessionNote =>
  ({ id: "n", patient_id: "p", visit_id: null, date: "2026-08-09", record: "שיחה עם הגננת על המעברים בגן", personal: "לבדוק שוב את נושא השינה", case_input_id: null, recordings: [], ...over });
const visit = { kind: "observation" } as Visit;

describe("what a note answers a search by", () => {
  it("its record, every word in any order; an empty search finds all", () => {
    expect(noteMatches(note({}), null, "", false)).toBe(true);
    expect(noteMatches(note({}), null, "מעברים הגננת", false)).toBe(true);
    expect(noteMatches(note({}), null, "מעברים רופאה", false)).toBe(false);
  });

  it("a word that ends in a final letter finds its longer forms too", () => {
    const n = { record: "ביקור בשני גנים", personal: "", date: "2026-11-08" } as SessionNote;
    expect(noteMatches(n, null, "גן", false)).toBe(true);
    expect(noteMatches(n, null, "גנים", false)).toBe(true);
    expect(noteMatches(n, null, "בית", false)).toBe(false);
  });

  it("its date as it is shown, and the kind of its visit", () => {
    expect(noteMatches(note({}), null, "09.08", false)).toBe(true);
    expect(noteMatches(note({}), visit, "תצפית", false)).toBe(true);
    expect(noteMatches(note({}), null, "תצפית", false)).toBe(false);
  });

  it("the personal note, but never in discreet mode", () => {
    expect(noteMatches(note({}), null, "שינה", false)).toBe(true);
    expect(noteMatches(note({}), null, "שינה", true)).toBe(false);
  });
});

describe("on the card", () => {
  it("a card with a few notes has no search box", async () => {
    await openCard("נועם");
    const box = await screen.findByRole("region", { name: "סיכומי פגישות" });
    expect(within(box).queryByRole("searchbox")).toBeNull();
  });

  it("from six notes on: the search narrows the list, says how many, and clears", async () => {
    const { user } = await openCard("נועם", undefined, async (served, id) => {
      for (let i = 0; i < 6; i += 1) {
        await ask(served, { op: "save_note", note: note({ id: "", patient_id: id, date: `2026-0${i + 3}-10`, record: i === 2 ? "שיחת טלפון עם קלינאית התקשורת" : `מפגש מספר ${i + 1}`, personal: "" }) }, "note");
      }
    });
    const box = await screen.findByRole("region", { name: "סיכומי פגישות" });
    const search = await within(box).findByRole("searchbox", { name: "חיפוש בסיכומים" });
    await user.type(search, "קלינאית");
    await waitFor(() => expect(within(box).getAllByRole("button", { name: /^עריכה של הסיכום/ })).toHaveLength(1));
    expect(box).toHaveTextContent("סיכום אחד מתאים");
    expect(box).toHaveTextContent("שיחת טלפון עם קלינאית התקשורת");
    await user.clear(search);
    await user.type(search, "אין מילה כזו");
    expect(await within(box).findByText(/אין סיכום שמתאים לחיפוש/)).toBeInTheDocument();
    await user.click(within(box).getByRole("button", { name: "לנקות את החיפוש" }));
    await waitFor(() => expect(within(box).getAllByRole("button", { name: /^עריכה של הסיכום/ }).length).toBeGreaterThan(1));
    expect(search).toHaveValue("");
  });

  it("in discreet mode a word of a personal note finds nothing", async () => {
    const { user } = await openCard("נועם", undefined, async (served, id) => {
      for (let i = 0; i < 6; i += 1) {
        await ask(served, { op: "save_note", note: note({ id: "", patient_id: id, date: `2026-0${i + 3}-10`, record: `מפגש מספר ${i + 1}`, personal: i === 1 ? "חשד שלי בלבד" : "" }) }, "note");
      }
    });
    setDiscreet(true);
    const box = await screen.findByRole("region", { name: "סיכומי פגישות" });
    await user.type(await within(box).findByRole("searchbox", { name: "חיפוש בסיכומים" }), "חשד");
    expect(await within(box).findByText(/אין סיכום שמתאים לחיפוש/)).toBeInTheDocument();
  });
});

describe("the patients' search also reads the session notes of every card (D-216)", () => {
  async function openList() {
    const { openOn } = await import("./patientsTestKit");
    const opened = await openOn({ name: "clinic", tab: "patients" });
    const search = await screen.findByRole("searchbox", { name: "חיפוש" });
    return { ...opened, search };
  }
  /** A word that is in a demo note's record. */
  async function aWordOf(served: Parameters<typeof ask>[0]): Promise<{ word: string }> {
    const { rowsOf, cardOf } = await import("./patientsTestKit");
    for (const row of await rowsOf(served)) {
      const card = await cardOf(served, row.patient.id);
      const withRecord = card.notes.find((n) => n.record.trim().split(/\s+/).some((w) => w.length >= 5));
      if (withRecord) {
        const word = withRecord.record.trim().split(/\s+/).find((w) => w.length >= 5) ?? "";
        return { word: word.replace(/[.,:]/g, "") };
      }
    }
    throw new Error("the demo clinic has no note with a record");
  }

  it("a word of a record lists the note under the patients, and a press opens that family's card", async () => {
    const opened = await openList();
    const { word } = await aWordOf(opened);
    await opened.user.type(opened.search, word);
    const box = await screen.findByRole("region", { name: "נמצא בסיכומי הפגישות" });
    expect(box).toHaveTextContent(word);
    expect(opened.requests().some((r) => r.op === "search_notes" && r.text === word && r.personal)).toBe(true);
    // No card has the word: the page does not say "not found" above what it found.
    expect(screen.queryByText("לא נמצא מטופל כזה")).toBeNull();
    expect(screen.getByText("אין מטופל בשם הזה. החיפוש נמצא בסיכומים:")).toBeInTheDocument();
    const names = within(box).getAllByRole("button");
    await opened.user.click(names[0] as HTMLElement);
    // The card of the family whose note it is (the newest note that holds the word).
    expect(opened.went.some((r) => r.name === "clinic" && r.patientId !== undefined)).toBe(true);
  });

  it("one letter asks nothing; a word no note holds shows no such box", async () => {
    const opened = await openList();
    await opened.user.type(opened.search, "א");
    await new Promise((r) => setTimeout(r, 400));
    expect(opened.requests().some((r) => r.op === "search_notes")).toBe(false);
    await opened.user.clear(opened.search);
    await opened.user.type(opened.search, "מילהשאיןבשוםסיכום");
    await waitFor(() => expect(opened.requests().some((r) => r.op === "search_notes")).toBe(true));
    expect(screen.queryByRole("region", { name: "נמצא בסיכומי הפגישות" })).toBeNull();
  });

  it("in discreet mode the personal notes are not read and the record's words are not shown", async () => {
    const opened = await openList();
    const { word } = await aWordOf(opened);
    setDiscreet(true);
    await opened.user.type(opened.search, word);
    const box = await screen.findByRole("region", { name: "נמצא בסיכומי הפגישות" });
    expect(box).toHaveTextContent("הרשומה מוסתרת במצב דיסקרטי.");
    expect(box).not.toHaveTextContent(word);
    expect(box).toHaveTextContent("במצב דיסקרטי לא מחפשים בתרשומות האישיות.");
    // A word nothing holds: the empty page says the personal notes were not read.
    await opened.user.clear(opened.search);
    await opened.user.type(opened.search, "מילהשאיןבשוםסיכום");
    expect(await screen.findByText(/במצב דיסקרטי לא מחפשים בתרשומות האישיות\.$/)).toBeInTheDocument();
    const asked = opened.requests().filter((r) => r.op === "search_notes");
    expect(asked.length).toBeGreaterThan(0);
    expect(asked.every((r) => r.op === "search_notes" && !r.personal)).toBe(true);
  });
});
