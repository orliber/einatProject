// How the pages cope with the rules of D-176..D-179: a value of a later version of the
// program is shown in plain words and is refused on save in the core's words; rows that
// could not be read are said first; the anonymous line of the cards that were erased is
// shown and explained; the erase dialog says what goes and what stays; a mark that cannot be
// taken back says why. And the small things: the name of the button that opens a visit, no
// link from a card to itself, a read that failed can always be tried again, and the visit
// dialog is one dialog from its skeleton to its form. Every name is invented.
import { configure, screen, waitFor, within } from "@testing-library/react";
import type { Alert } from "../../ipc/generated/Alert";
import type { ClinicReply } from "../../ipc/generated/ClinicReply";
import type { ClinicRequest } from "../../ipc/generated/ClinicRequest";
import type { DiaryErased } from "../../ipc/generated/DiaryErased";
import type { Override } from "../../test/host";
import { forgetDiscreet } from "./ClinicScreen";
import { erasedWords, forgetMoneyView } from "./MoneyTab";
import { holding, openOn, refusing } from "./officeTestKit";
import { ask, cardOf, openCard } from "./patientsTestKit";

configure({ asyncUtilTimeout: 5000 });
vi.setConfig({ testTimeout: 30_000 });

beforeEach(() => {
  vi.useFakeTimers({ toFake: ["Date"] });
  vi.setSystemTime(new Date(2026, 10, 15, 8, 0));
});

afterEach(() => {
  vi.useRealTimers();
  forgetDiscreet();
  forgetMoneyView();
  window.history.replaceState({}, "", "/");
});

const TODAY = { name: "clinic", tab: "today" } as const;
const TASKS = { name: "clinic", tab: "tasks" } as const;
const MONEY = { name: "clinic", tab: "money" } as const;
const region = (name: string) => screen.findByRole("region", { name });
const visitOn = async (date: string) => within(await region("פגישות")).getByText(date).closest("li") as HTMLElement;

/** The core's answer to one request, changed before the window gets it: what an older
 *  version of the program reads after a newer one wrote. */
function patched<K extends ClinicReply["kind"]>(op: ClinicRequest["op"], kind: K, change: (reply: Extract<ClinicReply, { kind: K }>, request: ClinicRequest) => void): Override {
  return async (cmd, args, core) => {
    if (cmd === "copy_secret") return 60;
    const request = args.request as ClinicRequest;
    if (cmd !== "clinic" || request.op !== op) return undefined;
    const reply = structuredClone(await core.handle(cmd, args)) as ClinicReply;
    if (reply.kind === kind) change(reply as Extract<ClinicReply, { kind: K }>, request);
    return reply;
  };
}

const UNREADABLE: Alert = {
  kind: "unreadable", patient_id: null, patient_name: null, visit_id: null, task_id: null, since: "2026-11-15",
  text: "רישום אחד בקליניקה לא נקרא, ולכן הוא לא מוצג ולא נספר בסיכומים. הוא לא נמחק ולא שונה. כדאי לשחזר מגיבוי, או לפנות לתמיכה.",
};
const ERASED: DiaryErased = { label: "רשומות של כרטיסים שנמחקו", held: 3, cancelled: 1, no_show: 0, charged: 180_000, received: 120_000 };

describe("a value of a later version (D-178)", () => {
  const later = patched("patient_card", "card", ({ data }) => {
    data.patient.status = "unknown";
    for (const c of data.patient.contacts.slice(0, 1)) c.role = "unknown";
    for (const v of data.visits.slice(0, 1)) v.visit.kind = "unknown";
    for (const v of data.visits.slice(1, 2)) v.visit.status = "unknown";
    for (const p of data.payments.slice(0, 1)) p.method = "unknown";
    for (const t of data.tasks.slice(0, 1)) t.kind = "unknown";
  });

  it("is shown in plain words on the card, and nothing breaks", async () => {
    await openCard("נועם", later);
    const head = (await screen.findByRole("heading", { name: "נועם" })).closest("header") as HTMLElement;
    expect(head).toHaveTextContent("מצב לא מוכר");
    expect(within(head).getByRole("list", { name: "אנשי קשר" })).toHaveTextContent("קשר לא מוכר");
    expect(within(await visitOn("20.11.2026")).getByText("סוג לא מוכר")).toBeInTheDocument();
    expect(within(await visitOn("01.11.2026")).getByText("מצב לא מוכר")).toBeInTheDocument();
    expect(await region("כספים")).toHaveTextContent("אמצעי לא מוכר");
    // The rest of the card is all there.
    expect(await region("מה דורש אותי")).toBeInTheDocument();
    expect(screen.getByRole("list", { name: "שלבי האבחון" })).toBeInTheDocument();
  });

  it("stays chosen in the visit's form with a word about it, and a save is refused in the core's words", async () => {
    const { user, requests } = await openCard("נועם", later);
    await user.click(within(await visitOn("20.11.2026")).getByRole("button", { name: /פתיחת הפגישה/ }));
    const dialog = await screen.findByRole("dialog", { name: "פרטי הפגישה" });
    const kind = await within(dialog).findByLabelText("סוג הפגישה");
    expect(kind).toHaveValue("unknown");
    expect(within(kind).getByRole("option", { name: "סוג לא מוכר" })).toBeInTheDocument();
    expect(dialog).toHaveTextContent("הסוג שנשמר לפגישה הזו לא מוכר לגרסה הזו של התוכנה.");
    await user.click(within(dialog).getByRole("button", { name: "שמירה" }));
    expect(await within(dialog).findByText("בשדה \"סוג הפגישה\" יש ערך שהגרסה הזו של התוכנה לא מכירה. כדי לשמור צריך לבחור ערך מהרשימה, או לעדכן את התוכנה.")).toBeInTheDocument();
    // Once she picks a kind from the list, the word is gone and the visit is saved.
    await user.selectOptions(kind, "feedback");
    expect(within(kind).queryByRole("option", { name: "סוג לא מוכר" })).toBeNull();
    await user.click(within(dialog).getByRole("button", { name: "שמירה" }));
    await waitFor(() => expect(screen.queryByRole("dialog", { name: "פרטי הפגישה" })).toBeNull());
    expect(requests().filter((r) => r.op === "save_visit").map((r) => r.op === "save_visit" && r.visit.kind)).toEqual(["unknown", "feedback"]);
  });

  it("a method the program does not know is a total of its own in the diary", async () => {
    await openOn(MONEY, patched("diary", "diary", ({ data }) => {
      data.by_method.push({ method: "unknown", amount: 12_300 });
      for (const line of data.days.flatMap((d) => d.lines).filter((l) => l.method !== null).slice(0, 1)) line.method = "unknown";
    }));
    const totals = await region("סיכום נובמבר 2026");
    expect(totals).toHaveTextContent("אמצעי לא מוכר: 123 ₪");
    expect((await screen.findAllByRole("cell", { name: "אמצעי לא מוכר" })).length).toBe(1);
  });
});

describe("rows that could not be read (D-178)", () => {
  const unread = patched("overview", "overview", ({ data }) => { data.alerts.unshift(UNREADABLE); });

  it("are said first on the front page, about no patient and with nothing to open", async () => {
    await openOn(TODAY, unread);
    const needs = await region("מה דורש אותי");
    const first = within(needs).getAllByRole("listitem")[0] as HTMLElement;
    expect(within(first).getByText("רישום שלא נקרא")).toBeInTheDocument();
    expect(first).toHaveTextContent(UNREADABLE.text);
    expect(within(first).queryByRole("button")).toBeNull();
  });

  it("are the first group of what needs her on the follow-ups tab", async () => {
    await openOn(TASKS, unread);
    const needs = await region("מה דורש אותי");
    const group = await within(needs).findByRole("region", { name: "רישום שלא נקרא: 1" });
    expect(group).toHaveTextContent(UNREADABLE.text);
    expect(within(needs).getAllByRole("region")[0]).toBe(group);
  });
});

describe("the anonymous line of the cards that were erased (D-179)", () => {
  const erased = patched("diary", "diary", (reply, request) => {
    if (request.op === "diary" && (request.month === "2026-11" || request.month === "2026-03")) reply.data.erased = { ...ERASED };
  });

  it("is in the month's summary, with one sentence that says what it is", async () => {
    await openOn(MONEY, erased);
    const line = await within(await region("סיכום נובמבר 2026")).findByRole("group", { name: "רשומות של כרטיסים שנמחקו" });
    expect(erasedWords(ERASED)).toBe("3 פגישות התקיימו · פגישה אחת בוטלה · חויב 1,800 ₪ · התקבל 1,200 ₪");
    expect(line).toHaveTextContent(erasedWords(ERASED));
    expect(line).toHaveTextContent("הסכומים האלה כלולים בסיכום החודש, כמספרים בלבד: בלי שם ובלי תאריך, ולכן הם לא מופיעים באף יום.");
    // Totals only: nothing to open, and no day of the diary carries it.
    expect(within(line).queryByRole("button")).toBeNull();
    for (const table of screen.queryAllByRole("table")) expect(table).not.toHaveTextContent("כרטיסים שנמחקו");
  });

  it("a month with no such line shows none", async () => {
    await openOn(MONEY);
    await region("סיכום נובמבר 2026");
    expect(screen.queryByRole("group", { name: "רשומות של כרטיסים שנמחקו" })).toBeNull();
  });

  it("is in the yearly summary as the sum of its months, with its own sentence", async () => {
    const { user, requests } = await openOn(MONEY, erased);
    await user.click(screen.getByRole("radio", { name: "סיכום שנתי" }));
    await screen.findByRole("table", { name: "סיכום 2026 לפי חודשים" });
    const line = await screen.findByRole("group", { name: "רשומות של כרטיסים שנמחקו" });
    expect(line).toHaveTextContent("6 פגישות התקיימו · 2 פגישות בוטלו · חויב 3,600 ₪ · התקבל 2,400 ₪");
    expect(line).toHaveTextContent("המספרים האלה כלולים בסיכום השנה. הם נשארו מכרטיסים שנמחקו, בלי שם ובלי תאריך, כדי שסיכום של שנה שעברה לא ישתנה.");
    // Read from the twelve months of the diary: the year's own answer has no such line.
    expect(new Set(requests().flatMap((r) => (r.op === "diary" ? [r.month] : []))).size).toBeGreaterThanOrEqual(12);
  });

  it("when the months could not be read, the year still shows, and says what it could not tell apart", async () => {
    let refuse = false;
    const { user } = await openOn(MONEY, (cmd, args) => {
      if (cmd === "copy_secret") return 60;
      if (refuse && cmd === "clinic" && (args.request as ClinicRequest).op === "diary") {
        throw { code: "refused", message: "היומן לא נקרא מהכספת.", details: [], action: { kind: "dismiss" } };
      }
      return undefined;
    });
    await region("סיכום נובמבר 2026");
    // From now on the diary is refused; the year's figures are another request.
    refuse = true;
    await user.click(screen.getByRole("radio", { name: "סיכום שנתי" }));
    expect(await screen.findByRole("table", { name: "סיכום 2026 לפי חודשים" })).toBeInTheDocument();
    expect(await screen.findByText("אם נמחקו כרטיסים, מה שנשאר מהם כלול בסיכום הזה. הפירוט שלהם לא נקרא כרגע.")).toBeInTheDocument();
    expect(screen.queryByRole("group", { name: "רשומות של כרטיסים שנמחקו" })).toBeNull();
  });
});

describe("erasing a card", () => {
  it("says before she confirms what is removed, and that only anonymous numbers stay", async () => {
    const { user, requests } = await openCard("שירה");
    await user.click(await screen.findByRole("button", { name: "מחיקת הכרטיס" }));
    const dialog = await screen.findByRole("dialog", { name: "מחיקת הכרטיס" });
    const removed = within(dialog).getByText("מה נמחק").closest("div") as HTMLElement;
    expect(removed).toHaveTextContent("הכרטיס ופרטי הקשר, הפגישות, סיכומי הפגישות, החיובים, התקבולים, המעקבים, וההיסטוריה של כל אלה.");
    const stays = within(dialog).getByText("מה נשאר").closest("div") as HTMLElement;
    expect(stays).toHaveTextContent("מספרים בלבד, בלי שם, בלי תאריך ובלי טקסט");
    expect(stays).toHaveTextContent("כרטיס שלא קרה בו כלום לא משאיר אחריו דבר.");
    expect(dialog).toHaveTextContent("אי אפשר לבטל אותה");
    // All of it is there before anything was asked of the vault.
    expect(requests().some((r) => r.op === "erase_patient")).toBe(false);
    expect(within(dialog).getByRole("button", { name: "למחוק לצמיתות" })).toBeDisabled();
  });
});

describe("a change the core refuses says why, in the core's words", () => {
  it("a visit with a session note does not go back to “מתוכננת” (D-176)", async () => {
    const { user, id, core, calls } = await openCard("נועם");
    await user.click(within(await visitOn("25.10.2026")).getByRole("button", { name: /פתיחת הפגישה/ }));
    const dialog = await screen.findByRole("dialog", { name: "פרטי הפגישה" });
    const marks = await within(dialog).findByRole("radiogroup", { name: "מה קרה בפגישה" });
    await user.click(within(marks).getByRole("radio", { name: "מתוכננת" }));
    expect(dialog).toHaveTextContent("חזרה ל\"מתוכננת\" היא תיקון של סימון שנעשה בטעות. היא אפשרית רק כשלפגישה אין סיכום, וכשמה שהמשפחה כבר שילמה לא עולה על מה שנשאר לחייב בלי הפגישה הזו.");
    await user.click(within(dialog).getByRole("button", { name: "שמירה" }));
    expect(await within(dialog).findByText("לפגישה הזו יש סיכום, ולכן היא לא יכולה לחזור להיות \"מתוכננת\". אם היא לא התקיימה, אפשר לסמן אותה כמבוטלת ולכתוב מה קרה.")).toBeInTheDocument();
    // The dialog stays with what she chose, and the visit is as it was.
    expect(within(marks).getByRole("radio", { name: "מתוכננת" })).toBeChecked();
    const card = await cardOf({ core, calls, caseId: "" }, id);
    expect(card.visits.find((v) => v.visit.date === "2026-10-25")?.visit.status).toBe("held");
  });

  it("a visit that was marked once is not deleted (D-176)", async () => {
    const { user, id, core, calls } = await openCard("תמר", undefined, async (served, patientId) => {
      // Marked by mistake and put back: it happened, so it stays in the diary.
      const v = (await cardOf(served, patientId)).visits.find((x) => x.visit.date === "2026-11-16")?.visit;
      if (!v) throw new Error("no demo visit on 16.11");
      await ask(served, { op: "save_visit", visit: { ...v, status: "held" }, repeat: null, scope: "one" }, "saved");
      await ask(served, { op: "save_visit", visit: { ...v, status: "planned" }, repeat: null, scope: "one" }, "saved");
    });
    await user.click(within(await visitOn("16.11.2026")).getByRole("button", { name: /פתיחת הפגישה/ }));
    const dialog = await screen.findByRole("dialog", { name: "פרטי הפגישה" });
    await user.click(await within(dialog).findByRole("button", { name: "מחיקת הפגישה" }));
    const confirm = await screen.findByRole("dialog", { name: /מחיק/ });
    const yes = within(confirm).getAllByRole("button").find((b) => /למחוק|מחיקה/.test(b.textContent ?? "") && !/ביטול/.test(b.textContent ?? "")) as HTMLElement;
    await user.click(yes);
    expect(await screen.findByText("הפגישה הזו כבר סומנה פעם (התקיימה, בוטלה או לא הגיעו), ולכן היא נשארת ביומן. אם היא לא תתקיים, אפשר לסמן אותה כמבוטלת.")).toBeInTheDocument();
    expect((await cardOf({ core, calls, caseId: "" }, id)).visits.some((v) => v.visit.date === "2026-11-16")).toBe(true);
  });

  it("a payment that may not move to another card stays in its dialog with the reason (D-177)", async () => {
    const why = "אי אפשר להעביר תקבול למטופל אחר. אפשר לבטל את התקבול הזה, לכתוב למה, ולרשום תקבול חדש בכרטיס הנכון.";
    const { user } = await openCard("יובל", refusing("save_payment", why));
    await user.click(within(await region("כספים")).getByRole("button", { name: "תקבול חדש" }));
    const dialog = await screen.findByRole("dialog", { name: "תקבול חדש" });
    await user.click(within(dialog).getByRole("button", { name: "רישום התקבול" }));
    expect(await within(dialog).findByText(why)).toBeInTheDocument();
  });
});

describe("the small things", () => {
  it("the button that opens a visit is named by what it shows: the date when it is there, then the hours", async () => {
    await openCard("נועם");
    const open = within(await visitOn("20.11.2026")).getByRole("button", { name: /פתיחת הפגישה/ });
    const name = open.getAttribute("aria-label") ?? "";
    expect(name).toMatch(/^20\.11\.2026 \d\d:\d\d–\d\d:\d\d, פתיחת הפגישה של נועם$/);
    // Each piece of text she sees on it is in its name, in the order shown (WCAG 2.5.3).
    const seen = [...open.querySelectorAll("span")].map((s) => s.textContent ?? "");
    expect(seen).toHaveLength(2);
    expect(name.startsWith(seen.join(" "))).toBe(true);
  });

  it("on a list of one day the name starts with the hours", async () => {
    await openOn(TODAY);
    const todays = await region("הפגישות של היום");
    const open = within(todays).getAllByRole("button", { name: /פתיחת הפגישה של/ })[0] as HTMLElement;
    expect(open.getAttribute("aria-label")).toMatch(/^\d\d:\d\d–\d\d:\d\d, פתיחת הפגישה של /);
    expect(open.getAttribute("aria-label")?.startsWith(open.textContent ?? "-")).toBe(true);
  });

  it("an alert on a patient's own card names the child without a link to the card she is on", async () => {
    await openCard("נועם");
    const needs = await region("מה דורש אותי");
    expect(within(needs).getAllByText("נועם").length).toBeGreaterThan(0);
    expect(within(needs).queryByRole("button", { name: "נועם" })).toBeNull();
    // What an alert is for stays: the way to the note it asks for.
    expect(within(needs).getByRole("button", { name: "לכתוב סיכום" })).toBeInTheDocument();
  });

  it("the same alert on the front page does lead to the card", async () => {
    await openOn(TODAY);
    const needs = await region("מה דורש אותי");
    expect(within(needs).getAllByRole("button", { name: "נועם" }).length).toBeGreaterThan(0);
  });

  it("a read that failed can always be tried again, also after “הבנתי”", async () => {
    let refuse = true;
    const { user } = await openOn(TASKS, (cmd, args) => {
      if (cmd === "copy_secret") return 60;
      if (refuse && cmd === "clinic" && (args.request as ClinicRequest).op === "tasks") {
        throw { code: "refused", message: "המעקבים לא נקראו מהכספת.", details: [], action: { kind: "dismiss" } };
      }
      return undefined;
    });
    const mine = await region("המעקבים שלי");
    expect(await within(mine).findByRole("alert")).toHaveTextContent("המעקבים לא נקראו מהכספת.");
    const understood = within(mine).queryByRole("button", { name: "הבנתי" });
    if (understood) await user.click(understood);
    // Whatever the message offered, the tab is never left empty.
    const again = within(mine).getByRole("button", { name: "לנסות שוב" });
    refuse = false;
    await user.click(again);
    expect(await within(mine).findByLabelText("לחזור לרופאת הילדים")).toBeInTheDocument();
    expect(within(mine).queryByRole("button", { name: "לנסות שוב" })).toBeNull();
  });

  it("the visit dialog is one dialog from its skeleton to its form; focus goes to the first thing to fill, and back to the button on close", async () => {
    const { over, release } = holding("settings");
    const { user } = await openOn(TODAY, over);
    await region("הפגישות של היום");
    const opener = screen.getByRole("button", { name: "פגישה חדשה" });
    await user.click(opener);
    const dialog = await screen.findByRole("dialog", { name: "פגישה חדשה" });
    expect(within(dialog).getByRole("status", { name: "טוען את פרטי הפגישה" })).toBeInTheDocument();
    // While it loads, the focus is in the dialog, not behind it.
    expect(dialog.contains(document.activeElement)).toBe(true);
    release();
    await within(dialog).findByLabelText("סוג הפגישה");
    // The same element: nothing closed and opened again under a screen reader.
    expect(screen.getAllByRole("dialog")).toEqual([dialog]);
    expect(dialog.isConnected).toBe(true);
    expect(dialog.contains(document.activeElement)).toBe(true);
    expect(document.activeElement?.id).toBe("visit-patient");
    await user.click(within(dialog).getByRole("button", { name: "ביטול" }));
    await waitFor(() => expect(screen.queryByRole("dialog")).toBeNull());
    expect(opener).toHaveFocus();
  });
});
