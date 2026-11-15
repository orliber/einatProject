// A screen with fields that are saved only by a button (the case details) says here that it holds
// changes. Going elsewhere then asks first, instead of dropping what she filled in.

export interface LeaveGuard {
  /** Whether something was changed and not saved. */
  dirty: () => boolean;
  /** Saves; resolves `false` when it did not (the screen shows why), and she stays. */
  save: () => Promise<boolean>;
}

let guard: LeaveGuard | null = null;

/** Registers the screen's guard; the returned function removes it (on unmount). */
export function setLeaveGuard(g: LeaveGuard): () => void {
  guard = g;
  return () => {
    if (guard === g) guard = null;
  };
}

/** The guard that holds unsaved changes right now, if any. */
export function unsavedGuard(): LeaveGuard | null {
  return guard && guard.dirty() ? guard : null;
}
