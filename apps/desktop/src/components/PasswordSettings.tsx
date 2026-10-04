// Settings: a new password, or a new printed recovery kit. Either one asks for a new backup,
// because older backups keep opening only with what was in use on their day (D-024).
import { useState, type FormEvent } from "react";
import { useQueryClient } from "@tanstack/react-query";
import { useApp } from "../App";
import { ipc } from "../ipc/client";
import { RecoveryKitPaper } from "./RecoveryKit";
import { Dialog, ErrorLine } from "./ui";

/** The current password, or the recovery kit for whoever forgot it. */
function CurrentSecret(props: { value: string; onChange: (v: string) => void; withRecovery: boolean; onToggle: () => void }) {
  return (
    <>
      <div className="field">
        <label htmlFor="current-secret">{props.withRecovery ? "ערכת השחזור (מהדף המודפס)" : "הסיסמה הנוכחית"}</label>
        <input id="current-secret" className={props.withRecovery ? "input mono" : "input"} type={props.withRecovery ? "text" : "password"}
          dir={props.withRecovery ? "ltr" : undefined} autoComplete={props.withRecovery ? "off" : "current-password"} autoFocus
          placeholder={props.withRecovery ? "7K3M-Q9WD-…" : undefined} value={props.value} onChange={(e) => props.onChange(e.target.value)} />
      </div>
      <button type="button" className="link-btn align-start" onClick={props.onToggle}>
        {props.withRecovery ? "אימות עם הסיסמה" : "הסיסמה נשכחה? אימות עם ערכת השחזור"}
      </button>
    </>
  );
}

function ChangePasswordDialog({ onClose }: { onClose: () => void }) {
  const { notify } = useApp();
  const qc = useQueryClient();
  const [current, setCurrent] = useState("");
  const [withRecovery, setWithRecovery] = useState(false);
  const [pw, setPw] = useState("");
  const [pw2, setPw2] = useState("");
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);

  async function save(e?: FormEvent) {
    e?.preventDefault();
    setError(null);
    if (pw.length < 12) return setError("הסיסמה החדשה צריכה להיות באורך 12 תווים לפחות. מומלץ משפט של כמה מילים.");
    if (pw !== pw2) return setError("שתי הסיסמאות החדשות לא זהות.");
    setBusy(true);
    try {
      await ipc.changePassword(current, withRecovery, pw);
      await qc.invalidateQueries({ queryKey: ["backup"] });
      notify("הסיסמה הוחלפה. כדאי לגבות עכשיו: גיבויים קודמים נפתחים רק בסיסמה הקודמת.");
      onClose();
    } catch (err) {
      setError((err as { message?: string }).message ?? "הסיסמה לא הוחלפה.");
      setCurrent("");
      setBusy(false);
    }
  }

  return (
    <Dialog narrow title="החלפת סיסמה" onClose={onClose}
      footer={<>
        <button type="button" className="btn" onClick={onClose}>ביטול</button>
        <button type="button" className="btn btn-primary" disabled={busy || !current || !pw} onClick={() => void save()}>{busy ? "מחליפה…" : "החלפה"}</button>
      </>}>
      <form className="stack" onSubmit={(e) => void save(e)}>
        <CurrentSecret value={current} onChange={setCurrent} withRecovery={withRecovery}
          onToggle={() => { setWithRecovery(!withRecovery); setCurrent(""); setError(null); }} />
        <div className="field">
          <label htmlFor="new-pw">סיסמה חדשה</label>
          <input id="new-pw" className="input" type="password" autoComplete="new-password" value={pw} onChange={(e) => setPw(e.target.value)} />
        </div>
        <div className="field">
          <label htmlFor="new-pw2">שוב, לאימות</label>
          <input id="new-pw2" className="input" type="password" autoComplete="new-password" value={pw2} onChange={(e) => setPw2(e.target.value)} />
        </div>
        <ul className="pw-checks" aria-label="דרישות הסיסמה">
          <li className={pw.length >= 12 ? "met" : undefined}>{pw.length >= 12 ? "✓" : "○"} 12 תווים לפחות</li>
          <li className={pw2.length > 0 && pw === pw2 ? "met" : undefined}>{pw2.length > 0 && pw === pw2 ? "✓" : "○"} שתי הסיסמאות זהות</li>
        </ul>
        <button type="submit" hidden />
        <ErrorLine error={error} />
      </form>
    </Dialog>
  );
}

function NewKitDialog({ onClose }: { onClose: () => void }) {
  const { notify } = useApp();
  const qc = useQueryClient();
  const [current, setCurrent] = useState("");
  const [withRecovery, setWithRecovery] = useState(false);
  const [kit, setKit] = useState<string | null>(null);
  const [typed, setTyped] = useState("");
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);

  async function issue(e?: FormEvent) {
    e?.preventDefault();
    setBusy(true);
    setError(null);
    try {
      const created = await ipc.newRecoveryKit(current, withRecovery);
      setCurrent("");
      setKit(created.recovery_key);
      await qc.invalidateQueries({ queryKey: ["backup"] });
    } catch (err) {
      setError((err as { message?: string }).message ?? "לא נוצרה ערכה.");
      setCurrent("");
    } finally {
      setBusy(false);
    }
  }

  async function confirm(e?: FormEvent) {
    e?.preventDefault();
    setError(null);
    const ok = await ipc.confirmRecoveryKey(typed).catch(() => false);
    if (!ok) return setError("הערכה שהוקלדה לא תואמת. כדאי לבדוק את הדף ולנסות שוב.");
    setKit(null);
    notify("ערכת השחזור החדשה נשמרה. כדאי לגבות עכשיו.");
    onClose();
  }

  if (kit) {
    // No closing until the new kit is confirmed: the old one no longer opens the vault.
    return (
      <Dialog title="ערכת השחזור החדשה"
        footer={<button type="button" className="btn btn-primary" disabled={typed.trim().length < 20} onClick={() => void confirm()}>סיום</button>}>
        <div className="stack">
          <p>מדפיסים או מעתיקים לדף, ושומרים במקום בטוח, לא במחשב ולא בטלפון. הערכה הקודמת כבר לא פותחת את הכספת.</p>
          <RecoveryKitPaper recoveryKey={kit} />
          <form className="stack" onSubmit={(e) => void confirm(e)}>
            <div className="field">
              <label htmlFor="kit-typed">כדי לוודא שהערכה נשמרה, הקלידי אותה מהדף</label>
              <input id="kit-typed" className="input mono" dir="ltr" placeholder="XXXX-XXXX-…" value={typed} onChange={(e) => setTyped(e.target.value)} />
            </div>
            <ErrorLine error={error} />
          </form>
        </div>
      </Dialog>
    );
  }
  return (
    <Dialog narrow title="ערכת שחזור חדשה" onClose={onClose}
      footer={<>
        <button type="button" className="btn" onClick={onClose}>ביטול</button>
        <button type="button" className="btn btn-primary" disabled={busy || !current} onClick={() => void issue()}>{busy ? "יוצרת…" : "יצירת ערכה חדשה"}</button>
      </>}>
      <form className="stack" onSubmit={(e) => void issue(e)}>
        <p>למשל אם הדף אבד או שמישהו ראה אותו. הערכה הקודמת תפסיק לפתוח את הכספת, אבל גיבויים קודמים עדיין נפתחים בה: לא לזרוק אותה עד שיש גיבוי חדש.</p>
        <CurrentSecret value={current} onChange={setCurrent} withRecovery={withRecovery}
          onToggle={() => { setWithRecovery(!withRecovery); setCurrent(""); setError(null); }} />
        <ErrorLine error={error} />
      </form>
    </Dialog>
  );
}

/** Windows Hello on: the password proves it is her, then Windows asks for her PIN or face. */
function HelloDialog({ onClose }: { onClose: () => void }) {
  const { notify, refresh } = useApp();
  const [password, setPassword] = useState("");
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  async function turnOn(e?: FormEvent) {
    e?.preventDefault();
    if (!password) return;
    setBusy(true);
    setError(null);
    try {
      await ipc.setWindowsHello(true, password);
      setPassword("");
      await refresh();
      notify("מעכשיו אפשר לפתוח את הכספת עם Windows Hello. הסיסמה ממשיכה לעבוד.");
      onClose();
    } catch (err) {
      setError((err as { message?: string }).message ?? "ההפעלה נכשלה.");
    } finally {
      setBusy(false);
    }
  }
  return (
    <Dialog narrow title="פתיחה עם Windows Hello" onClose={onClose}
      footer={<>
        <button type="button" className="btn" onClick={onClose}>ביטול</button>
        <button type="button" className="btn btn-primary" disabled={busy || !password} onClick={() => void turnOn()}>{busy ? "מפעילה…" : "הפעלה"}</button>
      </>}>
      <form className="stack" onSubmit={(e) => void turnOn(e)}>
        <p>הכספת תיפתח עם קוד ה-PIN, הפנים או טביעת האצבע של Windows, רק במחשב הזה. הסיסמה וערכת השחזור ממשיכות לעבוד, וכדאי לזכור את הסיסמה: במחשב אחר או אחרי שחזור מגיבוי רק היא פותחת.</p>
        <div className="field">
          <label htmlFor="hello-password">הסיסמה הנוכחית</label>
          <input id="hello-password" className="input" type="password" autoComplete="current-password" autoFocus
            value={password} onChange={(e) => setPassword(e.target.value)} />
        </div>
        <ErrorLine error={error} />
      </form>
    </Dialog>
  );
}

function HelloSetting() {
  const { status, refresh, notify, fail } = useApp();
  const [open, setOpen] = useState(false);
  if (!status.hello_available && !status.hello_on) return null;
  async function turnOff() {
    try {
      await ipc.setWindowsHello(false, "");
      await refresh();
      notify("Windows Hello כבר לא פותח את הכספת. הסיסמה פותחת אותה.");
    } catch (err) {
      fail(err as never);
    }
  }
  return (
    <div className="row">
      {status.hello_on ? (
        <>
          <span>פתיחה עם Windows Hello: פעילה</span>
          <button type="button" className="btn" onClick={() => void turnOff()}>כיבוי</button>
        </>
      ) : (
        <button type="button" className="btn" onClick={() => setOpen(true)}>פתיחה עם Windows Hello…</button>
      )}
      {open && <HelloDialog onClose={() => setOpen(false)} />}
    </div>
  );
}

export function PasswordSettings() {
  const [open, setOpen] = useState<"password" | "kit" | null>(null);
  return (
    <section className="card setting" aria-labelledby="s-password">
      <h2 id="s-password">סיסמה וערכת שחזור</h2>
      <p className="muted small">הסיסמה פותחת את הכספת ביומיום; ערכת השחזור המודפסת פותחת אותה אם הסיסמה נשכחה. אחרי החלפה של אחת מהן כדאי לגבות מחדש.</p>
      <div className="row">
        <button type="button" className="btn" onClick={() => setOpen("password")}>החלפת סיסמה…</button>
        <button type="button" className="btn" onClick={() => setOpen("kit")}>ערכת שחזור חדשה…</button>
      </div>
      <HelloSetting />
      {open === "password" && <ChangePasswordDialog onClose={() => setOpen(null)} />}
      {open === "kit" && <NewKitDialog onClose={() => setOpen(null)} />}
    </section>
  );
}
