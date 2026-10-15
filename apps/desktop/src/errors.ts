// The last failure shown by `fail`, so `ErrorLine` can offer its next step (F6) without every
// call site passing more than the message. A failure whose next step is "try again" keeps the
// call that failed, so the same command runs again: nothing is prepared again (a failed send
// keeps its approval in the core). Only commands that answer nothing and carry no password,
// key or file are kept (`ipc/client.ts`); a screen that must take in an answer passes its own
// retry to `ErrorLine` instead.
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

/** Nothing kept survives a lock: the vault that call was for is closed. */
export function forgetFailures(): void {
  last = null;
  lastReplay = null;
}

/** The remembered failure, but only if `message` is the one it produced. */
export function failureFor(message: string): UiError | null {
  return last && last.message === message ? last : null;
}

/** The failed call of that failure, to run again, once: after it worked it is forgotten. */
export function replayFor(e: UiError | null): (() => Promise<unknown>) | null {
  if (!e || !lastReplay || lastReplay.error !== e) return null;
  const { run } = lastReplay;
  return async () => {
    const out = await run();
    if (lastReplay?.error === e) lastReplay = null;
    return out;
  };
}
