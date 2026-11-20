// What the tests of the patient list, the patient card and the waiting list share: the clinic
// as the app shows it over the demo core (the route is the app's, and `go` changes it), and
// direct questions to the demo core. Every name is invented.
import { render } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { useState } from "react";
import type { Route } from "../../App";
import { dataOf, type ReplyData, type ReplyKind } from "../../ipc/clinic";
import type { ClinicReply } from "../../ipc/generated/ClinicReply";
import type { ClinicRequest } from "../../ipc/generated/ClinicRequest";
import type { PatientCard } from "../../ipc/generated/PatientCard";
import type { PatientRow } from "../../ipc/generated/PatientRow";
import { appApi, Host, serve, type Override, type Served } from "../../test/host";
import { SettingsScreen } from "../SettingsScreen";
import { ClinicScreen } from "./ClinicScreen";

/** The Sunday morning the tests hold the clock at. */
export const NOW = { date: "2026-11-15", minute: 480 };

function Clinic({ first, onGo, notify }: { first: Route; onGo: (r: Route) => void; notify: (text: string) => void }) {
  const [route, setRoute] = useState(first);
  const app = appApi({ route, notify, go: (r) => { onGo(r); setRoute(r); } });
  return (
    <Host app={app}>
      {route.name === "clinic" ? <ClinicScreen tab={route.tab} patientId={route.patientId} />
        // The clinic's settings are a part of the program's settings (D-205).
        : route.name === "settings" ? <SettingsScreen key={route.pane ?? ""} pane={route.pane} />
        : <p>מסך אחר: {route.name}</p>}
    </Host>
  );
}

/** The protected clipboard is the app's own; here it only says how long the copy stays. */
export const withClipboard: Override = (cmd) => (cmd === "copy_secret" ? 60 : undefined);

/** A refusal of the vault for one clinic request, in her words; everything else is answered as usual. */
export function refusing(op: ClinicRequest["op"], message: string): Override {
  return (cmd, args) => {
    if (cmd === "copy_secret") return 60;
    if (cmd === "clinic" && (args.request as ClinicRequest).op === op) throw { code: "refused", message, details: [], action: { kind: "dismiss" } };
    return undefined;
  };
}

/** One question to the demo core, past the window. */
export async function ask<K extends ReplyKind>(served: Served, request: ClinicRequest, kind: K): Promise<ReplyData<K>> {
  return dataOf(request.op, (await served.core.handle("clinic", { request })) as ClinicReply, kind);
}

export const rowsOf = (served: Served): Promise<PatientRow[]> => ask(served, { op: "patients", today: NOW }, "patients");

/** The id of a demo patient, by the child's name. */
export async function idOf(served: Served, name: string): Promise<string> {
  const id = (await rowsOf(served)).find((r) => r.patient.name === name)?.patient.id;
  if (id === undefined) throw new Error(`no demo patient named ${name}`);
  return id;
}

export const cardOf = (served: Served, id: string): Promise<PatientCard> => ask(served, { op: "patient_card", id, today: NOW }, "card");

/** Shows the clinic on `first` over a core that is already served (and, perhaps, prepared). */
export function show(served: Served, first: Route) {
  const user = userEvent.setup();
  const went: Route[] = [];
  const notes: string[] = [];
  const view = render(<Clinic first={first} onGo={(r) => went.push(r)} notify={(text) => notes.push(text)} />);
  const requests = () => served.calls.filter((c) => c.cmd === "clinic").map((c) => c.args.request as ClinicRequest);
  return { ...served, user, view, went, notes, requests };
}

/** Serves the demo core and shows the clinic on `first`. */
export async function openOn(first: Route, over: Override = withClipboard) {
  return show(await serve(over), first);
}

/** Serves the demo core and opens the card of the demo patient called `name`. */
export async function openCard(name: string, over: Override = withClipboard, prepare?: (served: Served, id: string) => Promise<void>) {
  const served = await serve(over);
  const id = await idOf(served, name);
  await prepare?.(served, id);
  return { ...show(served, { name: "clinic", tab: "patients", patientId: id }), id };
}
