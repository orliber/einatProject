import { render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { mockIPC } from "@tauri-apps/api/mocks";
import { AppContext, type AppApi } from "../App";
import { status } from "../test/fixtures";
import { clearKept } from "../kept";
import { SectionGuide } from "./SectionGuide";

afterEach(() => clearKept());

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
    // She confirms first: her text is deleted from every case.
    expect(calls.some(([cmd]) => cmd === "set_section_guide")).toBe(false);
    await user.click(screen.getByRole("button", { name: "להחזיר" }));
    expect(calls).toContainEqual(["set_section_guide", { sectionKey: "background", text: "" }]);
  });

  it("opens the editor again on the saved text, not on what was typed before", async () => {
    mockIPC(() => null);
    const user = userEvent.setup();
    const view = (guide: string) => (
      <AppContext.Provider value={api()}>
        <SectionGuide caseId="c1" sectionKey="background" guide={guide} isDefault={false} caseGuide="" onSaved={vi.fn()} />
      </AppContext.Provider>
    );
    const { rerender } = render(view("ישן"));
    await user.click(screen.getByText("איך לכתוב את הסעיף הזה"));
    await user.click(screen.getByRole("button", { name: "עריכה" }));
    await user.type(screen.getByLabelText("ההנחיה לסעיף, לכל התיקים"), " ועוד");
    await user.click(screen.getByRole("button", { name: "ביטול" }));
    rerender(view("חדש"));
    await user.click(screen.getByRole("button", { name: "עריכה" }));
    expect(screen.getByLabelText("ההנחיה לסעיף, לכל התיקים")).toHaveValue("חדש");
  });

  it("lets go of the held text when the editor closes, and stays open on what she saved", async () => {
    const calls: [string, unknown][] = [];
    mockIPC((cmd, args) => {
      calls.push([cmd, args]);
      return null;
    });
    const user = userEvent.setup();
    render(
      <AppContext.Provider value={api()}>
        <SectionGuide caseId="c1" sectionKey="background" guide="שורה" isDefault caseGuide="" onSaved={vi.fn()} />
      </AppContext.Provider>,
    );
    await user.click(screen.getByText("איך לכתוב את הסעיף הזה"));
    await user.click(screen.getByRole("button", { name: "עריכה" }));
    await user.type(screen.getByLabelText("ההנחיה לסעיף, לכל התיקים"), " נוספת");
    const holds = () => calls.filter(([cmd]) => cmd === "hold_unsaved").map(([, a]) => a);
    await waitFor(() => expect(holds().at(-1)).toMatchObject({ edit: { text: "שורה נוספת", view: "background" } }));
    await user.keyboard("{Escape}");
    await waitFor(() => expect(holds().at(-1)).toEqual({ edit: null }));
    // The guide is still open after the editor closed.
    expect(screen.getByRole("button", { name: "עריכה" })).toBeVisible();
    expect(screen.getByRole("group")).toHaveAttribute("open");
  });
});
