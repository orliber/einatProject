// U7: search inside a case, in materials and paragraphs, as text. Invented names only.
import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import type { CaseDetail } from "../../ipc/client";
import { CaseSearch, searchCase } from "./CaseSearch";

const detail = {
  inputs: [{ id: "i1", case_id: "c", kind: "observation", title: "תצפית בגן", content: "נועה בנתה מגדל קוביות <b>גבוה</b> ושמחה.", created_at: 0 }],
  sections: [{ key: "play", title: "משחק", paragraphs: [{ id: "d1", text: "בבניית המגדל נועה הייתה קשובה. מגדל נוסף נבנה בסוף." }] }],
} as unknown as CaseDetail;

describe("searchCase", () => {
  it("finds a word in materials and paragraphs, ignoring niqqud, with the text around it", () => {
    const hits = searchCase(detail, "מִגְדָּל");
    expect(hits.map((h) => [h.where, h.target, h.count])).toEqual([["material", "i1", 1], ["paragraph", "play", 2]]);
    expect(hits[0]?.match).toBe("מגדל");
    expect(searchCase(detail, "א")).toEqual([]);
    expect(searchCase(detail, "אין כזה")).toEqual([]);
  });
});

describe("CaseSearch", () => {
  it("shows results as text and opens the section of a paragraph", async () => {
    const onOpen = vi.fn();
    const user = userEvent.setup();
    const { container } = render(<CaseSearch detail={detail} onClose={vi.fn()} onOpen={onOpen} />);
    await user.type(screen.getByRole("searchbox", { name: "חיפוש בתיק" }), "גבוה");
    expect(container.querySelector("b")).toBeNull();
    expect(screen.getByText("בחומרים (1)")).toBeInTheDocument();
    await user.clear(screen.getByRole("searchbox", { name: "חיפוש בתיק" }));
    await user.type(screen.getByRole("searchbox", { name: "חיפוש בתיק" }), "קשובה");
    await user.click(screen.getByRole("button", { name: /משחק/ }));
    expect(onOpen).toHaveBeenCalledWith(expect.objectContaining({ where: "paragraph", target: "play" }));
  });
});
