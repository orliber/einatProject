// A message to the family from her own device (D-201): one button that opens a short menu,
// WhatsApp, SMS, email, or copy only. Nothing is sent by the program: the chat opens on her
// computer with the family's number (or address), the filled text waits on the protected
// clipboard, and she pastes it and presses send herself.
import { useApp } from "../../App";
import { AddMenu } from "../../components/Menu";
import { isDiscreet, shownName } from "../../discreet";
import { asUiError, ipc } from "../../ipc/client";
import { clinicApi } from "../../ipc/clinic";
import type { Channel } from "../../ipc/generated/Channel";
import type { MessageKind } from "../../ipc/generated/MessageKind";

/** When the protected clipboard lets go of a copy (the app says how long: 20 s on a Mac). */
export function clearsIn(seconds: number): string {
  return seconds === 60 ? "יימחק מהלוח בעוד דקה" : `יימחק מהלוח בעוד ${seconds} שניות`;
}

/** What opened on her computer, as the toast says it. */
const OPENED: Record<Exclude<Channel, "unknown">, string> = {
  whatsapp: "וואטסאפ נפתח",
  sms: "חלון ה-SMS נפתח",
  email: "תוכנת המייל נפתחה",
};

/** What the core says in place of a contact who has no name on the card (`contact_label` in
 *  crates/dv-domain/src/clinic/channels.rs; the browser preview's stand-in says the same). It
 *  is no name: the toast shows it as it is, never as initials ("א. ה."), also in discreet mode. */
export const NAMELESS_CONTACT = "איש הקשר";

/** What the message is, as the toasts name it. */
const WHAT: Record<MessageKind, string> = {
  reminder: "התזכורת",
  payment: "הודעת התשלום",
};

/**
 * Sending a message to the family, for a menu that offers it ("תזכורת", "הודעת תשלום"). The
 * toast names the contact as the screen may show it now: initials in discreet mode (D-192; a gap
 * since D-201, closed in D-204), and "איש הקשר" as it is (`NAMELESS_CONTACT`). It never carries
 * the message itself.
 */
export function useSend(kind: MessageKind, patientId: string, visitId: string | null = null): {
  send: (channel: Exclude<Channel, "unknown">) => Promise<void>;
  copy: () => Promise<void>;
} {
  const { notify, fail } = useApp();

  /** Opens the chat on her device; the text goes to the protected clipboard only. */
  async function send(channel: Exclude<Channel, "unknown">) {
    try {
      const handed = await ipc.sendMessage(kind, patientId, visitId, channel, null);
      const to = handed.to === NAMELESS_CONTACT ? NAMELESS_CONTACT : shownName(handed.to, isDiscreet());
      // Initials end with their own full stop ("י. ל."): the sentence does not add a second one.
      notify(`${OPENED[channel]} מול ${to}${to.endsWith(".") ? "" : "."} ${WHAT[kind]} בלוח המוגן: מדביקים (Ctrl+V) ושולחים. ${clearsIn(handed.clears_in)}.`);
    } catch (e) {
      notify(fail(asUiError(e)));
    }
  }

  /** Copy only, for any other way she sends. */
  async function copy() {
    try {
      const text = await clinicApi.message(kind, patientId, visitId);
      // The text carries names: only the protected clipboard (out of clipboard history and
      // cloud sync, cleared by itself). Never the ordinary one.
      const seconds = await ipc.copySecret(text);
      notify(`${WHAT[kind]} הועתקה ללוח המוגן, ואפשר להדביק אותה בהודעה. ${clearsIn(seconds)}.`);
    } catch (e) {
      notify(fail(asUiError(e)));
    }
  }

  return { send, copy };
}

export function SendMenu({ kind, patientId, visitId = null, label }: {
  kind: MessageKind;
  patientId: string;
  visitId?: string | null;
  label: string;
}) {
  const { send, copy } = useSend(kind, patientId, visitId);
  return (
    <AddMenu label={label} className="btn btn-small" items={[
      { label: "לשלוח בוואטסאפ", run: () => void send("whatsapp") },
      { label: "לשלוח ב-SMS", run: () => void send("sms") },
      { label: "לשלוח במייל", run: () => void send("email") },
      { label: "רק להעתיק", run: () => void copy() },
    ]}>{label}</AddMenu>
  );
}
