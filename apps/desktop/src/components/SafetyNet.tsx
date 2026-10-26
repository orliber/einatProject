// "The safety net" (D-107), on General and on the security page: can she still get in if the
// password is forgotten (sign-in with Google), and do her cases survive a broken or lost
// computer (a recent backup). The core makes the row from the two statuses; when a part is
// missing the row turns red and each missing part has its one button.
import { keepPreviousData, useQuery, useQueryClient } from "@tanstack/react-query";
import { useState } from "react";
import { useApp } from "../App";
import { ipc, type NetPart } from "../ipc/client";
import { useBackupStatus, useWriteBackup } from "./Backup";
import { TurnOnDialog, useGoogleStatus } from "./GoogleRecovery";
import { ErrorLine } from "./ui";
import "./SafetyNet.css";

function Part({ p }: { p: NetPart }) {
  return (
    <span className={`net-part ${p.state}`}>
      {p.text}
      {p.state !== "unavailable" && (
        <>
          <span className="net-mark" aria-hidden="true">{p.state === "ok" ? "✓" : "!"}</span>
          <span className="visually-hidden">{p.state === "ok" ? " (תקין)" : " (חסר)"}</span>
        </>
      )}
    </span>
  );
}

export function SafetyNet() {
  const { notify, fail } = useApp();
  const qc = useQueryClient();
  // Made again whenever either status is: a backup made or Google turned on anywhere in Settings.
  const google = useGoogleStatus();
  const backup = useBackupStatus();
  const net = useQuery({
    queryKey: ["safety-net", google.dataUpdatedAt, backup.dataUpdatedAt],
    queryFn: ipc.safetyNet,
    placeholderData: keepPreviousData,
  });
  const { write, busy, error } = useWriteBackup();
  const [turningOn, setTurningOn] = useState(false);

  if (net.isError) return <ErrorLine error={fail(net.error as never)} retry={() => void qc.invalidateQueries({ queryKey: ["safety-net"] })} />;
  const n = net.data;
  if (!n) return null;
  const googleMissing = n.google.state === "missing";
  const backupMissing = n.backup.state === "missing";
  return (
    <section className={`safety-net ${n.ok ? "ok" : "missing"}`} aria-labelledby="s-net">
      <div className="net-line">
        <h2 id="s-net" className="net-title">רשת הביטחון</h2>
        <p className="net-parts">
          אם תשכחי את הסיסמה: <Part p={n.google} /><span className="net-dot" aria-hidden="true"> · </span><Part p={n.backup} />
        </p>
      </div>
      {googleMissing && (
        <div className="net-fix">
          <p className="net-hint">{n.google.hint}</p>
          <button type="button" className="btn btn-small" onClick={() => setTurningOn(true)}>להפעיל כניסה עם גוגל</button>
        </div>
      )}
      {backupMissing && (
        <div className="net-fix">
          <p className="net-hint">{n.backup.hint}</p>
          <button type="button" className="btn btn-small btn-primary" disabled={busy} onClick={() => void write()}>
            {busy ? "מגבה…" : "לגבות עכשיו"}
          </button>
        </div>
      )}
      <ErrorLine error={error} />
      {turningOn && <TurnOnDialog onClose={() => setTurningOn(false)} onTurnedOn={async () => {
        await qc.invalidateQueries({ queryKey: ["google"] });
        notify("הכניסה עם גוגל הופעלה. אם הסיסמה תישכח, נכנסים מהמחשב הזה עם חשבון הגוגל.");
      }} />}
    </section>
  );
}
