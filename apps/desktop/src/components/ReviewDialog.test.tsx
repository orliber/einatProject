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

  it("nothing is asked: what was hidden is on the card, and sending stays open", async () => {
    const calls: { cmd: string; args: unknown }[] = [];
    mockIPC((cmd, args) => {
      calls.push({ cmd, args });
      return cmd === "change_role" ? [] : null;
    });
    const hidden = [
      { token: "רינת", tag: "[גננת_2]", role: "teacher" as const, reason: "אחרי 'הגננת'", uncertain: false, kind: "name" as const },
      { token: "שאלון", tag: "ש[ילד]", role: "child" as const, reason: "שם שמופיע בתיק בתוך מילה", uncertain: true, kind: "declared_in_word" as const },
      { token: "חן", tag: "[אדם_1]", role: "other" as const, reason: "שם שהוא גם מילה", uncertain: true, kind: "name" as const },
    ];
    const reprepare = vi.fn(async () => prepared({ auto_hidden: hidden.slice(0, 1) }));
    reprepare.mockResolvedValueOnce(prepared({ auto_hidden: hidden }));
    const onSend = vi.fn(async () => undefined);
    const user = userEvent.setup();
    render(
      <AppContext.Provider value={api()}>
        <ReviewDialog title="t" caseId="c1" prepared={prepared({ auto_hidden: hidden })}
          reprepare={reprepare} onSend={onSend} onClose={vi.fn()} />
      </AppContext.Provider>,
    );
    expect(screen.queryByRole("alert")).toBeNull();
    const card = screen.getByRole("region", { name: "מה הוסתר אוטומטית" });
    expect(card).toHaveTextContent("הסתרתי אוטומטית 3 פרטים");
    expect(card).toHaveTextContent("אחרי 'הגננת'");
    expect(card).toHaveTextContent("גננת_2");
    // A name inside a word first, then the weaker signs, then the rest.
    const words = Array.from(card.querySelectorAll("li b")).map((b) => b.textContent);
    expect(words).toEqual(["שאלון", "חן", "רינת"]);
    expect(screen.getByRole("button", { name: /שליחה/ })).toBeEnabled();

    await user.selectOptions(screen.getByLabelText("התפקיד של רינת"), "school_teacher");
    expect(calls).toContainEqual({ cmd: "change_role", args: { caseId: "c1", tag: "[גננת_2]", role: "school_teacher" } });

    await user.click(screen.getByRole("button", { name: "להחזיר את חן" }));
    expect(calls).toContainEqual({ cmd: "restore_auto_hidden", args: { caseId: "c1", token: "חן", tag: "[אדם_1]" } });
    expect(reprepare).toHaveBeenCalledTimes(2);
    expect(await screen.findByText("הסתרתי אוטומטית פרט אחד")).toBeInTheDocument();
    await user.click(screen.getByRole("button", { name: /שליחה/ }));
    expect(onSend).toHaveBeenCalledWith("abc");
  });

  it("a question without a case shows the card without restoring", () => {
    const hidden = [{ token: "רינת", tag: "[אדם_1]", role: "other" as const, reason: "שם פרטי", uncertain: false, kind: "name" as const }];
    render(
      <AppContext.Provider value={api()}>
        <ReviewDialog title="t" caseId={null} prepared={prepared({ auto_hidden: hidden })}
          reprepare={vi.fn()} onSend={vi.fn()} onClose={vi.fn()} />
      </AppContext.Provider>,
    );
    expect(screen.getByRole("region", { name: "מה הוסתר אוטומטית" })).toHaveTextContent("בשאלה כללית אי אפשר להחזיר");
    expect(screen.queryByRole("button", { name: /להחזיר/ })).toBeNull();
  });

  it("a blocked request says why and cannot be sent", () => {
    const blocked = [{ code: "foreign_tag", message: "תגית שלא שייכת לתיק הזה", detail: "[אדם_9]" }];
    render(
      <AppContext.Provider value={api()}>
        <ReviewDialog title="t" caseId="c1" prepared={prepared({ approval_id: null, blocked })}
          reprepare={vi.fn()} onSend={vi.fn()} onClose={vi.fn()} />
      </AppContext.Provider>,
    );
    expect(screen.getByText(/תגית שלא שייכת לתיק הזה/)).toBeInTheDocument();
    expect(screen.getByRole("button", { name: /שליחה/ })).toBeDisabled();
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
