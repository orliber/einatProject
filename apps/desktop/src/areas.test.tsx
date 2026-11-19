// The program's two parts (D-191) and discreet mode across both (D-192), over the in-memory
// demo core. Every name is the fake core's invented data.
import { configure, render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { mockIPC } from "@tauri-apps/api/mocks";
import { App } from "./App";
import { FakeCore } from "./web/fakeCore";
import { a11yViolations } from "./test/a11y";

configure({ asyncUtilTimeout: 5000 });
vi.setConfig({ testTimeout: 30_000 });

async function openApp(prep?: (core: FakeCore) => Promise<unknown>) {
  const core = new FakeCore();
  await core.handle("unlock", { password: "x" });
  await prep?.(core);
  mockIPC((cmd, args) => core.handle(cmd, args));
  const user = userEvent.setup();
  const view = render(<App />);
  await screen.findByRole("button", { name: /בתיק של נועם$/ });
  return { core, user, view };
}

const parts = () => screen.getByRole("navigation", { name: "ניווט ראשי" });
const part = (name: string) => within(parts()).getByRole("button", { name: new RegExp(name) });

describe("two parts: writing reports and running the clinic", () => {
  it("opens on the reports, with their three pages in the row under the bar", async () => {
    const { view } = await openApp();
    expect(within(parts()).getAllByRole("button").map((b) => b.textContent)).toEqual(["כתיבת הדוחות", "ניהול הקליניקהבטא"]);
    expect(part("כתיבת הדוחות")).toHaveAttribute("aria-current", "true");
    const row = screen.getByRole("navigation", { name: "כתיבת הדוחות" });
    expect(within(row).getAllByRole("button").map((b) => b.textContent)).toEqual(["תיקים", "התייעצות", "הסגנון שלי"]);
    expect(within(row).getByRole("button", { name: "תיקים" })).toHaveAttribute("aria-current", "page");
    // The clinic's tabs are not on the reports' pages.
    expect(screen.queryByRole("tablist", { name: "חלקי הקליניקה" })).toBeNull();
    expect(await a11yViolations(view.container)).toEqual([]);
  });

  it("the clinic has its own row, and the program opens next time where she worked last", async () => {
    const { user, view } = await openApp();
    await user.click(part("ניהול הקליניקה"));
    const tabs = await screen.findByRole("tablist", { name: "חלקי הקליניקה" });
    expect(within(tabs).getByRole("tab", { name: "היום" })).toHaveAttribute("aria-selected", "true");
    expect(part("ניהול הקליניקה")).toHaveAttribute("aria-current", "true");
    expect(screen.queryByRole("navigation", { name: "כתיבת הדוחות" })).toBeNull();
    // Only the part is kept, as one word.
    expect(window.localStorage.getItem("dv.area")).toBe("clinic");

    view.unmount();
    render(<App />);
    expect(await screen.findByRole("tablist", { name: "חלקי הקליניקה" })).toBeInTheDocument();
    await user.click(part("כתיבת הדוחות"));
    expect(await screen.findByRole("button", { name: /בתיק של נועם$/ })).toBeInTheDocument();
    expect(window.localStorage.getItem("dv.area")).toBeNull();
  });

  it("the settings belong to the whole program: a button in the bar, in neither part", async () => {
    const { user } = await openApp();
    await user.click(screen.getByRole("button", { name: "הגדרות" }));
    await screen.findByRole("heading", { level: 1 });
    expect(screen.getByRole("button", { name: "הגדרות" })).toHaveAttribute("aria-current", "page");
    expect(part("כתיבת הדוחות")).not.toHaveAttribute("aria-current");
    expect(part("ניהול הקליניקה")).not.toHaveAttribute("aria-current");
    expect(screen.queryByRole("navigation", { name: "כתיבת הדוחות" })).toBeNull();
    // A visit to the settings does not change where the program opens.
    expect(window.localStorage.getItem("dv.area")).toBeNull();
  });
});

describe("the bar's tools (D-204)", () => {
  it("the bar's tools are one family: an eye switch with no word, “עזרה למסך הזה (F1)”, “הגדרות”, then “נעילה”", async () => {
    const { user } = await openApp();
    const header = document.querySelector(".topbar") as HTMLElement;
    const tools = header.querySelector(".topbar-tools") as HTMLElement;
    const eye = within(tools).getByRole("switch", { name: "מצב דיסקרטי" });
    expect(Array.from(tools.querySelectorAll("button"))).toEqual([
      eye,
      within(tools).getByRole("button", { name: "עזרה למסך הזה (F1)" }),
      within(tools).getByRole("button", { name: "הגדרות" }),
      within(tools).getByRole("button", { name: "נעילה" }),
    ]);
    // One family: every tool a full-size button, the first three icons only.
    for (const b of tools.querySelectorAll("button")) expect(b).not.toHaveClass("btn-small");
    expect(Array.from(tools.querySelectorAll("button.btn.icon-btn"))).toHaveLength(3);
    expect(eye.textContent).toBe("");
    expect(eye).toHaveAttribute("aria-checked", "false");
    await user.click(eye);
    expect(eye).toHaveAttribute("aria-checked", "true");
    await user.click(eye);
    expect(eye).toHaveAttribute("aria-checked", "false");
    // "מוצפן" stays in the bar, before the tools.
    expect(within(header).getByText("מוצפן, שמור במחשב הזה")).toBeInTheDocument();
  });
});

describe("discreet mode in the reports too", () => {
  it("shows the child's initials in the list, the notices and the case's bar", async () => {
    const { user } = await openApp();
    const toggle = screen.getByRole("switch", { name: "מצב דיסקרטי" });
    await user.click(toggle);
    expect(toggle).toHaveAttribute("aria-checked", "true");
    const list = await screen.findByRole("button", { name: /בתיק של נ\.$/ });
    expect(screen.queryByRole("button", { name: /בתיק של נועם$/ })).toBeNull();
    const main = screen.getByRole("main");
    await waitFor(() => expect(main).not.toHaveTextContent("נועם"));

    await user.click(list);
    await screen.findByRole("article", { name: "הדוח, כמו בקובץ" });
    const bar = document.querySelector(".case-bar") as HTMLElement;
    expect(within(bar).getByText("נ.")).toBeInTheDocument();
    expect(bar).not.toHaveTextContent("נועם");
  });

  it("in discreet mode the card shows initials and the row's name button is named “…בתיק של נ.”", async () => {
    const { user } = await openApp();
    await user.click(screen.getByRole("switch", { name: "מצב דיסקרטי" }));
    const card = screen.getByRole("region", { name: "להמשיך איפה שהפסקת" });
    await waitFor(() => expect(within(card).getByText("נ.")).toHaveClass("resume-name"));
    expect(card).not.toHaveTextContent("נועם");
    const name = screen.getByRole("button", { name: /בתיק של נ\.$/ });
    expect(name).toHaveClass("case-open");
    expect(name).toHaveTextContent(/^נ\.$/);
    expect(screen.getByRole("main")).not.toHaveTextContent("נועם");
  });

  it("is one switch: turned on in the reports, it is on in the clinic", async () => {
    const { user } = await openApp();
    await user.click(screen.getByRole("switch", { name: "מצב דיסקרטי" }));
    await user.click(part("ניהול הקליניקה"));
    await screen.findByRole("tablist", { name: "חלקי הקליניקה" });
    expect(screen.getByRole("switch", { name: "מצב דיסקרטי" })).toHaveAttribute("aria-checked", "true");
    await user.click(screen.getByRole("switch", { name: "מצב דיסקרטי" }));
    await user.click(part("כתיבת הדוחות"));
    expect(await screen.findByRole("button", { name: /בתיק של נועם$/ })).toBeInTheDocument();
  });
});

describe("from a case to the child's card in the clinic (D-195)", () => {
  it("the case's menu opens the linked card, on the clinic's patients tab", async () => {
    const { user } = await openApp();
    await user.click(screen.getByRole("button", { name: /בתיק של נועם$/ }));
    await screen.findByRole("article", { name: "הדוח, כמו בקובץ" });
    await user.click(screen.getByRole("button", { name: /^עוד פעולות/ }));
    await user.click(await screen.findByRole("menuitem", { name: "כרטיס המטופל בקליניקה" }));
    const tabs = await screen.findByRole("tablist", { name: "חלקי הקליניקה" });
    expect(within(tabs).getByRole("tab", { name: "מטופלים" })).toHaveAttribute("aria-selected", "true");
    expect(await screen.findByRole("button", { name: "חזרה לרשימת המטופלים" })).toBeInTheDocument();
  });

  it("a case with no card has no such line", async () => {
    const { user } = await openApp((core) => core.handle("create_case", {
      meta: { code: "תיק-בדיקה", age: null, child_gender: null, current_section: null, retention_until: null, consent: null },
      identities: [{ id: null, role: "child", value: "אורי", aliases: [] }],
    }));
    await user.click(await screen.findByRole("button", { name: /בתיק של אורי$/ }));
    await user.click(await screen.findByRole("button", { name: /^עוד פעולות/ }));
    await screen.findByRole("menuitem", { name: "פרטים ושמות להסתרה" });
    expect(screen.queryByRole("menuitem", { name: "כרטיס המטופל בקליניקה" })).toBeNull();
  });
});

describe("keys (D-191, D-192)", () => {
  it("Ctrl+2 and Ctrl+1 switch the parts, Ctrl+Shift+D turns discreet mode on and off", async () => {
    const { user } = await openApp();
    await user.keyboard("{Control>}2{/Control}");
    expect(await screen.findByRole("tablist", { name: "חלקי הקליניקה" })).toBeInTheDocument();
    await user.keyboard("{Control>}1{/Control}");
    expect(await screen.findByRole("button", { name: /בתיק של נועם$/ })).toBeInTheDocument();
    await user.keyboard("{Control>}{Shift>}D{/Shift}{/Control}");
    expect(screen.getByRole("switch", { name: "מצב דיסקרטי" })).toHaveAttribute("aria-checked", "true");
    expect(await screen.findByRole("button", { name: /בתיק של נ\.$/ })).toBeInTheDocument();
    await user.keyboard("{Control>}{Shift>}D{/Shift}{/Control}");
    expect(screen.getByRole("switch", { name: "מצב דיסקרטי" })).toHaveAttribute("aria-checked", "false");
  });
});

describe("review fixes", () => {
  it("in discreet mode an earlier consultation shows neither the child's name nor her question", async () => {
    const { user } = await openApp(async (core) => {
      const cases = (await core.handle("list_cases", {})) as { id: string; child_name: string | null }[];
      const noam = cases.find((c) => c.child_name === "נועם")?.id ?? null;
      (core as unknown as { convs: unknown[] }).convs.push({
        id: "conv-test", caseId: noam, updated: Date.now(),
        turns: [{ role: "user", text: "מה לכתוב על נועם בסיכום?", hidden: [], demo: true, at: Date.now() }],
      });
    });
    await user.click(screen.getByRole("switch", { name: "מצב דיסקרטי" }));
    await user.click(within(screen.getByRole("navigation", { name: "כתיבת הדוחות" })).getByRole("button", { name: "התייעצות" }));
    const history = await screen.findByRole("navigation", { name: "שיחות קודמות" });
    await waitFor(() => expect(history).toHaveTextContent("על התיק של נ."));
    expect(history).not.toHaveTextContent("נועם");
    await user.click(screen.getByRole("switch", { name: "מצב דיסקרטי" }));
    await waitFor(() => expect(history).toHaveTextContent("מה לכתוב על נועם בסיכום?"));
  });

  it("Ctrl+2 does nothing while a dialog is open, so what she typed there is not lost", async () => {
    const { user } = await openApp();
    await user.click(screen.getByRole("button", { name: "+ תיק חדש" }));
    const dialog = await screen.findByRole("dialog", { name: "תיק חדש" });
    await user.type(within(dialog).getAllByRole("textbox")[0] as HTMLElement, "אורי");
    await user.keyboard("{Control>}2{/Control}");
    expect(screen.getByRole("dialog", { name: "תיק חדש" })).toBeInTheDocument();
    expect(screen.queryByRole("tablist", { name: "חלקי הקליניקה" })).toBeNull();
  });
});
