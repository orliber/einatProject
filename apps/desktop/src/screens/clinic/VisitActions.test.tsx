// What a visit offers on a list (review item 3 of 18.11.2026, D-203): the one step that fits the
// time, and the rest under "⋯" in a fixed order. The names are invented; the clock is given
// (Sunday 15.11.2026 at 10:00), and the chat is never really opened (mocked).
import { configure, render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { useState } from "react";
import { forgetDiscreet, setDiscreet } from "../../discreet";
import type { Today } from "../../ipc/generated/Today";
import type { Visit } from "../../ipc/generated/Visit";
import type { VisitView } from "../../ipc/generated/VisitView";
import { a11yViolations } from "../../test/a11y";
import { appApi, Host, serve, type Override } from "../../test/host";
import { VisitActions } from "./VisitActions";

configure({ asyncUtilTimeout: 5000 });

afterEach(() => {
  forgetDiscreet();
});

const NOW: Today = { date: "2026-11-15", minute: 600 };

/** A visit of נועם, 09:40–10:30 today: under way at 10:00. */
function viewOf(over: Partial<Visit> = {}, hasNote = false): VisitView {
  return {
    visit: {
      id: "v-invented", patient_id: "p-invented", kind: "assessment", title: "", date: "2026-11-15", start: 580, minutes: 50, place: "", status: "planned",
      status_note: "", fee: 0, charge_missed: false, series_id: null, note: "", ...over,
    },
    patient_name: "נועם לדוגמה",
    has_note: hasNote,
    overlaps: false,
  };
}

interface Handlers {
  onHold: ReturnType<typeof vi.fn<(v: Visit) => void>>;
  onNote: ReturnType<typeof vi.fn<(v: Visit) => void>>;
  onOpen: ReturnType<typeof vi.fn<() => void>>;
  onNoShow: ReturnType<typeof vi.fn<(v: Visit) => void>>;
}

/** The screen around the actions: a hold marks the visit at once, as the clinic's lists do. */
function Harness({ initial, now, on, card = false }: { initial: VisitView; now: Today; on: Handlers; card?: boolean }) {
  const [view, setView] = useState(initial);
  return (
    <div className="row">
      <VisitActions view={view} patientId="p-invented" now={now}
        onHold={(v) => { on.onHold(v); setView((w) => ({ ...w, visit: { ...v, status: "held" } })); }}
        onNote={on.onNote} onOpen={on.onOpen}
        {...(card ? { onNoShow: on.onNoShow, reminders: false, onCard: true } : {})} />
    </div>
  );
}

/** The chat opens with an invented parent (the visit's patient is not in the demo core). */
const opened: Override = (cmd) => (cmd === "send_message" ? { to: "יעל לדוגמה", clears_in: 20 } : undefined);

async function show(initial: VisitView, { now = NOW, card = false }: { now?: Today; card?: boolean } = {}) {
  const served = await serve(opened);
  const on: Handlers = { onHold: vi.fn<(v: Visit) => void>(), onNote: vi.fn<(v: Visit) => void>(), onOpen: vi.fn<() => void>(), onNoShow: vi.fn<(v: Visit) => void>() };
  const notes: string[] = [];
  const app = appApi({ notify: (text: string) => void notes.push(text) });
  const user = userEvent.setup();
  const view = render(<Host app={app}><Harness initial={initial} now={now} on={on} card={card} /></Host>);
  /** The same screen with another visit, or later (a new key: the harness starts over). */
  const rerender = (next: VisitView, at: Today = now) =>
    view.rerender(<Host app={app}><Harness key={JSON.stringify([next, at])} initial={next} now={at} on={on} card={card} /></Host>);
  return { ...served, on, notes, user, view, rerender };
}

const more = () => screen.getByRole("button", { name: /^עוד פעולות לפגישה/ });

/** The words of "⋯", in order; the menu is closed again after. */
async function menuOf(user: ReturnType<typeof userEvent.setup>): Promise<string[]> {
  await user.click(more());
  const menu = await screen.findByRole("menu");
  const words = within(menu).getAllByRole("menuitem").map((i) => i.textContent ?? "");
  await user.keyboard("{Escape}");
  await waitFor(() => expect(screen.queryByRole("menu")).toBeNull());
  return words;
}

const REMINDERS = ["לשלוח תזכורת בוואטסאפ", "לשלוח תזכורת ב-SMS", "לשלוח תזכורת במייל", "להעתיק את התזכורת"];

describe("one action by the time, the rest under “⋯”", () => {
  it("a planned visit under way shows a plain “התקיימה”; once it is over, the same button is the primary", async () => {
    const { view, rerender } = await show(viewOf());
    expect(within(view.container).getByRole("button", { name: "התקיימה" })).not.toHaveClass("btn-primary");
    // 10:30: it is over and still not marked.
    rerender(viewOf(), { ...NOW, minute: 630 });
    expect(within(view.container).getByRole("button", { name: "התקיימה" })).toHaveClass("btn-primary");
    // No reminder once it has begun.
    expect(within(view.container).queryByRole("button", { name: "תזכורת" })).toBeNull();
  });

  it("a visit still ahead shows “תזכורת” (the SendMenu trigger), and its “⋯” offers “התקיימה” only when it is today", async () => {
    const { user, view, rerender } = await show(viewOf({ start: 660 }));
    expect(within(view.container).getByRole("button", { name: "תזכורת" })).toHaveAttribute("aria-haspopup", "menu");
    expect(within(view.container).queryByRole("button", { name: "התקיימה" })).toBeNull();
    expect(await menuOf(user)).toContain("התקיימה");
    rerender(viewOf({ date: "2026-11-16", start: 540 }));
    expect(within(view.container).getByRole("button", { name: "תזכורת" })).toBeInTheDocument();
    expect(await menuOf(user)).not.toContain("התקיימה");
  });

  it("after the hold, the same button element keeps the focus and reads “לכתוב סיכום”", async () => {
    const { user, view, on } = await show(viewOf());
    const button = within(view.container).getByRole("button", { name: "התקיימה" });
    await user.click(button);
    expect(on.onHold).toHaveBeenCalledTimes(1);
    expect(within(view.container).getByRole("button", { name: "לכתוב סיכום" })).toBe(button);
    expect(button).toHaveFocus();
    expect(button).not.toHaveClass("btn-primary");
    await user.click(button);
    expect(on.onNote).toHaveBeenCalledTimes(1);
  });

  it("a held visit with a note shows only “⋯”, with “לפתוח את הסיכום” and “לפתוח את הפגישה”", async () => {
    const { user, view, on } = await show(viewOf({ status: "held" }, true));
    expect(within(view.container).getAllByRole("button")).toEqual([more()]);
    expect(await menuOf(user)).toEqual(["לפתוח את הסיכום", "לפתוח את הפגישה"]);
    await user.click(more());
    await user.click(await screen.findByRole("menuitem", { name: "לפתוח את הפגישה" }));
    expect(on.onOpen).toHaveBeenCalledTimes(1);
  });

  it("a cancelled visit's “⋯” holds only “לפתוח את הפגישה”", async () => {
    const { user, view } = await show(viewOf({ status: "cancelled" }));
    expect(within(view.container).getAllByRole("button")).toEqual([more()]);
    expect(await menuOf(user)).toEqual(["לפתוח את הפגישה"]);
  });

  it("menu order: “התקיימה”, “לכתוב סיכום”, the four reminder items, “לפתוח את הפגישה”", async () => {
    // A planned visit offers either "התקיימה" (later today) or the reminders (once it has begun)
    // in its "⋯", never both: each menu keeps its part of the one order.
    const { user, rerender } = await show(viewOf({ start: 660 }));
    expect(await menuOf(user)).toEqual(["התקיימה", "לכתוב סיכום", "לפתוח את הפגישה"]);
    rerender(viewOf());
    expect(await menuOf(user)).toEqual(["לכתוב סיכום", ...REMINDERS, "לפתוח את הפגישה"]);
  });

  it("the reminder in “⋯” opens the chat for this visit", async () => {
    const { user, notes, calls } = await show(viewOf());
    await user.click(more());
    await user.click(await screen.findByRole("menuitem", { name: "לשלוח תזכורת בוואטסאפ" }));
    await waitFor(() => expect(notes).toHaveLength(1));
    expect(notes[0]).toMatch(/^וואטסאפ נפתח מול יעל לדוגמה\. התזכורת בלוח המוגן/);
    const sent = calls.filter((c) => c.cmd === "send_message");
    expect(sent).toHaveLength(1);
    expect(sent[0]?.args).toMatchObject({ kind: "reminder", patientId: "p-invented", visitId: "v-invented", channel: "whatsapp" });
  });

  it("reminders={false} with onNoShow: no reminder items, “לא הגיעו” is offered, and a visit later today has no primary", async () => {
    const { user, view, on } = await show(viewOf({ start: 660 }), { card: true });
    expect(within(view.container).getAllByRole("button")).toEqual([more()]);
    expect(await menuOf(user)).toEqual(["התקיימה", "לא הגיעו", "לכתוב סיכום", "לפתוח את הפגישה"]);
    await user.click(more());
    await user.click(await screen.findByRole("menuitem", { name: "לא הגיעו" }));
    expect(on.onNoShow).toHaveBeenCalledTimes(1);
  });

  it("onCard names the menu by date", async () => {
    await show(viewOf({ start: 660 }), { card: true });
    expect(more()).toHaveAccessibleName(/^עוד פעולות לפגישה מיום 15\.11\.2026/);
    expect(more()).toHaveAccessibleName("עוד פעולות לפגישה מיום 15.11.2026, 11:00");
  });

  it("in discreet mode the menu's name has initials and no element has a title with the name", async () => {
    setDiscreet(true);
    const { user, view } = await show(viewOf({ start: 660 }));
    expect(more()).toHaveAccessibleName("עוד פעולות לפגישה של נ. ל., 11:00");
    await user.click(more());
    await screen.findByRole("menu");
    for (const el of document.querySelectorAll("[title], [aria-label]")) {
      expect(`${el.getAttribute("title") ?? ""} ${el.getAttribute("aria-label") ?? ""}`).not.toMatch(/נועם|לדוגמה/);
    }
    expect(document.body.textContent).not.toMatch(/נועם|לדוגמה/);
    expect(view.container.querySelectorAll("[title]").length).toBeGreaterThan(0); // "תזכורת" keeps its own title
  });

  it("passes the accessibility check", async () => {
    const { user, view, rerender } = await show(viewOf({ start: 660 }));
    expect(await a11yViolations(view.container)).toEqual([]);
    rerender(viewOf());
    expect(await a11yViolations(view.container)).toEqual([]);
    await user.click(more());
    await screen.findByRole("menu");
    expect(await a11yViolations(document.body)).toEqual([]);
  });
});
