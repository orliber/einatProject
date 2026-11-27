// The age computed from a birth date goes into the fields at once: a "שימוש בגיל" not pressed
// lost it before. A date still being typed never leaves a wrong age behind. Invented dates only.
import { fireEvent, render, screen } from "@testing-library/react";
import { useState } from "react";
import { AgeField } from "./AgeField";

function Form() {
  const [age, setAge] = useState({ years: "", months: "" });
  return <AgeField idPrefix="t" years={age.years} months={age.months} onChange={(years, months) => setAge({ years, months })} />;
}

describe("the age from a birth date", () => {
  it("fills the years and months as soon as the date is a date, with no button to press", () => {
    render(<Form />);
    fireEvent.click(screen.getByRole("button", { name: "חישוב מתאריך לידה" }));
    fireEvent.change(screen.getByLabelText("תאריך האבחון"), { target: { value: "28.09.2026" } });
    fireEvent.change(screen.getByLabelText("תאריך לידה"), { target: { value: "20.05.2021" } });
    expect(screen.getByLabelText("שנים")).toHaveValue("5");
    expect(screen.getByLabelText("חודשים")).toHaveValue("4");
    expect(screen.getByRole("status")).toHaveTextContent("הגיל 5:4 הוזן בשדות.");
    fireEvent.click(screen.getByRole("button", { name: "סיום" }));
    expect(screen.getByLabelText("שנים")).toHaveValue("5");
    expect(screen.queryByLabelText("תאריך לידה")).toBeNull();
  });

  it("a date that stops being one (its year still typed) puts back the age typed before", () => {
    render(<Form />);
    fireEvent.change(screen.getByLabelText("שנים"), { target: { value: "6" } });
    fireEvent.click(screen.getByRole("button", { name: "חישוב מתאריך לידה" }));
    fireEvent.change(screen.getByLabelText("תאריך האבחון"), { target: { value: "28.09.2026" } });
    // "1.5.20" is already a date (2020); "1.5.202" is not.
    fireEvent.change(screen.getByLabelText("תאריך לידה"), { target: { value: "1.5.20" } });
    expect(screen.getByLabelText("שנים")).toHaveValue("6");
    expect(screen.getByLabelText("חודשים")).toHaveValue("4");
    fireEvent.change(screen.getByLabelText("תאריך לידה"), { target: { value: "1.5.202" } });
    expect(screen.getByLabelText("שנים")).toHaveValue("6");
    expect(screen.getByLabelText("חודשים")).toHaveValue("");
    fireEvent.change(screen.getByLabelText("תאריך לידה"), { target: { value: "1.5.2021" } });
    expect(screen.getByLabelText("שנים")).toHaveValue("5");
    expect(screen.getByLabelText("חודשים")).toHaveValue("4");
  });
});
