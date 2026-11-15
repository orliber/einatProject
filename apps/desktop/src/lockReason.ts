// Why the vault last locked in this window: an idle lock, or anything else (by hand, sleep,
// first start). One flag in the session's storage, with no case data; the lock screen reads it.
const KEY = "dv-lock-was-idle";

export function noteLockReason(idle: boolean): void {
  try {
    sessionStorage.setItem(KEY, idle ? "1" : "0");
  } catch {
    // No storage: the lock screen says the neutral sentence.
  }
}

export function lockWasIdle(): boolean {
  try {
    return sessionStorage.getItem(KEY) === "1";
  } catch {
    return false;
  }
}
