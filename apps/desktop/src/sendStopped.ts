// Whether she stopped sending (the emergency stop), for the sign in the top bar and the case's
// bar while she works elsewhere. Only this one yes or no, kept for the window's session (it
// outlives the reload at a lock, like the stop itself, and goes with the program, as the stop
// does). The core's answer always wins. Its own small module, so the bars that show the sign
// do not pull the settings page into the main bundle.
import { useSyncExternalStore } from "react";

const STOP_KEY = "dv.sendStopped";
function readStop(): boolean {
  try {
    return sessionStorage.getItem(STOP_KEY) === "1";
  } catch {
    return false;
  }
}
let sendStopped = readStop();
const stopListeners = new Set<() => void>();

/** What the core last said about the emergency stop. */
export function rememberSendStopped(on: boolean): void {
  if (on === sendStopped) return;
  sendStopped = on;
  try {
    if (on) sessionStorage.setItem(STOP_KEY, "1");
    else sessionStorage.removeItem(STOP_KEY);
  } catch {
    // Storage off: the sign shows until the next reload.
  }
  stopListeners.forEach((l) => l());
}

/** Sending to the AI is stopped (emergency stop), as far as the window knows. */
export function useSendStopped(): boolean {
  return useSyncExternalStore((l) => {
    stopListeners.add(l);
    return () => stopListeners.delete(l);
  }, () => sendStopped);
}
