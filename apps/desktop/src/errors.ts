// The last failure shown by `fail`, so `ErrorLine` can offer the next step (F2, F4, F5)
// without every call site passing more than the message.
import type { UiError } from "./ipc/client";

let last: UiError | null = null;

export function rememberFailure(e: UiError): void {
  last = e;
}

/** The remembered failure, but only if `message` is the one it produced. */
export function failureFor(message: string): UiError | null {
  return last && last.message === message ? last : null;
}
