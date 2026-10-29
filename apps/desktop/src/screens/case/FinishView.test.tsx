// The finish page: the report as the file will be beside what is checked before it goes out.
// Ready, nothing to take out yet, something that stops the file, drafts still waiting, places
// marked to complete (completed right here), paragraphs worth a second look, and the failures
// she may see. Over the demo core; every name and detail is invented.
import { configure, render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import type { CaseDetail } from "../../ipc/client";
import { clearKept } from "../../kept";
import { a11yViolations } from "../../test/a11y";
import { appApi, CaseHost, Host, serve, type Override } from "../../test/host";
import type { FakeCore } from "../../web/fakeCore";
import { FinishView } from "./FinishView";

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

describe("the finish page", () => {
  it("ready: what goes into the file, names back in place, and the file can be made; passes the accessibility check", async () => {
    const { side, user, onExport, onOpen, view } = await openFinish();
    // Sections with materials are still empty: the file can be made, the report is not ready.
    // (The heading says "almost" until the check is back too: wait for the check's own line.)
    expect(await within(side).findByText("2 סעיפים מאושרים ייכנסו לקובץ.")).toBeInTheDocument();
    expect(within(side).getByRole("heading", { name: "כמעט מוכן" })).toBeInTheDocument();
    expect(within(side).getByText(/יש חומרים, (והוא עוד ריק|והם עוד ריקים)\./)).toBeInTheDocument();
    expect(within(side).getByText("השמות חוזרים למקומם בקובץ.")).toBeInTheDocument();
    expect(produce()).toBeEnabled();
    await user.click(produce());
    expect(onExport).toHaveBeenCalledTimes(1);
    await user.click(within(side).getByRole("button", { name: "לדוח" }));
    expect(onOpen).toHaveBeenCalledWith("");
    // The preview: the child's name, the approved sections, and what stays out.
    const preview = screen.getByRole("region", { name: "תצוגה מקדימה של הקובץ" });
    expect(within(preview).getByText("נועם")).toBeInTheDocument();
    expect(within(preview).getByRole("heading", { name: "סיבת הפניה" })).toBeInTheDocument();
    expect(within(preview).getAllByText("לא ייכנס לקובץ (אין פסקה מאושרת)").length).toBeGreaterThan(0);
    expect(await a11yViolations(view.container)).toEqual([]);
  });

  it("ready only when no section that has materials is left empty", async () => {
    const { side } = await openFinish(async (cmd, args, core) => {
      if (cmd !== "check_export") return undefined;
      return { ...((await core.handle(cmd, args)) as object), empty_sections: [] };
    });
    expect(await within(side).findByRole("heading", { name: "הדוח מוכן להוצאה" })).toBeInTheDocument();
    expect(within(side).queryByText(/יש חומרים/)).toBeNull();
    expect(produce()).toBeEnabled();
  });

  it("the preview can be reached and scrolled from the keyboard", async () => {
    const { user } = await openFinish();
    const preview = screen.getByRole("region", { name: "תצוגה מקדימה של הקובץ" });
    expect(preview).toHaveAttribute("tabindex", "0");
    preview.focus();
    await user.keyboard("{Shift>}{Tab}{/Shift}");
    expect(preview).not.toHaveFocus();
    await user.tab();
    expect(preview).toHaveFocus();
  });

  it("nothing approved yet: nothing to take out, and the file waits", async () => {
    const { side } = await openFinish(undefined, {
      caseOf: async (core) => ((await core.handle("list_cases", {})) as { id: string; child_name: string | null }[]).find((c) => c.child_name === "מאיה")?.id ?? "",
    });
    expect(await within(side).findByRole("heading", { name: "עוד אין מה להוציא" })).toBeInTheDocument();
    expect(within(side).getByText("עוד אין סעיף מאושר.")).toBeInTheDocument();
    expect(produce()).toBeDisabled();
    expect(within(side).getByText("צריך לפחות סעיף מאושר אחד.")).toBeInTheDocument();
    expect(within(side).queryByText("השמות חוזרים למקומם בקובץ.")).toBeNull();
  });

  it("something that stops the file is listed with a way to fix it", async () => {
    const { side, user, onOpen } = await openFinish(async (cmd, args, core) => {
      if (cmd !== "check_export") return undefined;
      return { ...((await core.handle(cmd, args)) as object), blocking: ["סיבת הפניה: נשאר תפקיד שלא הוחלף בשם."] };
    });
    expect(await within(side).findByRole("heading", { name: "כמעט מוכן" })).toBeInTheDocument();
    expect(within(side).getByText("סיבת הפניה: נשאר תפקיד שלא הוחלף בשם.")).toBeInTheDocument();
    expect(produce()).toBeDisabled();
    expect(within(side).getByText("קודם מתקנים את מה שמסומן למעלה.")).toBeInTheDocument();
    await user.click(within(side).getByRole("button", { name: "לתיקון" }));
    expect(onOpen).toHaveBeenCalledWith("referral");
  });

  it("drafts still waiting are named and do not stop the file", async () => {
    const { side, user, onOpen } = await openFinish(reshape((d) => {
      const s = sectionOf(d, "kindergarten");
      s.paragraphs = [{ ...(sectionOf(d, "referral").paragraphs[0] as Section["paragraphs"][number]), id: "waiting-1", status: "proposed" }];
    }));
    const line = await within(side).findByText("טיוטות שעוד לא אושרו לא ייכנסו: מסגרת חינוכית.");
    expect(produce()).toBeEnabled();
    await user.click(within(line.closest("li") as HTMLElement).getByRole("button", { name: "לאישור" }));
    expect(onOpen).toHaveBeenCalledWith("kindergarten");
  });

  it("an approved paragraph with a warning is offered for a second look", async () => {
    const { side, user, onOpen } = await openFinish(reshape((d) => {
      (sectionOf(d, "background").paragraphs[0] as Section["paragraphs"][number]).warnings = ["מספר שלא מופיע בטבלת הציונים"];
    }));
    expect(await within(side).findByText("פסקה מאושרת אחת שווה מבט נוסף:")).toBeInTheDocument();
    await user.click(within(side).getByRole("button", { name: /מספר שלא מופיע בטבלת הציונים/ }));
    expect(onOpen).toHaveBeenCalledWith("background");
    expect(produce()).toBeEnabled();
  });

  it("a place marked to complete is completed right here; the file goes out either way", async () => {
    const { side, user, calls, view, caseId } = await openFinish(undefined, {
      caseOf: async (c, noam) => {
        await c.handle("add_own_paragraph", { caseId: noam, sectionKey: "kindergarten", text: "בגן הוא משחק בעיקר לבד. [חסר: כמה ילדים בקבוצה]" });
        return noam;
      },
    });
    const item = await within(side).findByText("מקום אחד מסומן להשלמה");
    const group = item.closest("li") as HTMLElement;
    expect(within(group).getByText("הקובץ יוצא גם כך: הסימונים נשארים בו, ואפשר להשלים אותם בוורד.")).toBeInTheDocument();
    expect(produce()).toBeEnabled();
    await user.click(within(group).getByRole("button", { name: /להשלמה כאן/ }));
    const box = within(group).getByRole("textbox", { name: "השלמת הפסקה" });
    expect(box).toHaveFocus();
    expect(await a11yViolations(view.container)).toEqual([]);
    // Cancel keeps the paragraph as it was.
    await user.click(within(group).getByRole("button", { name: "ביטול" }));
    expect(calls.some((c) => c.cmd === "edit_paragraph")).toBe(false);
    await user.click(within(group).getByRole("button", { name: /להשלמה כאן/ }));
    const again = within(group).getByRole("textbox", { name: "השלמת הפסקה" });
    await user.clear(again);
    await user.type(again, "בגן הוא משחק בעיקר לבד, בקבוצה של שמונה ילדים.");
    await user.click(within(group).getByRole("button", { name: "שמירה" }));
    await waitFor(() => expect(within(side).queryByText("מקום אחד מסומן להשלמה")).toBeNull());
    const edit = calls.find((c) => c.cmd === "edit_paragraph")?.args;
    expect(edit).toMatchObject({ caseId, text: "בגן הוא משחק בעיקר לבד, בקבוצה של שמונה ילדים." });
    expect(String(edit?.openedWith)).toContain("[חסר: כמה ילדים בקבוצה]");
  });

  it("a check that fails says why, in Hebrew, instead of waiting forever", async () => {
    const { side } = await openFinish((cmd) => {
      if (cmd === "check_export") throw { code: "internal", message: "הבדיקה לפני ההוצאה לא הצליחה.", details: [] };
      return undefined;
    });
    expect(await within(side).findByRole("alert")).toHaveTextContent("הבדיקה לפני ההוצאה לא הצליחה.");
    expect(within(side).queryByText("בודקת שהדוח מוכן להוצאה")).toBeNull();
    expect(produce()).toBeDisabled();
  });

  it("a completion that cannot be saved says why and keeps her text", async () => {
    const { side, user } = await openFinish((cmd) => {
      if (cmd === "edit_paragraph") throw { code: "internal", message: "הפסקה לא נשמרה.", details: [] };
      return undefined;
    }, {
      caseOf: async (c, noam) => {
        await c.handle("add_own_paragraph", { caseId: noam, sectionKey: "kindergarten", text: "פסקה מומצאת. [חסר: פרט]" });
        return noam;
      },
    });
    const group = (await within(side).findByText("מקום אחד מסומן להשלמה")).closest("li") as HTMLElement;
    await user.click(within(group).getByRole("button", { name: /להשלמה כאן/ }));
    await user.type(within(group).getByRole("textbox", { name: "השלמת הפסקה" }), " נוסף");
    await user.click(within(group).getByRole("button", { name: "שמירה" }));
    expect(await within(side).findByRole("alert")).toHaveTextContent("הפסקה לא נשמרה.");
    expect(within(group).getByRole("textbox", { name: "השלמת הפסקה" })).toHaveValue("פסקה מומצאת. [חסר: פרט] נוסף");
  });
});
