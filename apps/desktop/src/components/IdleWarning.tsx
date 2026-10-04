import { he } from "../i18n/he";

/** The last minute before the idle lock: one click keeps the vault open. */
export function IdleWarning({ seconds, onKeep }: { seconds: number; onKeep: () => void }) {
  return (
    <div className="idle-warning" role="alert">
      <span>{he.lock.idleSoon(seconds)}</span>
      <button type="button" className="btn btn-primary" onClick={onKeep}>
        {he.lock.keepWorking}
      </button>
    </div>
  );
}
