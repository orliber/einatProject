// A message to the family from her own device (D-201): one button that opens a short menu,
// WhatsApp, SMS, email, or copy only. Nothing is sent by the program: the chat opens on her
// computer with the family's number (or address), the filled text waits on the protected
// clipboard, and she pastes it and presses send herself.
import { useApp } from "../../App";
import { AddMenu } from "../../components/Menu";
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

/** What the message is, as the toasts name it. */
const WHAT: Record<MessageKind, string> = {
  reminder: "התזכורת",
  payment: "הודעת התשלום",
};

export function SendMenu({ kind, patientId, visitId = null, label }: {
  kind: MessageKind;
  patientId: string;
  visitId?: string | null;
  label: string;
}) {
  const { notify, fail } = useApp();

  /** Opens the chat on her device; the text goes to the protected clipboard only. */
  async function send(channel: Exclude<Channel, "unknown">) {
    try {
      const handed = await ipc.sendMessage(kind, patientId, visitId, channel, null);
      notify(`${OPENED[channel]} מול ${handed.to}. ${WHAT[kind]} בלוח המוגן: מדביקים (Ctrl+V) ושולחים. ${clearsIn(handed.clears_in)}.`);
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

  return (
    <AddMenu label={label} className="btn btn-small" items={[
      { label: "לשלוח בוואטסאפ", run: () => void send("whatsapp") },
      { label: "לשלוח ב-SMS", run: () => void send("sms") },
      { label: "לשלוח במייל", run: () => void send("email") },
      { label: "רק להעתיק", run: () => void copy() },
    ]}>{label}</AddMenu>
  );
}
