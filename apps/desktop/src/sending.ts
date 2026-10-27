// Round 26: no window before sending. A request the gate cleared goes at once; the review
// screen shows only when she asked to see every request (`status.review_always`, advanced, in
// settings). What the gate still refuses after the fix loop (a test trap, a fault, a request
// that cannot be cut to size) is a line with "לנסות שוב", never a window.
import type { Prepared } from "./ipc/client";

/** Why a request did not go, in her words. The gate's details (which may quote a name) stay
 *  off the screen: only its messages. */
export function notSentMessage(prepared: Prepared): string {
  // A reason that ends its own sentence does not get a second period after it.
  const reasons = [...new Set(prepared.blocked.map((b) => b.message.trim().replace(/\.+$/, "")))];
  return `הבקשה לא נשלחה${reasons.length ? `: ${reasons.join(" · ")}` : ""}. אפשר לנסות שוב.`;
}
