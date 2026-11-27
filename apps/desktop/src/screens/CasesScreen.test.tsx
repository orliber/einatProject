// The case list: a search in the recycle bin stays there, a name found only in the bin says so,
// the follow-up item agrees with the child's gender, and the count under the table agrees with
// the tree. Every case is the demo core's invented one.
import { configure, render, screen, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { mockIPC } from "@tauri-apps/api/mocks";
import { App } from "../App";
import { FakeCore } from "../web/fakeCore";
import type { CaseSummary } from "../ipc/client";
import { footLine } from "./CasesScreen";

configure({ asyncUtilTimeout: 5000 });
vi.setConfig({ testTimeout: 30_000 });

async function openList(prep?: (core: FakeCore) => Promise<void>) {
  const core = new FakeCore();
  await core.handle("unlock", { password: "x" });
  await prep?.(core);
  mockIPC((cmd, args) => core.handle(cmd, args));
  const user = userEvent.setup();
  render(<App />);
  return { user, core };
}

async function binNoam(core: FakeCore) {
  const noam = ((await core.handle("list_cases", {})) as CaseSummary[]).find((c) => c.child_name === "נועם");
  if (!noam) throw new Error("no case");
  await core.handle("delete_case", { caseId: noam.id });
}

const search = () => screen.getByRole("searchbox", { name: "חיפוש תיקים" });

describe("the case list", () => {
  it("a search typed in the recycle bin looks in the bin, and stays there", async () => {
    const { user } = await openList(binNoam);
    await user.click(await screen.findByRole("button", { name: /סל המחזור/ }));
    expect(await screen.findByRole("button", { name: "שחזור" })).toBeInTheDocument();
    await user.type(search(), "נועם");
    // Still the bin: its path, and the case with its "שחזור".
    expect(within(screen.getByRole("navigation", { name: "מיקום" })).getByText("סל המחזור")).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "שחזור" })).toBeInTheDocument();
    await user.clear(search());
    await user.type(search(), "שם שלא קיים");
    expect(screen.getByText("אין בסל המחזור תיק בשם הזה.")).toBeInTheDocument();
    expect(screen.queryByText(/התיק לא קיים|לא נמצא תיק או תיקייה/)).toBeNull();
  });

  it("a name found only in the recycle bin says so from the list, one click from it", async () => {
    const { user } = await openList(binNoam);
    await screen.findAllByRole("button", { name: /^פתיחת התיקייה/ });
    await user.type(search(), "נועם");
    expect(await screen.findByText("נמצא תיק אחד בשם הזה בסל המחזור.")).toBeInTheDocument();
    await user.click(screen.getByRole("button", { name: "לסל המחזור" }));
    expect(await screen.findByRole("button", { name: "שחזור" })).toBeInTheDocument();
    expect(search()).toHaveValue("נועם");
  });

  it("a restored case is said to be back in the list in plain words", async () => {
    const { user } = await openList(binNoam);
    await user.click(await screen.findByRole("button", { name: /סל המחזור/ }));
    await user.click(await screen.findByRole("button", { name: "שחזור" }));
    expect(await screen.findByText("התיק שוחזר אל רשימת התיקים.")).toBeInTheDocument();
  });

  it("the follow-up item agrees with the child's gender", async () => {
    const { user } = await openList(async (core) => {
      await core.handle("create_sample_case", {});
    });
    await user.click(await screen.findByRole("button", { name: "פעולות על התיק של נועם" }));
    expect(await screen.findByRole("menuitem", { name: "אבחון מעקב לילד הזה" })).toBeInTheDocument();
    await user.keyboard("{Escape}");
    await user.click(screen.getByRole("button", { name: "פעולות על התיק של תמר" }));
    expect(await screen.findByRole("menuitem", { name: "אבחון מעקב לילדה הזו" })).toBeInTheDocument();
  });

  it("the count under the table agrees with the tree when cases sit in folders", () => {
    expect(footLine(1, 1, false)).toBe("תיק אחד");
    expect(footLine(0, 1, false)).toBe("0 תיקים כאן, תיק אחד בסך הכול");
    expect(footLine(1, 3, false)).toBe("תיק אחד כאן, 3 תיקים בסך הכול");
    expect(footLine(2, null, true)).toBe("2 תיקים בכל התיקיות");
  });
});
