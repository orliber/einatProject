// The bug report she copies for Or: what a failure says, never a path (D-120).
import { bugReportText, withoutPaths } from "./BugReport";

describe("bug report", () => {
  it("cuts a path out of a detail, with anything after it", () => {
    expect(withoutPaths("file error: NotFound (os error 2)")).toBe("file error: NotFound (os error 2)");
    expect(withoutPaths("failed at path /home/someone/TEST-0042 בדוי.docx")).toBe("failed at path [נתיב הוסר]");
    expect(withoutPaths("open C:\\Users\\Fictional User\\x")).toBe("open [נתיב הוסר]");
  });

  it("no detail line in the report holds a path separator", () => {
    const text = bugReportText({
      version: "0.9.0",
      commit: "dev",
      platform: "windows",
      keyCheck: null,
      screen: "הגדרות",
      failure: { code: "internal", details: ["Error: C:\\Users\\Fictional User\\AppData\\TEST-0042", "plain"] },
      crashes: [],
    });
    const details = text.split("\n").slice(text.split("\n").indexOf("פרטים:") + 1, -1);
    expect(details).toEqual(["Error: [נתיב הוסר]", "plain"]);
    expect(text).not.toContain("TEST-0042");
    expect(text).not.toContain("Fictional");
  });
});
