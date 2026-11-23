// The requests of 22.11.2026 on their own, over the demo core: asking the AI to word the
// intake's sections while the materials are collected (D-211), and opening the case's
// materials and her own reports to read beside the report (D-212). Every name is invented.
import { configure, render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { clearKept } from "../../kept";
import { ReadDocsDialog } from "../../components/ReadDocs";
import { appApi, CaseHost, Host, serve, type Override } from "../../test/host";
import { MaterialsView } from "./MaterialsView";
import { Reports } from "../style/Reports";
import type { StyleOverview } from "../../ipc/client";

configure({ asyncUtilTimeout: 5000 });
vi.setConfig({ testTimeout: 20_000 });
afterEach(() => clearKept());

const mine = { id: "s1", title: "דוח בדוי א", format: "docx", added_at: 1_790_000_000, words: 900, headings: ["סיכום"], analyzed: true, analysis_demo: false, analysis_items: 3, analysis_inferences: 0 };
const kept = { id: "s1", title: "דוח בדוי א", parts: [{ heading: "סיכום", text: "[ילד] מפגין סקרנות, ועם זאת ניכר קושי במעברים." }] };
const myReports: Override = (cmd) =>
  cmd === "style_overview" ? ({ sources: [mine] } as unknown as StyleOverview) : cmd === "style_source_text" ? kept : undefined;

describe("asking the AI while the materials are collected (D-211)", () => {
  it("offers the intake and the framework's report, says which sections are worded and which stay, and sends each with her words", async () => {
    const served = await serve();
    const app = appApi();
    const draft = vi.fn(async () => true);
    const user = userEvent.setup();
    render(
      <Host app={app}>
        <CaseHost caseId={served.caseId}>{(api) => <MaterialsView api={{ ...api, draft }} />}</CaseHost>
      </Host>,
    );
    const group = await screen.findByRole("group", { name: /^בקשה ל-.* כבר עכשיו$/ });
    expect(within(group).getByRole("button", { name: "מהדיווח של המסגרת" })).toBeInTheDocument();
    await user.click(within(group).getByRole("button", { name: "מהאינטייק" }));
    const window = await screen.findByRole("dialog", { name: /אינטייק עם ההורים$/ });
    // The two sections she already approved stay as they are; the third is worded.
    expect(within(window).getByText(/^ינוסח הסעיף:/).parentElement).toHaveTextContent(/לפי ההורים|בבית/);
    expect(within(window).getByText(/נשאר כמו שהוא/)).toHaveTextContent(/בסעיפים .+ · .+ נשאר/);
    await user.type(within(window).getByLabelText("מה חשוב לך שייכתב? (לא חובה)"), "בקצרה");
    await user.click(within(window).getByRole("button", { name: "✦ לנסח" }));
    await waitFor(() => expect(draft).toHaveBeenCalledOnce());
    expect(draft).toHaveBeenCalledWith("parents_view", expect.stringContaining("בקצרה"));
    await waitFor(() => expect(app.notify).toHaveBeenCalledWith("הטיוטה מחכה לך בכתיבת הדוח, לאישור או לעריכה."));
    expect(screen.queryByRole("dialog")).toBeNull();
  });

  it("with no wish of hers the usual request goes, and a case without consent is offered nothing", async () => {
    const served = await serve();
    const draft = vi.fn(async () => true);
    const user = userEvent.setup();
    const view = render(
      <Host app={appApi()}>
        <CaseHost caseId={served.caseId}>{(api) => <MaterialsView api={{ ...api, draft }} />}</CaseHost>
      </Host>,
    );
    await user.click(await screen.findByRole("button", { name: "מהדיווח של המסגרת" }));
    await user.click(within(await screen.findByRole("dialog")).getByRole("button", { name: "✦ לנסח" }));
    await waitFor(() => expect(draft).toHaveBeenCalledWith("kindergarten", undefined));
    view.unmount();

    const bare = await serve((cmd, args, core) => {
      if (cmd !== "case_detail") return undefined;
      return core.handle(cmd, args).then((d) => {
        const detail = d as { meta: { consent: unknown } };
        return { ...detail, meta: { ...detail.meta, consent: null } };
      });
    });
    render(
      <Host app={appApi()}>
        <CaseHost caseId={bare.caseId}>{(api) => <MaterialsView api={api} />}</CaseHost>
      </Host>,
    );
    await screen.findByRole("heading", { name: "מה יש בתיק" });
    expect(screen.queryByRole("group", { name: /כבר עכשיו$/ })).toBeNull();
  });
});

describe("documents opened to read (D-212)", () => {
  it("beside the report: the case's materials and her reports in one list, each opens as plain text, with a way back", async () => {
    const served = await serve(myReports);
    const user = userEvent.setup();
    render(
      <Host app={appApi()}>
        <CaseHost caseId={served.caseId}>{(api) => <ReadDocsDialog inputs={api.detail.inputs} onClose={() => undefined} />}</CaseHost>
      </Host>,
    );
    const list = await screen.findByRole("dialog", { name: "מסמכים לקריאה" });
    await user.click(within(list).getByRole("button", { name: "שיחה עם הגננת" }));
    const material = await screen.findByRole("dialog", { name: "שיחה עם הגננת" });
    expect(material.querySelector(".read-doc-text")?.textContent).not.toBe("");
    await user.click(within(material).getByRole("button", { name: "→ לכל המסמכים" }));
    await user.click(await screen.findByRole("button", { name: "דוח בדוי א" }));
    const report = await screen.findByRole("dialog", { name: "דוח בדוי א" });
    expect(await within(report).findByText("[ילד] מפגין סקרנות, ועם זאת ניכר קושי במעברים.")).toBeInTheDocument();
    expect(within(report).getByText(/אחרי הסתרה/)).toBeInTheDocument();
    expect(served.calls.some((c) => c.cmd === "style_source_text" && c.args.id === "s1")).toBe(true);
  });

  it("a material's name in a section opens that material straight away", async () => {
    const served = await serve(myReports);
    render(
      <Host app={appApi()}>
        <CaseHost caseId={served.caseId}>{(api) => <ReadDocsDialog inputs={api.detail.inputs} first={api.detail.inputs[0]?.id} onClose={() => undefined} />}</CaseHost>
      </Host>,
    );
    expect(await screen.findByRole("button", { name: "→ לכל המסמכים" })).toBeInTheDocument();
    expect(screen.queryByRole("dialog", { name: "מסמכים לקריאה" })).toBeNull();
  });

  it("from the list of her reports: the name and “לקריאה” open the report as it is kept", async () => {
    await serve(myReports);
    const user = userEvent.setup();
    render(
      <Host app={appApi()}>
        <Reports o={{ sources: [mine] } as unknown as StyleOverview} adding={false} onAdd={() => undefined} onDelete={() => undefined} more={[]} />
      </Host>,
    );
    await user.click(screen.getByRole("button", { name: "לראות את הרשימה" }));
    await user.click(screen.getByRole("button", { name: "לקריאה" }));
    const report = await screen.findByRole("dialog", { name: "דוח בדוי א" });
    expect(await within(report).findByRole("heading", { name: "סיכום" })).toBeInTheDocument();
  });
});
