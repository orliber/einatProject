// Consent form version (D-115): a recorded consent is never relabelled, a new one is on today's form.
import { CONSENT_FORM_VERSION, consentToSave, olderFormNote } from "./consentForm";

const old = { given_on: "2026-09-01", form_version: "v1", given_by: "שני ההורים" };

describe("consent form version", () => {
  it("is the version printed on the form", () => {
    expect(CONSENT_FORM_VERSION).toBe("v3");
  });

  it("records a new consent on the current form", () => {
    expect(consentToSave(null, true, "2026-10-26", " האם ")).toEqual({
      given_on: "2026-10-26", form_version: CONSENT_FORM_VERSION, given_by: "האם",
    });
  });

  it("keeps the version a consent was recorded with, also when its date or signer changes", () => {
    expect(consentToSave(old, true, "2026-09-02", "האב")).toEqual({
      given_on: "2026-09-02", form_version: "v1", given_by: "האב",
    });
  });

  it("puts a re-signed consent on the current form, only when she says the parents signed again", () => {
    expect(consentToSave(old, true, "2026-10-26", "שני ההורים", true)?.form_version).toBe(CONSENT_FORM_VERSION);
    expect(consentToSave(old, true, "2026-10-26", "שני ההורים", false)?.form_version).toBe("v1");
  });

  it("saves no consent when she has none, and names the parents when no signer is typed", () => {
    expect(consentToSave(old, false, "2026-09-01", "האם")).toBeNull();
    expect(consentToSave(null, true, "2026-10-26", "  ")?.given_by).toBe("ההורים");
  });

  it("notes an older form quietly, and nothing for today's form, no consent or the sample case", () => {
    expect(olderFormNote(old)).toContain("נוסח v1");
    expect(olderFormNote({ ...old, form_version: CONSENT_FORM_VERSION })).toBeNull();
    expect(olderFormNote(null)).toBeNull();
    expect(olderFormNote({ ...old, form_version: "תיק לדוגמה" })).toBeNull();
  });
});
