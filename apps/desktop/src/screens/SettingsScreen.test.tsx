// The settings over the demo core: every part opens from the side and passes the accessibility
// check, each kind of row saves and says so, and what she sees when the core refuses. Every
// name is invented.
import { configure, render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { a11yViolations } from "../test/a11y";
import { status } from "../test/fixtures";
import { appApi, Host, serve, type Override } from "../test/host";
import { SettingsScreen } from "./SettingsScreen";
import { SETTINGS_PANES, type SettingsPane } from "./settingsPanes";

configure({ asyncUtilTimeout: 5000 });
vi.setConfig({ testTimeout: 30_000 });

beforeEach(() => {
  // No daily update check from the top bar while the settings are tested.
  localStorage.setItem("dv.updateAuto", "off");
});
afterEach(() => localStorage.removeItem("dv.updateAuto"));

async function openSettings(over?: Override, pane?: SettingsPane, st = status()) {
  const served = await serve(over);
  const app = appApi({ status: st });
  const user = userEvent.setup();
  const view = render(<Host app={app}><SettingsScreen pane={pane} /></Host>);
  const nav = screen.getByRole("navigation", { name: "חלקי ההגדרות" });
  return { ...served, app, user, view, nav };
}

const paneTitle = () => document.querySelector(".settings-pane-title")?.textContent;

describe("settings", () => {
  it("every part opens from the side, is marked as the current one, and passes the accessibility check", async () => {
    const { user, nav, view } = await openSettings();
    for (const p of SETTINGS_PANES) {
      await user.click(within(nav).getByRole("button", { name: p.label }));
      expect(paneTitle()).toBe(p.label);
      expect(within(nav).getByRole("button", { name: p.label })).toHaveAttribute("aria-current", "page");
      // Each part has its content (the report part once its details have loaded).
      await waitFor(() => expect(document.querySelectorAll(".settings-pane section").length).toBeGreaterThan(0));
      expect(await a11yViolations(view.container), p.label).toEqual([]);
    }
  });

  it("opens on the part it was sent to", async () => {
    const { nav } = await openSettings(undefined, "backup");
    expect(paneTitle()).toBe("גיבוי");
    expect(within(nav).getByRole("button", { name: "גיבוי" })).toHaveAttribute("aria-current", "page");
  });

  it("the parts are reached from the keyboard", async () => {
    const { user, nav } = await openSettings();
    within(nav).getByRole("button", { name: "כללי" }).focus();
    await user.tab();
    expect(within(nav).getByRole("button", { name: "חיבור ל-AI" })).toHaveFocus();
    await user.keyboard("{Enter}");
    expect(paneTitle()).toBe("חיבור ל-AI");
  });
});

describe("a row saves and says so", () => {
  it("her names are saved, split at the commas", async () => {
    const { user, calls, app } = await openSettings();
    const names = screen.getByLabelText("השמות שלך");
    expect(names).toHaveValue("רותם בדויה");
    await user.clear(names);
    await user.type(names, "רותם בדויה, ר. בדויה ,");
    await user.click(within(names.closest(".field") as HTMLElement).getByRole("button", { name: "שמירה" }));
    await waitFor(() => expect(app.notify).toHaveBeenCalledWith("השמות נשמרו."));
    expect(calls.find((c) => c.cmd === "set_practitioner")?.args).toEqual({ names: ["רותם בדויה", "ר. בדויה"] });
    expect(app.refresh).toHaveBeenCalled();
  });

  it("“show me every request” is one switch that saves at once", async () => {
    const { user, calls, app } = await openSettings(undefined, "privacy");
    const sw = screen.getByRole("switch", { name: "להציג לי כל בקשה לפני שליחה" });
    expect(sw).toHaveAttribute("aria-checked", "false");
    sw.focus();
    await user.keyboard(" ");
    await waitFor(() => expect(app.notify).toHaveBeenCalledWith("כל בקשה תוצג לפני שליחה."));
    expect(calls.find((c) => c.cmd === "set_review_always")?.args).toEqual({ on: true });
  });

  it("the lock time is chosen from a short list", async () => {
    const { user, calls, app } = await openSettings(undefined, "privacy");
    const group = screen.getByRole("radiogroup", { name: "נעילה אוטומטית אחרי" });
    expect(within(group).getAllByRole("radio").map((r) => r.textContent)).toEqual(["5", "10", "15", "30"]);
    expect(within(group).getByRole("radio", { name: "10" })).toHaveAttribute("aria-checked", "true");
    await user.click(within(group).getByRole("radio", { name: "15" }));
    await waitFor(() => expect(app.notify).toHaveBeenCalledWith("הכספת תינעל אחרי 15 דקות בלי פעילות."));
    expect(calls.find((c) => c.cmd === "set_lock_minutes")?.args).toEqual({ minutes: 15 });
  });

  it("the report's details are saved, without empty signature lines", async () => {
    const { user, calls, app } = await openSettings(undefined, "report");
    const title = await screen.findByLabelText("כותרת");
    await user.clear(title);
    await user.type(title, "דוח אבחון בדוי");
    const signature = screen.getByLabelText("חתימה");
    await user.clear(signature);
    await user.type(signature, "רותם בדויה{Enter}{Enter}פסיכולוגית התפתחותית{Enter}");
    await user.click(within(title.closest("section") as HTMLElement).getByRole("button", { name: "שמירה" }));
    await waitFor(() => expect(app.notify).toHaveBeenCalledWith("הגדרות הדוח נשמרו."));
    expect(calls.find((c) => c.cmd === "set_report_settings")?.args.settings).toMatchObject({
      title: "דוח אבחון בדוי", signature: ["רותם בדויה", "פסיכולוגית התפתחותית"],
    });
  });

  it("another model is chosen from the list", async () => {
    const { user, calls, app } = await openSettings(undefined, "ai");
    const model = screen.getByLabelText("דגם");
    const options = within(model).getAllByRole("option") as HTMLOptionElement[];
    const other = options.find((o) => o.value !== "claude-opus-5");
    if (!other) throw new Error("the allowed list has a second model");
    await user.selectOptions(model, other.value);
    await waitFor(() => expect(app.notify).toHaveBeenCalledWith("הדגם עודכן."));
    expect(calls.find((c) => c.cmd === "set_model")?.args).toEqual({ model: other.value });
  });
});

describe("when the core refuses", () => {
  it("a key that cannot be saved says why, in Hebrew, and is not announced as saved", async () => {
    const { user, app } = await openSettings(undefined, "ai");
    const save = within(screen.getByLabelText("מפתח API").closest(".field") as HTMLElement).getByRole("button", { name: "שמירה" });
    expect(save).toBeDisabled();
    await user.type(screen.getByLabelText("מפתח API"), "sk-ant-בדוי");
    // A provider that keeps data asks for her written ZDR agreement first.
    const ack = screen.queryByRole("checkbox", { name: /יש לי הסכם ZDR בכתב/ });
    if (ack) {
      expect(save).toBeDisabled();
      await user.click(ack);
    }
    await user.click(save);
    expect(await screen.findByRole("alert")).toHaveTextContent("מפתח API מוזן רק בתוכנה המותקנת");
    expect(app.notify).not.toHaveBeenCalled();
  });

  it("a refused change says why, and the line goes when she moves to another part", async () => {
    const { user, nav } = await openSettings((cmd) => {
      if (cmd === "set_practitioner") throw { code: "internal", message: "השמות לא נשמרו.", details: [] };
      return undefined;
    });
    await user.click(within(screen.getByLabelText("השמות שלך").closest(".field") as HTMLElement).getByRole("button", { name: "שמירה" }));
    expect(await screen.findByRole("alert")).toHaveTextContent("השמות לא נשמרו.");
    await user.click(within(nav).getByRole("button", { name: "פרטיות" }));
    expect(screen.queryByText("השמות לא נשמרו.")).toBeNull();
  });

  it("report details that cannot be read say so in the report part, not an empty page", async () => {
    const { user, nav } = await openSettings((cmd) => {
      if (cmd === "report_settings") throw { code: "internal", message: "הגדרות הדוח לא נקראו.", details: [] };
      return undefined;
    });
    await user.click(within(nav).getByRole("button", { name: "הדוח" }));
    expect(await screen.findByRole("alert")).toHaveTextContent("הגדרות הדוח לא נקראו.");
  });
});
