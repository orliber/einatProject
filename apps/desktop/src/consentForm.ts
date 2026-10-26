// The parents' consent form (docs/external/CONSENT_DRAFT.md): which version a consent is
// recorded under (D-115). A consent keeps the version it was signed on; only a new one gets
// the current form. Change the constant together with the form, never one without the other.
import type { Consent } from "./ipc/generated/Consent";

/** The version printed at the bottom of the form parents sign today. */
export const CONSENT_FORM_VERSION = "v3";

/** The sample case's marker (`dv-core` cases.rs): not a form at all. */
const SAMPLE = "תיק לדוגמה";

/** The consent to save: the one recorded before keeps its version (never relabelled);
 *  a consent recorded now is on the current form. `null` when she has no consent. */
export function consentToSave(before: Consent | null, given: boolean, givenOn: string, givenBy: string): Consent | null {
  if (!given) return null;
  return {
    given_on: givenOn,
    form_version: before?.form_version ?? CONSENT_FORM_VERSION,
    given_by: givenBy.trim() || "ההורים",
  };
}

/** A quiet line for a consent signed on an older form; `null` when there is nothing to say. */
export function olderFormNote(consent: Consent | null): string | null {
  if (!consent || consent.form_version === CONSENT_FORM_VERSION || consent.form_version === SAMPLE) return null;
  return `ההסכמה נחתמה לפי נוסח ${consent.form_version} של הטופס, ומכסה רק את מה שהיה כתוב בו. הנוסח היום: ${CONSENT_FORM_VERSION}.`;
}
