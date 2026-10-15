import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { mockIPC } from "@tauri-apps/api/mocks";
import { AppContext, type AppApi } from "../App";
import { status } from "../test/fixtures";
import { SectionGuide } from "./SectionGuide";

function api(): AppApi {
  return { status: status(), go: vi.fn(), refresh: vi.fn(async () => undefined), notify: vi.fn(), fail: (e) => e.message, lockNow: vi.fn(async () => undefined) };
}

const INSTITUTE = "רקע משפחתי: מי ההורים והאחים.\nהריון, לידה והתפתחות בשנים הראשונות.";

describe("SectionGuide", () => {
  it("shows the institute's lines, edits them for every case, and adds a line for this case", async () => {
    const calls: [string, unknown][] = [];
    mockIPC((cmd, args) => {
      calls.push([cmd, args]);
      return null;
    });
    const saved = vi.fn(async () => undefined);
    const user = userEvent.setup();
    render(
      <AppContext.Provider value={api()}>
        <SectionGuide caseId="c1" sectionKey="background" guide={INSTITUTE} isDefault caseGuide="" onSaved={saved} />
      </AppContext.Provider>,
    );
    await user.click(screen.getByText("איך לכתוב את הסעיף הזה"));
    expect(screen.getAllByRole("listitem")).toHaveLength(2);
    expect(screen.getByText("הנוסח של המכון.")).toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "החזרה לנוסח המכון" })).toBeNull();

    await user.click(screen.getByRole("button", { name: "עריכה" }));
    const box = screen.getByLabelText("ההנחיה לסעיף, לכל התיקים");
    await user.clear(box);
    await user.type(box, "שורה אחת שלי");
    await user.click(screen.getByRole("button", { name: "שמירה" }));
    expect(calls).toContainEqual(["set_section_guide", { sectionKey: "background", text: "שורה אחת שלי" }]);
    expect(saved).toHaveBeenCalledTimes(1);

    await user.click(screen.getByRole("button", { name: "+ תוספת לתיק הזה" }));
    await user.type(screen.getByLabelText("תוספת לתיק הזה בלבד"), "להזכיר את המעבר דירה");
    await user.click(screen.getByRole("button", { name: "שמירה" }));
    expect(calls).toContainEqual(["set_case_guide", { caseId: "c1", sectionKey: "background", text: "להזכיר את המעבר דירה" }]);
  });

  it("offers the institute's text back once she edited it, and shows the case's lines", async () => {
    const calls: [string, unknown][] = [];
    mockIPC((cmd, args) => {
      calls.push([cmd, args]);
      return null;
    });
    const user = userEvent.setup();
    render(
      <AppContext.Provider value={api()}>
        <SectionGuide caseId="c1" sectionKey="background" guide="שורה שלי" isDefault={false} caseGuide="בתיק הזה: המעבר" onSaved={vi.fn()} />
      </AppContext.Provider>,
    );
    await user.click(screen.getByText("איך לכתוב את הסעיף הזה"));
    expect(screen.getByText("הנוסח שלך, לכל התיקים.")).toBeInTheDocument();
    expect(screen.getByText("בתיק הזה: המעבר")).toBeInTheDocument();
    await user.click(screen.getByRole("button", { name: "החזרה לנוסח המכון" }));
    expect(calls).toContainEqual(["set_section_guide", { sectionKey: "background", text: "" }]);
  });
});
