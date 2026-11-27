// The patient picker reads what she typed as the patients' search does. Names are invented.
import { pickerMatches } from "./parts";

const row = (name: string, status = "active") => ({ patient: { name, status } });
const names = (rows: { patient: { name: string } }[]) => rows.map((r) => r.patient.name);

describe("the patient picker's matches", () => {
  it("finds a name typed with a regular letter where the name has a final one", () => {
    const all = [row("דניאל שלום"), row("תמר")];
    expect(names(pickerMatches(all, "שלומ"))).toEqual(["דניאל שלום"]);
    expect(names(pickerMatches(all, "שלום"))).toEqual(["דניאל שלום"]);
  });

  it("lists a name that starts with what she typed first, then a word that starts with it, then the rest", () => {
    const all = [row("איתי"), row("נועה תבור"), row("תמר")];
    expect(names(pickerMatches(all, "ת"))).toEqual(["תמר", "נועה תבור", "איתי"]);
  });

  it("with nothing typed, the open cards", () => {
    expect(names(pickerMatches([row("איתי"), row("יעל", "closed")], "  "))).toEqual(["איתי"]);
  });
});
