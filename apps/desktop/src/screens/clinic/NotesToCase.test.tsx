// Records that were not copied to the diagnostic case yet: the card says how many, and one
// window copies the chosen ones. Over the demo core; every name is invented.
import { configure, screen, waitFor, within } from "@testing-library/react";
import type { PatientCard } from "../../ipc/generated/PatientCard";
import type { SessionNote } from "../../ipc/generated/SessionNote";
import { a11yViolations } from "../../test/a11y";
import { forgetDiscreet } from "./ClinicScreen";
import { notesForCase } from "./NotesToCase";
import { cardOf, openCard, refusing } from "./patientsTestKit";

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
  ({ id: "n", patient_id: "p", visit_id: null, date: "2026-08-09", record: "רשומה בדויה", personal: "", case_input_id: null, recordings: [], ...over });
const card = (notes: SessionNote[], linked: PatientCard["case"]): PatientCard => ({ notes, case: linked, visits: [] } as unknown as PatientCard);
const linked = { code: "תיק-1", in_trash: false } as PatientCard["case"];

describe("which records can still go to the case", () => {
  it("written and not copied yet, oldest first", () => {
    const notes = [note({ id: "late", date: "2026-10-01" }), note({ id: "sent", case_input_id: "i1" }), note({ id: "empty", record: "  ", personal: "רק לעצמי" }), note({ id: "early", date: "2026-08-01" })];
    expect(notesForCase(card(notes, linked)).map((n) => n.id)).toEqual(["early", "late"]);
  });

  it("none without a case, or with a case in the recycle bin", () => {
    expect(notesForCase(card([note({})], null))).toEqual([]);
    expect(notesForCase(card([note({})], { ...linked, in_trash: true } as PatientCard["case"]))).toEqual([]);
  });
});

describe("on the card", () => {
  it("says how many records wait, copies the chosen ones, and then the line is gone", async () => {
    const opened = await openCard("נועם");
    const { user, requests, notes } = opened;
    const before = notesForCase(await cardOf(opened, opened.id));
    expect(before.length).toBeGreaterThanOrEqual(2);
    const box = await screen.findByRole("region", { name: "סיכומי פגישות" });
    expect(box).toHaveTextContent(`${before.length} רשומות עוד לא הועברו לחומרי תיק האבחון`);
    await user.click(within(box).getByRole("button", { name: "להעביר לתיק" }));
    const dialog = await screen.findByRole("dialog", { name: "להעביר רשומות לתיק האבחון" });
    expect(dialog).toHaveTextContent("התרשומת האישית וההקלטות נשארות בכרטיס");
    expect(await a11yViolations(dialog)).toEqual([]);
    const boxes = within(dialog).getAllByRole("checkbox");
    expect(boxes).toHaveLength(before.length);
    // One is left out: only the others are copied.
    await user.click(boxes[0] as HTMLElement);
    expect(requests().some((r) => r.op === "note_to_case")).toBe(false);
    await user.click(within(dialog).getByRole("button", { name: before.length - 1 === 1 ? "להעביר רשומה אחת" : `להעביר ${before.length - 1} רשומות` }));
    await waitFor(() => expect(screen.queryByRole("dialog")).toBeNull());
    expect(requests().filter((r) => r.op === "note_to_case").map((r) => (r as { id: string }).id)).toEqual(before.slice(1).map((n) => n.id));
    expect(notes.some((t) => t.includes("הועתק"))).toBe(true);
    const after = await cardOf(opened, opened.id);
    expect(notesForCase(after).map((n) => n.id)).toEqual([before[0]?.id]);
    // What went is the record alone.
    await waitFor(() => expect(box).toHaveTextContent("רשומה אחת עוד לא הועברה"));
  });

  it("a refusal stays in the window, in her words", async () => {
    const { user } = await openCard("נועם", refusing("note_to_case", "תיק האבחון נמצא בסל המחזור."));
    const box = await screen.findByRole("region", { name: "סיכומי פגישות" });
    await user.click(within(box).getByRole("button", { name: "להעביר לתיק" }));
    const dialog = await screen.findByRole("dialog", { name: "להעביר רשומות לתיק האבחון" });
    await user.click(within(dialog).getByRole("button", { name: /^להעביר/ }));
    expect(await within(dialog).findByText("תיק האבחון נמצא בסל המחזור.")).toBeInTheDocument();
  });

  it("a card without a case has no such line", async () => {
    await openCard("תמר");
    const box = await screen.findByRole("region", { name: "סיכומי פגישות" });
    expect(within(box).queryByRole("button", { name: "להעביר לתיק" })).toBeNull();
  });
});
