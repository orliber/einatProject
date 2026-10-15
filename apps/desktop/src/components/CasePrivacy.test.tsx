import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { mockIPC } from "@tauri-apps/api/mocks";
import type { ReactNode } from "react";
import { AppContext, type AppApi } from "../App";
import type { Identity } from "../ipc/generated/Identity";
import { status } from "../test/fixtures";
import { CasePrivacyCard } from "./CasePrivacy";

function api(): AppApi {
  return { status: status(), go: vi.fn(), refresh: vi.fn(async () => undefined), notify: vi.fn(), fail: (e) => e.message, lockNow: vi.fn(async () => undefined) };
}

function Wrap({ children }: { children: ReactNode }) {
  const qc = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  return (
    <QueryClientProvider client={qc}>
      <AppContext.Provider value={api()}>{children}</AppContext.Provider>
    </QueryClientProvider>
  );
}

// Fabricated names.
const person = (id: string, value: string, source: Identity["source"]): Identity => ({
  id, case_id: "c1", role: "teacher", tag: `[גננת_${id}]`, value, aliases: [], source, reason: source === "manual" ? "" : "אחרי 'הגננת'",
});

describe("CasePrivacyCard", () => {
  it("lists only the names the program hid by itself; 'להחזיר' and 'נכון' work from here", async () => {
    const calls: [string, unknown][] = [];
    let now = [person("1", "רינת", "auto"), person("2", "מיכל", "manual"), person("3", "חן", "metadata")];
    mockIPC((cmd, args) => {
      calls.push([cmd, args]);
      if (cmd === "restore_auto_hidden") now = now.filter((i) => i.value !== "חן");
      if (cmd === "case_detail") return { identities: now };
      return null;
    });
    const changed = vi.fn(async () => undefined);
    const user = userEvent.setup();
    render(
      <Wrap>
        <CasePrivacyCard caseId="c1" identities={now} onChanged={changed} />
      </Wrap>,
    );
    await user.click(screen.getByText("2 שמות שהתוכנה הסתירה לבד"));
    expect(screen.queryByText("מיכל")).toBeNull();
    expect(screen.getAllByText("אחרי 'הגננת'")).toHaveLength(2);
    expect(screen.getByText("רינת")).toBeInTheDocument();

    await user.click(screen.getByRole("button", { name: "להחזיר את חן" }));
    expect(calls).toContainEqual(["restore_auto_hidden", { caseId: "c1", token: "חן", tag: "[גננת_3]" }]);
    expect(changed).toHaveBeenLastCalledWith(now);

    await user.click(screen.getByRole("button", { name: "רינת: נכון, תמיד להסתיר בתיק הזה" }));
    expect(calls).toContainEqual(["confirm_auto_hidden", { caseId: "c1", token: "רינת", tag: "[גננת_1]" }]);
  });

  it("opens what left the computer for this case", async () => {
    mockIPC((cmd) => (cmd === "sent_history" ? [] : null));
    const user = userEvent.setup();
    render(
      <Wrap>
        <CasePrivacyCard caseId="c1" identities={[]} onChanged={vi.fn(async () => undefined)} />
      </Wrap>,
    );
    await user.click(screen.getByRole("button", { name: "מה יצא מהמחשב בתיק הזה" }));
    expect(await screen.findByText("עוד לא יצאה שום בקשה.")).toBeInTheDocument();
  });
});
