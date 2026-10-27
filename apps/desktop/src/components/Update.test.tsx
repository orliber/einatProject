// Updates (D-033, D-038): the settings row (check now, the newest already, a check that fails),
// the window of a new version (now, later, a download that fails, a computer that does not
// install by itself), the chip in the top bar, and the daily check that she can turn off.
// The versions and notes are invented.
import { configure, render, renderHook, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { mockIPC } from "@tauri-apps/api/mocks";
import type { UpdateView } from "../ipc/client";
import { a11yViolations } from "../test/a11y";
import { appApi, Host, serve, type Override } from "../test/host";
import { checkNow, LockScreenUpdate, UpdateChip, UpdateDialog, UpdateSettings, versionAtLeast } from "./Update";
import { forgetHold, useHoldUnsaved } from "./Unsaved";

configure({ asyncUtilTimeout: 5000 });

const UPDATE: UpdateView = { version: "0.9.3", current: "0.9.2", published: "2026-10-27", notes: ["שינוי בדוי ראשון", "שינוי בדוי שני"], size_mb: 14, can_install: true };

// What was found is kept for the whole program; each test starts with nothing found.
beforeEach(async () => {
  mockIPC((cmd) => (cmd === "check_update" ? null : undefined));
  await checkNow();
  localStorage.removeItem("dv.updateCheckedAt");
  localStorage.removeItem("dv.updateAuto");
  localStorage.removeItem("dv.updateFound");
});

/** The module as it starts after a reload (at a lock) or a restart: with what is in storage. */
async function freshModule() {
  vi.resetModules();
  return import("./Update");
}

async function show(node: React.ReactNode, over?: Override) {
  const served = await serve(over);
  const app = appApi();
  const user = userEvent.setup();
  const view = render(<Host app={app}>{node}</Host>);
  return { ...served, app, user, view };
}

const settings = () => screen.getByRole("region", { name: "עדכונים" });

describe("the updates row in the settings", () => {
  it("shows this version, and says when it is already the newest; passes the accessibility check", async () => {
    const { user, view } = await show(<UpdateSettings />, (cmd) => (cmd === "check_update" ? null : cmd === "ping" ? { ipc_version: 1, core_version: "0.9.2", build_commit: "t", fips_active: false, platform: "windows", slowest_key_check_ms: 0, key_check_wait_ms: 0 } : undefined));
    expect(await within(settings()).findByText("גרסה 0.9.2")).toBeInTheDocument();
    await user.click(within(settings()).getByRole("button", { name: "בדיקת עדכונים" }));
    expect(await within(settings()).findByRole("status")).toHaveTextContent("זו הגרסה החדשה ביותר.");
    expect(await a11yViolations(view.container)).toEqual([]);
  });

  it("a newer version opens its window at once, and the row offers it from then on", async () => {
    const { user } = await show(<UpdateSettings />, (cmd) => (cmd === "check_update" ? UPDATE : undefined));
    await user.click(within(settings()).getByRole("button", { name: "בדיקת עדכונים" }));
    const dialog = await screen.findByRole("dialog", { name: "גרסה 0.9.3 מוכנה" });
    await user.click(within(dialog).getByRole("button", { name: "אחר כך" }));
    expect(screen.queryByRole("dialog")).toBeNull();
    expect(settings()).toHaveTextContent("גרסה 0.9.3 מוכנה");
    await user.click(within(settings()).getByRole("button", { name: "לעדכון" }));
    expect(await screen.findByRole("dialog", { name: "גרסה 0.9.3 מוכנה" })).toBeInTheDocument();
  });

  it("a notice that is not signed by Or is refused, and she is told in Hebrew", async () => {
    const { user } = await show(<UpdateSettings />, (cmd) => {
      if (cmd === "check_update") throw { code: "update", message: "ההודעה על הגרסה החדשה לא חתומה כמו שצריך, ולכן לא הותקן כלום.", details: [] };
      return undefined;
    });
    await user.click(within(settings()).getByRole("button", { name: "בדיקת עדכונים" }));
    expect(await within(settings()).findByRole("alert")).toHaveTextContent("ההודעה על הגרסה החדשה לא חתומה כמו שצריך, ולכן לא הותקן כלום.");
    expect(screen.queryByRole("dialog")).toBeNull();
    expect(within(settings()).getByRole("button", { name: "בדיקת עדכונים" })).toBeEnabled();
  });

  it("the daily check can be turned off, and then the program does not ask by itself", async () => {
    const { user, calls } = await show(<UpdateSettings />);
    const daily = within(settings()).getByRole("switch", { name: "לבדוק פעם ביום אם יש גרסה חדשה" });
    expect(daily).toHaveAttribute("aria-checked", "true");
    daily.focus();
    await user.keyboard(" ");
    expect(daily).toHaveAttribute("aria-checked", "false");
    expect(localStorage.getItem("dv.updateAuto")).toBe("off");
    // The chip mounts the daily check: with it off, nothing is asked.
    render(<Host app={appApi()}><UpdateChip /></Host>);
    await new Promise((r) => setTimeout(r, 50));
    expect(calls.some((c) => c.cmd === "check_update")).toBe(false);
  });
});

describe("the daily check and the chip", () => {
  it("asks once a day, and the chip appears only when there is a new version", async () => {
    const { calls } = await show(<UpdateChip />, (cmd) => (cmd === "check_update" ? UPDATE : undefined));
    expect(await screen.findByRole("button", { name: /יש גרסה חדשה|גרסה חדשה מוכנה/ })).toBeInTheDocument();
    expect(calls.filter((c) => c.cmd === "check_update")).toHaveLength(1);
    // Fetched and checked in the background, before her click.
    await waitFor(() => expect(calls.some((c) => c.cmd === "prepare_update")).toBe(true));
    expect(await screen.findByRole("button", { name: /גרסה חדשה מוכנה/ })).toBeInTheDocument();
  });

  it("checked today already: nothing is asked again", async () => {
    localStorage.setItem("dv.updateCheckedAt", String(Date.now()));
    const { calls, view } = await show(<UpdateChip />, (cmd) => (cmd === "check_update" ? UPDATE : undefined));
    await new Promise((r) => setTimeout(r, 50));
    expect(calls.some((c) => c.cmd === "check_update")).toBe(false);
    expect(view.container).toBeEmptyDOMElement();
  });

  it("a version found is remembered: after a reload the chip is back without a new check, and the installer is fetched again (regression)", async () => {
    await show(<UpdateChip />, (cmd) => (cmd === "check_update" ? UPDATE : undefined));
    await screen.findByRole("button", { name: /יש גרסה חדשה|גרסה חדשה מוכנה/ });
    expect(JSON.parse(localStorage.getItem("dv.updateFound") ?? "null")).toEqual(UPDATE);
    localStorage.setItem("dv.updateCheckedAt", String(Date.now()));
    const fresh = await freshModule();
    const { calls } = await show(<fresh.UpdateChip />, (cmd) => (cmd === "check_update" ? UPDATE : cmd === "ping" ? { ipc_version: 1, core_version: "0.9.2", build_commit: "t", fips_active: false, platform: "windows", slowest_key_check_ms: 0, key_check_wait_ms: 0 } : undefined));
    expect(await screen.findByRole("button", { name: /גרסה חדשה מוכנה/ })).toBeInTheDocument();
    expect(calls.some((c) => c.cmd === "check_update")).toBe(false);
    expect(calls.filter((c) => c.cmd === "prepare_update")).toHaveLength(1);
    vi.resetModules();
  });

  it("a remembered version that is already running (after the install) is forgotten (regression)", async () => {
    localStorage.setItem("dv.updateFound", JSON.stringify(UPDATE));
    localStorage.setItem("dv.updateCheckedAt", String(Date.now()));
    const fresh = await freshModule();
    const { calls, view } = await show(<fresh.UpdateChip />, (cmd) => (cmd === "ping" ? { ipc_version: 1, core_version: "0.9.3", build_commit: "t", fips_active: false, platform: "windows", slowest_key_check_ms: 0, key_check_wait_ms: 0 } : undefined));
    await waitFor(() => expect(calls.some((c) => c.cmd === "ping")).toBe(true));
    await new Promise((r) => setTimeout(r, 50));
    expect(view.container).toBeEmptyDOMElement();
    expect(localStorage.getItem("dv.updateFound")).toBeNull();
    expect(calls.some((c) => c.cmd === "prepare_update")).toBe(false);
    vi.resetModules();
  });

  it("a check that finds nothing newer forgets the remembered version; a broken entry is dropped", async () => {
    localStorage.setItem("dv.updateFound", '{"version": 7}');
    const fresh = await freshModule();
    const { view } = await show(<fresh.UpdateChip />, (cmd) => (cmd === "check_update" ? null : undefined));
    await new Promise((r) => setTimeout(r, 50));
    expect(view.container).toBeEmptyDOMElement();
    expect(localStorage.getItem("dv.updateFound")).toBeNull();
    vi.resetModules();
    mockIPC((cmd) => (cmd === "check_update" ? UPDATE : undefined));
    await checkNow();
    expect(localStorage.getItem("dv.updateFound")).not.toBeNull();
    mockIPC((cmd) => (cmd === "check_update" ? null : undefined));
    await checkNow();
    expect(localStorage.getItem("dv.updateFound")).toBeNull();
  });

  it("compares versions by number", () => {
    expect(versionAtLeast("0.9.3", "0.9.3")).toBe(true);
    expect(versionAtLeast("0.9.10", "0.9.3")).toBe(true);
    expect(versionAtLeast("0.9.2", "0.9.3")).toBe(false);
    expect(versionAtLeast("1.0.0-beta", "0.9.3")).toBe(true);
    expect(versionAtLeast("v0.10.0", "0.9.3")).toBe(true);
  });

  it("on the lock screen, only a version that installs here is offered", async () => {
    const { view } = await show(<LockScreenUpdate />, (cmd) => (cmd === "check_update" ? { ...UPDATE, can_install: false } : undefined));
    await new Promise((r) => setTimeout(r, 50));
    expect(view.container).toBeEmptyDOMElement();
  });
});

describe("the window of a new version", () => {
  it("says what is new and installs only on her click; passes the accessibility check", async () => {
    const onClose = vi.fn();
    const { user, calls, view } = await show(<UpdateDialog update={UPDATE} onClose={onClose} />, (cmd) => (cmd === "install_update" ? new Promise(() => undefined) : undefined));
    const dialog = screen.getByRole("dialog", { name: "גרסה 0.9.3 מוכנה" });
    expect(dialog).toHaveTextContent("עכשיו מותקנת 0.9.2 · פורסמה 27.10.2026");
    expect(within(dialog).getAllByRole("listitem").map((li) => li.textContent)).toEqual(["שינוי בדוי ראשון", "שינוי בדוי שני"]);
    expect(dialog).toHaveTextContent("התיקים והסיסמה נשארים.");
    expect(await a11yViolations(view.container)).toEqual([]);
    expect(calls.some((c) => c.cmd === "install_update")).toBe(false);
    await user.click(within(dialog).getByRole("button", { name: "לעדכן עכשיו" }));
    expect(calls.some((c) => c.cmd === "install_update")).toBe(true);
    // While it downloads and installs, the window stays: no "later", no ×, no Escape.
    expect(within(dialog).getByRole("button", { name: /מורידה ובודקת|מתעדכנת/ })).toBeDisabled();
    expect(within(dialog).getByRole("button", { name: "אחר כך" })).toBeDisabled();
    expect(within(dialog).queryByRole("button", { name: "סגירה" })).toBeNull();
    await user.keyboard("{Escape}");
    expect(onClose).not.toHaveBeenCalled();
  });

  it("what she is typing reaches the core before the install starts, as before a lock (regression)", async () => {
    const { user, calls } = await show(<UpdateDialog update={UPDATE} onClose={vi.fn()} />, (cmd) => (cmd === "install_update" ? new Promise(() => undefined) : undefined));
    const held = renderHook(() => useHoldUnsaved("c1", "פסקה משלי", "טקסט בדוי באמצע הקלדה"));
    await user.click(screen.getByRole("button", { name: "לעדכן עכשיו" }));
    await waitFor(() => expect(calls.some((c) => c.cmd === "install_update")).toBe(true));
    const cmds = calls.map((c) => c.cmd);
    expect(cmds.indexOf("hold_unsaved")).toBeGreaterThanOrEqual(0);
    expect(cmds.indexOf("hold_unsaved")).toBeLessThan(cmds.indexOf("install_update"));
    held.unmount();
    forgetHold();
  });

  it("a download that fails says why, and she can try again or leave it for later", async () => {
    const onClose = vi.fn();
    const { user } = await show(<UpdateDialog update={UPDATE} onClose={onClose} />, (cmd) => {
      if (cmd === "install_update") throw { code: "update", message: "ההורדה לא הושלמה. אפשר לנסות שוב מאוחר יותר.", details: [] };
      return undefined;
    });
    const dialog = screen.getByRole("dialog");
    await user.click(within(dialog).getByRole("button", { name: "לעדכן עכשיו" }));
    expect(await within(dialog).findByRole("alert")).toHaveTextContent("ההורדה לא הושלמה. אפשר לנסות שוב מאוחר יותר.");
    expect(within(dialog).getByRole("button", { name: "לעדכן עכשיו" })).toBeEnabled();
    await user.keyboard("{Escape}");
    expect(onClose).toHaveBeenCalledTimes(1);
  });

  it("where the program does not install by itself, it says where the new version comes from", async () => {
    const onClose = vi.fn();
    const { user } = await show(<UpdateDialog update={{ ...UPDATE, can_install: false }} onClose={onClose} />);
    const dialog = screen.getByRole("dialog");
    expect(dialog).toHaveTextContent("במחשב הזה העדכון לא מותקן לבד. מתקינים את הגרסה החדשה מהקישור ששלח אור.");
    expect(within(dialog).queryByRole("button", { name: "לעדכן עכשיו" })).toBeNull();
    await user.click(within(dialog).getByRole("button", { name: "הבנתי" }));
    expect(onClose).toHaveBeenCalled();
  });
});
