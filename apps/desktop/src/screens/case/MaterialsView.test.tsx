// The materials of a case on their own, over the demo core: what the list and the header say,
// a file that was read in full and saved at once (with "לבדיקה" and "ביטול"), a file that was
// not read in full (the import window), a file that was refused, the window of one material
// with what will be hidden marked, and deleting, restoring and editing, failures included.
// The demo core cannot read a file through the IPC mock, so the reading is answered here.
// Every name is invented.
import { configure, render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { clearKept } from "../../kept";
import type { ImportPreview } from "../../ipc/client";
import { a11yViolations } from "../../test/a11y";
import { appApi, CaseHost, Host, serve, type Override } from "../../test/host";
import { MaterialsView } from "./MaterialsView";

configure({ asyncUtilTimeout: 5000 });
vi.setConfig({ testTimeout: 20_000 });

afterEach(() => clearKept());

const read = (over: Partial<ImportPreview> = {}): ImportPreview => ({
  file_name: "סיכום בדוי.docx", format: "docx", pages: 2, title: "  סיכום טיפול בדוי  ", suggested_kind: "prior_report",
  body: "סיכום טיפול בדוי של שנה קודמת.", preview: [{ text: "סיכום טיפול בדוי של שנה קודמת.", mark: null, label: null }],
  suspects: [], auto_hidden: [], hidden: [], left_out: [], name_suggestions: [], warnings: [], ...over,
});

const refused = (message: string) => ({ code: "unsupported", message, details: [] });

async function openMaterials(over?: Override) {
  const served = await serve(over);
  const app = appApi();
  const onWrite = vi.fn();
  const user = userEvent.setup();
  const view = render(
    <Host app={app}>
      <CaseHost caseId={served.caseId}>{(api) => <MaterialsView api={api} onWrite={onWrite} />}</CaseHost>
    </Host>,
  );
  await screen.findByRole("heading", { name: "מה יש בתיק" });
  return { ...served, app, onWrite, user, view };
}

const list = () => screen.getByRole("region", { name: "רשימת החומרים" });
const upload = () => screen.getByLabelText("העלאת מסמך");
const file = (name = "סיכום בדוי.docx") => new File(["תוכן בדוי"], name);
/** The action the last notice offered ("ביטול" after a delete). */
function lastAction(app: ReturnType<typeof appApi>) {
  const call = vi.mocked(app.notify).mock.calls.at(-1);
  return call?.[1] as { label: string; run: () => void } | undefined;
}

describe("the materials of a case", () => {
  it("says what there is, newest first, what is still missing, and passes the accessibility check", async () => {
    const { user, onWrite, view } = await openMaterials();
    expect(screen.getByText("יש חומר ל-10 מתוך 11 סעיפים. הכול נשמר מוצפן.")).toBeInTheDocument();
    expect(within(list()).getByText("4 חומרים")).toBeInTheDocument();
    expect(within(list()).getByText("4 חומרים עוד לא נקראו. הסיכום יופיע אחרי המיון.")).toBeInTheDocument();
    const titles = within(list()).getAllByRole("button").map((b) => within(b).getByText(/./, { selector: ".material-title" }).textContent);
    expect(titles).toEqual(["ציוני WPPSI-IV", "מפגש שני", "שיחה עם הגננת", "אינטייק עם ההורים"]);
    // What is there is marked as there; the one missing slot offers its way in.
    const needs = screen.getByRole("list", { name: "מה עוד חסר" });
    expect(within(needs).getAllByText("יש:")).toHaveLength(4);
    // (jsdom drops the spaces that open the inner spans; a browser reads them.)
    expect(within(needs).getByRole("button", { name: /^אבחונים קודמים ?\(לא חובה\) ?\(העלאת מסמך\)$/ })).toBeInTheDocument();
    await user.click(screen.getByRole("button", { name: "לכתיבת הדוח ←" }));
    expect(onWrite).toHaveBeenCalledOnce();
    expect(await a11yViolations(view.container)).toEqual([]);
  });

  it("an empty case says so, and offers no way on to writing", async () => {
    await openMaterials((cmd, args, core) => {
      if (cmd !== "case_detail") return undefined;
      return core.handle(cmd, args).then((d) => ({ ...(d as object), inputs: [], routing: [] }));
    });
    expect(screen.getByText("עוד אין חומרים. הכול נשמר מוצפן.")).toBeInTheDocument();
    expect(screen.getByText("מתחילים מאינטייק עם ההורים, או גוררים מסמך.")).toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "לכתיבת הדוח ←" })).toBeNull();
    expect(within(screen.getByRole("list", { name: "מה עוד חסר" })).queryByText("יש:")).toBeNull();
  });

  it("a file read in full is saved at once, trimmed, with “לבדיקה” and “ביטול” beside it", async () => {
    const { user, app, calls } = await openMaterials((cmd) => (cmd === "import_document" ? read() : undefined));
    await user.upload(upload(), file());
    const saved = await screen.findByRole("status");
    expect(saved).toHaveTextContent('"סיכום טיפול בדוי" נשמר בתיק.');
    expect(calls.find((c) => c.cmd === "add_input")?.args).toMatchObject({ kind: "prior_report", title: "סיכום טיפול בדוי", content: "סיכום טיפול בדוי של שנה קודמת." });
    expect(await within(list()).findByText("5 חומרים")).toBeInTheDocument();
    // No window opened for a file that was read in full.
    expect(screen.queryByRole("dialog")).toBeNull();

    await user.click(within(saved).getByRole("button", { name: "ביטול" }));
    await waitFor(() => expect(app.notify).toHaveBeenCalledWith("הייבוא בוטל."));
    expect(screen.queryByRole("status")).toBeNull();
    expect(within(list()).getByText("4 חומרים")).toBeInTheDocument();
    expect(within(list()).queryByText("סיכום טיפול בדוי")).toBeNull();
  });

  it("“לבדיקה” opens what was saved in its window", async () => {
    const { user } = await openMaterials((cmd) => (cmd === "import_document" ? read() : undefined));
    await user.upload(upload(), file());
    await user.click(within(await screen.findByRole("status")).getByRole("button", { name: "לבדיקה" }));
    const window = await screen.findByRole("dialog", { name: "סיכום טיפול בדוי" });
    expect(within(window).getByText("סיכום טיפול בדוי של שנה קודמת.")).toBeInTheDocument();
    expect(screen.queryByRole("status")).toBeNull();
  });

  it("a file chosen from a slot's “+” is saved as that slot's kind, not the guess", async () => {
    const { user, calls } = await openMaterials((cmd) => (cmd === "import_document" ? read({ suggested_kind: "free_text" }) : undefined));
    await user.click(screen.getByRole("button", { name: "הוספה לאבחונים קודמים" }));
    await user.click(await screen.findByRole("menuitem", { name: "העלאת קובץ מהמחשב (וורד, PDF)" }));
    await user.upload(upload(), file());
    await screen.findByRole("status");
    expect(calls.find((c) => c.cmd === "add_input")?.args.kind).toBe("prior_report");
  });

  it("a file not read in full opens the import window with what was left out, and nothing is saved until she says so", async () => {
    const preview = read({
      body: "דנה סיפרה על שנה בדויה.",
      preview: [{ text: "דנה", mark: "replaced", label: "שם אם" }, { text: " סיפרה על שנה בדויה.", mark: null, label: null }],
      left_out: ["כותרת עליונה בדויה"],
    });
    const { user, app, calls } = await openMaterials((cmd) => (cmd === "import_document" ? preview : undefined));
    await user.upload(upload(), file());
    const window = await screen.findByRole("dialog", { name: "ייבוא מסמך" });
    expect(within(window).getByText("לא נקלט מהקובץ")).toBeInTheDocument();
    expect(within(window).getByText("כותרת עליונה בדויה")).toBeInTheDocument();
    // What will be hidden is marked in the text she is about to keep.
    expect(within(window).getByTitle("יוסתר: שם אם")).toHaveTextContent("דנה");
    expect(calls.some((c) => c.cmd === "add_input")).toBe(false);
    expect(screen.queryByRole("status")).toBeNull();
    await user.click(within(window).getByRole("button", { name: "שמירה בתיק" }));
    await waitFor(() => expect(app.notify).toHaveBeenCalledWith("המסמך נשמר בתיק."));
    expect(screen.queryByRole("dialog")).toBeNull();
    expect(within(list()).getByText("5 חומרים")).toBeInTheDocument();
  });

  it("a file that cannot be read says why in Hebrew, and saves nothing", async () => {
    const why = "בתצוגה בדפדפן אפשר לייבא רק Word (docx) או טקסט.";
    const { user, calls } = await openMaterials((cmd) => {
      if (cmd === "import_document") throw refused(why);
      return undefined;
    });
    await user.upload(upload(), file("סריקה בדויה.heic"));
    expect(await screen.findByRole("alert")).toHaveTextContent(why);
    expect(calls.some((c) => c.cmd === "add_input")).toBe(false);
    expect(screen.getByRole("button", { name: "בוחרים קובץ" })).toBeInTheDocument();
  });

  it("a material opens in its window from the keyboard, with what will be hidden marked, and Escape closes it", async () => {
    const { user, view } = await openMaterials();
    const intake = within(list()).getByRole("button", { name: /אינטייק עם ההורים/ });
    intake.focus();
    await user.keyboard("{Enter}");
    const window = await screen.findByRole("dialog", { name: "אינטייק עם ההורים" });
    expect(await within(window).findByTitle("יוסתר: שם אם")).toHaveTextContent("דנה");
    expect(within(window).getByTitle("יוסתר: שם גננת / מחנכת")).toHaveTextContent("מיכל");
    expect(within(window).getByText("מזין בדוח:")).toBeInTheDocument();
    expect(within(window).getByText("סיבת הפניה · רקע: הריון, לידה והתפתחות · תיאור תפקוד והתנהגות (לפי ההורים)")).toBeInTheDocument();
    expect(within(window).getByText(/^\(לפי סוג החומר\)/)).toBeInTheDocument();
    expect(await a11yViolations(view.container)).toEqual([]);
    await user.keyboard("{Escape}");
    await waitFor(() => expect(screen.queryByRole("dialog")).toBeNull());
  });

  it("deleting closes the window with a way back; a way back that is gone says why", async () => {
    const { user, app } = await openMaterials();
    await user.click(within(list()).getByRole("button", { name: /מפגש שני/ }));
    await user.click(within(await screen.findByRole("dialog", { name: "מפגש שני" })).getByRole("button", { name: "מחיקה" }));
    await waitFor(() => expect(screen.queryByRole("dialog")).toBeNull());
    expect(app.notify).toHaveBeenCalledWith("החומר נמחק מהתיק.", expect.objectContaining({ label: "ביטול" }));
    expect(within(list()).queryByText("מפגש שני")).toBeNull();
    const undo = lastAction(app);
    undo?.run();
    await waitFor(() => expect(app.notify).toHaveBeenLastCalledWith("החומר הוחזר לתיק."));
    expect(await within(list()).findByText("מפגש שני")).toBeInTheDocument();
    // The same "ביטול" a second time: the material is back already, nothing is held to restore.
    undo?.run();
    expect(await screen.findByRole("alert")).toHaveTextContent("אי אפשר להחזיר את החומר הזה עוד. אפשר להוסיף אותו מחדש.");
  });

  it("a delete that fails is said in the material's window, not behind it, and the material stays", async () => {
    const { user } = await openMaterials((cmd) => {
      if (cmd === "delete_input") throw { code: "io", message: "לא הצלחתי לשמור את השינוי בכספת.", details: [] };
      return undefined;
    });
    await user.click(within(list()).getByRole("button", { name: /שיחה עם הגננת/ }));
    const window = await screen.findByRole("dialog", { name: "שיחה עם הגננת" });
    await user.click(within(window).getByRole("button", { name: "מחיקה" }));
    expect(await within(window).findByRole("alert")).toHaveTextContent("לא הצלחתי לשמור את השינוי בכספת.");
    expect(screen.getAllByRole("alert")).toHaveLength(1);
    expect(within(list()).getByText("שיחה עם הגננת")).toBeInTheDocument();
  });

  it("editing opens the text in its window and saves the change; a failed save keeps the text and says why", async () => {
    let failOnce = true;
    const { user, app, calls } = await openMaterials((cmd) => {
      if (cmd === "update_input" && failOnce) {
        failOnce = false;
        throw { code: "io", message: "השינוי לא נשמר. אפשר לנסות שוב.", details: [] };
      }
      return undefined;
    });
    await user.click(within(list()).getByRole("button", { name: /שיחה עם הגננת/ }));
    await user.click(within(await screen.findByRole("dialog", { name: "שיחה עם הגננת" })).getByRole("button", { name: "עריכה" }));
    const editor = await screen.findByRole("dialog", { name: "עריכת חומר" });
    // An edit keeps its kind: there is nothing to choose.
    expect(within(editor).queryByLabelText("סוג החומר")).toBeNull();
    const title = within(editor).getByLabelText("כותרת");
    await user.clear(title);
    await user.type(title, "שיחה שנייה עם הגננת");
    await user.click(within(editor).getByRole("button", { name: "שמירה בתיק" }));
    expect(await within(editor).findByRole("alert")).toHaveTextContent("השינוי לא נשמר. אפשר לנסות שוב.");
    expect(title).toHaveValue("שיחה שנייה עם הגננת");
    await user.click(within(editor).getByRole("button", { name: "שמירה בתיק" }));
    await waitFor(() => expect(app.notify).toHaveBeenCalledWith("נשמר בתיק."));
    expect(calls.filter((c) => c.cmd === "update_input").at(-1)?.args).toMatchObject({ title: "שיחה שנייה עם הגננת" });
    expect(await within(list()).findByText("שיחה שנייה עם הגננת")).toBeInTheDocument();
  });

  it("the sections a material feeds are hers to change", async () => {
    const { user, app, calls } = await openMaterials();
    await user.click(within(list()).getByRole("button", { name: /שיחה עם הגננת/ }));
    await user.click(within(await screen.findByRole("dialog", { name: "שיחה עם הגננת" })).getByRole("button", { name: "שינוי הסעיפים" }));
    const pick = await screen.findByRole("dialog", { name: "לאילו סעיפים החומר הזה מזין?" });
    expect(within(pick).getByRole("checkbox", { name: /מסגרת חינוכית/ })).toBeChecked();
    expect(within(pick).getByText("לפי סוג החומר")).toBeInTheDocument();
    await user.click(within(pick).getByRole("checkbox", { name: /הופעה והתרשמות/ }));
    await user.click(within(pick).getByRole("button", { name: "שמירה" }));
    await waitFor(() => expect(app.notify).toHaveBeenCalledWith("הסעיפים של החומר עודכנו."));
    expect(calls.find((c) => c.cmd === "set_input_sections")?.args.sections).toEqual(["kindergarten", "appearance"]);
  });
});
