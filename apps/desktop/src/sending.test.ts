import { prepared } from "./test/fixtures";
import { notSentMessage } from "./sending";

const block = (message: string) => ({ code: "pattern", message, detail: null });

describe("why a request did not go", () => {
  it("names each reason once, with one period at the end", () => {
    expect(notSentMessage(prepared({ approval_id: null, blocked: [] }))).toBe("הבקשה לא נשלחה. אפשר לנסות שוב.");
    expect(notSentMessage(prepared({ approval_id: null, blocked: [block("נמצא פרט מזהה (מספר, תאריך, כתובת…)"), block("נמצא פרט מזהה (מספר, תאריך, כתובת…)")] })))
      .toBe("הבקשה לא נשלחה: נמצא פרט מזהה (מספר, תאריך, כתובת…). אפשר לנסות שוב.");
    expect(notSentMessage(prepared({ approval_id: null, blocked: [block("הבקשה גדולה מהמותר."), block("נמצאה מחרוזת מלכודת")] })))
      .toBe("הבקשה לא נשלחה: הבקשה גדולה מהמותר · נמצאה מחרוזת מלכודת. אפשר לנסות שוב.");
  });
});
