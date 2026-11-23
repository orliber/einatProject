import { describe, expect, it } from "vitest";
import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { pageName, SkipLink, usePageTitle } from "./PageAids";
import type { Route } from "../App";

function Titled({ route }: { route: Route }) {
  usePageTitle(route);
  return null;
}

describe("the page's aids for the keyboard and for a screen reader", () => {
  it("names every page, and a patient's card or a case without whose it is", () => {
    expect(pageName({ name: "cases" })).toBe("תיקים");
    expect(pageName({ name: "case", id: "c-1", view: "report" })).toBe("תיק: כתיבת הדוח");
    expect(pageName({ name: "clinic" })).toBe("ניהול הקליניקה: היום");
    expect(pageName({ name: "clinic", tab: "money" })).toBe("ניהול הקליניקה: כספים");
    // The id is all the route knows of the patient, and even that stays out of the title.
    expect(pageName({ name: "clinic", patientId: "p-77" })).toBe("ניהול הקליניקה: כרטיס מטופל");
    expect(pageName({ name: "case", id: "c-77", view: "materials" })).not.toContain("c-77");
  });

  it("sets the window's title to the page, and gives the program's name back when it closes", () => {
    const { rerender, unmount } = render(<Titled route={{ name: "cases" }} />);
    expect(document.title).toBe("תיקים · כספת האבחון");
    rerender(<Titled route={{ name: "settings" }} />);
    expect(document.title).toBe("הגדרות · כספת האבחון");
    unmount();
    expect(document.title).toBe("כספת האבחון");
  });

  it("is the first stop of Tab, and sends the keyboard to the page's content", async () => {
    const user = userEvent.setup();
    render(<>
      <SkipLink />
      <header><button type="button">נעילה</button></header>
      <main><h1>תיקים</h1><button type="button">+ תיק חדש</button></main>
    </>);
    await user.tab();
    const skip = screen.getByRole("button", { name: "דילוג לתוכן העמוד" });
    expect(skip).toHaveFocus();
    await user.keyboard("{Enter}");
    expect(screen.getByRole("main")).toHaveFocus();
    // The next Tab is the first thing in the content, not the bar again.
    await user.tab();
    expect(screen.getByRole("button", { name: "+ תיק חדש" })).toHaveFocus();
  });

  it("lands on the part that stands for the content where a page has no main of its own", async () => {
    const user = userEvent.setup();
    render(<><SkipLink /><div role="main" data-testid="work"><button type="button">חומרים</button></div></>);
    await user.click(screen.getByRole("button", { name: "דילוג לתוכן העמוד" }));
    expect(screen.getByTestId("work")).toHaveFocus();
  });
});
