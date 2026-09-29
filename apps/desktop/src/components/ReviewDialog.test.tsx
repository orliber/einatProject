import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { mockIPC } from "@tauri-apps/api/mocks";
import { AppContext, type AppApi } from "../App";
import { prepared, status } from "../test/fixtures";
import { ReviewDialog } from "./ReviewDialog";
import { makePassphrase } from "./ExportDialog";

function api(): AppApi {
  return { status: status(), go: vi.fn(), refresh: vi.fn(async () => undefined), notify: vi.fn(), fail: (e) => e.message, lockNow: vi.fn(async () => undefined) };
}

describe("ReviewDialog", () => {
  it("shows both sides: the name on the case side, the role on Claude's side", () => {
    render(
      <AppContext.Provider value={api()}>
        <ReviewDialog title="t" caseId="c1" prepared={prepared()} reprepare={vi.fn()} onSend={vi.fn()} onClose={vi.fn()} />
      </AppContext.Provider>,
    );
    expect(screen.getByLabelText("מה שכתוב בתיק")).toHaveTextContent("נועם");
    expect(screen.getByLabelText("מה Claude יקבל")).not.toHaveTextContent("נועם");
    expect(screen.getByLabelText("מה Claude יקבל")).toHaveTextContent("ילד");
  });

  it("an unknown name blocks sending until it is decided", async () => {
    const decided: unknown[] = [];
    mockIPC((cmd, args) => {
      if (cmd === "decide_suspect") decided.push(args);
      return null;
    });
    const suspect = { token: "יובל", kind: "unknown_name" as const, message: "שם לא מוכר", suggested_role: "other_child" as const };
    const reprepare = vi.fn(async () => prepared());
    const onSend = vi.fn(async () => undefined);
    const user = userEvent.setup();
    render(
      <AppContext.Provider value={api()}>
        <ReviewDialog title="t" caseId="c1" prepared={prepared({ approval_id: null, suspects: [suspect] })}
          reprepare={reprepare} onSend={onSend} onClose={vi.fn()} />
      </AppContext.Provider>,
    );
    const send = screen.getByRole("button", { name: /שליחה/ });
    expect(send).toBeDisabled();
    expect(screen.getByRole("alert")).toHaveTextContent("יובל");

    await user.click(screen.getByRole("button", { name: /להסתיר כ/ }));
    expect(decided).toEqual([{ caseId: "c1", token: "יובל", decision: { decision: "hide", role: "other_child" } }]);
    expect(reprepare).toHaveBeenCalled();
    const enabled = await screen.findByRole("button", { name: /שליחה/ });
    expect(enabled).toBeEnabled();
    await user.click(enabled);
    expect(onSend).toHaveBeenCalledWith("abc");
  });

  it("an ambiguous word offers 'it is the name' and 'ordinary word'", () => {
    const suspect = { token: "שאלון", kind: "ambiguous_word" as const, message: "", suggested_role: "other" as const };
    render(
      <AppContext.Provider value={api()}>
        <ReviewDialog title="t" caseId="c1" prepared={prepared({ approval_id: null, suspects: [suspect] })}
          reprepare={vi.fn()} onSend={vi.fn()} onClose={vi.fn()} />
      </AppContext.Provider>,
    );
    expect(screen.getByRole("button", { name: "זה השם, להסתיר" })).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "מילה רגילה" })).toBeInTheDocument();
  });
});

describe("makePassphrase", () => {
  it("is long enough for the export and differs each time", () => {
    const a = makePassphrase();
    expect(a.length).toBeGreaterThanOrEqual(10);
    expect(a.split("-")).toHaveLength(5);
    expect(new Set(Array.from({ length: 20 }, makePassphrase)).size).toBeGreaterThan(15);
  });
});
