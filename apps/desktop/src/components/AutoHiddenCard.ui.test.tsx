// The card's buttons: "להחזיר" asks once and then says what it did; a name kept in another case
// has no role menu (UIC-9). Every name is invented.
import { render, screen, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { mockIPC } from "@tauri-apps/api/mocks";
import { AppContext, type AppApi } from "../App";
import type { AutoHidden } from "../ipc/generated/AutoHidden";
import { status } from "../test/fixtures";
import { AutoHiddenCard } from "./AutoHiddenCard";

function api(): AppApi {
  return { status: status(), go: vi.fn(), refresh: vi.fn(async () => undefined), notify: vi.fn(), fail: (e) => e.message, lockNow: vi.fn(async () => undefined) };
}

const item = (token: string, tag: string, extra: Partial<AutoHidden> = {}): AutoHidden => ({
  token, tag, role: "other", reason: "שם פרטי", uncertain: false, kind: "name", ...extra,
});

function mount(items: AutoHidden[]) {
  const calls: { cmd: string; args: unknown }[] = [];
  mockIPC((cmd, args) => {
    calls.push({ cmd, args });
    return null;
  });
  const onChanged = vi.fn(async () => undefined);
  const view = render(
    <AppContext.Provider value={api()}>
      <AutoHiddenCard caseId="c1" items={items} onChanged={onChanged} />
    </AppContext.Provider>,
  );
  return { calls, onChanged, view, user: userEvent.setup() };
}

describe("the auto-hidden card", () => {
  it("“להחזיר” asks first, “ביטול” keeps the word hidden, and a yes says the word now goes out as written", async () => {
    const { calls, onChanged, user, view } = mount([item("יובל", "[אדם_1]"), item("רינת", "[גננת_2]", { role: "teacher" })]);
    await user.click(screen.getByRole("button", { name: "להחזיר את יובל" }));
    const ask = screen.getByRole("group", { name: "להחזיר את יובל?" });
    expect(ask).toHaveTextContent('"יובל" יישלח ל-Claude כמו שהוא, בכל התיק הזה.');
    await user.click(within(ask).getByRole("button", { name: "ביטול" }));
    expect(calls.some((c) => c.cmd === "restore_auto_hidden")).toBe(false);
    expect(screen.queryByRole("group", { name: "להחזיר את יובל?" })).toBeNull();

    await user.click(screen.getByRole("button", { name: "להחזיר את יובל" }));
    await user.click(screen.getByRole("button", { name: "כן, להחזיר את יובל" }));
    expect(calls).toContainEqual({ cmd: "restore_auto_hidden", args: { caseId: "c1", token: "יובל", tag: "[אדם_1]" } });
    expect(onChanged).toHaveBeenCalled();
    // The text built again no longer lists it; what was done is still said.
    view.rerender(
      <AppContext.Provider value={api()}>
        <AutoHiddenCard caseId="c1" items={[item("רינת", "[גננת_2]", { role: "teacher" })]} onChanged={onChanged} />
      </AppContext.Provider>,
    );
    expect(screen.getByRole("status")).toHaveTextContent('"יובל" יישלח מעכשיו כמו שהוא, בכל התיק הזה.');
  });

  it("a name kept in another case has no role menu, only “להחזיר” (UIC-9)", () => {
    mount([item("אח", "[תיק_אחר]", { kind: "other_case", reason: "נשמר כשם בתיק אחר" }), item("רינת", "[גננת_2]", { role: "teacher" })]);
    expect(screen.queryByLabelText("התפקיד של אח")).toBeNull();
    expect(screen.getByRole("button", { name: "להחזיר את אח" })).toBeInTheDocument();
    expect(screen.getByLabelText("התפקיד של רינת")).toBeInTheDocument();
  });
});
