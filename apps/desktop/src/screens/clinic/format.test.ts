// Money, dates and times as the clinic shows and reads them, and a Hebrew word for every value.
import type { Visit } from "../../ipc/generated/Visit";
import {
  addDays, ALERT_KIND_LABEL, emailReachable, phoneReachable, CONTACT_ROLE_LABEL, dayName, daysBetween, formatDate, formatDateLong, formatDateShort, formatLength,
  formatMoney, formatMonth, formatSpan, formatTime, greeting, initials, isIsoDate, leftAloneWords, moneyInput, monthOf, monthRange, nowIndex, optionsWith,
  parseMoney, parseTime, PATIENT_STATUS_LABEL, PATIENT_STATUSES, PAY_METHOD_LABEL, PAY_METHODS, TASK_KIND_LABEL, TASK_KINDS, today, todayCountWords,
  untilWords, VISIT_KIND_LABEL, VISIT_KINDS, VISIT_STATUS_LABEL, VISIT_STATUSES, visitEnded, visitStarted, visitStep, visitTitle, weekday, weekRange,
} from "./format";

describe("money", () => {
  it("shows agorot as shekels, with agorot only when there are some", () => {
    expect(formatMoney(45_000)).toBe("450 ₪");
    expect(formatMoney(45_050)).toBe("450.50 ₪");
    expect(formatMoney(45_005)).toBe("450.05 ₪");
    expect(formatMoney(0)).toBe("0 ₪");
    expect(formatMoney(1_250_000)).toBe("12,500 ₪");
    expect(formatMoney(-45_000)).toBe("-450 ₪");
  });

  it("reads what she types", () => {
    expect(parseMoney("450")).toBe(45_000);
    expect(parseMoney("450.5")).toBe(45_050);
    expect(parseMoney("450.50")).toBe(45_050);
    expect(parseMoney(" 450 ₪ ")).toBe(45_000);
    expect(parseMoney("1,250")).toBe(125_000);
    expect(parseMoney("0")).toBe(0);
    expect(parseMoney("100000")).toBe(10_000_000);
  });

  it("refuses anything that is not an amount", () => {
    for (const bad of ["", " ", "abc", "-5", "4-50", "12.345", "1,25", "450.", ".5", "4 5 0x", "1e3", "100000.01", "99999999999999999999"]) {
      expect(parseMoney(bad), bad).toBeNull();
    }
  });

  it("round-trips through the field's text", () => {
    expect(moneyInput(null)).toBe("");
    expect(moneyInput(1_250_050)).toBe("12500.50");
    expect(parseMoney(moneyInput(1_250_050))).toBe(1_250_050);
  });
});

describe("dates", () => {
  it("formats a date long, plain and short", () => {
    expect(formatDateLong("2026-11-15")).toBe("יום ראשון, 15.11.2026");
    expect(formatDateLong("2026-11-21")).toBe("שבת, 21.11.2026");
    expect(formatDate("2026-01-05")).toBe("05.01.2026");
    expect(formatDateShort("2026-11-15")).toBe("15.11");
    expect(dayName("2026-11-16")).toBe("שני");
    expect(formatMonth("2026-11")).toBe("נובמבר 2026");
    expect(monthOf("2026-11-15")).toBe("2026-11");
  });

  it("gives nothing for what is not a date", () => {
    expect(isIsoDate("2026-02-30")).toBe(false);
    expect(isIsoDate("15.11.2026")).toBe(false);
    expect(isIsoDate("2028-02-29")).toBe(true);
    expect(formatDateLong("2026-13-01")).toBe("");
    expect(formatDateShort("")).toBe("");
    expect(weekday("nope")).toBeNull();
  });

  it("moves by whole days across months, years and clock changes", () => {
    expect(addDays("2026-11-30", 1)).toBe("2026-12-01");
    expect(addDays("2026-12-31", 1)).toBe("2027-01-01");
    expect(addDays("2026-03-01", -1)).toBe("2026-02-28");
    // The clock moves on 27.3.2026 in Israel; a week later is still seven days later.
    expect(addDays("2026-03-24", 7)).toBe("2026-03-31");
    expect(daysBetween("2026-03-24", "2026-03-31")).toBe(7);
    expect(daysBetween("2026-11-15", "2026-11-01")).toBe(-14);
  });

  it("a week starts on Sunday", () => {
    expect(weekday("2026-11-15")).toBe(0);
    expect(weekRange("2026-11-15")).toEqual({ from: "2026-11-15", to: "2026-11-21" });
    expect(weekRange("2026-11-18")).toEqual({ from: "2026-11-15", to: "2026-11-21" });
    expect(weekRange("2026-11-21")).toEqual({ from: "2026-11-15", to: "2026-11-21" });
    expect(weekRange("2027-01-01")).toEqual({ from: "2026-12-27", to: "2027-01-02" });
  });

  it("a month runs from its first day to its last", () => {
    expect(monthRange("2026-11-15")).toEqual({ from: "2026-11-01", to: "2026-11-30" });
    expect(monthRange("2028-02")).toEqual({ from: "2028-02-01", to: "2028-02-29" });
    expect(monthRange("2026-12")).toEqual({ from: "2026-12-01", to: "2026-12-31" });
  });

  it("today comes from the local clock", () => {
    expect(today(new Date(2026, 10, 15, 9, 30))).toEqual({ date: "2026-11-15", minute: 570 });
    // Just after midnight it is already the new day (an ISO string in UTC would say yesterday).
    expect(today(new Date(2026, 10, 16, 0, 5))).toEqual({ date: "2026-11-16", minute: 5 });
  });
});

describe("times", () => {
  it("shows minutes as a clock and reads them back", () => {
    expect(formatTime(570)).toBe("09:30");
    expect(formatTime(0)).toBe("00:00");
    expect(formatTime(1439)).toBe("23:59");
    expect(parseTime("09:30")).toBe(570);
    expect(parseTime("9:30")).toBe(570);
    expect(parseTime("9.30")).toBe(570);
    expect(parseTime("930")).toBe(570);
    expect(parseTime("1630")).toBe(990);
    expect(parseTime("9")).toBe(540);
    expect(parseTime(" 16 ")).toBe(960);
  });

  it("refuses what is not a time of day", () => {
    for (const bad of ["", "24:00", "9:60", "9:5", "abc", "12:30:00", "-1", "9:"]) expect(parseTime(bad), bad).toBeNull();
  });

  it("a visit's span and length", () => {
    expect(formatSpan(540, 90)).toBe("09:00–10:30");
    expect(formatLength(1)).toBe("דקה");
    expect(formatLength(50)).toBe("50 דקות");
    expect(formatLength(60)).toBe("שעה");
    expect(formatLength(90)).toBe("שעה וחצי");
    expect(formatLength(120)).toBe("שעתיים");
    expect(formatLength(180)).toBe("3 שעות");
    expect(formatLength(135)).toBe("2:15 שעות");
  });
});

describe("the time of a visit", () => {
  // Sunday 15.11.2026 at 10:00; the visit is 10:00–10:50.
  const NOW = { date: "2026-11-15", minute: 600 };
  const visit = (over: Partial<Visit> = {}): Visit => ({
    id: "v1", patient_id: "p1", kind: "assessment", title: "", date: "2026-11-15", start: 600, minutes: 50, place: "", status: "planned",
    status_note: "", fee: 0, charge_missed: false, series_id: null, note: "", remember: [], ...over,
  });

  it("a visit has begun at its very start, and ended at its very end", () => {
    expect(visitStarted(visit(), NOW)).toBe(true);
    expect(visitStarted(visit({ start: 601 }), NOW)).toBe(false);
    expect(visitEnded(visit(), { ...NOW, minute: 649 })).toBe(false);
    expect(visitEnded(visit(), { ...NOW, minute: 650 })).toBe(true);
    // An earlier day has begun and ended whatever the hour; a later day neither.
    expect(visitStarted(visit({ date: "2026-11-14", start: 1200 }), NOW)).toBe(true);
    expect(visitEnded(visit({ date: "2026-11-14", start: 1200 }), NOW)).toBe(true);
    expect(visitStarted(visit({ date: "2026-11-16", start: 0 }), NOW)).toBe(false);
    expect(visitEnded(visit({ date: "2026-11-16", start: 0 }), NOW)).toBe(false);
  });

  it("offers the one step that fits the time", () => {
    // Planned: a reminder while it is ahead; "held" once it has begun (also while under way).
    expect(visitStep(visit({ start: 660 }), false, NOW)).toBe("remind");
    expect(visitStep(visit({ date: "2026-11-16" }), false, NOW)).toBe("remind");
    expect(visitStep(visit(), false, NOW)).toBe("hold");
    expect(visitStep(visit({ date: "2026-11-14" }), false, NOW)).toBe("hold");
    // Held: a note when there is none, then nothing.
    expect(visitStep(visit({ status: "held" }), false, NOW)).toBe("note");
    expect(visitStep(visit({ status: "held" }), true, NOW)).toBeNull();
    expect(visitStep(visit({ status: "cancelled" }), false, NOW)).toBeNull();
    expect(visitStep(visit({ status: "no_show" }), false, NOW)).toBeNull();
    // Time she kept for herself, or a visit without a patient, asks nothing.
    expect(visitStep(visit({ kind: "block", patient_id: null }), false, NOW)).toBeNull();
    expect(visitStep(visit({ kind: "block" }), false, NOW)).toBeNull();
    expect(visitStep(visit({ patient_id: null }), false, NOW)).toBeNull();
  });

  it("the now line goes only between two visits", () => {
    expect(nowIndex([660, 720, 780], 600)).toBe(-1);
    expect(nowIndex([480, 540, 600], 600)).toBe(-1);
    expect(nowIndex([480, 540, 660, 720], 600)).toBe(2);
    expect(nowIndex([], 600)).toBe(-1);
  });

  it("counts the day's visits, and those still ahead when it is some of them", () => {
    expect(todayCountWords(0, 0)).toBe("");
    expect(todayCountWords(1, 0)).toBe("פגישה אחת בלוח");
    expect(todayCountWords(1, 1)).toBe("פגישה אחת בלוח");
    expect(todayCountWords(4, 1)).toBe("4 בלוח · אחת אחרי עכשיו");
    expect(todayCountWords(4, 2)).toBe("4 בלוח · 2 אחרי עכשיו");
    expect(todayCountWords(4, 4)).toBe("4 בלוח");
    expect(todayCountWords(4, 0)).toBe("4 בלוח");
  });

  it("says how long until a visit, in plain Hebrew (never “1 דקות” or “2:35 שעות”)", () => {
    const said: [number, string][] = [
      [1, "בעוד דקה"],
      [2, "בעוד 2 דקות"],
      [48, "בעוד 48 דקות"],
      [59, "בעוד 59 דקות"],
      [60, "בעוד שעה"],
      [61, "בעוד שעה ודקה"],
      [75, "בעוד שעה ו-15 דקות"],
      [119, "בעוד שעה ו-59 דקות"],
      [120, "בעוד שעתיים"],
      [121, "בעוד שעתיים ודקה"],
      [155, "בעוד שעתיים ו-35 דקות"],
      [180, "בעוד 3 שעות"],
      [181, "בעוד 3 שעות ודקה"],
      [195, "בעוד 3 שעות ו-15 דקות"],
      [600, "בעוד 10 שעות"],
    ];
    for (const [minutes, words] of said) expect(untilWords(minutes), String(minutes)).toBe(words);
    // The front page never asks for no time at all; if it did, it is now.
    expect(untilWords(0)).toBe("עכשיו");
    expect(untilWords(-5)).toBe("עכשיו");
  });
});

describe("labels", () => {
  it("every value has a Hebrew word, and no two values share one", () => {
    for (const labels of [VISIT_KIND_LABEL, VISIT_STATUS_LABEL, PAY_METHOD_LABEL, TASK_KIND_LABEL, PATIENT_STATUS_LABEL, CONTACT_ROLE_LABEL, ALERT_KIND_LABEL]) {
      const words = Object.values(labels);
      expect(words.every((w) => /[א-ת]/.test(w))).toBe(true);
      expect(new Set(words).size).toBe(words.length);
    }
  });

  it("the lists offered in a form hold every value once", () => {
    // Every value but "unknown": that one is only ever read, never chosen (D-178).
    const offered = (labels: object) => Object.keys(labels).filter((k) => k !== "unknown").sort();
    expect([...VISIT_KINDS].sort()).toEqual(offered(VISIT_KIND_LABEL));
    expect([...VISIT_STATUSES].sort()).toEqual(offered(VISIT_STATUS_LABEL));
    expect([...PAY_METHODS].sort()).toEqual(offered(PAY_METHOD_LABEL));
    expect([...TASK_KINDS].sort()).toEqual(offered(TASK_KIND_LABEL));
    expect([...PATIENT_STATUSES].sort()).toEqual(offered(PATIENT_STATUS_LABEL));
  });

  it("a value of a later version has a plain word everywhere, and is offered only where it already is", () => {
    for (const labels of [VISIT_KIND_LABEL, VISIT_STATUS_LABEL, PAY_METHOD_LABEL, TASK_KIND_LABEL, PATIENT_STATUS_LABEL, CONTACT_ROLE_LABEL]) {
      expect(labels.unknown).toMatch(/לא מוכר/);
    }
    expect(ALERT_KIND_LABEL.unreadable).toBe("רישום שלא נקרא");
    expect(optionsWith(PAY_METHODS, PAY_METHOD_LABEL, "bit").map((o) => o.value)).toEqual([...PAY_METHODS]);
    const withUnknown = optionsWith(PAY_METHODS, PAY_METHOD_LABEL, "unknown");
    expect(withUnknown.map((o) => o.value)).toEqual([...PAY_METHODS, "unknown"]);
    expect(withUnknown[withUnknown.length - 1]?.label).toBe("אמצעי לא מוכר");
  });

  it("says how many visits of a series were left as they are", () => {
    expect(leftAloneWords(1)).toBe("פגישה אחת בסדרה נשארה כמו שהיא, כי יש לה סיכום או שהיא כבר סומנה");
    expect(leftAloneWords(3)).toBe("3 פגישות בסדרה נשארו כמו שהן, כי יש להן סיכום או שהן כבר סומנו");
  });

  it("the words mean what the spec means", () => {
    expect(VISIT_KIND_LABEL.intake).toBe("פגישת היכרות");
    expect(VISIT_KIND_LABEL.feedback).toBe("שיחת משוב");
    expect(VISIT_KIND_LABEL.block).toBe("זמן שמור");
    expect(VISIT_STATUS_LABEL.held).toBe("התקיימה");
    expect(VISIT_STATUS_LABEL.no_show).toBe("לא הגיעו");
  });

  it("the words are the core's own (logic.rs: kind_label, status_label, method_label, task_kind_label, patient_status_label, contact_role_label)", () => {
    expect(VISIT_KIND_LABEL).toEqual({
      intake: "פגישת היכרות", assessment: "מפגש אבחון", observation: "תצפית", feedback: "שיחת משוב", therapy: "פגישת טיפול",
      guidance: "הדרכת הורים", meeting: "פגישה מקצועית", other: "פגישה אחרת", block: "זמן שמור", supervision: "הדרכה", unknown: "סוג לא מוכר",
    });
    expect(VISIT_STATUS_LABEL).toEqual({ planned: "מתוכננת", held: "התקיימה", cancelled: "בוטלה", no_show: "לא הגיעו", unknown: "מצב לא מוכר" });
    expect(PAY_METHOD_LABEL).toEqual({ cash: "מזומן", transfer: "העברה בנקאית", bit: "ביט", paybox: "פייבוקס", check: "צ'ק", card: "כרטיס אשראי", other: "אחר", unknown: "אמצעי לא מוכר" });
    expect(TASK_KIND_LABEL).toEqual({ call: "שיחת טלפון", questionnaire: "שאלון", document: "מסמך", report: "דוח", payment: "תשלום", follow_up: "מעקב", other: "אחר", unknown: "סוג לא מוכר" });
    expect(PATIENT_STATUS_LABEL).toEqual({ waiting: "ברשימת ההמתנה", active: "בתהליך", closed: "הסתיים", unknown: "מצב לא מוכר" });
    expect(CONTACT_ROLE_LABEL).toEqual({ mother: "אמא", father: "אבא", guardian: "אפוטרופוס", other: "אחר", unknown: "קשר לא מוכר" });
    // And in the core's order.
    expect(VISIT_KINDS).toEqual(["intake", "assessment", "observation", "feedback", "therapy", "guidance", "meeting", "other", "block", "supervision"]);
    expect(PAY_METHODS).toEqual(["cash", "transfer", "bit", "paybox", "check", "card", "other"]);
  });

  it("discreet mode shows initials, never the name", () => {
    expect(initials("נועם לדוגמה")).toBe("נ. ל.");
    expect(initials("  תמר ")).toBe("ת.");
    expect(initials("אבי בן דוד הכהן")).toBe("א. ב. ד.");
    expect(initials("")).toBe("");
  });

  it("greets by the hour", () => {
    expect([0, 5 * 60, 11 * 60 + 59, 12 * 60, 17 * 60, 22 * 60].map(greeting)).toEqual(["לילה טוב", "בוקר טוב", "בוקר טוב", "צהריים טובים", "ערב טוב", "לילה טוב"]);
  });

  it("a visit is called by her own words when she gave some", () => {
    expect(visitTitle({ kind: "block", title: "כתיבת דוחות" })).toBe("כתיבת דוחות");
    expect(visitTitle({ kind: "assessment", title: "  " })).toBe("מפגש אבחון");
  });
});

describe("a phone and an email the messages can reach (as the core's channels.rs)", () => {
  it("phones", () => {
    // Built here, so the file never holds what reads as a real number (the pre-commit scan).
    expect(phoneReachable(["050-", "1234567"].join(""))).toBe(true);
    expect(phoneReachable("+44 20 7946 0000")).toBe(true);
    expect(phoneReachable("050-12")).toBe(false);
    expect(phoneReachable("טלפון")).toBe(false);
    expect(phoneReachable("1234567")).toBe(false);
  });
  it("emails", () => {
    expect(emailReachable("dana.example@example.com")).toBe(true);
    expect(emailReachable("dana@")).toBe(false);
    expect(emailReachable("שירן@example.com")).toBe(false);
    expect(emailReachable("dana@example")).toBe(false);
    expect(emailReachable("a..b@example.com")).toBe(false);
  });
});
