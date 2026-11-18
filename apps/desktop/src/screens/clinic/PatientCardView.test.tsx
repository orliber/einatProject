// A patient's card over the demo core: what she sees of the child, the assessment's steps,
// the link to the diagnostic case, visits, session notes, money, follow-ups, erasing, the
// failure paths with their rollback, discreet mode and accessibility. Every name is invented;
// the clock is held at a Sunday morning so the demo clinic's dates are the same on every run.
import { configure, screen, waitFor, within } from "@testing-library/react";
import type { ClinicRequest } from "../../ipc/generated/ClinicRequest";
import { a11yViolations } from "../../test/a11y";
import type { Override } from "../../test/host";
import { forgetDiscreet } from "./ClinicScreen";
import { ask, cardOf, openCard, openOn, refusing, rowsOf } from "./patientsTestKit";

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

const region = (name: string) => screen.findByRole("region", { name });
/** The row of a visit on the card, by its date. */
const visitOn = async (date: string) => within(await region("פגישות")).getByText(date).closest("li") as HTMLElement;

describe("the patient's card", () => {
  it("reads like the child's story: who, how to reach the family, where the assessment stands", async () => {
    const { view } = await openCard("נועם");
    expect(await screen.findByRole("heading", { name: "נועם" })).toBeInTheDocument();
    const head = screen.getByRole("heading", { name: "נועם" }).closest(".pcard-head") as HTMLElement;
    expect(head).toHaveTextContent("בתהליך");
    expect(head).toHaveTextContent(/בן 5 שנים/);
    expect(head).toHaveTextContent("גן חצב");
    expect(head).toHaveTextContent("קושי במעברים ובוויסות בגן. הגננת ביקשה הערכה.");
    expect(head).toHaveTextContent("רופאת הילדים");
    expect(head).toHaveTextContent(/נולד \d\d\.\d\d\.\d{4}/);
    const contacts = within(head).getByRole("list", { name: "אנשי קשר" });
    // The copy button is an icon with a name of its own, so it adds no text to the line.
    expect(within(contacts).getAllByRole("listitem").map((li) => li.textContent)).toEqual(["אמאמיכל050-555-01", "אבאאלון052-555-02"]);
    // The details are one band of cells under the name, each with its own small heading.
    expect(within(head).getAllByRole("heading", { level: 3 }).map((h) => h.textContent)).toEqual(["אנשי קשר", "סיבת הפנייה", "מי הפנה", "תיק האבחון"]);

    // The assessment as steps: each says where it stands in words, and one is "now".
    const steps = within(screen.getByRole("list", { name: "שלבי האבחון" })).getAllByRole("listitem");
    expect(steps.map((s) => s.textContent)).toEqual([
      "✓פגישת היכרות: הושלםהתקיימה ביום 11.10.2026",
      "✓מפגשי אבחון: הושלם2 מפגשים התקיימו",
      expect.stringMatching(/^●כתיבת הדוח: עכשיוהדוח: \d+ מתוך \d+ סעיפים$/),
      "●שיחת משוב: עכשיונקבעה ליום 20.11.2026",
      "●תשלום: עכשיונותרו 600 ₪ לתשלום",
    ]);
    expect(steps[0]).not.toHaveAttribute("aria-current");
    expect(steps[3]).toHaveAttribute("aria-current", "step");

    // What needs her, for this child only, and without the child's name: it is the heading.
    const needs = await region("מה דורש אותי");
    expect(within(needs).getByText("שיחת משוב קרובה")).toBeInTheDocument();
    expect(within(needs).getByText("פגישה בלי סיכום")).toBeInTheDocument();
    expect(within(needs).queryByText("המתנה ארוכה")).toBeNull();
    expect(needs).not.toHaveTextContent("נועם");
    expect(await a11yViolations(view.container)).toEqual([]);
  });

  it("the head is a plain box, not a <header>: the case cell's dialogs, with a header and a footer of their own, open inside it", async () => {
    const { user } = await openCard("איתי");
    const head = (await screen.findByRole("heading", { name: "איתי" })).closest(".pcard-head") as HTMLElement;
    expect(head.tagName).toBe("DIV");
    expect(head.closest("header")).toBeNull();
    expect(screen.queryAllByRole("banner").filter((b) => b.contains(head))).toEqual([]);
    // The picker of cases opens in the band, inside the head: its own header is the only one.
    await user.click(within(head).getByRole("button", { name: "לקשר לתיק קיים" }));
    const picker = await screen.findByRole("dialog");
    expect(head).toContainElement(picker);
    expect([...head.querySelectorAll("header")].every((h) => picker.contains(h))).toBe(true);
  });

  it("every button that shows a word is named starting with that word", async () => {
    const { user } = await openCard("נועם");
    // "עריכה" of a note and of a follow-up says what it edits after the word she sees.
    const notes = await region("סיכומי פגישות");
    const tasks = await region("מעקבים");
    const edits = [...within(notes).getAllByRole("button", { name: /^עריכה/ }), ...within(tasks).queryAllByRole("button", { name: /^עריכה/ })];
    expect(edits.length).toBeGreaterThan(0);
    for (const b of [...notes.querySelectorAll("button"), ...tasks.querySelectorAll("button")].filter((b) => b.textContent === "עריכה")) {
      expect(edits).toContain(b);
    }
    expect(within(notes).getByRole("button", { name: "עריכה של הסיכום מיום 25.10.2026" })).toHaveTextContent(/^עריכה$/);
    await user.click(within(tasks).getByRole("button", { name: "מעקב חדש" }));
    const add = await screen.findByRole("dialog", { name: "מעקב חדש" });
    await user.type(within(add).getByLabelText("מה צריך לעשות"), "לבקש סיכום מהגננת");
    await user.click(within(add).getByRole("button", { name: "הוספה" }));
    expect(await within(tasks).findByRole("button", { name: "עריכה של המעקב: לבקש סיכום מהגננת" })).toHaveTextContent(/^עריכה$/);
    // A visit's time shows its date above its hours; the name starts with both, as she sees them.
    const visits = await region("פגישות");
    const times = within(visits).getAllByRole("button", { name: /פתיחת הפגישה/ });
    expect(times.length).toBeGreaterThan(1);
    for (const time of times) {
      const seen = time.textContent ?? "";
      expect(seen).toMatch(/^\d\d\.\d\d\.\d{4} \d\d:\d\d–\d\d:\d\d$/);
      expect(time.getAttribute("aria-label")).toMatch(new RegExp(`^${seen.replace(/[.]/g, "\\.")}, `));
    }
  });

  it("shows the shape of the card while it loads, and says so when the card is not there", async () => {
    const { user, went } = await openOn({ name: "clinic", tab: "patients", patientId: "patient-nowhere" });
    expect(await screen.findByRole("heading", { name: "הכרטיס לא נמצא" })).toBeInTheDocument();
    await user.click(screen.getByRole("button", { name: "חזרה לרשימת המטופלים" }));
    expect(went[went.length - 1]).toEqual({ name: "clinic", tab: "patients" });
  });

  it("a phone number is copied through the protected clipboard only", async () => {
    const { user, calls, notes } = await openCard("נועם");
    await user.click(await screen.findByRole("button", { name: "העתקת הטלפון של אמא מיכל" }));
    await waitFor(() => expect(notes).toEqual(["מספר הטלפון הועתק ללוח המוגן. יימחק מהלוח בעוד דקה."]));
    expect(calls.filter((c) => c.cmd === "copy_secret").map((c) => c.args)).toEqual([{ text: "050-555-01" }]);
  });

  it("the header's actions: a new visit for this child, editing the details, closing and reopening", async () => {
    const served = await openCard("איתי");
    const { user, requests, notes, id } = served;
    const head = (await screen.findByRole("heading", { name: "איתי" })).closest(".pcard-head") as HTMLElement;

    await user.click(within(head).getByRole("button", { name: "פגישה חדשה" }));
    const visit = await screen.findByRole("dialog", { name: "פגישה חדשה" });
    expect(await within(visit).findByRole("group", { name: "מטופל" })).toHaveTextContent("איתי");
    await user.click(within(visit).getByRole("button", { name: "ביטול" }));

    await user.click(within(head).getByRole("button", { name: "עריכת הפרטים" }));
    const edit = await screen.findByRole("dialog", { name: "פרטי המטופל" });
    expect(within(edit).getByLabelText("שם הילד או הילדה")).toHaveValue("איתי");
    await user.click(within(edit).getByRole("button", { name: "ביטול" }));

    // Closing is the rare step: it waits under "⋯".
    expect(within(head).queryByRole("button", { name: "סגירת הכרטיס" })).toBeNull();
    await user.click(within(head).getByRole("button", { name: "עוד פעולות לכרטיס, כמו סגירת הכרטיס" }));
    expect((await screen.findAllByRole("menuitem")).map((m) => m.textContent)).toEqual(["סגירת הכרטיס"]);
    await user.click(screen.getByRole("menuitem", { name: "סגירת הכרטיס" }));
    const confirm = await screen.findByRole("dialog", { name: "סגירת הכרטיס" });
    expect(confirm).toHaveTextContent("שום דבר לא נמחק");
    expect(confirm).toHaveTextContent("יש עוד פגישה מתוכננת אחת ביומן.");
    expect(requests().some((r) => r.op === "save_patient")).toBe(false);
    await user.click(within(confirm).getByRole("button", { name: "לסגור את הכרטיס" }));
    await waitFor(() => expect(screen.queryByRole("dialog")).toBeNull());
    expect(requests().find((r) => r.op === "save_patient")).toMatchObject({ patient: { id, status: "closed" } });
    expect(await within(head).findByText("הסתיים")).toBeInTheDocument();
    // The same "⋯" stays, named for what it now offers, and the focus is back on it.
    const more = within(head).getByRole("button", { name: "עוד פעולות לכרטיס, כמו פתיחה מחדש" });
    await waitFor(() => expect(more).toHaveFocus());

    await user.click(more);
    await user.click(await screen.findByRole("menuitem", { name: "פתיחה מחדש" }));
    expect(await within(head).findByText("בתהליך")).toBeInTheDocument();
    expect(notes).toContain("הכרטיס נפתח מחדש");
    expect((await cardOf(served, id)).patient.status).toBe("active");
  });

  it("a card with no one who referred has no “מי הפנה”", async () => {
    await openCard("מאיה");
    const head = (await screen.findByRole("heading", { name: "מאיה" })).closest(".pcard-head") as HTMLElement;
    // An empty detail is left out of the band, not shown as a blank cell.
    expect(within(head).getAllByRole("heading", { level: 3 }).map((h) => h.textContent)).toEqual(["אנשי קשר", "סיבת הפנייה", "תיק האבחון"]);
    expect(head).not.toHaveTextContent("מי הפנה");
    expect(head).toHaveTextContent("ליווי אחרי אבחון.");
    expect(within(head).getByRole("region", { name: "תיק האבחון" })).toHaveTextContent("עוד אין תיק.");
  });

  it("a long reason folds, and “להציג הכל” opens it", async () => {
    const long = Array.from({ length: 6 }, (_, i) => `שורה ${i + 1} בסיבת הפנייה של ילד בדוי.`).join("\n");
    const { user } = await openCard("נועם", undefined, async (served, id) => {
      const { patient } = await cardOf(served, id);
      await ask(served, { op: "save_patient", patient: { ...patient, reason: long } }, "patient");
    });
    const head = (await screen.findByRole("heading", { name: "נועם" })).closest(".pcard-head") as HTMLElement;
    const cell = within(head).getByRole("heading", { name: "סיבת הפנייה" }).parentElement as HTMLElement;
    const text = within(cell).getByText(/שורה 1 בסיבת הפנייה/);
    expect(text).toHaveClass("pcard-note-folded");
    const more = within(cell).getByRole("button", { name: "להציג הכל" });
    expect(more).toHaveAttribute("aria-expanded", "false");
    expect(more).toHaveAttribute("aria-controls", text.id);
    await user.click(more);
    expect(text).not.toHaveClass("pcard-note-folded");
    expect(within(cell).getByRole("button", { name: "לקצר" })).toHaveAttribute("aria-expanded", "true");
    // A short one is whole from the start, with nothing to open.
    const referred = within(head).getByRole("heading", { name: "מי הפנה" }).parentElement as HTMLElement;
    expect(within(referred).queryByRole("button")).toBeNull();
  });

  it("passes the accessibility check with the header's menu open", async () => {
    const { user } = await openCard("נועם");
    const head = (await screen.findByRole("heading", { name: "נועם" })).closest(".pcard-head") as HTMLElement;
    const more = within(head).getByRole("button", { name: "עוד פעולות לכרטיס, כמו סגירת הכרטיס" });
    await user.click(more);
    expect(await screen.findByRole("menuitem", { name: "סגירת הכרטיס" })).toBeInTheDocument();
    expect(await a11yViolations(document.body)).toEqual([]);
    // "ביטול" in the dialog it opens gives the focus back to the "⋯" too.
    await user.click(screen.getByRole("menuitem", { name: "סגירת הכרטיס" }));
    await user.click(within(await screen.findByRole("dialog", { name: "סגירת הכרטיס" })).getByRole("button", { name: "ביטול" }));
    expect(screen.queryByRole("dialog")).toBeNull();
    // (The button is built anew the first time its menu opens, so it is looked up again.)
    await waitFor(() => expect(within(head).getByRole("button", { name: "עוד פעולות לכרטיס, כמו סגירת הכרטיס" })).toHaveFocus());
  });
});

describe("the diagnostic case on the card", () => {
  it("a linked case shows its code and how many sections are approved, and opens where the case list would open it", async () => {
    const served = await openCard("נועם");
    const { user, went, caseId, id } = served;
    const link = (await cardOf(served, id)).case;
    const box = await region("תיק האבחון");
    expect(box).toHaveTextContent(link?.code ?? "?");
    expect(link?.total).toBeGreaterThan(0);
    expect(box).toHaveTextContent(`${link?.approved} מתוך ${link?.total} סעיפים אושרו`);

    await user.click(within(box).getByRole("button", { name: "לפתוח את התיק" }));
    expect(went[went.length - 1]).toEqual({ name: "case", id: caseId, view: (link?.approved ?? 0) > 0 ? "report" : "materials" });
  });

  it("unlinking asks first and keeps the case; a free case can be linked from a small picker", async () => {
    const served = await openCard("נועם");
    const { user, requests, notes, caseId, id } = served;
    const box = await region("תיק האבחון");
    await user.click(await within(box).findByRole("button", { name: "לנתק" }));
    const confirm = await screen.findByRole("dialog", { name: "לנתק את התיק מהכרטיס" });
    expect(confirm).toHaveTextContent("התיק עצמו לא נמחק ולא משתנה");
    await user.click(within(confirm).getByRole("button", { name: "לנתק" }));
    await waitFor(() => expect(screen.queryByRole("dialog")).toBeNull());
    expect(requests().find((r) => r.op === "link_case")).toEqual({ op: "link_case", patient_id: id, case_id: null });
    expect(notes).toContain("התיק נותק מהכרטיס");
    expect(await within(box).findByText("עוד אין תיק.")).toBeInTheDocument();

    await user.click(within(box).getByRole("button", { name: "לקשר לתיק קיים" }));
    const picker = await screen.findByRole("dialog", { name: "לקשר לתיק קיים" });
    const choice = await within(picker).findByRole("button", { name: /^נועם · / });
    await user.click(choice);
    await waitFor(() => expect(screen.queryByRole("dialog")).toBeNull());
    expect(requests().filter((r) => r.op === "link_case")[1]).toEqual({ op: "link_case", patient_id: id, case_id: caseId });
    expect(await within(box).findByRole("button", { name: "לפתוח את התיק" })).toBeInTheDocument();
  });

  it("a card without a case opens one, says so, and offers to go to it", async () => {
    const served = await openCard("תמר");
    const { user, went, requests, notes, id } = served;
    const box = await region("תיק האבחון");
    await user.click(await within(box).findByRole("button", { name: "לפתוח תיק" }));
    expect(await within(box).findByRole("status")).toHaveTextContent("התיק נפתח, והוא מקושר לכרטיס.");
    expect(requests().find((r) => r.op === "create_case")).toMatchObject({ patient_id: id, today: { date: "2026-11-15" } });
    expect(notes).toContain("תיק האבחון נפתח");
    expect(box).toHaveTextContent("תיק-2026-1115");
    const made = (await cardOf(served, id)).case;
    expect(box).toHaveTextContent(`0 מתוך ${made?.total} סעיפים אושרו`);
    await user.click(within(box).getByRole("button", { name: "לעבור לתיק" }));
    expect(went[went.length - 1]).toEqual({ name: "case", id: made?.id, view: "materials" });
  });

  it("a refusal to open a case is said in the box, and nothing is claimed", async () => {
    const { user, notes } = await openCard("תמר", refusing("create_case", "התיק לא נפתח."));
    const box = await region("תיק האבחון");
    await user.click(await within(box).findByRole("button", { name: "לפתוח תיק" }));
    expect(await within(box).findByText("התיק לא נפתח.")).toBeInTheDocument();
    expect(within(box).queryByRole("status")).toBeNull();
    expect(notes).not.toContain("תיק האבחון נפתח");
  });

  it("a case in the recycle bin is said plainly, with the way to the case list", async () => {
    const { user, went } = await openCard("נועם", undefined, async (served) => {
      await served.core.handle("delete_case", { caseId: served.caseId });
    });
    const box = await region("תיק האבחון");
    expect(await within(box).findByText(/נמצא בסל המחזור/)).toBeInTheDocument();
    expect(within(box).queryByRole("button", { name: "לפתוח את התיק" })).toBeNull();
    expect(within(box).getByRole("button", { name: "לנתק" })).toBeInTheDocument();
    await user.click(within(box).getByRole("button", { name: "לרשימת התיקים" }));
    expect(went[went.length - 1]).toEqual({ name: "cases" });
  });

  it("the picker says when there is no case to link", async () => {
    const { user } = await openCard("תמר");
    const box = await region("תיק האבחון");
    await user.click(await within(box).findByRole("button", { name: "לקשר לתיק קיים" }));
    const picker = await screen.findByRole("dialog", { name: "לקשר לתיק קיים" });
    // The demo's only case that is free or not: either a list or the sentence, never a blank.
    await waitFor(() => expect(within(picker).queryByRole("status", { name: "טוען את התיקים" })).toBeNull());
    const free = within(picker).queryByRole("list", { name: "תיקים לבחירה" });
    if (free === null) expect(within(picker).getByRole("status")).toHaveTextContent("אין כרגע תיק שאפשר לקשר");
    else expect(within(free).queryByRole("button", { name: /^נועם · / })).toBeNull();
    expect(await a11yViolations(picker)).toEqual([]);
  });
});

describe("visits on the card", () => {
  it("lists what is ahead under its own heading, and what was, newest first", async () => {
    await openCard("נועם");
    const visits = await region("פגישות");
    const ahead = within(visits).getByRole("heading", { name: "הפגישות הקרובות" }).parentElement as HTMLElement;
    expect(within(ahead).getAllByRole("listitem")).toHaveLength(1);
    expect(ahead).toHaveTextContent("20.11.2026");
    expect(ahead).toHaveTextContent("שיחת משוב");
    const was = within(visits).getByRole("heading", { name: "פגישות שהיו" }).parentElement as HTMLElement;
    expect(within(was).getAllByRole("listitem").map((li) => /\d\d\.\d\d\.\d{4}/.exec(li.textContent ?? "")?.[0])).toEqual(["01.11.2026", "25.10.2026", "11.10.2026"]);
    // A visit of a day still ahead has nothing to mark and nothing to sum up yet, and no "⋯":
    // its time opens it.
    expect(within(ahead).queryByRole("button", { name: "התקיימה" })).toBeNull();
    expect(within(ahead).queryByRole("button", { name: /סיכום/ })).toBeNull();
    expect(within(ahead).queryByRole("button", { name: /^עוד פעולות לפגישה/ })).toBeNull();
    // One that was held with no note offers the note; one with a note says so.
    expect(within(await visitOn("01.11.2026")).getByRole("button", { name: "לכתוב סיכום" })).toBeInTheDocument();
    expect(within(await visitOn("25.10.2026")).getByText("יש סיכום")).toBeInTheDocument();
  });

  it("a weekly series lists every session that is still ahead", async () => {
    await openCard("מאיה");
    const visits = await region("פגישות");
    // Eight weekly sessions: three were held, today's and four more are ahead.
    const ahead = within(visits).getByRole("heading", { name: "הפגישות הקרובות" }).parentElement as HTMLElement;
    expect(within(ahead).getAllByRole("listitem")).toHaveLength(5);
    expect(within(visits).queryByRole("navigation")).toBeNull();
  });

  it("marking today's visit “התקיימה” shows at once and is saved", async () => {
    const { user, requests } = await openCard("תמר");
    const row = await visitOn("15.11.2026");
    expect(within(row).getByText("מתוכננת")).toBeInTheDocument();
    // At 08:00 her 09:00 visit has not begun: it has no step of its own, and "⋯" offers "התקיימה" first.
    expect(within(row).queryByRole("button", { name: "התקיימה" })).toBeNull();
    expect(within(row).queryByRole("button", { name: "לכתוב סיכום" })).toBeNull();
    await user.click(within(row).getByRole("button", { name: "עוד פעולות לפגישה מיום 15.11.2026, 09:00" }));
    expect((await screen.findAllByRole("menuitem")).map((m) => m.textContent)).toEqual(["התקיימה", "לא הגיעו", "לכתוב סיכום", "לפתוח את הפגישה"]);
    await user.click(screen.getByRole("menuitem", { name: "התקיימה" }));
    expect(within(await visitOn("15.11.2026")).getByText("התקיימה")).toBeInTheDocument();
    await waitFor(() => expect(requests().some((r) => r.op === "save_visit" && r.visit.status === "held" && r.scope === "one")).toBe(true));
    // After the vault answered it is among the visits that were, and the money knows it.
    const visits = await region("פגישות");
    await waitFor(() => {
      const was = within(visits).getByRole("heading", { name: "פגישות שהיו" }).parentElement as HTMLElement;
      expect(was).toHaveTextContent("15.11.2026");
    });
    expect(within(await visitOn("15.11.2026")).queryByRole("button", { name: "התקיימה" })).toBeNull();
    await waitFor(async () => expect(await region("כספים")).toHaveTextContent("1,050 ₪"));
  });

  it("a mark the vault refuses rolls back, and says why", async () => {
    const { user, notes } = await openCard("תמר", refusing("save_visit", "הפגישה לא נשמרה."));
    const row = await visitOn("15.11.2026");
    await user.click(within(row).getByRole("button", { name: /^עוד פעולות לפגישה מיום 15\.11\.2026/ }));
    await user.click(await screen.findByRole("menuitem", { name: "לא הגיעו" }));
    await waitFor(() => expect(notes).toContain("הפגישה לא נשמרה."));
    await waitFor(async () => expect(within(await visitOn("15.11.2026")).getByText("מתוכננת")).toBeInTheDocument());
    // Its "⋯" offers the marks again.
    await user.click(within(await visitOn("15.11.2026")).getByRole("button", { name: /^עוד פעולות לפגישה מיום 15\.11\.2026/ }));
    expect((await screen.findAllByRole("menuitem")).map((m) => m.textContent)).toEqual(["התקיימה", "לא הגיעו", "לכתוב סיכום", "לפתוח את הפגישה"]);
  });

  it("a visit that has begun shows only “התקיימה” and “⋯”, whose menu has “לא הגיעו”, “לכתוב סיכום” and “לפתוח את הפגישה”", async () => {
    vi.setSystemTime(new Date(2026, 10, 15, 9, 30));
    const { user, requests } = await openCard("תמר");
    const row = await visitOn("15.11.2026");
    const actions = row.querySelector(".clinic-visit-actions") as HTMLElement;
    expect(within(actions).getAllByRole("button").map((b) => b.getAttribute("aria-label") ?? b.textContent)).toEqual([
      "התקיימה", "עוד פעולות לפגישה מיום 15.11.2026, 09:00",
    ]);
    // Under way, not over yet: the step is there, not yet the main one.
    expect(within(actions).getByRole("button", { name: "התקיימה" })).not.toHaveClass("btn-primary");
    await user.click(within(actions).getByRole("button", { name: "עוד פעולות לפגישה מיום 15.11.2026, 09:00" }));
    expect((await screen.findAllByRole("menuitem")).map((m) => m.textContent)).toEqual(["לא הגיעו", "לכתוב סיכום", "לפתוח את הפגישה"]);
    await user.keyboard("{Escape}");
    // Held, it offers its note as the one step; no reminder anywhere on the card.
    await user.click(within(actions).getByRole("button", { name: "התקיימה" }));
    await waitFor(() => expect(requests().some((r) => r.op === "save_visit" && r.visit.status === "held")).toBe(true));
    const held = await visitOn("15.11.2026");
    expect(within(held).getByText("התקיימה")).toBeInTheDocument();
    expect(within(held).getByRole("button", { name: "לכתוב סיכום" })).toBeInTheDocument();
    expect(within(await region("פגישות")).queryByRole("button", { name: /תזכורת/ })).toBeNull();
  });

  it("a note saved from “לכתוב סיכום” takes that step off the row, and the focus goes to the row's “⋯”", async () => {
    const { user } = await openCard("נועם");
    await user.click(within(await visitOn("01.11.2026")).getByRole("button", { name: "לכתוב סיכום" }));
    const note = await screen.findByRole("dialog", { name: "סיכום מפגש אבחון" });
    await user.type(await within(note).findByLabelText("רשומה"), "המשך המבחן הקוגניטיבי.");
    await user.click(within(note).getByRole("button", { name: "שמירה" }));
    await waitFor(() => expect(screen.queryByRole("dialog")).toBeNull());
    await waitFor(async () => expect(within(await visitOn("01.11.2026")).queryByRole("button", { name: "לכתוב סיכום" })).toBeNull());
    const more = within(await visitOn("01.11.2026")).getByRole("button", { name: "עוד פעולות לפגישה מיום 01.11.2026, 09:00" });
    await waitFor(() => expect(document.activeElement).toBe(more));
  });

  describe("a visit marked on the card moves to the other list, and the focus goes with it", () => {
    /** The group of the card's visits under `heading`. */
    const group = async (heading: string) => within(await region("פגישות")).getByRole("heading", { name: heading }).parentElement as HTMLElement;
    const MORE = "עוד פעולות לפגישה מיום 15.11.2026, 09:00";

    it("Enter on “התקיימה” of a visit that has begun: the focus is on its step in “פגישות שהיו”, now “לכתוב סיכום”", async () => {
      vi.setSystemTime(new Date(2026, 10, 15, 9, 30));
      const { user } = await openCard("תמר");
      within(await visitOn("15.11.2026")).getByRole("button", { name: "התקיימה" }).focus();
      await user.keyboard("{Enter}");
      await waitFor(async () => expect(await group("פגישות שהיו")).toHaveTextContent("15.11.2026"));
      const moved = await visitOn("15.11.2026");
      expect(await group("פגישות שהיו")).toContainElement(moved);
      await waitFor(() => expect(document.activeElement).toBe(within(moved).getByRole("button", { name: "לכתוב סיכום" })));
      // It stays there after the vault's answer, and Enter goes on to the note.
      await new Promise((r) => setTimeout(r, 50));
      expect(document.activeElement).toBe(within(await visitOn("15.11.2026")).getByRole("button", { name: "לכתוב סיכום" }));
      await user.keyboard("{Enter}");
      expect(await screen.findByRole("dialog", { name: "סיכום מפגש אבחון" })).toBeInTheDocument();
    });

    it("with its note written ahead the row has no step left: the focus is on its “⋯”", async () => {
      vi.setSystemTime(new Date(2026, 10, 15, 9, 30));
      const { user } = await openCard("תמר", undefined, async (served, id) => {
        const visit = (await cardOf(served, id)).visits.find((v) => v.visit.date === "2026-11-15")?.visit;
        if (!visit) throw new Error("no visit today in the demo core");
        await ask(served, { op: "save_note", note: { id: "", patient_id: id, visit_id: visit.id, date: visit.date, record: "הכנה למפגש", personal: "", case_input_id: null } }, "note");
      });
      within(await visitOn("15.11.2026")).getByRole("button", { name: "התקיימה" }).focus();
      await user.keyboard("{Enter}");
      await waitFor(async () => expect(await group("פגישות שהיו")).toHaveTextContent("15.11.2026"));
      const moved = await visitOn("15.11.2026");
      expect(within(moved).queryByRole("button", { name: "לכתוב סיכום" })).toBeNull();
      await waitFor(() => expect(document.activeElement).toBe(within(moved).getByRole("button", { name: MORE })));
    });

    it("“לא הגיעו” from “⋯”: the focus is on the moved row's “⋯”", async () => {
      const { user } = await openCard("תמר");
      within(await visitOn("15.11.2026")).getByRole("button", { name: MORE }).focus();
      await user.keyboard("{Enter}");
      const item = await screen.findByRole("menuitem", { name: "לא הגיעו" });
      item.focus();
      await user.keyboard("{Enter}");
      await waitFor(async () => expect(await group("פגישות שהיו")).toHaveTextContent("15.11.2026"));
      await waitFor(async () => expect(document.activeElement).toBe(within(await visitOn("15.11.2026")).getByRole("button", { name: MORE })));
      await new Promise((r) => setTimeout(r, 50));
      expect(document.activeElement).toBe(within(await visitOn("15.11.2026")).getByRole("button", { name: MORE }));
    });

    it("marked while the visits that were show another page: the page that holds the row opens, and the focus is on it", async () => {
      vi.setSystemTime(new Date(2026, 10, 15, 9, 30));
      const { user } = await openCard("תמר", undefined, async (served, id) => {
        // Nine more visits that were, in September: the list of the visits that were has two pages.
        for (let day = 1; day <= 9; day++) {
          await ask(served, {
            op: "save_visit", repeat: null, scope: "one",
            visit: {
              id: "", patient_id: id, kind: "assessment", title: "", date: `2026-09-0${day}`, start: 600, minutes: 50, place: "", status: "held",
              status_note: "", fee: 0, charge_missed: false, series_id: null, note: "",
            },
          }, "saved");
        }
      });
      const was = await group("פגישות שהיו");
      await user.click(within(was).getByRole("button", { name: "הבא" }));
      await waitFor(async () => expect(within(await group("פגישות שהיו")).getByRole("status")).toHaveTextContent(/^עמוד 2 מתוך/));
      within(await visitOn("15.11.2026")).getByRole("button", { name: "התקיימה" }).focus();
      await user.keyboard("{Enter}");
      await waitFor(async () => expect(within(await group("פגישות שהיו")).getByRole("status")).toHaveTextContent(/^עמוד 1 מתוך/));
      await waitFor(async () => expect(document.activeElement).toBe(within(await visitOn("15.11.2026")).getByRole("button", { name: "לכתוב סיכום" })));
    });

    it("a mark the vault refuses moves the row back, and the focus with it, on “התקיימה”", async () => {
      vi.setSystemTime(new Date(2026, 10, 15, 9, 30));
      // The refusal comes a moment later, as from the vault: the row has moved by then.
      const refuse = refusing("save_visit", "הפגישה לא נשמרה.");
      const late: Override = async (cmd, args, core) => {
        if (cmd === "clinic" && (args.request as ClinicRequest).op === "save_visit") await new Promise((r) => setTimeout(r, 300));
        return refuse(cmd, args, core);
      };
      const { user, notes } = await openCard("תמר", late);
      within(await visitOn("15.11.2026")).getByRole("button", { name: "התקיימה" }).focus();
      await user.keyboard("{Enter}");
      await waitFor(async () => expect(await group("פגישות שהיו")).toHaveTextContent("15.11.2026"));
      await waitFor(async () => expect(document.activeElement).toBe(within(await visitOn("15.11.2026")).getByRole("button", { name: "לכתוב סיכום" })));
      await waitFor(() => expect(notes).toContain("הפגישה לא נשמרה."));
      await waitFor(async () => expect(await group("הפגישות הקרובות")).toHaveTextContent("15.11.2026"));
      await waitFor(async () => expect(document.activeElement).toBe(within(await visitOn("15.11.2026")).getByRole("button", { name: "התקיימה" })));
    });
  });

  it("the time opens the visit, “לכתוב סיכום” opens its note, and an alert leads to the same note", async () => {
    const { user } = await openCard("נועם");
    await user.click(within(await visitOn("20.11.2026")).getByRole("button", { name: /פתיחת הפגישה/ }));
    const visit = await screen.findByRole("dialog", { name: "פרטי הפגישה" });
    await user.click(within(visit).getByRole("button", { name: "סגירה" }));

    await user.click(within(await visitOn("01.11.2026")).getByRole("button", { name: "לכתוב סיכום" }));
    const note = await screen.findByRole("dialog", { name: "סיכום מפגש אבחון" });
    expect(note).toHaveTextContent("נועם · 01.11.2026, 09:00");
    await user.click(within(note).getByRole("button", { name: "סגירה" }));

    await user.click(within(await region("מה דורש אותי")).getByRole("button", { name: "לכתוב סיכום" }));
    expect(await screen.findByRole("dialog", { name: "סיכום מפגש אבחון" })).toHaveTextContent("01.11.2026, 09:00");
  });
});

describe("session notes on the card", () => {
  it("shows the record, marks a personal note apart, and shows its text only when she asks", async () => {
    const { user } = await openCard("נועם");
    const notes = await region("סיכומי פגישות");
    const items = within(notes).getAllByRole("listitem").filter((li) => li.classList.contains("pcard-note"));
    expect(items).toHaveLength(2);
    // Newest first: the testing session, then the first meeting.
    const [session, intake] = items as [HTMLElement, HTMLElement];
    expect(session).toHaveTextContent("25.10.2026");
    expect(session).toHaveTextContent("מפגש אבחון");
    expect(session).toHaveTextContent("כלים שהועברו: מבחן קוגניטיבי, חלק ראשון.");
    expect(within(session).queryByText("יש תרשומת אישית")).toBeNull();
    expect(intake).toHaveTextContent("11.10.2026");
    expect(intake).toHaveTextContent("פגישת היכרות");

    // The personal note: a mark that it exists, and no more than that until she asks.
    expect(within(intake).getByText("יש תרשומת אישית")).toBeInTheDocument();
    expect(notes).not.toHaveTextContent("להזכיר להורים להביא את דוח הגננת.");
    const show = within(intake).getByRole("button", { name: "להציג את התרשומת האישית" });
    expect(show).toHaveAttribute("aria-expanded", "false");
    await user.click(show);
    expect(intake).toHaveTextContent("להזכיר להורים להביא את דוח הגננת.");
    expect(intake).toHaveTextContent("שלך בלבד. לא מודפסת, לא מיוצאת ולא עוברת לתיק האבחון.");
    await user.click(within(intake).getByRole("button", { name: "להסתיר את התרשומת האישית" }));
    expect(notes).not.toHaveTextContent("להזכיר להורים להביא את דוח הגננת.");

    // A long record is folded, and opens.
    const more = within(intake).getByRole("button", { name: "להציג את כל הרשומה" });
    expect(more).toHaveAttribute("aria-expanded", "false");
    await user.click(more);
    expect(within(intake).getByRole("button", { name: "לקצר" })).toHaveAttribute("aria-expanded", "true");

    // Every saved state of the note, on request.
    await user.click(within(session).getByRole("button", { name: "היסטוריית שינויים" }));
    expect(await within(session).findByText("נוצר")).toBeInTheDocument();
  });

  it("a note opens for editing, a note on its own can be added, and one sent to the case says so", async () => {
    const served = await openCard("נועם");
    const { user, requests } = served;
    const notes = await region("סיכומי פגישות");
    await user.click(within(notes).getByRole("button", { name: "עריכה של הסיכום מיום 25.10.2026" }));
    const dialog = await screen.findByRole("dialog", { name: "סיכום מפגש אבחון" });
    expect(await within(dialog).findByLabelText("רשומה")).toHaveValue("כלים שהועברו: מבחן קוגניטיבי, חלק ראשון.\n\nהתנהגות במפגש: שיתף פעולה, נזקק להפסקות קצרות.");
    await user.click(within(dialog).getByRole("button", { name: "להעביר לחומרי התיק" }));
    expect(await within(dialog).findByText(/^הועברה לתיק /)).toBeInTheDocument();
    await user.click(within(dialog).getByRole("button", { name: "סגירה" }));
    expect(await within(notes).findByText(/^הועבר לתיק /)).toBeInTheDocument();

    await user.click(within(notes).getByRole("button", { name: "סיכום שיחה" }));
    const call = await screen.findByRole("dialog", { name: "סיכום שיחה" });
    await user.type(await within(call).findByLabelText("רשומה"), "שיחת טלפון עם הגננת.");
    await user.click(within(call).getByRole("button", { name: "שמירה" }));
    await waitFor(() => expect(screen.queryByRole("dialog")).toBeNull());
    expect(requests().find((r) => r.op === "save_note")).toMatchObject({ note: { visit_id: null, record: "שיחת טלפון עם הגננת.", date: "2026-11-15" } });
    const added = await within(notes).findByText("שיחת טלפון עם הגננת.");
    expect(added.closest("li")).toHaveTextContent("שיחה, לא צמודה לפגישה");
  });

  it("a card with no notes says how one is written", async () => {
    await openCard("איתי");
    expect(await region("סיכומי פגישות")).toHaveTextContent("עוד אין סיכומים.");
  });
});

describe("money on the card", () => {
  it("shows the balance, with what is owed in the warning colour, and every line by date", async () => {
    const { view } = await openCard("יובל");
    const money = await region("כספים");
    const figures = Array.from(money.querySelectorAll("dl > div")).map((d) => d.textContent);
    expect(figures).toEqual(["חויב3,300 ₪", "שולם1,650 ₪", "יתרה לתשלום1,650 ₪"]);
    expect(money.querySelector("dl .clinic-money-owed")).toHaveTextContent("1,650 ₪");
    const lines = within(within(money).getByRole("list", { name: /חיובים ותקבולים/ })).getAllByRole("listitem").filter((li) => li.classList.contains("pcard-line"));
    // Four sessions, the report, two payments: newest first.
    expect(lines.map((li) => /\d\d\.\d\d\.\d{4}/.exec(li.textContent ?? "")?.[0])).toEqual([
      "01.10.2026", "01.10.2026", "13.09.2026", "13.09.2026", "06.09.2026", "27.08.2026", "27.08.2026",
    ]);
    expect(money).toHaveTextContent("כתיבת דוח אבחון");
    expect(money).toHaveTextContent("העברה בנקאית · קבלה 1020");
    expect(await a11yViolations(view.container)).toEqual([]);
  });

  it("a family that owes nothing has no warning and no payment message", async () => {
    await openCard("תמר");
    const money = await region("כספים");
    expect(Array.from(money.querySelectorAll("dl > div")).map((d) => d.textContent)).toEqual(["חויב450 ₪", "שולם450 ₪", "יתרה לתשלוםאין"]);
    expect(money.querySelector(".clinic-money-owed")).toBeNull();
    expect(within(money).queryByRole("button", { name: "הודעת תשלום" })).toBeNull();
    expect(money).toHaveTextContent("מזומן · בלי מספר קבלה");
  });

  it("“ביטול” of a money dialog with typed text asks first", async () => {
    const { user } = await openCard("נועם");
    const money = await region("כספים");
    await user.click(within(money).getByRole("button", { name: "חיוב חדש" }));
    const dialog = await screen.findByRole("dialog", { name: "חיוב חדש" });
    await user.type(within(dialog).getByLabelText("על מה החיוב"), "צילום");
    await user.click(within(dialog).getByRole("button", { name: "ביטול" }));
    expect(within(dialog).getByRole("alertdialog", { name: "לסגור בלי לשמור" })).toBeInTheDocument();
    await user.click(within(dialog).getByRole("button", { name: "להמשיך לערוך" }));
    expect(within(dialog).getByLabelText("על מה החיוב")).toHaveValue("צילום");
  });

  it("“חיוב חדש” records what, how much and when", async () => {
    const { user, requests, notes, id } = await openCard("תמר");
    const money = await region("כספים");
    await user.click(within(money).getByRole("button", { name: "חיוב חדש" }));
    const dialog = await screen.findByRole("dialog", { name: "חיוב חדש" });
    expect(await a11yViolations(dialog)).toEqual([]);
    await user.click(within(dialog).getByRole("button", { name: "רישום החיוב" }));
    expect(within(dialog).getByRole("alert")).toHaveTextContent("צריך לכתוב על מה החיוב.");
    await user.type(within(dialog).getByLabelText("על מה החיוב"), "כתיבת דוח אבחון");
    await user.type(within(dialog).getByLabelText("סכום"), "1200");
    expect(within(dialog).getByLabelText("תאריך")).toHaveValue("15.11.2026");
    await user.click(within(dialog).getByRole("button", { name: "רישום החיוב" }));
    await waitFor(() => expect(screen.queryByRole("dialog")).toBeNull());
    expect(requests().find((r) => r.op === "save_charge")).toEqual({
      op: "save_charge", charge: { id: "", patient_id: id, date: "2026-11-15", amount: 120_000, what: "כתיבת דוח אבחון", voided: false, void_reason: "" },
    });
    expect(notes).toContain("החיוב נרשם");
    await waitFor(() => expect(money.querySelector("dl .clinic-money-owed")).toHaveTextContent("1,200 ₪"));
    expect(within(money).getByRole("button", { name: "הודעת תשלום" })).toBeInTheDocument();
  });

  it("“תקבול חדש” starts from what is owed, and records the method, the receipt and the payer", async () => {
    const { user, requests, notes, id } = await openCard("יובל");
    const money = await region("כספים");
    await user.click(within(money).getByRole("button", { name: "תקבול חדש" }));
    const dialog = await screen.findByRole("dialog", { name: "תקבול חדש" });
    expect(await a11yViolations(dialog)).toEqual([]);
    const amount = within(dialog).getByLabelText("סכום");
    expect(amount).toHaveValue("1650");
    await user.clear(amount);
    await user.click(within(dialog).getByRole("button", { name: "רישום התקבול" }));
    expect(within(dialog).getByRole("alert")).toHaveTextContent("צריך לכתוב סכום גדול מאפס");
    await user.type(amount, "650");
    await user.selectOptions(within(dialog).getByLabelText("אמצעי תשלום"), "ביט");
    await user.type(within(dialog).getByLabelText("מספר קבלה"), "1077");
    await user.type(within(dialog).getByLabelText("מי שילם (לא חובה)"), "סבתא");
    await user.type(within(dialog).getByLabelText("הערה (לא חובה)"), "תשלום חלקי");
    await user.click(within(dialog).getByRole("button", { name: "רישום התקבול" }));
    await waitFor(() => expect(screen.queryByRole("dialog")).toBeNull());
    expect(requests().find((r) => r.op === "save_payment")).toEqual({
      op: "save_payment",
      payment: { id: "", patient_id: id, date: "2026-11-15", amount: 65_000, method: "bit", receipt: "1077", payer: "סבתא", note: "תשלום חלקי", voided: false, void_reason: "" },
    });
    expect(notes).toContain("התקבול נרשם");
    await waitFor(() => expect(money.querySelector("dl .clinic-money-owed")).toHaveTextContent("1,000 ₪"));
    expect(money).toHaveTextContent("ביט · קבלה 1077 · שילם: סבתא · תשלום חלקי");
  });

  it("a payment the vault refuses stays in the dialog with what she typed", async () => {
    const { user } = await openCard("יובל", refusing("save_payment", "התקבול לא נרשם."));
    const money = await region("כספים");
    await user.click(within(money).getByRole("button", { name: "תקבול חדש" }));
    const dialog = await screen.findByRole("dialog", { name: "תקבול חדש" });
    await user.click(within(dialog).getByRole("button", { name: "רישום התקבול" }));
    expect(await within(dialog).findByText("התקבול לא נרשם.")).toBeInTheDocument();
    expect(within(dialog).getByLabelText("סכום")).toHaveValue("1650");
    expect(money.querySelector("dl .clinic-money-owed")).toHaveTextContent("1,650 ₪");
  });

  it("a payment is voided only with a reason, stays on the list as voided, and keeps its history", async () => {
    const { user, requests, notes } = await openCard("יובל");
    const money = await region("כספים");
    await user.click(within(money).getByRole("button", { name: "ביטול התקבול מיום 13.09.2026" }));
    const confirm = await screen.findByRole("dialog", { name: "ביטול התקבול" });
    expect(confirm).toHaveTextContent("הרישום לא נמחק");
    await user.click(within(confirm).getByRole("button", { name: "לבטל את התקבול" }));
    expect(await within(confirm).findByText("צריך לכתוב למה הרישום מבוטל.")).toBeInTheDocument();
    expect(requests().some((r) => r.op === "void_payment")).toBe(false);
    await user.type(within(confirm).getByLabelText("סיבת הביטול"), "נרשם פעמיים");
    await user.click(within(confirm).getByRole("button", { name: "לבטל את התקבול" }));
    await waitFor(() => expect(screen.queryByRole("dialog")).toBeNull());
    expect(requests().find((r) => r.op === "void_payment")).toMatchObject({ reason: "נרשם פעמיים" });
    expect(notes).toContain("התקבול בוטל");

    const line = (await within(money).findByText("סיבת הביטול: נרשם פעמיים")).closest("li") as HTMLElement;
    expect(within(line).getByText("בוטל")).toBeInTheDocument();
    expect(within(line).queryByRole("button", { name: /ביטול התקבול/ })).toBeNull();
    // It no longer counts.
    expect(money.querySelector("dl .clinic-money-owed")).toHaveTextContent("2,850 ₪");
    await user.click(within(line).getByRole("button", { name: "היסטוריית שינויים" }));
    expect(await within(line).findByText("בוטל", { selector: ".clinic-history .chip" })).toBeInTheDocument();
    expect(within(line).getByText("נוצר")).toBeInTheDocument();
  });

  it("a charge is voided the same way", async () => {
    const { user, requests } = await openCard("יובל");
    const money = await region("כספים");
    await user.click(within(money).getByRole("button", { name: "ביטול החיוב מיום 01.10.2026" }));
    const confirm = await screen.findByRole("dialog", { name: "ביטול החיוב" });
    await user.type(within(confirm).getByLabelText("סיבת הביטול"), "הוסכם אחרת");
    await user.click(within(confirm).getByRole("button", { name: "לבטל את החיוב" }));
    await waitFor(() => expect(screen.queryByRole("dialog")).toBeNull());
    expect(requests().find((r) => r.op === "void_charge")).toMatchObject({ reason: "הוסכם אחרת" });
    await waitFor(() => expect(money.querySelector("dl .clinic-money-owed")).toHaveTextContent("450 ₪"));
  });

  it("“הודעת תשלום” fills the message and copies it through the protected clipboard only", async () => {
    const { user, calls, notes } = await openCard("יובל");
    const money = await region("כספים");
    await user.click(within(money).getByRole("button", { name: "הודעת תשלום" }));
    await user.click(await screen.findByRole("menuitem", { name: "רק להעתיק" }));
    await waitFor(() => expect(notes).toEqual(["הודעת התשלום הועתקה ללוח המוגן, ואפשר להדביק אותה בהודעה. יימחק מהלוח בעוד דקה."]));
    const copied = calls.filter((c) => c.cmd === "copy_secret");
    expect(copied).toHaveLength(1);
    expect(String(copied[0]?.args.text)).toMatch(/^שלום ענת, היתרה לתשלום היא 1,?650 ₪\./);
  });
});

describe("follow-ups on the card", () => {
  it("ticking one shows it done at once and saves it as done today; unticking opens it again", async () => {
    const { user, requests } = await openCard("נועם");
    const tasks = await region("מעקבים");
    const box = within(tasks).getByLabelText("לחזור לרופאת הילדים");
    expect(box).not.toBeChecked();
    await user.click(box);
    expect(within(tasks).getByLabelText("לחזור לרופאת הילדים")).toBeChecked();
    await waitFor(() => expect(requests().some((r) => r.op === "save_task" && r.task.done_on === "2026-11-15")).toBe(true));
    expect(await within(tasks).findByText("בוצע ביום 15.11.2026")).toBeInTheDocument();
    await user.click(within(tasks).getByLabelText("לחזור לרופאת הילדים"));
    await waitFor(() => expect(requests().filter((r) => r.op === "save_task").pop()).toMatchObject({ task: { done_on: null } }));
    await waitFor(() => expect(within(tasks).getByLabelText("לחזור לרופאת הילדים")).not.toBeChecked());
  });

  it("a tick the vault refuses rolls back, and says why", async () => {
    const { user, notes } = await openCard("נועם", refusing("save_task", "המשימה לא נשמרה."));
    const tasks = await region("מעקבים");
    await user.click(within(tasks).getByLabelText("לחזור לרופאת הילדים"));
    await waitFor(() => expect(notes).toContain("המשימה לא נשמרה."));
    await waitFor(() => expect(within(tasks).getByLabelText("לחזור לרופאת הילדים")).not.toBeChecked());
  });

  it("adds a follow-up for this child, and edits one", async () => {
    const { user, requests, notes, id } = await openCard("תמר");
    const tasks = await region("מעקבים");
    // What was done stays on the card, after what is open.
    expect(within(tasks).getByLabelText("לשלוח שאלון לגננת")).not.toBeChecked();
    expect(within(tasks).getByLabelText("לבקש מההורים את סיכום קלינאית התקשורת")).toBeChecked();

    await user.click(within(tasks).getByRole("button", { name: "מעקב חדש" }));
    const dialog = await screen.findByRole("dialog", { name: "מעקב חדש" });
    expect(await a11yViolations(dialog)).toEqual([]);
    await user.click(within(dialog).getByRole("button", { name: "הוספה" }));
    expect(within(dialog).getByRole("alert")).toHaveTextContent("צריך לכתוב מה צריך לעשות.");
    await user.type(within(dialog).getByLabelText("מה צריך לעשות"), "לתאם תצפית עם הגננת");
    await user.selectOptions(within(dialog).getByLabelText("סוג"), "שיחת טלפון");
    await user.type(within(dialog).getByLabelText("עד מתי (לא חובה)"), "18.11.2026");
    await user.click(within(dialog).getByRole("button", { name: "הוספה" }));
    await waitFor(() => expect(screen.queryByRole("dialog")).toBeNull());
    expect(requests().find((r) => r.op === "save_task")).toEqual({
      op: "save_task", task: { id: "", patient_id: id, kind: "call", title: "לתאם תצפית עם הגננת", due: "2026-11-18", done_on: null, note: "" },
    });
    expect(notes).toContain("המעקב נוסף");
    expect(await within(tasks).findByLabelText("לתאם תצפית עם הגננת")).not.toBeChecked();

    await user.click(within(tasks).getByRole("button", { name: "עריכה של המעקב: לתאם תצפית עם הגננת" }));
    const edit = await screen.findByRole("dialog", { name: "עריכת מעקב" });
    const title = within(edit).getByLabelText("מה צריך לעשות");
    await user.clear(title);
    await user.type(title, "לתאם תצפית");
    await user.click(within(edit).getByRole("button", { name: "שמירה" }));
    await waitFor(() => expect(screen.queryByRole("dialog")).toBeNull());
    expect(requests().filter((r) => r.op === "save_task").pop()).toMatchObject({ task: { title: "לתאם תצפית", kind: "call", due: "2026-11-18" } });
    expect(await within(tasks).findByLabelText("לתאם תצפית")).toBeInTheDocument();
  });
});

describe("erasing a card", () => {
  it("asks for the child's name, and a name that is not the same sends nothing", async () => {
    const { user, requests } = await openCard("שירה");
    await user.click(await screen.findByRole("button", { name: "מחיקת הכרטיס" }));
    const dialog = await screen.findByRole("dialog", { name: "מחיקת הכרטיס" });
    expect(await a11yViolations(dialog)).toEqual([]);
    const erase = within(dialog).getByRole("button", { name: "למחוק לצמיתות" });
    expect(erase).toBeDisabled();
    await user.type(within(dialog).getByLabelText(/כדי למחוק, יש להקליד את שם הילד או הילדה/), "שיר");
    await user.click(erase);
    expect(dialog).toHaveTextContent("השם שהוקלד לא זהה לשם שבכרטיס.");
    expect(requests().some((r) => r.op === "erase_patient")).toBe(false);
  });

  it("a card where nothing happened is erased, and she is back on the list", async () => {
    const served = await openCard("שירה");
    const { user, requests, went, notes, id } = served;
    await user.click(await screen.findByRole("button", { name: "מחיקת הכרטיס" }));
    const dialog = await screen.findByRole("dialog", { name: "מחיקת הכרטיס" });
    await user.type(within(dialog).getByLabelText(/כדי למחוק/), "שירה");
    const reads = () => requests().filter((r) => r.op === "patient_card" && r.id === id).length;
    const before = reads();
    await user.click(within(dialog).getByRole("button", { name: "למחוק לצמיתות" }));
    await waitFor(() => expect(went[went.length - 1]).toEqual({ name: "clinic", tab: "patients" }));
    expect(requests().find((r) => r.op === "erase_patient")).toEqual({ op: "erase_patient", id, name: "שירה" });
    expect(notes).toContain("הכרטיס נמחק");
    expect(screen.queryByRole("dialog")).toBeNull();
    expect((await rowsOf(served)).some((r) => r.patient.id === id)).toBe(false);
    // The list she lands on no longer has the family, and nothing asked for the card again.
    await screen.findByRole("table", { name: /רשימת המטופלים/ });
    expect(reads()).toBe(before);
  });

  it("the core's refusal is shown as it is, and closing the card is offered in its place", async () => {
    const served = await openCard("יובל");
    const { user, requests, notes, id } = served;
    await user.click(await screen.findByRole("button", { name: "מחיקת הכרטיס" }));
    const dialog = await screen.findByRole("dialog", { name: "מחיקת הכרטיס" });
    expect(within(dialog).queryByRole("button", { name: "לסגור את הכרטיס" })).toBeNull();
    await user.type(within(dialog).getByLabelText(/כדי למחוק/), "יובל");
    await user.click(within(dialog).getByRole("button", { name: "למחוק לצמיתות" }));
    const refusal = await within(dialog).findByRole("alert");
    expect(refusal.textContent).toMatch(/^בכרטיס הזה יש פגישות, סיכומים או רישומי כספים, ורשומה כזו צריך לשמור\. אפשר לסגור את הכרטיס במקום למחוק אותו\. מחיקה תהיה אפשרית החל מיום \d\d\.\d\d\.\d{4}\.$/);
    // The card is still there, whole.
    expect((await cardOf(served, id)).visits.length).toBeGreaterThan(0);

    await user.click(within(dialog).getByRole("button", { name: "לסגור את הכרטיס" }));
    await waitFor(() => expect(screen.queryByRole("dialog")).toBeNull());
    expect(requests().find((r) => r.op === "save_patient")).toMatchObject({ patient: { id, status: "closed" } });
    expect(notes).toContain("הכרטיס נסגר, ונשמר תחת \"הסתיים\"");
    expect(await screen.findByRole("button", { name: "עוד פעולות לכרטיס, כמו פתיחה מחדש" })).toBeInTheDocument();
    expect(screen.getByText("הסתיים", { selector: ".pcard-head .chip" })).toBeInTheDocument();
  });
});

describe("discreet mode on the card", () => {
  it("shows initials for the child and the parents, hides phone numbers, and never shows a personal note", async () => {
    const { user, calls } = await openCard("נועם");
    await screen.findByRole("heading", { name: "נועם" });
    const notes = await region("סיכומי פגישות");
    // She had the personal note open; turning discreet mode on takes it off the screen.
    await user.click(within(notes).getByRole("button", { name: "להציג את התרשומת האישית" }));
    expect(notes).toHaveTextContent("להזכיר להורים להביא את דוח הגננת.");
    await user.click(screen.getByRole("switch", { name: "מצב דיסקרטי" }));

    expect(screen.getByRole("heading", { name: "נ." })).toBeInTheDocument();
    const panel = screen.getByRole("tabpanel");
    for (const text of ["נועם", "מיכל", "אלון", "050-555-01", "052-555-02", "להזכיר להורים להביא את דוח הגננת."]) {
      expect(within(panel).queryByText(text, { exact: false }), text).toBeNull();
    }
    expect(within(notes).getByText("יש תרשומת אישית")).toBeInTheDocument();
    expect(within(notes).getByText("לא מוצגת במצב דיסקרטי.")).toBeInTheDocument();
    expect(within(notes).queryByRole("button", { name: /התרשומת האישית/ })).toBeNull();
    // The number can still be copied, without being shown.
    await user.click(screen.getByRole("button", { name: "העתקת הטלפון של אמא מ." }));
    await waitFor(() => expect(calls.filter((c) => c.cmd === "copy_secret").map((c) => c.args)).toEqual([{ text: "050-555-01" }]));
    // The dialogs of the card speak in initials too.
    await user.click(screen.getByRole("button", { name: "עוד פעולות לכרטיס, כמו סגירת הכרטיס" }));
    await user.click(await screen.findByRole("menuitem", { name: "סגירת הכרטיס" }));
    const confirm = await screen.findByRole("dialog", { name: "סגירת הכרטיס" });
    expect(confirm).toHaveTextContent("הכרטיס של נ. יעבור");
    expect(confirm).not.toHaveTextContent("נועם");
  });

  it("leaves who paid in initials, and her notes on payments and follow-ups off the card's lists", async () => {
    const { user } = await openCard("נועם", undefined, async (served, id) => {
      await ask(served, { op: "save_payment", payment: { id: "", patient_id: id, date: "2026-11-10", amount: 10_000, method: "cash", receipt: "77", payer: "דודה שירה", note: "הביאה במעטפה", voided: false, void_reason: "" } }, "payment");
      await ask(served, { op: "save_task", task: { id: "", patient_id: id, kind: "follow_up", title: "לשלוח שאלון", due: null, done_on: null, note: "רק אחרי השיחה עם הגננת" } }, "task");
    });
    const money = await region("כספים");
    const tasks = await region("מעקבים");
    expect(money).toHaveTextContent("שילם: דודה שירה");
    expect(money).toHaveTextContent("הביאה במעטפה");
    expect(tasks).toHaveTextContent("רק אחרי השיחה עם הגננת");
    await user.click(screen.getByRole("switch", { name: "מצב דיסקרטי" }));
    expect(money).toHaveTextContent("שילם: ד. ש.");
    for (const text of ["שירה", "הביאה במעטפה"]) expect(money, text).not.toHaveTextContent(text);
    expect(tasks).toHaveTextContent("לשלוח שאלון");
    expect(tasks).not.toHaveTextContent("רק אחרי השיחה עם הגננת");
  });

  it("in discreet mode the reason says “מוסתרת במצב דיסקרטי” and the needs show no initials", async () => {
    const { user } = await openCard("נועם");
    const head = (await screen.findByRole("heading", { name: "נועם" })).closest(".pcard-head") as HTMLElement;
    const needs = await region("מה דורש אותי");
    await user.click(screen.getByRole("switch", { name: "מצב דיסקרטי" }));
    const cell = (label: string) => within(head).getByRole("heading", { name: label }).parentElement as HTMLElement;
    expect(cell("סיבת הפנייה")).toHaveTextContent("מוסתרת במצב דיסקרטי");
    expect(cell("מי הפנה")).toHaveTextContent("מוסתר במצב דיסקרטי");
    for (const text of ["קושי במעברים", "רופאת הילדים", "גן חצב"]) expect(head, text).not.toHaveTextContent(text);
    // The case is said by its code alone.
    const box = within(head).getByRole("region", { name: "תיק האבחון" });
    expect(box).not.toHaveTextContent("נועם");
    expect(box).not.toHaveTextContent("נ.");
    // The needs are this child's, and the heading already says whose: no name, not even initials.
    expect(within(needs).getByText("פגישה בלי סיכום")).toBeInTheDocument();
    expect(needs).not.toHaveTextContent("נ.");
    expect(needs).not.toHaveTextContent("נועם");
  });

  it("the picker of cases shows the child's name in initials", async () => {
    const { user } = await openCard("נועם");
    const box = await region("תיק האבחון");
    await user.click(await within(box).findByRole("button", { name: "לנתק" }));
    await user.click(within(await screen.findByRole("dialog", { name: "לנתק את התיק מהכרטיס" })).getByRole("button", { name: "לנתק" }));
    await waitFor(() => expect(screen.queryByRole("dialog")).toBeNull());
    await user.click(screen.getByRole("switch", { name: "מצב דיסקרטי" }));
    await user.click(await within(box).findByRole("button", { name: "לקשר לתיק קיים" }));
    const picker = await screen.findByRole("dialog", { name: "לקשר לתיק קיים" });
    expect(await within(picker).findByRole("button", { name: /^נ\. · / })).toBeInTheDocument();
    expect(picker).not.toHaveTextContent("נועם");
  });
});
