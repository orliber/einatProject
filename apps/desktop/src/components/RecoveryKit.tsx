// The printed recovery kit: the only way into the vault if the password is forgotten.
import "./RecoveryKit.css";

function printKit() {
  document.body.classList.add("printing-kit");
  const done = () => {
    document.body.classList.remove("printing-kit");
    window.removeEventListener("afterprint", done);
  };
  window.addEventListener("afterprint", done);
  window.print();
}

export function RecoveryKitPaper({ recoveryKey }: { recoveryKey: string }) {
  const groups = recoveryKey.split("-").filter(Boolean);
  return (
    <div className="paper kit">
      <div className="kit-head"><span>ערכת שחזור · כספת האבחון</span><span>{new Date().toLocaleDateString("he-IL")}</span></div>
      <div className="kit-grid" dir="ltr">
        {groups.map((g, i) => (
          <span key={i} className={i === groups.length - 1 ? "kit-check" : undefined}>{g}</span>
        ))}
      </div>
      <p className="hint">אין הבדל בין אותיות גדולות וקטנות. הקבוצה האחרונה בודקת טעויות הקלדה.</p>
      <div className="row no-print">
        <button type="button" className="btn btn-primary" onClick={printKit}>הדפסה</button>
      </div>
    </div>
  );
}
