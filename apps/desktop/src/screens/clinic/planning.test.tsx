// Planning in the clinic over the demo core (D-196..D-199): the starter kit and the whole
// pathway from a card, free times in the visit dialog, the holidays in the calendar, and the
// new settings. Every name is invented; the clock is held at Sunday 15.11.2026, 08:00.
import { configure, screen, waitFor, within } from "@testing-library/react";
import { a11yViolations } from "../../test/a11y";
import { forgetDiscreet } from "./ClinicScreen";
import { forgetSettingsDraft } from "./ClinicSettingsTab";
import { ask, cardOf, NOW, openCard, openOn } from "./patientsTestKit";

configure({ asyncUtilTimeout: 5000 });
vi.setConfig({ testTimeout: 30_000 });

beforeEach(() => {
  vi.useFakeTimers({ toFake: ["Date"] });
  vi.setSystemTime(new Date(2026, 10, 15, 8, 0));
});

afterEach(() => {
  vi.useRealTimers();
  forgetDiscreet();
  forgetSettingsDraft();
  window.history.replaceState({}, "", "/");
});

describe("planning from a card", () => {
  it("opens the starter kit once, as follow-ups with due dates", async () => {
    const { user, notes, id, ...served } = await openCard("עומר");
    const before = (await cardOf(served, id)).tasks.length;
    await user.click(await screen.findByRole("button", { name: "ערכת פתיחה" }));
    await waitFor(() => expect(notes).toContain("נפתחו 4 מעקבים מערכת הפתיחה"));
    const tasks = (await cardOf(served, id)).tasks;
    expect(tasks.length).toBe(before + 4);
    expect(tasks.map((t) => t.title)).toEqual(expect.arrayContaining(["שאלון להורים", "שאלון למורה או לגננת"]));
    expect(tasks.find((t) => t.title === "שאלון להורים")?.due).toBe("2026-11-22");
    // Asking again opens nothing, and says why.
    await user.click(screen.getByRole("button", { name: "ערכת פתיחה" }));
    await waitFor(() => expect(notes).toContain("כל המעקבים של ערכת הפתיחה כבר פתוחים בכרטיס הזה."));
    expect((await cardOf(served, id)).tasks.length).toBe(before + 4);
  });

  it("proposes the whole pathway, lets her look, and saves it all at once", async () => {
    const { user, notes, id, view, ...served } = await openCard("עומר");
    const before = (await cardOf(served, id)).visits.length;
    await user.click(await screen.findByRole("button", { name: "לקבוע את כל המסלול" }));
    const dialog = await screen.findByRole("dialog", { name: /לקבוע את מסלול האבחון/ });
    const rows = await within(dialog).findByRole("list", { name: "פגישות המסלול" });
    const items = within(rows).getAllByRole("group");
    expect(items.map((g) => g.getAttribute("aria-label"))).toEqual(["פגישה 1: פגישת היכרות", "פגישה 2: מפגש אבחון", "פגישה 3: מפגש אבחון", "פגישה 4: שיחת משוב"]);
    // Nothing is in the vault before she says so.
    expect((await cardOf(served, id)).visits.length).toBe(before);
    expect(await a11yViolations(view.container)).toEqual([]);
    await user.click(within(dialog).getByRole("button", { name: "לקבוע 4 פגישות" }));
    await waitFor(() => expect(notes.some((n) => n.startsWith("נקבעו 4 פגישות של מסלול האבחון"))).toBe(true));
    const card = await cardOf(served, id);
    expect(card.visits.length).toBe(before + 4);
    expect(card.patient.status).toBe("active");
  });

  it("closes at once when the proposal was not touched; a step moved to a day off says so, and closing then asks", async () => {
    const { user } = await openCard("עומר");
    await user.click(await screen.findByRole("button", { name: "לקבוע את כל המסלול" }));
    let dialog = await screen.findByRole("dialog", { name: /לקבוע את מסלול האבחון/ });
    await within(dialog).findByRole("list", { name: "פגישות המסלול" });
    await user.keyboard("{Escape}");
    await waitFor(() => expect(screen.queryByRole("dialog", { name: /לקבוע את מסלול האבחון/ })).toBeNull());

    await user.click(screen.getByRole("button", { name: "לקבוע את כל המסלול" }));
    dialog = await screen.findByRole("dialog", { name: /לקבוע את מסלול האבחון/ });
    const rows = await within(dialog).findByRole("list", { name: "פגישות המסלול" });
    const second = within(rows).getAllByRole("group")[1] as HTMLElement;
    const date = within(second).getByLabelText("תאריך");
    await user.clear(date);
    // 05.12.2026 is a Saturday; the demo clinic works Sunday to Thursday.
    await user.type(date, "05.12.2026");
    expect(await within(second).findByText("שבת אינה יום עבודה בהגדרות הקליניקה.")).toBeInTheDocument();
    await user.keyboard("{Escape}");
    expect(within(dialog).getByRole("alertdialog", { name: "לסגור בלי לשמור" })).toBeInTheDocument();
  });

  it("a step moved and put back as it was is not a change: closing does not ask", async () => {
    const { user } = await openCard("עומר");
    await user.click(await screen.findByRole("button", { name: "לקבוע את כל המסלול" }));
    const dialog = await screen.findByRole("dialog", { name: /לקבוע את מסלול האבחון/ });
    const rows = await within(dialog).findByRole("list", { name: "פגישות המסלול" });
    const second = within(rows).getAllByRole("group")[1] as HTMLElement;
    const date = within(second).getByLabelText("תאריך");
    const was = (date as HTMLInputElement).value;
    await user.clear(date);
    await user.type(date, "05.12.2026");
    await user.clear(date);
    await user.type(date, was);
    await user.keyboard("{Escape}");
    await waitFor(() => expect(screen.queryByRole("dialog", { name: /לקבוע את מסלול האבחון/ })).toBeNull());
  });

  it("asks for a start day when the box is empty, and offers nothing to save", async () => {
    const { user } = await openCard("עומר");
    await user.click(await screen.findByRole("button", { name: "לקבוע את כל המסלול" }));
    const dialog = await screen.findByRole("dialog", { name: /לקבוע את מסלול האבחון/ });
    await within(dialog).findByRole("list", { name: "פגישות המסלול" });
    await user.clear(within(dialog).getByLabelText("מתחילים מיום"));
    expect(await within(dialog).findByText(/צריך לכתוב את היום שמתחילים ממנו/)).toBeInTheDocument();
    expect(within(dialog).queryByRole("list", { name: "פגישות המסלול" })).toBeNull();
    expect(within(dialog).getByRole("button", { name: "לקבוע" })).toBeDisabled();
  });

  it("writes none of the pathway when one of its visits is refused", async () => {
    const { id, ...served } = await openCard("עומר");
    const before = await cardOf(served, id);
    const visits = await ask(served, { op: "propose_pathway", patient_id: id, today: NOW, from: "2026-11-16" }, "proposed");
    // A year the clinic does not keep, on the third visit only.
    const bad = visits.map((v, i) => (i === 2 ? { ...v, date: "2300-01-01" } : v));
    await expect(ask(served, { op: "save_pathway", patient_id: id, visits: bad, package: false, today: NOW }, "saved")).rejects.toBeTruthy();
    const after = await cardOf(served, id);
    expect(after.visits.length).toBe(before.visits.length);
    expect(after.patient.status).toBe(before.patient.status);
  });
});

describe("free times and holidays", () => {
  it("offers free times in a new visit, and a pick fills the date and the hour", async () => {
    const { user } = await openCard("נועם");
    await user.click((await screen.findAllByRole("button", { name: /פגישה חדשה/ }))[0] as HTMLElement);
    const dialog = await screen.findByRole("dialog", { name: "פגישה חדשה" });
    await user.click(await within(dialog).findByRole("button", { name: "להציע שעות פנויות" }));
    const offer = await within(dialog).findByRole("group", { name: "שעות פנויות ביומן" });
    const first = (await within(offer).findAllByRole("button", { name: /בשעה/ }))[0] as HTMLElement;
    const label = first.getAttribute("aria-label") ?? "";
    const hour = /בשעה (\d\d:\d\d)/.exec(label)?.[1] ?? "?";
    await user.click(first);
    expect(within(dialog).getByLabelText("שעת התחלה")).toHaveValue(hour);
  });

  it("a free time on another day shows its date in the date box", async () => {
    const { user } = await openCard("נועם");
    await user.click((await screen.findAllByRole("button", { name: /פגישה חדשה/ }))[0] as HTMLElement);
    const dialog = await screen.findByRole("dialog", { name: "פגישה חדשה" });
    const shown = (within(dialog).getByLabelText("תאריך") as HTMLInputElement).value;
    await user.click(await within(dialog).findByRole("button", { name: "להציע שעות פנויות" }));
    const offer = await within(dialog).findByRole("group", { name: "שעות פנויות ביומן" });
    const slots = await within(offer).findAllByRole("button", { name: /בשעה/ });
    const other = slots.find((b) => !(b.getAttribute("aria-label") ?? "").includes(shown));
    const date = /(\d\d\.\d\d\.\d{4})/.exec(other?.getAttribute("aria-label") ?? "")?.[1];
    expect(date).toBeDefined();
    await user.click(other as HTMLElement);
    expect(within(dialog).getByLabelText("תאריך")).toHaveValue(date);
    // The keyboard stays with the free times, not thrown back to the top of the form.
    expect(document.activeElement).toBe(within(dialog).getByRole("group", { name: "שעות פנויות ביומן" }));
    // Hidden while the length is not a number: the focus goes back to the button that opens them.
    await user.clear(within(dialog).getByLabelText("משך (דקות)"));
    await user.click(within(dialog).getByRole("button", { name: "להסתיר" }));
    const opener = within(dialog).getByRole("button", { name: "להציע שעות פנויות" });
    expect(document.activeElement).toBe(opener);
    expect(opener).toHaveAttribute("aria-disabled", "true");
  });

  it("names the holidays in the calendar's days", async () => {
    const { user } = await openOn({ name: "clinic", tab: "calendar" });
    await user.click(await screen.findByRole("radio", { name: "חודש" }));
    // December 2026 holds Hanukkah.
    await user.click(screen.getByRole("button", { name: "החודש הבא" }));
    expect(await screen.findByText("חנוכה (נר 1)")).toBeInTheDocument();
  });
});

describe("the new settings", () => {
  it("keeps a longer pathway, a package and a new starter follow-up", async () => {
    const { user, notes, ...served } = await openOn({ name: "settings", pane: "clinic-pathway" });
    await user.click(await screen.findByRole("button", { name: "שלב נוסף" }));
    const fee = screen.getByLabelText("מחיר חבילת אבחון (₪)");
    await user.type(fee, "4500");
    await user.click(screen.getByRole("button", { name: "מעקב נוסף" }));
    await user.type(screen.getByLabelText("מה לבקש", { selector: "#settings-starter-4-title" }), "סיכום מהמרפאה בעיסוק");
    await user.click(screen.getByRole("button", { name: "שמירה" }));
    await waitFor(() => expect(notes).toContain("הגדרות הקליניקה נשמרו"));
    const settings = await ask(served, { op: "settings" }, "settings");
    expect(settings.pathway.length).toBe(5);
    expect(settings.package_fee).toBe(450_000);
    expect(settings.starter_tasks.map((t) => t.title)).toContain("סיכום מהמרפאה בעיסוק");
    void NOW;
  });
});
