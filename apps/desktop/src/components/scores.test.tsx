// The score table (round 54): bands in words, a score that cannot be, a pasted column, what is
// still missing, and moving with Enter. Every score is invented.
import { act, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { mockIPC } from "@tauri-apps/api/mocks";
import { AppContext, type AppApi } from "../App";
import type { ScoreSheet } from "../ipc/client";
import { status } from "../test/fixtures";
import { instruments } from "../web/scores";
import { a11yViolations } from "../test/a11y";
import { bandOf, forgetScoreDrafts, missingOf, pasteCells, scoresDraft, ScoresView, tableOrder } from "./ScoresView";

function api(): AppApi {
  return { status: status(), go: vi.fn(), refresh: vi.fn(async () => undefined), notify: vi.fn(), fail: (e) => e.message, lockNow: vi.fn(async () => undefined) };
}

const wisc = () => {
  const i = instruments.find((x) => x.key === "wisc_v");
  if (!i) throw new Error("no WISC-V table");
  return i;
};

let saved: ScoreSheet[] = [];
/** Her answers in Settings → המבחנים (D-098); the rest await approval. All approved by default. */
const allApproved = () => Object.fromEntries(instruments.map((i) => [i.key, "approved" as const]));
let answers: Record<string, "approved" | "not_used"> = allApproved();

function mount(props: Partial<Parameters<typeof ScoresView>[0]> = {}) {
  saved = [];
  mockIPC((cmd, args) => {
    if (cmd === "score_instruments")
      return instruments
        .map((instrument) => ({ instrument, status: answers[instrument.key] ?? "awaiting", open_cases: 0 }))
        .filter((a) => a.status !== "not_used");
    if (cmd === "preview_scores") return "טקסט הציונים";
    if (cmd === "save_scores") {
      saved.push((args as { sheet: ScoreSheet }).sheet);
      return { id: "in-1" };
    }
    return null;
  });
  const onClose = vi.fn();
  const onSaved = vi.fn(async () => undefined);
  const view = render(
    <AppContext.Provider value={api()}>
      <ScoresView caseId="c1" age={{ years: 9, months: 2 }} onClose={onClose} onSaved={onSaved} {...props} />
    </AppContext.Provider>,
  );
  return { ...view, onClose, onSaved };
}

const field = (name: string) => screen.getByLabelText(name, { selector: "input" });

afterEach(() => {
  forgetScoreDrafts();
  answers = allApproved();
});

describe("her approval of the tables (D-098)", () => {
  it("marks an instrument awaiting approval, and leaves out one she does not give", async () => {
    answers = { wppsi_iv: "approved", wisc_v: "not_used" }; // the rest await approval
    mount();
    // The age fits WISC-V, which she does not give: the table opens at another instrument.
    const picker = await screen.findByRole("radiogroup", { name: "הכלי" });
    expect(within(picker).queryByRole("radio", { name: /WISC-V/ })).toBeNull();
    expect(within(picker).getByRole("radio", { name: /WPPSI-IV/ }).textContent).not.toContain("טיוטה");
    expect(within(picker).getByRole("radio", { name: /BRIEF-P/ }).textContent).toContain("טיוטה");
    expect(within(picker).getAllByRole("radio").filter((r) => r.getAttribute("aria-checked") === "true")).toHaveLength(1);
  });

  it("says why a saved table of an instrument she does not give cannot be edited", async () => {
    answers = { wisc_v: "not_used" };
    const sheet: ScoreSheet = { instrument: "wisc_v", module: "", cutoff: null, entries: [{ measure: "fsiq", value: 99, note: "" }], notes: "" };
    mount({ sheet, input: { id: "in-9" } as never });
    expect(await screen.findByText(/לא מעבירה את הכלי הזה/)).toBeTruthy();
  });
});

describe("the score helpers", () => {
  it("reads a band from the table, and a column from a spreadsheet", () => {
    const si = wisc().measures.find((m) => m.key === "similarities");
    expect(si && bandOf(si, 12)?.label).toBe("ממוצע");
    expect(si && bandOf(si, 4)?.label).toBe("נמוך");
    expect(pasteCells("12\r\n9\r\n\r\n11\r\n")).toEqual(["12", "9", "", "11"]);
    expect(pasteCells("8\t10\t14")).toEqual(["8", "10", "14"]);
    expect(pasteCells("12")).toBeNull();
  });

  it("asks only for what is missing in a group she started", () => {
    const i = wisc();
    expect(missingOf(i, {})).toEqual([]);
    const missing = missingOf(i, { similarities: { value: "10", note: "" } }).map((m) => m.key);
    expect(missing).toContain("vocabulary");
    expect(missing).not.toContain("similarities");
    expect(missing).not.toContain("fsiq");
  });
});

describe("ScoresView", () => {
  it("opens on the instrument for the child's age, with the band of each score in words", async () => {
    mount();
    expect(await screen.findByRole("radio", { name: "WISC-V" })).toHaveAttribute("aria-checked", "true");
    await waitFor(() => expect(screen.getByRole("radio", { name: "WISC-V" })).toHaveFocus());
    fireEvent.change(field("שיתופיות"), { target: { value: "14" } });
    fireEvent.change(field("מנת משכל כללית"), { target: { value: "78" } });
    const row = (name: string) => field(name).closest("tr") as HTMLElement;
    expect(within(row("שיתופיות")).getByText("ממוצע גבוה")).toBeInTheDocument();
    expect(within(row("מנת משכל כללית")).getByText("גבולי")).toBeInTheDocument();
    // The profile follows the typing.
    expect(screen.getByRole("img", { name: /פרופיל תת-המבחנים: שיתופיות 14/ })).toBeInTheDocument();
    expect(screen.getByRole("img", { name: /פרופיל המדדים: מנת משכל כללית 78/ })).toBeInTheDocument();
    expect(await a11yViolations(document.body)).toEqual([]);
  });

  it("marks a score that cannot be with a line under its field, keeps a comma score, and saves nothing wrong", async () => {
    mount();
    await screen.findByRole("radio", { name: "WISC-V" });
    fireEvent.change(field("אוצר מילים"), { target: { value: "25" } });
    fireEvent.change(field("מידע"), { target: { value: "9,5" } });
    expect(field("אוצר מילים")).toHaveAttribute("aria-invalid", "true");
    expect(field("אוצר מילים")).toHaveAccessibleDescription("אפשרי 1–19");
    expect(field("מידע")).toHaveAttribute("aria-invalid", "false");
    expect(screen.queryByRole("dialog")).toBeNull();
    await userEvent.setup().click(screen.getByRole("button", { name: "שמירה בתיק" }));
    expect(screen.getByText(/יש ציונים מחוץ לטווח האפשרי/)).toBeInTheDocument();
    expect(saved).toEqual([]);
  });

  it("fills the scores in order from a pasted column, and says what did not fit", async () => {
    mount();
    await screen.findByRole("radio", { name: "WISC-V" });
    const order = tableOrder(wisc());
    const at = order.findIndex((m) => m.key === "similarities");
    fireEvent.paste(field("שיתופיות"), { clipboardData: { getData: () => "12\n9\n\n11\n" } });
    expect(field("שיתופיות")).toHaveValue("12");
    expect(field("אוצר מילים")).toHaveValue("9");
    expect(field("מידע")).toHaveValue("");
    expect(field("הבנה")).toHaveValue("11");
    expect(order[at + 3]?.key).toBe("comprehension");
    expect(screen.getByRole("status")).toHaveTextContent("הודבקו 3 ציונים, לפי הסדר.");
    expect(field(order[at + 4]?.name_he ?? "")).toHaveFocus();
    // From the last field, the rest is said, not dropped silently.
    fireEvent.paste(field("מחיקה"), { clipboardData: { getData: () => "10\n10\n10" } });
    expect(field("מחיקה")).toHaveValue("10");
    expect(screen.getByRole("status")).toHaveTextContent("2 ערכים לא נכנסו");
  });

  it("says in one line what is still missing", async () => {
    mount();
    await screen.findByRole("radio", { name: "WISC-V" });
    expect(screen.queryByText("עוד לא הוזנו:")).toBeNull();
    fireEvent.change(field("שיתופיות"), { target: { value: "10" } });
    const line = screen.getByText("עוד לא הוזנו:").closest("p") as HTMLElement;
    expect(line).toHaveTextContent("אוצר מילים");
    expect(line).not.toHaveTextContent("מנת משכל כללית");
  });

  it("moves to the next score with Enter and to the note with Tab, and saves a whole WISC-V from the keyboard", async () => {
    const { onSaved } = mount();
    const user = userEvent.setup();
    await waitFor(() => expect(screen.getByRole("radio", { name: "WISC-V" })).toHaveFocus());
    await user.tab();
    expect(field("מנת משכל כללית")).toHaveFocus();
    await user.tab();
    expect(screen.getByLabelText("הערה למנת משכל כללית")).toHaveFocus();
    await user.tab({ shift: true });
    const scores: Record<string, string> = {
      similarities: "11", vocabulary: "12", block_design: "9", visual_puzzles: "10", matrix: "8",
      figure_weights: "10", digit_span: "7", picture_span: "9", coding: "6", symbol_search: "8",
    };
    for (const m of tableOrder(wisc())) {
      expect(field(m.name_he)).toHaveFocus();
      await user.keyboard(`${scores[m.key] ?? ""}{Enter}`);
    }
    expect(screen.getByLabelText(/הערות על ההעברה/)).toHaveFocus();
    await user.tab();
    await user.tab();
    expect(screen.getByRole("button", { name: "שמירה בתיק" })).toHaveFocus();
    await user.keyboard("{Enter}");
    await waitFor(() => expect(onSaved).toHaveBeenCalledWith("in-1"));
    expect(saved[0]?.instrument).toBe("wisc_v");
    expect(saved[0]?.entries).toHaveLength(10);
    expect(scoresDraft("c1")).toBeNull();
  });

  it("asks before leaving typed scores, and keeps them for the way back", async () => {
    const { onClose, unmount } = mount();
    await screen.findByRole("radio", { name: "WISC-V" });
    fireEvent.change(field("שיתופיות"), { target: { value: "13" } });
    await userEvent.setup().click(screen.getByRole("button", { name: "ביטול" }));
    expect(screen.getByRole("alertdialog", { name: "לצאת בלי לשמור" })).toBeInTheDocument();
    expect(onClose).not.toHaveBeenCalled();
    // Going to another stage instead: the scores wait in memory.
    unmount();
    expect(scoresDraft("c1")).toEqual({ inputId: null });
    mount();
    await screen.findByRole("radio", { name: "WISC-V" });
    expect(field("שיתופיות")).toHaveValue("13");
    await act(async () => {
      fireEvent.keyDown(field("שיתופיות"), { key: "Escape" });
    });
    await userEvent.setup().click(screen.getByRole("button", { name: "לצאת בלי לשמור" }));
    expect(scoresDraft("c1")).toBeNull();
  });

  it("writes the child's age in words when it is out of the instrument's ages", async () => {
    mount({ age: { years: 5, months: 4 } });
    await userEvent.setup().click(await screen.findByRole("radio", { name: "WISC-V" }));
    expect(screen.getByRole("note")).toHaveTextContent("גיל הילד/ה (5 שנים ו-4 חודשים) מחוץ לטווח הגילים של הכלי (מ-6 שנים עד 16 שנים ו-11 חודשים)");
  });
});
