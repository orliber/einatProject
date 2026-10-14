// The last failure shown by `fail`, so `ErrorLine` can offer its next step (F6) without every
// call site passing more than the message. A failure whose next step is "try again" keeps the
// call that failed, so the same command runs again: nothing is prepared again (a failed send
// keeps its approval in the core).
import type { UiError } from "./ipc/client";

let last: UiError | null = null;
let lastReplay: { error: UiError; run: () => Promise<unknown> } | null = null;

export function rememberFailure(e: UiError): void {
  last = e;
}

/** Keep the failed call itself, for "לנסות שוב". Only for failures that offer a retry. */
export function rememberReplay(e: UiError, run: () => Promise<unknown>): void {
  if (e.action.kind === "retry") lastReplay = { error: e, run };
}

/** The remembered failure, but only if `message` is the one it produced. */
export function failureFor(message: string): UiError | null {
  return last && last.message === message ? last : null;
}

/** The failed call of that failure, to run again. */
export function replayFor(e: UiError | null): (() => Promise<unknown>) | null {
  return e && lastReplay && lastReplay.error === e ? lastReplay.run : null;
}
