// The finish page: the report as the file will be beside one line that says where things stand
// and three folded lists (what stops the file, what is worth a look, proofreading). A note is a
// card: fixed right here, opened in its section, or left as it is. Over the demo core; every
// name and detail is invented.
import { configure, render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import type { CaseDetail } from "../../ipc/client";
import { clearKept } from "../../kept";
import { a11yViolations } from "../../test/a11y";
import { prepared } from "../../test/fixtures";
import { appApi, CaseHost, Host, serve, type Override } from "../../test/host";
import type { FakeCore } from "../../web/fakeCore";
import { FinishView } from "./FinishView";
import { excerptOf, needlesOf, NOTE_TITLE, noteWhy } from "./finishNotes";

configure({ asyncUtilTimeout: 5000 });
vi.setConfig({ testTimeout: 30_000 });

afterEach(() => clearKept());

type Section = CaseDetail["sections"][number];

/** Changes the case as the core gives it, for a state the demo case does not have. */
const reshape = (change: (d: CaseDetail) => void): Override => async (cmd, args, core) => {
  if (cmd !== "case_detail") return undefined;
  const d = structuredClone((await core.handle(cmd, args)) as CaseDetail);
  change(d);
  return d;
};

/** The demo case without its own notes, for a test that gives its own. */
const bare = (d: CaseDetail) => {
  for (const p of d.sections.flatMap((s) => s.paragraphs)) Object.assign(p, { notes: [], polish: [], warnings: [] });
};

/** Every needed section written, and no section with materials left empty. */
const written = (change: (d: CaseDetail) => void = () => undefined): Override => {
  const shaped = reshape((d) => {
    bare(d);
    const model = sectionOf(d, "referral").paragraphs[0] as Section["paragraphs"][number];
    for (const key of ["summary", "diagnoses", "recommendations"]) sectionOf(d, key).paragraphs = [{ ...model, id: `${key}-1` }];
    change(d);
  });
  return async (cmd, args, core) => {
    if (cmd !== "check_export") return shaped(cmd, args, core);
    return { ...((await core.handle(cmd, args)) as object), empty_sections: [], polish_notes: 0 };
  };
};

const first = (d: CaseDetail, key: string) => sectionOf(d, key).paragraphs[0] as Section["paragraphs"][number];

const sectionOf = (d: CaseDetail, key: string) => d.sections.find((s) => s.key === key) as Section;

async function openFinish(over?: Override, opts: { caseOf?: (core: FakeCore, noam: string) => Promise<string> } = {}) {
  const served = await serve(over);
  const caseId = opts.caseOf ? await opts.caseOf(served.core, served.caseId) : served.caseId;
  const app = appApi();
  const onExport = vi.fn();
  const onOpen = vi.fn();
  const user = userEvent.setup();
  const view = render(
    <Host app={app}>
      <CaseHost caseId={caseId}>{(api) => <FinishView api={api} onExport={onExport} onOpen={onOpen} />}</CaseHost>
    </Host>,
  );
  const side = await screen.findByRole("region", { name: "לפני שהקובץ יוצא" });
  return { ...served, caseId, app, onExport, onOpen, user, view, side };
}

const produce = () => screen.getByRole("button", { name: "הפקת הקובץ" });
const status = (side: HTMLElement) => within(side).getByRole("heading", { level: 1 });
/** A list's heading button, with its counter. */
const head = (side: HTMLElement, name: RegExp) => within(side).queryByRole("button", { name });
const cardOf = (el: HTMLElement) => el.closest("li.fin-card") as HTMLElement;
const count = (button: HTMLElement | null) => button?.querySelector(".fin-count")?.textContent;

describe("the finish page", () => {
  it("things worth a look: one line says so, the list that needs her is open, the file can be made; passes the accessibility check", async () => {
    const { side, user, onExport, view } = await openFinish();
    // The demo case: two notes on the background, one on the scores, sections still empty.
    await waitFor(() => expect(status(side)).toHaveTextContent("אפשר להוציא. 5 דברים שכדאי לבדוק קודם."));
    expect(within(side).getByText("אין שום דבר שעוצר את הקובץ.")).toBeInTheDocument();
    // Nothing stops the file: that list is not drawn at all.
    expect(head(side, /חייבים לתקן/)).toBeNull();
    const check = head(side, /כדאי לבדוק/) as HTMLElement;
    expect(check).toHaveAttribute("aria-expanded", "true");
    expect(count(check)).toBe("5");
    const polish = head(side, /ליטוש ניסוח/) as HTMLElement;
    expect(polish).toHaveAttribute("aria-expanded", "false");
    expect(count(polish)).toBe("2");
    // A card: what it is, where, the sentence with the words marked, and why.
    const card = cardOf(within(side).getByText("מספר שלא תואם את טבלת הציונים"));
    expect(within(card).getByText("פרופיל קוגניטיבי")).toBeInTheDocument();
    expect(card.querySelector("mark")).toHaveTextContent("115");
    expect(within(card).getByText("המספר 115 לא תואם את טבלת הציונים: הבנה מילולית 112")).toBeInTheDocument();
    expect(within(side).getByText("משפט בלי בסיס בחומרים")).toBeInTheDocument();
    expect(within(side).getByText("משפט שדומה לדוח קודם")).toBeInTheDocument();
    expect(within(side).getByText(/יש חומרים, (והוא עוד לא נכתב|והם עוד לא נכתבו)/)).toBeInTheDocument();
    // Beside the button, one quiet line: what goes in, and the names.
    expect(within(side).getByText(/3 סעיפים מאושרים ייכנסו לקובץ\. השמות חוזרים למקומם בקובץ\./)).toBeInTheDocument();
    expect(produce()).toBeEnabled();
    await user.click(produce());
    expect(onExport).toHaveBeenCalledTimes(1);
    // The preview: the child's name, the approved sections, and what stays out.
    const preview = screen.getByRole("region", { name: "תצוגה מקדימה של הקובץ" });
    expect(within(preview).getByText("נועם")).toBeInTheDocument();
    expect(within(preview).getByRole("heading", { name: "סיבת הפניה" })).toBeInTheDocument();
    expect(within(preview).getAllByText("לא ייכנס לקובץ (אין פסקה מאושרת)").length).toBeGreaterThan(0);
    expect(await a11yViolations(view.container)).toEqual([]);
  });

  it("ready only when no section that has materials is left empty, and the closing sections are written (D-163): then the page is nearly empty", async () => {
    const { side, view } = await openFinish(written());
    await waitFor(() => expect(status(side)).toHaveTextContent("הדוח מוכן להוצאה."));
    expect(within(side).getByText("אין שום דבר שעוצר את הקובץ.")).toBeInTheDocument();
    // No list is drawn when there is nothing in it.
    expect(side.querySelector(".fin-group")).toBeNull();
    expect(side.querySelector(".fin-card")).toBeNull();
    expect(produce()).toBeEnabled();
    expect(await a11yViolations(view.container)).toEqual([]);
  });

  it("empty closing sections are named, the report is not called ready, and the file can still go out (D-163)", async () => {
    const { side, user, onOpen } = await openFinish(async (cmd, args, core) => {
      if (cmd !== "check_export") return reshape(bare)(cmd, args, core);
      return { ...((await core.handle(cmd, args)) as object), empty_sections: [], polish_notes: 0 };
    });
    await waitFor(() => expect(status(side)).toHaveTextContent("אפשר להוציא. דבר אחד שכדאי לבדוק קודם."));
    const card = cardOf(within(side).getByText("סעיפים מסכמים שעוד ריקים"));
    expect(card).toHaveTextContent("סיכום");
    expect(card).toHaveTextContent("המלצות");
    expect(card).toHaveTextContent("הקובץ יכול לצאת גם בלעדיהם.");
    // The DSM section is written only for a child it fits: an empty one is not named.
    expect(card).not.toHaveTextContent("DSM");
    // A state of the report, not a note: it cannot be waved away.
    expect(within(card).queryByRole("button", { name: "בסדר, להשאיר" })).toBeNull();
    expect(produce()).toBeEnabled();
    await user.click(within(card).getByRole("button", { name: "לעבור לסעיף" }));
    expect(onOpen).toHaveBeenCalledWith(expect.stringMatching(/^(summary|diagnoses|recommendations)$/));
  });

  it("the preview can be reached and scrolled from the keyboard", async () => {
    await openFinish();
    const preview = screen.getByRole("region", { name: "תצוגה מקדימה של הקובץ" });
    expect(preview).toHaveAttribute("tabindex", "0");
    preview.focus();
    expect(preview).toHaveFocus();
  });

  it("nothing approved yet: nothing to take out, and the file waits", async () => {
    const { side } = await openFinish(undefined, {
      caseOf: async (core) => ((await core.handle("list_cases", {})) as { id: string; child_name: string | null }[]).find((c) => c.child_name === "מאיה")?.id ?? "",
    });
    await waitFor(() => expect(status(side)).toHaveTextContent("עוד אין מה להוציא."));
    expect(produce()).toBeDisabled();
    expect(within(side).getByText("צריך לפחות סעיף מאושר אחד.")).toBeInTheDocument();
    expect(within(side).queryByText(/השמות חוזרים למקומם בקובץ/)).toBeNull();
    expect(within(side).queryByText("אין שום דבר שעוצר את הקובץ.")).toBeNull();
  });

  it("something that stops the file is in the first list only, cannot be left, and holds the file", async () => {
    const { side, user, onOpen } = await openFinish(async (cmd, args, core) => {
      if (cmd !== "check_export") return undefined;
      return { ...((await core.handle(cmd, args)) as object), blocking: ["סיבת הפניה: נשאר תפקיד שלא הוחלף בשם.", "שורה שעוצרת בלי סעיף"] };
    });
    await waitFor(() => expect(status(side)).toHaveTextContent("נשארו 2 דברים שחייבים לתקן לפני ההוצאה."));
    const must = head(side, /חייבים לתקן/) as HTMLElement;
    expect(must).toHaveAttribute("aria-expanded", "true");
    expect(count(must)).toBe("2");
    // The other lists wait, folded.
    expect(head(side, /כדאי לבדוק/)).toHaveAttribute("aria-expanded", "false");
    const group = must.closest("section") as HTMLElement;
    const card = cardOf(within(group).getByText("נשאר תפקיד שלא הוחלף בשם."));
    expect(within(card).getByText("סיבת הפניה")).toBeInTheDocument();
    expect(within(group).queryByRole("button", { name: "בסדר, להשאיר" })).toBeNull();
    expect(within(group).queryByRole("button", { name: "לתקן כאן" })).toBeNull();
    expect(within(group).queryByText(/הערות שהשארת/)).toBeNull();
    expect(within(side).queryByText("אין שום דבר שעוצר את הקובץ.")).toBeNull();
    // Only there: opening the next list does not show it again.
    await user.click(head(side, /כדאי לבדוק/) as HTMLElement);
    expect(within(side).getAllByText("נשאר תפקיד שלא הוחלף בשם.")).toHaveLength(1);
    expect(produce()).toBeDisabled();
    expect(within(side).getByText("קודם מתקנים את מה שמסומן למעלה.")).toBeInTheDocument();
    await user.click(within(card).getByRole("button", { name: "לעבור לסעיף" }));
    expect(onOpen).toHaveBeenCalledWith("referral");
  });

  it("with one thing that stops the file, the line says one", async () => {
    const { side } = await openFinish(async (cmd, args, core) => {
      if (cmd !== "check_export") return undefined;
      return { ...((await core.handle(cmd, args)) as object), blocking: ["סיבת הפניה: נשאר תפקיד שלא הוחלף בשם."] };
    });
    await waitFor(() => expect(status(side)).toHaveTextContent("נשאר דבר אחד שחייבים לתקן לפני ההוצאה."));
    expect(produce()).toBeDisabled();
  });

  it("drafts still waiting are named and do not stop the file", async () => {
    const { side, user, onOpen } = await openFinish(reshape((d) => {
      const s = sectionOf(d, "kindergarten");
      s.paragraphs = [{ ...first(d, "referral"), id: "waiting-1", status: "proposed" }];
    }));
    const card = cardOf(await within(side).findByText("טיוטות שעוד לא אושרו"));
    expect(card).toHaveTextContent("לא ייכנסו לקובץ: מסגרת חינוכית.");
    expect(produce()).toBeEnabled();
    await user.click(within(card).getByRole("button", { name: "לעבור לסעיף" }));
    expect(onOpen).toHaveBeenCalledWith("kindergarten");
  });

  it("a warning the core sent without a kind is a card with a plain title, one click from its section", async () => {
    const { side, user, onOpen } = await openFinish(written((d) => {
      first(d, "background").warnings = ["מספר שלא מופיע בטבלת הציונים"];
    }));
    const card = cardOf(await within(side).findByText("מספר שלא מופיע בטבלת הציונים"));
    expect(within(card).getByText("הערה")).toBeInTheDocument();
    expect(status(side)).toHaveTextContent("אפשר להוציא. דבר אחד שכדאי לבדוק קודם.");
    await user.click(within(card).getByRole("button", { name: "לעבור לסעיף" }));
    expect(onOpen).toHaveBeenCalledWith("background");
    expect(produce()).toBeEnabled();
  });

  it("every kind of note the core keeps has its own plain title, with no English in it but DSM", () => {
    const titles = Object.values(NOTE_TITLE);
    expect(titles).toHaveLength(19);
    expect(new Set(titles).size).toBe(19);
    for (const t of titles) expect(t.replace("DSM", "")).not.toMatch(/[A-Za-z–—]/);
    expect(NOTE_TITLE.score_mismatch).toBe("מספר שלא תואם את טבלת הציונים");
    expect(NOTE_TITLE.name_in_answer).toBe("שם שנשאר בטקסט");
    expect(NOTE_TITLE.verify_unsupported).toBe("משפט בלי בסיס בחומרים");
    expect(NOTE_TITLE.past_report_overlap).toBe("משפט שדומה לדוח קודם");
    expect(NOTE_TITLE.polish).toBe("ניסוח");
  });

  it("what was found about an approved paragraph comes from the core's notes (D-161); proofreading is in its own list", async () => {
    const { side, user } = await openFinish(written((d) => {
      first(d, "background").notes = [
        { kind: "verify_unsupported", text: "לא נמצא לפסקה בסיס בחומרים" },
        { kind: "polish", text: "מילה כפולה: \"של של\"" },
      ];
    }));
    const check = (await within(side).findByRole("button", { name: /כדאי לבדוק/ })).closest("section") as HTMLElement;
    const card = cardOf(within(check).getByText("משפט בלי בסיס בחומרים"));
    expect(within(card).getByText("לא נמצא לפסקה בסיס בחומרים")).toBeInTheDocument();
    // No needle in the paragraph: its beginning, with nothing marked.
    expect(card.querySelector("mark")).toBeNull();
    expect(card.querySelector(".fin-quote")).toHaveTextContent(/^ההריון והלידה היו תקינים/);
    expect(check).not.toHaveTextContent("מילה כפולה");
    const polish = head(side, /ליטוש ניסוח/) as HTMLElement;
    expect(count(polish)).toBe("1");
    expect(polish).toHaveAttribute("aria-expanded", "false");
    await user.click(polish);
    expect(within(polish.closest("section") as HTMLElement).getByText("מילה כפולה")).toBeInTheDocument();
    expect(produce()).toBeEnabled();
  });

  it("with proofreading notes only, the report is ready and their list is the open one", async () => {
    const { side } = await openFinish(written((d) => {
      first(d, "background").polish = ["מילה כפולה: \"של של\"", "רווח כפול אחרי \"בגן\""];
    }));
    const polish = await within(side).findByRole("button", { name: /ליטוש ניסוח/ });
    expect(status(side)).toHaveTextContent("הדוח מוכן להוצאה.");
    expect(polish).toHaveAttribute("aria-expanded", "true");
    expect(count(polish)).toBe("2");
    expect(head(side, /כדאי לבדוק/)).toBeNull();
    expect(head(side, /חייבים לתקן/)).toBeNull();
    const group = polish.closest("section") as HTMLElement;
    expect(within(group).getByText("מילה כפולה")).toBeInTheDocument();
    expect(within(group).getByText("רווח כפול אחרי \"בגן\"")).toBeInTheDocument();
    expect(produce()).toBeEnabled();
  });

  it("\"fine, leave it\" takes the note off the list and the counter; it can be brought back; nothing is sent to the core", async () => {
    const { side, user, calls } = await openFinish(written((d) => {
      first(d, "background").notes = [
        { kind: "term", text: "כתוב «גן»; בשאר הדוח «מסגרת»" },
        { kind: "age", text: "בפסקה כתוב גיל 6, ובתיק 5" },
      ];
    }));
    const check = await within(side).findByRole("button", { name: /כדאי לבדוק/ });
    const group = check.closest("section") as HTMLElement;
    expect(count(check)).toBe("2");
    expect(status(side)).toHaveTextContent("אפשר להוציא. 2 דברים שכדאי לבדוק קודם.");
    const sent = calls.length;
    const note = within(group).getByText("מונח מקצועי").closest("li.fin-note") as HTMLElement;
    await user.click(within(note).getByRole("button", { name: "בסדר, להשאיר" }));
    expect(within(group).queryByText("מונח מקצועי")).toBeNull();
    expect(within(group).getByText("גיל שלא תואם את פרטי התיק")).toBeInTheDocument();
    expect(count(check)).toBe("1");
    expect(status(side)).toHaveTextContent("אפשר להוציא. דבר אחד שכדאי לבדוק קודם.");
    // The last one too: the list stays only for what she left, and the report is ready as the core says.
    await user.click(within(group).getByRole("button", { name: "בסדר, להשאיר" }));
    expect(group.querySelector(".fin-card")).toBeNull();
    expect(count(check)).toBe("0");
    expect(status(side)).toHaveTextContent("הדוח מוכן להוצאה.");
    expect(produce()).toBeEnabled();
    // Quietly at the end, folded; each can come back.
    const left = within(group).getByRole("button", { name: "הערות שהשארת (2)" });
    expect(left).toHaveAttribute("aria-expanded", "false");
    await user.click(left);
    const row = within(group).getByText("מונח מקצועי · רקע: הריון, לידה והתפתחות").closest("li") as HTMLElement;
    await user.click(within(row).getByRole("button", { name: "להחזיר" }));
    expect(cardOf(within(group).getByText("מונח מקצועי"))).toBeInTheDocument();
    expect(count(check)).toBe("1");
    expect(within(group).getByRole("button", { name: "הערות שהשארת (1)" })).toBeInTheDocument();
    // Only this list: no command went to the core for any of it.
    expect(calls.length).toBe(sent);
  });

  it("a note she left shows again once its paragraph was edited and still has it", async () => {
    const { side, user, calls } = await openFinish(written((d) => {
      first(d, "background").notes = [
        { kind: "age", text: "בפסקה כתוב גיל 6, ובתיק 5" },
        { kind: "term", text: "כתוב «גן»; בשאר הדוח «מסגרת»" },
      ];
    }));
    const check = await within(side).findByRole("button", { name: /כדאי לבדוק/ });
    const group = check.closest("section") as HTMLElement;
    const note = within(group).getByText("גיל שלא תואם את פרטי התיק").closest("li.fin-note") as HTMLElement;
    await user.click(within(note).getByRole("button", { name: "בסדר, להשאיר" }));
    expect(count(check)).toBe("1");
    expect(within(group).queryByText("גיל שלא תואם את פרטי התיק")).toBeNull();
    // She fixes the other note right here; the core still says the first one about the new wording.
    await user.click(within(group).getByRole("button", { name: "לתקן כאן" }));
    // While she fixes it, a note cannot be waved away by a slip.
    expect(within(group).queryByRole("button", { name: "בסדר, להשאיר" })).toBeNull();
    const box = within(group).getByRole("textbox", { name: "תיקון הפסקה" });
    await user.clear(box);
    await user.type(box, "ההריון והלידה היו תקינים, והוא במסגרת מגיל 6.");
    await user.click(within(group).getByRole("button", { name: "שמירה" }));
    await waitFor(() => expect(count(check)).toBe("2"));
    expect(within(group).getByText("גיל שלא תואם את פרטי התיק")).toBeInTheDocument();
    expect(within(group).queryByText(/הערות שהשארת/)).toBeNull();
    expect(calls.filter((c) => c.cmd === "edit_paragraph")).toHaveLength(1);
  });

  it("a place marked to complete is fixed right here, through the core; the file goes out either way", async () => {
    const { side, user, calls, view, caseId } = await openFinish(reshape(bare), {
      caseOf: async (c, noam) => {
        await c.handle("add_own_paragraph", { caseId: noam, sectionKey: "kindergarten", text: "בגן הוא משחק בעיקר לבד. [חסר: כמה ילדים בקבוצה]" });
        return noam;
      },
    });
    const card = cardOf(await within(side).findByText("מקום שסומן להשלמה"));
    expect(within(card).getByText("מסגרת חינוכית")).toBeInTheDocument();
    expect(card.querySelector("mark")).toHaveTextContent("[חסר: כמה ילדים בקבוצה]");
    expect(within(card).getByText("הקובץ יוצא גם כך: הסימון נשאר בו, ואפשר להשלים אותו בוורד.")).toBeInTheDocument();
    expect(produce()).toBeEnabled();
    await user.click(within(card).getByRole("button", { name: "לתקן כאן" }));
    const box = within(card).getByRole("textbox", { name: "תיקון הפסקה" });
    expect(box).toHaveFocus();
    expect(box).toHaveValue("בגן הוא משחק בעיקר לבד. [חסר: כמה ילדים בקבוצה]");
    expect(await a11yViolations(view.container)).toEqual([]);
    // Cancel keeps the paragraph as it was.
    await user.click(within(card).getByRole("button", { name: "ביטול" }));
    expect(calls.some((c) => c.cmd === "edit_paragraph")).toBe(false);
    await user.click(within(card).getByRole("button", { name: "לתקן כאן" }));
    const again = within(card).getByRole("textbox", { name: "תיקון הפסקה" });
    await user.clear(again);
    await user.type(again, "בגן הוא משחק בעיקר לבד, בקבוצה של שמונה ילדים.");
    await user.click(within(card).getByRole("button", { name: "שמירה" }));
    await waitFor(() => expect(within(side).queryByText("מקום שסומן להשלמה")).toBeNull());
    const edit = calls.find((c) => c.cmd === "edit_paragraph")?.args;
    expect(edit).toMatchObject({ caseId, text: "בגן הוא משחק בעיקר לבד, בקבוצה של שמונה ילדים." });
    expect(String(edit?.openedWith)).toContain("[חסר: כמה ילדים בקבוצה]");
  });

  it("a fix that cannot be saved says why and keeps her text", async () => {
    const shaped = reshape(bare);
    const { side, user } = await openFinish((cmd, args, core) => {
      if (cmd === "edit_paragraph") throw { code: "internal", message: "הפסקה לא נשמרה.", details: [] };
      return shaped(cmd, args, core);
    }, {
      caseOf: async (c, noam) => {
        await c.handle("add_own_paragraph", { caseId: noam, sectionKey: "kindergarten", text: "פסקה מומצאת. [חסר: פרט]" });
        return noam;
      },
    });
    const card = cardOf(await within(side).findByText("מקום שסומן להשלמה"));
    await user.click(within(card).getByRole("button", { name: "לתקן כאן" }));
    await user.type(within(card).getByRole("textbox", { name: "תיקון הפסקה" }), " נוסף");
    await user.click(within(card).getByRole("button", { name: "שמירה" }));
    expect(await within(side).findByRole("alert")).toHaveTextContent("הפסקה לא נשמרה.");
    expect(within(card).getByRole("textbox", { name: "תיקון הפסקה" })).toHaveValue("פסקה מומצאת. [חסר: פרט] נוסף");
  });

  it("a note and a paragraph are shown as text: markup in them appears as it was written, and nothing is marked up", async () => {
    const { side } = await openFinish(written((d) => {
      const p = first(d, "background");
      p.text = "ההריון היה תקין. בגן אמרו <script>alert(1)</script> על הילד, וזה <b>חשוב</b>.";
      p.notes = [{ kind: "term", text: "לא ברור: \"<script>alert(1)</script>\" ועוד <b>מודגש</b>" }];
    }));
    const card = cardOf(await within(side).findByText("מונח מקצועי"));
    // The words in question are found by plain search and marked; the sentence is the one around them.
    expect(card.querySelector("mark")?.textContent).toBe("<script>alert(1)</script>");
    expect(card.querySelector(".fin-quote")?.textContent).toBe("בגן אמרו <script>alert(1)</script> על הילד, וזה <b>חשוב</b>.");
    expect(card.querySelector(".fin-why")?.textContent).toBe("לא ברור: \"<script>alert(1)</script>\" ועוד <b>מודגש</b>");
    expect(card.querySelector("script")).toBeNull();
    expect(card.querySelector(".fin-quote b, .fin-why b")).toBeNull();
    const preview = screen.getByRole("region", { name: "תצוגה מקדימה של הקובץ" });
    expect(preview.querySelector("script, b")).toBeNull();
    expect(preview).toHaveTextContent("<b>חשוב</b>");
  });

  it("the sentence a note is about: quoted words first, then a whole number, else the paragraph's beginning", () => {
    expect(needlesOf("המספר 115 לא תואם את טבלת הציונים: הבנה מילולית 112 – לבדוק")).toEqual(["115", "112"]);
    expect(needlesOf("חוזר על קטע מדוח ישן («ההריון תקין»). רווח לפני \",\" אחרי \"הגיע\"")).toEqual(["ההריון תקין", "הגיע"]);
    const text = "הציון הכולל היה 1150. מדד ההבנה היה 115, בטווח הממוצע. משפט שלישי.";
    // 115 inside 1150 is not it; the sentence is the second one.
    expect(excerptOf(text, ["115"])).toEqual({ before: "מדד ההבנה היה ", mark: "115", after: ", בטווח הממוצע." });
    expect(excerptOf(text, ["אין כזה"])).toEqual({ before: text, mark: "", after: "" });
    const long = `${"מילה ".repeat(60)}כאן 42 ${"ועוד ".repeat(60)}`;
    const cut = excerptOf(long, ["42"]);
    expect(cut.mark).toBe("42");
    expect(cut.before.startsWith("…")).toBe(true);
    expect(cut.after.endsWith("…")).toBe(true);
    expect((cut.before + cut.mark + cut.after).length).toBeLessThan(200);
    expect(excerptOf("א".repeat(300), []).before).toHaveLength(141);
    expect(noteWhy("המספר 115 לא תואם – לבדוק")).toBe("המספר 115 לא תואם");
  });

  it("a click on a card brings its paragraph into the middle of the preview and lights it for a moment", async () => {
    const scroll = vi.fn();
    const had = Element.prototype.scrollIntoView;
    Element.prototype.scrollIntoView = scroll;
    try {
      const { side, user } = await openFinish();
      const card = cardOf(await within(side).findByText("מספר שלא תואם את טבלת הציונים"));
      await user.click(card.querySelector(".fin-quote") as HTMLElement);
      const preview = screen.getByRole("region", { name: "תצוגה מקדימה של הקובץ" });
      const lit = preview.querySelector(".a4-p.is-shown") as HTMLElement;
      expect(lit).toHaveTextContent(/מדד ההבנה המילולית היה 115/);
      expect(scroll).toHaveBeenCalledWith(expect.objectContaining({ block: "center" }));
      expect(scroll.mock.contexts[0]).toBe(lit);
      expect(card).toHaveClass("is-shown");
      // A card about sections lights the section.
      await user.click(cardOf(within(side).getByText(/סעיפ(ים|) מסכמ(ים|) שעוד ריק(ים|)/)).querySelector(".fin-quote") as HTMLElement);
      expect(preview.querySelector(".a4-section.is-shown")).not.toBeNull();
      expect(preview.querySelector(".a4-p.is-shown")).toBeNull();
    } finally {
      Element.prototype.scrollIntoView = had;
    }
  });

  it("the notes of the check of the whole report are cards in the second list, as text, and can be left", async () => {
    const shaped = written();
    const { side, user, onOpen } = await openFinish(async (cmd, args, core) => {
      if (cmd === "report_check_setting") return { on: true, cents_per_check: 15 };
      if (cmd === "prepare_report_check") return prepared({ approval_id: "a1" });
      if (cmd === "send_report_check") {
        return {
          notes: [
            { kind: "summary_only", sections: ["summary"], note: "<b>סקרנות</b> מופיעה רק בסיכום" },
            { kind: "other", sections: [], note: "שורה בלי סעיף" },
          ],
          checked: ["referral", "summary"], demo: false,
        };
      }
      return shaped(cmd, args, core);
    });
    await waitFor(() => expect(status(side)).toHaveTextContent("הדוח מוכן להוצאה."));
    await user.click(await within(side).findByRole("button", { name: "לבדוק את הדוח כולו" }));
    const card = cardOf(await within(side).findByText("מופיע רק בסיכום"));
    expect(status(side)).toHaveTextContent("אפשר להוציא. 2 דברים שכדאי לבדוק קודם.");
    expect(within(card).getByText("סיכום")).toBeInTheDocument();
    expect(card).toHaveTextContent("<b>סקרנות</b> מופיעה רק בסיכום");
    expect(card.querySelector(".fin-quote b")).toBeNull();
    await user.click(within(card).getByRole("button", { name: "לעבור לסעיף" }));
    expect(onOpen).toHaveBeenCalledWith("summary");
    // A note with no section is shown, without a place to go to.
    const plain = cardOf(within(side).getByText("שורה בלי סעיף"));
    expect(within(plain).queryByRole("button", { name: "לעבור לסעיף" })).toBeNull();
    await user.click(within(plain).getByRole("button", { name: "בסדר, להשאיר" }));
    expect(within(side).queryByText("שורה בלי סעיף")).toBeNull();
    expect(count(head(side, /כדאי לבדוק/))).toBe("1");
    expect(within(side).getByText(/נמצאו 2 הערות\. הן ברשימה "כדאי לבדוק"\./)).toBeInTheDocument();
  });

  it("notes of the check of the whole report with the same words about other sections are each their own card, and leaving one leaves only it", async () => {
    const shaped = written();
    const same = "אותו משפט מופיע בשני הסעיפים.";
    const errors = vi.spyOn(console, "error");
    try {
      const { side, user } = await openFinish(async (cmd, args, core) => {
        if (cmd === "report_check_setting") return { on: true, cents_per_check: 15 };
        if (cmd === "prepare_report_check") return prepared({ approval_id: "a1" });
        if (cmd === "send_report_check") {
          return {
            notes: [
              { kind: "repetition", sections: ["referral", "background"], note: same },
              { kind: "repetition", sections: ["kindergarten", "summary"], note: same },
              // The very same note twice is one card.
              { kind: "repetition", sections: ["kindergarten", "summary"], note: same },
            ],
            checked: ["referral", "summary"], demo: false,
          };
        }
        return shaped(cmd, args, core);
      });
      await waitFor(() => expect(status(side)).toHaveTextContent("הדוח מוכן להוצאה."));
      await user.click(await within(side).findByRole("button", { name: "לבדוק את הדוח כולו" }));
      await waitFor(() => expect(within(side).getAllByText(same)).toHaveLength(2));
      expect(count(head(side, /כדאי לבדוק/))).toBe("2");
      const firstCard = cardOf(within(side).getAllByText(same)[0] as HTMLElement);
      await user.click(within(firstCard).getByRole("button", { name: "בסדר, להשאיר" }));
      expect(within(side).getAllByText(same)).toHaveLength(1);
      expect(count(head(side, /כדאי לבדוק/))).toBe("1");
      expect(errors.mock.calls.some((c) => String(c[0]).includes("same key"))).toBe(false);
    } finally {
      errors.mockRestore();
    }
  });

  it("a check that fails says why, in Hebrew, instead of waiting forever", async () => {
    const { side } = await openFinish((cmd) => {
      if (cmd === "check_export") throw { code: "internal", message: "הבדיקה לפני ההוצאה לא הצליחה.", details: [] };
      return undefined;
    });
    expect(await within(side).findByRole("alert")).toHaveTextContent("הבדיקה לפני ההוצאה לא הצליחה.");
    expect(within(side).queryByText("בודקת שהדוח מוכן להוצאה")).toBeNull();
    expect(status(side)).not.toHaveTextContent(/מוכן|אפשר להוציא/);
    expect(produce()).toBeDisabled();
  });
});
