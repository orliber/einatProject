// The printed recovery kit: the only way into the vault if the password is forgotten.
import { useState } from "react";
import { ipc } from "../ipc/client";
import "./RecoveryKit.css";

/** Print only the kit. The app uses the system print window (the macOS webview ignores
 * `window.print()`); the browser preview falls back to the browser's own. */
async function printKit(): Promise<boolean> {
  document.body.classList.add("printing-kit");
  const done = () => {
    document.body.classList.remove("printing-kit");
    window.removeEventListener("afterprint", done);
  };
  window.addEventListener("afterprint", done);
  // The class only affects printing, so leaving it a while is harmless.
  window.setTimeout(done, 120_000);
  try {
    await ipc.printPage();
    return true;
  } catch {
    try {
      window.print();
      return true;
    } catch {
      done();
      return false;
    }
  }
}

export function RecoveryKitPaper({ recoveryKey }: { recoveryKey: string }) {
  const groups = recoveryKey.split("-").filter(Boolean);
  const [failed, setFailed] = useState(false);
  return (
    <div className="paper kit">
      <div className="kit-head"><span>ערכת שחזור · כספת האבחון</span><span>{new Date().toLocaleDateString("he-IL")}</span></div>
      <div className="kit-grid" dir="ltr">
        {groups.map((g, i) => (
          <span key={i} className={i === groups.length - 1 ? "kit-check" : undefined}>{g}</span>
        ))}
      </div>
      <p className="hint">אין הבדל בין אותיות גדולות וקטנות. הקבוצה האחרונה בודקת טעויות הקלדה.</p>
      {failed && <p className="error no-print">ההדפסה לא נפתחה. אפשר להעתיק את הקבוצות לדף בכתב יד: זה עובד בדיוק אותו דבר.</p>}
      <div className="row no-print">
        <button type="button" className="btn btn-primary" onClick={() => void printKit().then((ok) => setFailed(!ok))}>הדפסה</button>
      </div>
    </div>
  );
}
